// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning a pile of rooms into the shape the shell draws.
//!
//! All of it is pure, and deliberately so. Every branch here is reachable from
//! a hand-built [`RoomFacts`], which makes this the one part of the room list
//! that can be got right before the app is ever launched. It is also the part
//! that is easiest to get wrong: the ordering rule has three fallbacks, and on
//! a real account every single channel goes through the last two of them.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::dto::{Channel, ChannelKind, Rooms, Space};
use super::facts::{ChildFacts, RoomFacts, RoomKind};

/// Build the whole tree from every joined room.
///
/// Takes the lot rather than a diff. Recomputing twenty-six rooms is cheaper
/// than the bookkeeping needed to avoid it, and a function that always sees
/// everything cannot drift out of step with the account the way a patched
/// copy can.
pub(crate) fn assemble(facts: Vec<RoomFacts>) -> Rooms {
    let (spaces, rooms): (Vec<RoomFacts>, Vec<RoomFacts>) = facts
        .into_iter()
        .partition(|facts| facts.kind == RoomKind::Space);

    // Which rooms some joined space says are its own. This is the whole of the
    // orphan detection, and it deliberately asks the spaces rather than asking
    // each room who its parents are. A room whose only parent is a space this
    // account has not joined has no rail icon to sit under, so it belongs in
    // Home no matter what it claims, and this way round says that for free.
    let claimed: HashSet<&str> = spaces
        .iter()
        .flat_map(|space| space.children.iter().map(|child| child.id.as_str()))
        .collect();

    let space_ids: HashSet<&str> = spaces.iter().map(|space| space.id.as_str()).collect();
    let by_id: HashMap<&str, &RoomFacts> =
        rooms.iter().map(|room| (room.id.as_str(), room)).collect();

    let mut listed: Vec<Space> = spaces
        .iter()
        .map(|space| Space {
            id: space.id.clone(),
            name: space.name.clone(),
            avatar: space.avatar.clone(),
            channels: channels_of(space, &space_ids, &by_id),
        })
        .collect();
    listed.sort_by(|a, b| by_name(&a.name, &a.id, &b.name, &b.id));

    let mut rail = Vec::with_capacity(listed.len() + 1);
    rail.push(Space::home(home_channels(&rooms, &claimed)));
    rail.append(&mut listed);

    Rooms { spaces: rail }
}

/// The rooms belonging to no joined space.
///
/// Ordered by name, because there is nothing else to order them by. A space
/// arranges its children with `m.space.child`; nothing arranges these, and the
/// obvious alternative, most recent activity, needs the read receipts that
/// this milestone does not have. Name ordering is at least the same every
/// launch, which is what stops the list moving under the pointer.
fn home_channels(rooms: &[RoomFacts], claimed: &HashSet<&str>) -> Vec<Channel> {
    let mut orphans: Vec<&RoomFacts> = rooms
        .iter()
        .filter(|room| !claimed.contains(room.id.as_str()))
        .collect();

    orphans.sort_by(|a, b| by_name(&a.name, &a.id, &b.name, &b.id));
    orphans.into_iter().map(joined_channel).collect()
}

/// The channels of one space, in the order the spec asks for.
///
/// Subspaces are left out. They get a rail entry of their own, and rendering
/// one as a channel would give it two places to be clicked, one of which does
/// nothing useful.
fn channels_of(
    space: &RoomFacts,
    space_ids: &HashSet<&str>,
    by_id: &HashMap<&str, &RoomFacts>,
) -> Vec<Channel> {
    let mut children: Vec<&ChildFacts> = space
        .children
        .iter()
        .filter(|child| !space_ids.contains(child.id.as_str()))
        .collect();

    children.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

    children
        .into_iter()
        .map(|child| match by_id.get(child.id.as_str()) {
            Some(room) => joined_channel(room),
            // Listed by the space and never joined. Nothing local knows what
            // it is called, and saying so is the honest answer until the
            // hierarchy request fills it in.
            None => Channel {
                id: child.id.clone(),
                name: None,
                kind: ChannelKind::Text,
                avatar: None,
                joined: false,
                // Nothing to put here. A room this account is not in is a room
                // whose state it cannot read, so an empty list is not a guess,
                // it is the only honest answer.
                participants: Vec::new(),
            },
        })
        .collect()
}

