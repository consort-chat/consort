// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a space says about the rooms this account has not joined.
//!
//! A space advertises its children with `m.space.child`, and that event
//! carries a room ID, a list of servers, and nothing else. For a child the
//! account is in that is enough, because everything else is in the local
//! store. For a child it has never joined there is nothing local at all: no
//! name, no avatar, no room type.
//!
//! Hiding those would make Consort disagree with every other client about how
//! many channels a space has, and showing a room ID where a name goes is not a
//! room list. So the space is asked, through `/hierarchy`, which is the one
//! request in this whole module.
//!
//! ## Why it is not on the snapshot path
//!
//! [`super::watch`] recomputes on every sync that touched a room, forever. A
//! request in there would turn an idle client into a client that polls. This
//! is asked once per space per distinct set of unjoined children instead: on
//! an account where nothing changes, exactly once, and never again.
//!
//! That includes failures. A request that does not come back still counts as
//! having been asked, because the alternative is retrying on every sync of a
//! busy account, which is the poll this exists to avoid. The channels stay
//! unnamed until the space's child list changes or the client restarts, which
//! is a worse outcome than a name and a much better one than a poll.

use std::collections::{BTreeSet, HashMap};

use matrix_sdk::Client;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::api::client::space::get_hierarchy;

use super::dto::{ChannelKind, Rooms};
use super::facts::{RoomKind, classify};

/// How many pages of one space's hierarchy to read.
///
/// Synapse answers with a hundred rooms a page by default and this asks only
/// for direct children, so one page covers any space a person would make. The
/// cap is a backstop against a homeserver that paginates forever, and stopping
/// early is logged rather than silently truncating the list.
const MAX_PAGES: usize = 5;

/// What the homeserver said about one room.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Listed {
    /// `None` when the room genuinely has no name. Left as an absence rather
    /// than filled with the room ID, same as everywhere else here.
    name: Option<String>,
    avatar: Option<String>,
    kind: ChannelKind,
}

/// Names for rooms this account is not in, and a memory of what was asked.
#[derive(Debug, Default)]
pub(crate) struct Directory {
    /// Keyed by room ID.
    known: HashMap<String, Listed>,
    /// The set of unjoined children last asked about, keyed by space ID.
    ///
    /// The set rather than a flag, so that a space gaining or losing a child
    /// asks again and a space that is merely resynced does not.
    asked: HashMap<String, BTreeSet<String>>,
}

impl Directory {
    /// Fill in everything already known about unjoined children.
    ///
    /// Pure, and applied to every snapshot rather than only to the one that
    /// follows a request: a later sync rebuilds the tree from the local store,
    /// which knows nothing about any of this.
    pub(crate) fn apply(&self, rooms: &mut Rooms) {
        for space in &mut rooms.spaces {
            for channel in &mut space.channels {
                if channel.joined {
                    continue;
                }
                let Some(listed) = self.known.get(&channel.id) else {
                    continue;
                };

                channel.name.clone_from(&listed.name);
                channel.avatar.clone_from(&listed.avatar);
                channel.kind = listed.kind;
            }
        }
    }

    /// Ask about any space whose unjoined children have not been asked about.
    ///
    /// Returns whether anything new was learned, which is the caller's cue to
    /// apply and send the tree again. False is the ordinary answer, and on an
    /// account where every child is joined it is the only one.
    pub(crate) async fn refresh(&mut self, client: &Client, rooms: &Rooms) -> bool {
        let mut learned = false;

        for space in &rooms.spaces {
            let unjoined: BTreeSet<String> = space
                .channels
                .iter()
                .filter(|channel| !channel.joined)
                .map(|channel| channel.id.clone())
                .collect();

            if unjoined.is_empty() {
                // Nothing to ask. Forgetting that we asked means a child
                // removed and later put back is asked about again, which is
                // right: it may have been renamed in between.
                self.asked.remove(&space.id);
                continue;
            }

            if self.asked.get(&space.id) == Some(&unjoined) {
                continue;
            }

            // Recorded before the request rather than after it, so that a
            // homeserver that will not answer costs one attempt rather than
            // one per sync.
            self.asked.insert(space.id.clone(), unjoined);
            learned |= self.ask(client, &space.id).await;
        }

        learned
    }

