// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for the room list.
//!
//! One value describes the whole tree: every rail entry, and every channel
//! under each of them. The alternative, telling the frontend that a room was
//! added or renamed and letting it patch its own copy, is where room lists go
//! wrong. Twenty-six rooms serialise to a few kilobytes, and a value that is
//! always complete is a value a late subscriber can be handed as-is.
//!
//! None of the SDK's own types appear here, for the same reason they do not
//! appear in the verification DTOs: the wire format is a contract with
//! `app/src/lib/api.ts`, and pinning it to an upstream type means an SDK bump
//! can silently change what the webview receives.

use serde::{Deserialize, Serialize};

/// The rail entry holding rooms that belong to no joined space.
///
/// A real room ID always begins with `!`, so this string cannot collide with
/// one. That is the whole reason it is a plain `String` rather than an
/// `Option<String>` the frontend has to unwrap at every key and comparison.
pub const HOME_ID: &str = "home";

/// What the Home entry is called.
const HOME_NAME: &str = "Home";

/// Everything the shell draws.
///
/// Home is always present and always first, even when it is empty: the button
/// is part of the furniture, and a rail whose first entry moves depending on
/// whether the account has direct messages is a rail that jumps around.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rooms {
    pub spaces: Vec<Space>,
}

/// One entry in the left rail, and the channels underneath it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    /// The room ID, or [`HOME_ID`].
    pub id: String,
    pub name: String,
    /// An `mxc://` URI, not image bytes. Fetching the bytes is a separate
    /// command, because putting them here would multiply a small payload by
    /// the number of rooms and re-send all of it every time one is renamed.
    pub avatar: Option<String>,
    /// Sorted. See `snapshot::assemble` for the order and why it is that one.
    pub channels: Vec<Channel>,
}

impl Space {
    /// The Home entry, holding the rooms that belong to no joined space.
    pub(crate) fn home(channels: Vec<Channel>) -> Self {
        Self {
            id: HOME_ID.to_owned(),
            name: HOME_NAME.to_owned(),
            avatar: None,
            channels,
        }
    }
}

/// One room under a rail entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    /// `None` only when the room is listed by a space and has never been
    /// joined, so nothing local knows what it is called. A joined room always
    /// has a name, because the SDK calculates one when the room does not carry
    /// its own.
    ///
    /// Modelled as an absence rather than as the room ID standing in for a
    /// name, so that the interface cannot show somebody `!AbCdEf...` by
    /// forgetting a check.
    pub name: Option<String>,
    pub kind: ChannelKind,
    pub avatar: Option<String>,
    /// False for a room a space lists that this account has not joined. Those
    /// are shown, and shown as unavailable, because hiding them makes Consort
    /// disagree with every other client about how many channels a space has.
    pub joined: bool,
}

/// Which column a channel belongs in.
///
/// Two variants and no `Unknown`. A room that does not announce itself as a
/// call is a text room, which is what every client already assumes and what
/// the spec implies by having no room type at all for the ordinary case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelKind {
    Text,
    Voice,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> Channel {
        Channel {
            id: "!a:example.org".to_owned(),
            name: Some("general".to_owned()),
            kind: ChannelKind::Text,
            avatar: None,
            joined: true,
        }
    }

    #[test]
    fn home_cannot_collide_with_a_room_id() {
        // Every room ID in Matrix begins with a sigil. If that ever stopped
        // being true, Home would start shadowing somebody's room.
        assert!(!HOME_ID.starts_with('!'));
    }

    #[test]
    fn each_kind_is_tagged_the_way_the_frontend_reads_it() {
        let tag = |kind: ChannelKind| serde_json::to_string(&kind).unwrap();

        assert_eq!(tag(ChannelKind::Text), "\"text\"");
        assert_eq!(tag(ChannelKind::Voice), "\"voice\"");
    }

    #[test]
    fn a_channel_with_no_known_name_sends_null_rather_than_its_room_id() {
        let unknown = Channel {
            name: None,
            joined: false,
            ..channel()
        };

        let json = serde_json::to_value(&unknown).unwrap();

        assert!(json["name"].is_null());
        assert_eq!(json["joined"], false);
    }

    #[test]
    fn the_whole_tree_survives_a_round_trip() {
        let rooms = Rooms {
            spaces: vec![
                Space::home(vec![channel()]),
                Space {
                    id: "!space:example.org".to_owned(),
                    name: "Kahu HQ".to_owned(),
                    avatar: Some("mxc://example.org/abc".to_owned()),
                    channels: vec![Channel {
                        kind: ChannelKind::Voice,
                        ..channel()
                    }],
                },
            ],
        };

        let json = serde_json::to_string(&rooms).unwrap();
        let back: Rooms = serde_json::from_str(&json).unwrap();

        assert_eq!(back, rooms);
    }
}