/// Where one child sorts, following [MSC1772].
///
/// Three fallbacks, and on a real account almost nothing reaches the first
/// one: `order` is optional and hardly anybody sets it, so the timestamp and
/// the room ID do the actual work. That is the reason this is a named
/// function with tests rather than a closure.
///
/// Comparing `order` bytewise is comparing it by Unicode codepoint, which is
/// what the spec asks for, because ruma has already rejected any `order` that
/// is not printable ASCII.
///
/// A child with no `order` sorts after every child that has one, which is
/// where the leading boolean comes from: `false` orders before `true`.
///
/// [MSC1772]: https://spec.matrix.org/latest/client-server-api/#mspacechild
fn sort_key(child: &ChildFacts) -> (bool, &str, u64, &str) {
    (
        child.order.is_none(),
        child.order.as_deref().unwrap_or(""),
        // No timestamp means a stripped event, which a joined space does not
        // produce. Sorted last rather than first so that if it ever does
        // happen it does not push a real channel out of place.
        child.timestamp.unwrap_or(u64::MAX),
        child.id.as_str(),
    )
}

/// The order two named things are shown in.
///
/// Case-insensitive, because a list where `Zebra` sorts before `apple` looks
/// broken to everybody who is not thinking about ASCII. The ID breaks ties, so
/// two rooms with the same name still have a fixed order rather than whichever
/// one the store happened to return first.
fn by_name(a_name: &str, a_id: &str, b_name: &str, b_id: &str) -> Ordering {
    a_name
        .to_lowercase()
        .cmp(&b_name.to_lowercase())
        .then_with(|| a_id.cmp(b_id))
}