    /// Read one space's direct children off the homeserver.
    async fn ask(&mut self, client: &Client, space_id: &str) -> bool {
        let Ok(room_id) = RoomId::parse(space_id) else {
            // Home, which is a rail entry rather than a room. It has no
            // children to ask about and never reaches here with any, so this
            // is belt and braces rather than a case.
            return false;
        };

        let mut learned = false;
        let mut from = None;

        for page in 0..MAX_PAGES {
            let mut request = get_hierarchy::v1::Request::new(room_id.clone());
            // Direct children only. Without this a space of spaces walks the
            // whole tree to fill in two names.
            request.max_depth = Some(1u32.into());
            request.from = from;

            let response = match client.send(request).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        space = space_id,
                        page,
                        "could not read a space's children from the homeserver"
                    );
                    return learned;
                }
            };

            for chunk in response.rooms {
                let summary = chunk.summary;
                let listed = Listed {
                    name: summary.name,
                    avatar: summary.avatar_url.map(|uri| uri.to_string()),
                    kind: match classify(summary.room_type.as_ref().map(|kind| kind.as_str())) {
                        RoomKind::Voice => ChannelKind::Voice,
                        RoomKind::Space | RoomKind::Text => ChannelKind::Text,
                    },
                };

                let id = summary.room_id.to_string();
                if self.known.get(&id) != Some(&listed) {
                    self.known.insert(id, listed);
                    learned = true;
                }
            }

            from = response.next_batch;
            if from.is_none() {
                return learned;
            }
        }

        // Reached only by a homeserver handing out pages without end. Said out
        // loud, because a room list quietly missing its tail looks like a room
        // list that is complete.
        tracing::warn!(
            space = space_id,
            pages = MAX_PAGES,
            "stopped reading a space's children early; some may be unnamed"
        );
        learned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::dto::{Channel, Space};

    fn listed(name: Option<&str>, kind: ChannelKind) -> Listed {
        Listed {
            name: name.map(str::to_owned),
            avatar: None,
            kind,
        }
    }

    fn channel(id: &str, joined: bool) -> Channel {
        Channel {
            id: id.to_owned(),
            name: if joined {
                Some("known".to_owned())
            } else {
                None
            },
            kind: ChannelKind::Text,
            avatar: None,
            joined,
        }
    }

    fn rooms(channels: Vec<Channel>) -> Rooms {
        Rooms {
            spaces: vec![Space {
                id: "!s:example.org".to_owned(),
                name: "Kahu HQ".to_owned(),
                avatar: None,
                channels,
            }],
        }
    }

    fn directory(entries: Vec<(&str, Listed)>) -> Directory {
        Directory {
            known: entries
                .into_iter()
                .map(|(id, listed)| (id.to_owned(), listed))
                .collect(),
            asked: HashMap::new(),
        }
    }

    #[test]
    fn a_name_from_the_homeserver_reaches_an_unjoined_child() {
        let mut tree = rooms(vec![channel("!never:example.org", false)]);

        directory(vec![(
            "!never:example.org",
            listed(Some("announcements"), ChannelKind::Text),
        )])
        .apply(&mut tree);

        assert_eq!(
            tree.spaces[0].channels[0].name.as_deref(),
            Some("announcements")
        );
    }

    #[test]
    fn an_unjoined_call_room_becomes_a_voice_channel() {
        // The room type is in the same response as the name, so a channel
        // nobody has joined can still be shown as the voice channel it is.
        let mut tree = rooms(vec![channel("!never:example.org", false)]);

        directory(vec![(
            "!never:example.org",
            listed(Some("Lounge"), ChannelKind::Voice),
        )])
        .apply(&mut tree);

        assert_eq!(tree.spaces[0].channels[0].kind, ChannelKind::Voice);
    }

    #[test]
    fn a_joined_channel_is_left_alone() {
        // The local store is the better source for a room we are in, and a
        // stale hierarchy response must not overwrite it.
        let mut tree = rooms(vec![channel("!joined:example.org", true)]);

        directory(vec![(
            "!joined:example.org",
            listed(Some("something else"), ChannelKind::Voice),
        )])
        .apply(&mut tree);

        assert_eq!(tree.spaces[0].channels[0].name.as_deref(), Some("known"));
        assert_eq!(tree.spaces[0].channels[0].kind, ChannelKind::Text);
    }

    #[test]
    fn a_child_the_homeserver_did_not_mention_stays_unnamed() {
        let mut tree = rooms(vec![channel("!never:example.org", false)]);

        Directory::default().apply(&mut tree);

        assert_eq!(tree.spaces[0].channels[0].name, None);
    }

    #[test]
    fn a_room_the_homeserver_says_has_no_name_is_not_given_its_id() {
        // A room with no `m.room.name` comes back with a null name. Filling
        // that with the room ID is the one thing this must never do.
        let mut tree = rooms(vec![channel("!never:example.org", false)]);

        directory(vec![(
            "!never:example.org",
            listed(None, ChannelKind::Text),
        )])
        .apply(&mut tree);

        assert_eq!(tree.spaces[0].channels[0].name, None);
    }
}