/// A room this account is in, as a channel.
fn joined_channel(room: &RoomFacts) -> Channel {
    Channel {
        id: room.id.clone(),
        name: Some(room.name.clone()),
        kind: match room.kind {
            RoomKind::Voice => ChannelKind::Voice,
            // A space never reaches here: `channels_of` filters subspaces out
            // and `home_channels` only ever sees non-spaces. Written as an
            // ordinary arm rather than a panic because an unreachable panic in
            // a room list is a crash waiting for an account nobody tested.
            RoomKind::Space | RoomKind::Text => ChannelKind::Text,
        },
        avatar: room.avatar.clone(),
        joined: true,
        participants: room.participants.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::dto::Participant;

    fn room(id: &str, name: &str) -> RoomFacts {
        RoomFacts {
            id: id.to_owned(),
            name: name.to_owned(),
            avatar: None,
            kind: RoomKind::Text,
            children: Vec::new(),
            participants: Vec::new(),
        }
    }

    fn person(id: &str, name: &str) -> Participant {
        Participant::named(id, name)
    }

    fn voice(id: &str, name: &str) -> RoomFacts {
        RoomFacts {
            kind: RoomKind::Voice,
            ..room(id, name)
        }
    }

    fn space(id: &str, name: &str, children: Vec<ChildFacts>) -> RoomFacts {
        RoomFacts {
            kind: RoomKind::Space,
            children,
            ..room(id, name)
        }
    }

    /// A child with neither an order nor a meaningful timestamp: the shape
    /// every child on the account being developed against actually has.
    fn child(id: &str) -> ChildFacts {
        ChildFacts {
            id: id.to_owned(),
            order: None,
            timestamp: Some(0),
        }
    }

    fn ordered(id: &str, order: &str) -> ChildFacts {
        ChildFacts {
            order: Some(order.to_owned()),
            ..child(id)
        }
    }

    fn sent_at(id: &str, timestamp: u64) -> ChildFacts {
        ChildFacts {
            timestamp: Some(timestamp),
            ..child(id)
        }
    }

    fn home(rooms: &Rooms) -> &Space {
        &rooms.spaces[0]
    }

    fn ids(channels: &[Channel]) -> Vec<&str> {
        channels.iter().map(|channel| channel.id.as_str()).collect()
    }

    fn names(channels: &[Channel]) -> Vec<Option<&str>> {
        channels
            .iter()
            .map(|channel| channel.name.as_deref())
            .collect()
    }

    mod the_rail {
        use super::*;

        #[test]
        fn home_is_first_even_when_the_account_has_nothing_in_it() {
            let rooms = assemble(Vec::new());

            assert_eq!(rooms.spaces.len(), 1);
            assert_eq!(home(&rooms).id, "home");
            assert!(home(&rooms).channels.is_empty());
        }

        #[test]
        fn home_stays_first_when_a_space_would_sort_before_it() {
            // Home is furniture, not a list entry. If it sorted with the
            // others it would move whenever somebody joined a space whose
            // name began with an earlier letter.
            let rooms = assemble(vec![space("!a:example.org", "aardvark", Vec::new())]);

            assert_eq!(home(&rooms).id, "home");
            assert_eq!(rooms.spaces[1].id, "!a:example.org");
        }

        #[test]
        fn spaces_are_ordered_by_name_regardless_of_case() {
            let rooms = assemble(vec![
                space("!z:example.org", "apple", Vec::new()),
                space("!a:example.org", "Banana", Vec::new()),
                space("!m:example.org", "Cherry", Vec::new()),
            ]);

            let rail: Vec<&str> = rooms.spaces[1..]
                .iter()
                .map(|space| space.name.as_str())
                .collect();

            assert_eq!(rail, ["apple", "Banana", "Cherry"]);
        }

        #[test]
        fn a_space_carries_its_own_name_and_avatar() {
            let rooms = assemble(vec![RoomFacts {
                avatar: Some("mxc://example.org/abc".to_owned()),
                ..space("!s:example.org", "Kahu HQ", Vec::new())
            }]);

            assert_eq!(rooms.spaces[1].name, "Kahu HQ");
            assert_eq!(
                rooms.spaces[1].avatar.as_deref(),
                Some("mxc://example.org/abc")
            );
        }
    }

    mod home_holds_the_orphans {
        use super::*;

        #[test]
        fn a_room_no_space_claims_lands_in_home() {
            let rooms = assemble(vec![room("!dm:example.org", "aayejayy")]);

            assert_eq!(ids(&home(&rooms).channels), ["!dm:example.org"]);
        }

        #[test]
        fn a_room_a_joined_space_claims_does_not() {
            let rooms = assemble(vec![
                space(
                    "!s:example.org",
                    "Kahu HQ",
                    vec![child("!general:example.org")],
                ),
                room("!general:example.org", "general"),
            ]);

            assert!(home(&rooms).channels.is_empty());
            assert_eq!(ids(&rooms.spaces[1].channels), ["!general:example.org"]);
        }

        #[test]
        fn a_room_whose_only_space_is_one_we_have_not_joined_lands_in_home() {
            // The room may well carry an `m.space.parent` naming a space this
            // account has never seen. Asking the room would put it under a
            // rail icon that does not exist; asking the joined spaces puts it
            // where it can actually be clicked.
            let rooms = assemble(vec![
                space(
                    "!ours:example.org",
                    "Ours",
                    vec![child("!theirs:example.org")],
                ),
                room("!orphan:example.org", "somewhere else"),
                room("!theirs:example.org", "claimed"),
            ]);

            assert_eq!(ids(&home(&rooms).channels), ["!orphan:example.org"]);
        }

        #[test]
        fn home_is_ordered_by_name_and_two_rooms_sharing_one_stay_two_rooms() {
            // Two rooms called "Private Room" is not a hypothetical: the
            // account this was built against has exactly that, and one of them
            // is a call room. A list that deduplicates by name loses one.
            let rooms = assemble(vec![
                room("!b:example.org", "Private Room"),
                room("!a:example.org", "Private Room"),
                room("!c:example.org", "aardvark"),
            ]);

            assert_eq!(
                ids(&home(&rooms).channels),
                ["!c:example.org", "!a:example.org", "!b:example.org"]
            );
        }

        #[test]
        fn a_space_is_never_a_channel_of_home() {
            let rooms = assemble(vec![space("!s:example.org", "Kahu HQ", Vec::new())]);

            assert!(home(&rooms).channels.is_empty());
        }
    }

    mod ordering {
        use super::*;

        fn channels_of_the_space(children: Vec<ChildFacts>, rooms: Vec<RoomFacts>) -> Vec<String> {
            let mut facts = vec![space("!s:example.org", "Kahu HQ", children)];
            facts.extend(rooms);

            ids(&assemble(facts).spaces[1].channels)
                .into_iter()
                .map(str::to_owned)
                .collect()
        }

        #[test]
        fn an_order_beats_a_timestamp() {
            let order = channels_of_the_space(
                vec![
                    ChildFacts {
                        timestamp: Some(1),
                        ..ordered("!late:example.org", "a")
                    },
                    sent_at("!early:example.org", 0),
                ],
                Vec::new(),
            );

            assert_eq!(order, ["!late:example.org", "!early:example.org"]);
        }

        #[test]
        fn orders_are_compared_as_text_not_as_numbers() {
            let order = channels_of_the_space(
                vec![
                    ordered("!ten:example.org", "10"),
                    ordered("!two:example.org", "2"),
                ],
                Vec::new(),
            );

            assert_eq!(order, ["!ten:example.org", "!two:example.org"]);
        }

        #[test]
        fn a_timestamp_breaks_a_tie_between_children_with_no_order() {
            // The fallback that carries the whole account: not one child on it
            // sets an order, so this is the comparison every channel is placed
            // by.
            let order = channels_of_the_space(
                vec![
                    sent_at("!second:example.org", 2_000),
                    sent_at("!first:example.org", 1_000),
                ],
                Vec::new(),
            );

            assert_eq!(order, ["!first:example.org", "!second:example.org"]);
        }

        #[test]
        fn a_room_id_breaks_a_tie_between_equal_timestamps() {
            let order = channels_of_the_space(
                vec![
                    sent_at("!b:example.org", 1_000),
                    sent_at("!a:example.org", 1_000),
                ],
                Vec::new(),
            );

            assert_eq!(order, ["!a:example.org", "!b:example.org"]);
        }

        #[test]
        fn the_order_does_not_depend_on_the_order_the_store_returned() {
            // The state store makes no promise about the order it hands state
            // events back in, so the same account must not draw itself
            // differently on two launches.
            let forwards = channels_of_the_space(
                vec![sent_at("!a:example.org", 1), sent_at("!b:example.org", 2)],
                Vec::new(),
            );
            let backwards = channels_of_the_space(
                vec![sent_at("!b:example.org", 2), sent_at("!a:example.org", 1)],
                Vec::new(),
            );

            assert_eq!(forwards, backwards);
        }

        #[test]
        fn voice_and_text_share_one_ordering_rather_than_two() {
            // The two columns are a rendering decision. Sorting once and
            // splitting afterwards is what keeps a channel in the same place
            // relative to its neighbours when it changes type.
            let order = channels_of_the_space(
                vec![
                    sent_at("!voice:example.org", 1),
                    sent_at("!text:example.org", 2),
                ],
                vec![
                    voice("!voice:example.org", "Lounge"),
                    room("!text:example.org", "general"),
                ],
            );

            assert_eq!(order, ["!voice:example.org", "!text:example.org"]);
        }
    }

    mod channels {
        use super::*;

        #[test]
        fn a_call_room_is_a_voice_channel_and_everything_else_is_not() {
            let rooms = assemble(vec![
                space(
                    "!s:example.org",
                    "Kahu HQ",
                    vec![sent_at("!v:example.org", 1), sent_at("!t:example.org", 2)],
                ),
                voice("!v:example.org", "Lounge"),
                room("!t:example.org", "general"),
            ]);

            let kinds: Vec<ChannelKind> = rooms.spaces[1]
                .channels
                .iter()
                .map(|channel| channel.kind)
                .collect();

            assert_eq!(kinds, [ChannelKind::Voice, ChannelKind::Text]);
        }

        #[test]
        fn a_child_that_was_never_joined_has_no_name_and_says_so() {
            let rooms = assemble(vec![space(
                "!s:example.org",
                "Kahu HQ",
                vec![child("!unknown:example.org")],
            )]);

            let channels = &rooms.spaces[1].channels;

            assert_eq!(names(channels), [None]);
            assert!(!channels[0].joined);
            assert_eq!(channels[0].id, "!unknown:example.org");
        }

        #[test]
        fn a_joined_channel_always_has_a_name() {
            let rooms = assemble(vec![
                space("!s:example.org", "Kahu HQ", vec![child("!g:example.org")]),
                room("!g:example.org", "general"),
            ]);

            let channels = &rooms.spaces[1].channels;

            assert_eq!(names(channels), [Some("general")]);
            assert!(channels[0].joined);
        }

        #[test]
        fn a_channel_carries_its_avatar_uri_and_not_its_bytes() {
            let rooms = assemble(vec![
                space("!s:example.org", "Kahu HQ", vec![child("!g:example.org")]),
                RoomFacts {
                    avatar: Some("mxc://example.org/abc".to_owned()),
                    ..room("!g:example.org", "general")
                },
            ]);

            assert_eq!(
                rooms.spaces[1].channels[0].avatar.as_deref(),
                Some("mxc://example.org/abc")
            );
        }

        #[test]
        fn a_voice_channel_carries_the_people_in_it() {
            let rooms = assemble(vec![
                space("!s:example.org", "Kahu HQ", vec![child("!v:example.org")]),
                RoomFacts {
                    participants: vec![person("@a:example.org", "Ada")],
                    ..voice("!v:example.org", "Lounge")
                },
            ]);

            assert_eq!(
                rooms.spaces[1].channels[0].participants,
                [person("@a:example.org", "Ada")]
            );
        }

        #[test]
        fn a_channel_nobody_joined_has_nobody_in_it() {
            // Not a guess that it is empty. We are not in the room, so its
            // call state is not something this account can see at all.
            let rooms = assemble(vec![space(
                "!s:example.org",
                "Kahu HQ",
                vec![child("!never:example.org")],
            )]);

            assert!(rooms.spaces[1].channels[0].participants.is_empty());
        }

        #[test]
        fn one_room_in_two_spaces_appears_under_both() {
            let rooms = assemble(vec![
                space("!a:example.org", "A", vec![child("!shared:example.org")]),
                space("!b:example.org", "B", vec![child("!shared:example.org")]),
                room("!shared:example.org", "shared"),
            ]);

            assert_eq!(ids(&rooms.spaces[1].channels), ["!shared:example.org"]);
            assert_eq!(ids(&rooms.spaces[2].channels), ["!shared:example.org"]);
            assert!(home(&rooms).channels.is_empty());
        }
    }

    mod subspaces {
        use super::*;

        #[test]
        fn a_subspace_gets_its_own_rail_entry_and_is_not_a_channel() {
            let rooms = assemble(vec![
                space(
                    "!parent:example.org",
                    "Parent",
                    vec![child("!child:example.org")],
                ),
                space("!child:example.org", "Child", Vec::new()),
            ]);

            let parent = rooms
                .spaces
                .iter()
                .find(|space| space.id == "!parent:example.org")
                .unwrap();

            assert!(parent.channels.is_empty());
            assert!(
                rooms
                    .spaces
                    .iter()
                    .any(|space| space.id == "!child:example.org")
            );
        }

        #[test]
        fn a_space_that_lists_itself_does_not_become_its_own_channel() {
            let rooms = assemble(vec![space(
                "!s:example.org",
                "Kahu HQ",
                vec![child("!s:example.org")],
            )]);

            assert!(rooms.spaces[1].channels.is_empty());
        }
    }
}
