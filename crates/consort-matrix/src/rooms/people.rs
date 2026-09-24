// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Who is in one room, asked for when somebody opens the room's details.
//!
//! A command rather than a field on the room list, and the room list's own
//! shape is the reason. The whole tree is re-sent whenever anything in it
//! changes, so a member list per room would multiply a payload that is a few
//! kilobytes today by every room on the account and send all of it again every
//! time somebody picks up a call. This follows [`super::profile`] instead: one
//! room at a time, read when somebody asks, because asking costs something and
//! the roster cannot afford it.
//!
//! ## What one ask costs
//!
//! One `/members` request the first time, and nothing after it.
//! [`matrix_sdk::Room::members`] pulls the member list into the store once per
//! room per session and reads locally from then on. It has to: lazy loading
//! means a sync carries the people who have spoken rather than all of them, so
//! the store on its own would describe a smaller room than the one somebody is
//! standing in.
//!
//! ## Why the list is capped and the count is not
//!
//! [`LISTED`] rows cross the IPC. The count beside the heading is the length
//! of the store's own answer, taken before the cap, so a room of two thousand
//! says two thousand and lists the first hundred of them. A count that agreed
//! with the list would be the bug: the question a heading answers is how many
//! people are here, not how many of them fitted.

use matrix_sdk::Client;
use matrix_sdk::Room;
use matrix_sdk::RoomMemberships;
use matrix_sdk::room::RoomMember;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::events::room::member::MembershipState;
use serde::Serialize;

use super::dto::Participant;
use super::facts;
use crate::error::{Error, Result};

/// How many people one ask lists, per membership.
///
/// A cap rather than paging or lazy loading, and the panel this feeds is why.
/// It is a glance at who is here rather than a directory: there is nothing in
/// it to search with and nowhere to ask for page two, so a cursor protocol
/// would be machinery with no caller. A hundred holds every room a team
/// actually talks in, and it caps the cost that is really per row, which is
/// not the bytes but the avatar each one goes and fetches.
const LISTED: usize = 100;

/// Who is in one room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Members {
    /// The people who have joined.
    pub joined: Roster,
    /// The people who have been invited and have not answered.
    ///
    /// Kept apart from the joined rather than mixed in among them, because the
    /// difference is not decoration. Somebody invited cannot read what is
    /// being said here yet, and that is exactly the fact that matters when the
    /// next thing anybody does is paste something into the room.
    pub invited: Roster,
}

/// One membership's worth of people.
///
/// Room membership, rather than the other thing this application calls a
/// roster. Nobody here is connected to anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Roster {
    /// How many there are, whether or not all of them are in `shown`.
    pub count: u64,
    /// The first [`LISTED`] of them, in the order [`order`] describes.
    pub shown: Vec<Member>,
}

/// One person in a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    /// Who they are, in the shape a person's card takes, so that pressing a
    /// row hands the card what it needs rather than asking for it again.
    pub person: Participant,
    /// Why their name has their user ID in it, when it has.
    ///
    /// Absent, rather than null, for the ordinary person whose display name
    /// stands on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub naming: Option<Naming>,
}

/// Why a name carries a user ID.
///
/// Two different facts that [`facts::name_of_member`] answers with the same
/// shape of string, and they are worth telling apart. One is an absence and
/// the other is a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Naming {
    /// They have set no display name in this room, so their user ID is
    /// standing in for one. Unhelpful, and honest.
    Absent,
    /// Somebody else in this room goes by the same display name, so the user
    /// ID is the only thing telling the two of them apart. Drawn differently
    /// from the case above on purpose: a shared display name is where
    /// impersonation in a Matrix room actually happens.
    Shared,
}

/// Who is in `room_id`, the joined and the invited kept apart.
///
/// The one request in this module, and it is made once per room per session.
/// See the note at the top of the file.
pub async fn members(client: &Client, room_id: &str) -> Result<Members> {
    let room = room_of(client, room_id)?;

    // One read for both groups. Asking twice would be a second store query for
    // an answer the first one is already holding.
    let active = room.members(RoomMemberships::ACTIVE).await?;

    Ok(Members {
        joined: assemble(rows(&active, &MembershipState::Join)),
        invited: assemble(rows(&active, &MembershipState::Invite)),
    })
}

/// The room this account is in, by ID.
fn room_of(client: &Client, room_id: &str) -> Result<Room> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;

    client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })
}

/// The members whose membership is this one, named.
fn rows(members: &[RoomMember], membership: &MembershipState) -> Vec<Member> {
    members
        .iter()
        .filter(|member| member.membership() == membership)
        .map(seat)
        .collect()
}

/// One member, named the way every other name in Consort is.
fn seat(member: &RoomMember) -> Member {
    Member {
        person: Participant::named(member.user_id().as_str(), facts::name_of_member(member)),
        naming: facts::naming_of(member),
    }
}

/// Count, order and cap one membership's worth of people.
///
/// Pure, and split from the read above for the reason [`super::snapshot`] is
/// split from [`super::facts`]: a `RoomMember` cannot be built by hand, so a
/// function taking one is a function only a homeserver can reach, and the
/// counting and the ordering are the part worth testing.
fn assemble(mut people: Vec<Member>) -> Roster {
    // Taken before the cap, because it answers a question about the room
    // rather than about the list.
    let count = people.len() as u64;

    // Cached rather than computed per comparison: the key allocates, and a
    // room of two thousand would otherwise lowercase every name some twenty
    // times over.
    people.sort_by_cached_key(order);
    people.truncate(LISTED);

    Roster {
        count,
        shown: people,
    }
}

/// What a member list is sorted by.
///
/// The name on the row, lowercased so that `ada` and `Ada` land together, with
/// the user ID breaking the tie. The tie-break is what makes this a total
/// order rather than very nearly one: a user ID is unique within a room, so
/// nothing is left for a second sort to decide differently and one set of
/// people cannot come back in two different orders.
///
/// The drawn name rather than the display name underneath it, so that the
/// order is the order somebody reads. Somebody who has set no name is drawn as
/// their user ID and sorts under `@` with everybody else in that position.
fn order(member: &Member) -> (String, String) {
    (member.person.name.to_lowercase(), member.person.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str, name: &str) -> Member {
        Member {
            person: Participant::named(id, name),
            naming: None,
        }
    }

    fn names(roster: &Roster) -> Vec<&str> {
        roster
            .shown
            .iter()
            .map(|member| member.person.name.as_str())
            .collect()
    }

    fn crowd(size: usize) -> Vec<Member> {
        (0..size)
            .map(|n| person(&format!("@p{n:04}:example.org"), &format!("Person {n:04}")))
            .collect()
    }

    #[test]
    fn people_are_listed_by_the_name_on_their_row() {
        let roster = assemble(vec![
            person("@zoe:example.org", "Zoe"),
            person("@ada:example.org", "Ada"),
            person("@mel:example.org", "Mel"),
        ]);

        assert_eq!(names(&roster), vec!["Ada", "Mel", "Zoe"]);
    }

    #[test]
    fn a_capital_letter_does_not_move_somebody_to_the_end() {
        // Sorting on the bytes would put every lowercase name after every
        // uppercase one, which reads as two lists rather than one.
        let roster = assemble(vec![
            person("@bob:example.org", "bob"),
            person("@ada:example.org", "Ada"),
            person("@cal:example.org", "Cal"),
        ]);

        assert_eq!(names(&roster), vec!["Ada", "bob", "Cal"]);
    }

    #[test]
    fn two_people_with_one_name_are_separated_by_their_user_id() {
        // The tie-break, and the whole reason the order is total. Without it
        // the sort has a choice to make and nothing says it makes the same one
        // twice, so the rows would be free to swap between one ask and the
        // next.
        let roster = assemble(vec![
            person("@ada2:example.org", "Ada"),
            person("@ada1:example.org", "Ada"),
        ]);

        let ids: Vec<_> = roster
            .shown
            .iter()
            .map(|member| member.person.id.as_str())
            .collect();
        assert_eq!(ids, vec!["@ada1:example.org", "@ada2:example.org"]);
    }

    #[test]
    fn the_same_people_in_any_order_come_back_in_one_order() {
        // What "stable" has to mean for a list asked for afresh every time the
        // panel opens. The store hands its rows back in whatever order suits
        // it, and two asks that disagreed would reshuffle the panel.
        let forwards = assemble(vec![
            person("@ada:example.org", "Ada"),
            person("@mel:example.org", "Mel"),
            person("@zoe:example.org", "Zoe"),
        ]);
        let backwards = assemble(vec![
            person("@zoe:example.org", "Zoe"),
            person("@mel:example.org", "Mel"),
            person("@ada:example.org", "Ada"),
        ]);

        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_big_room_lists_a_hundred_of_its_people() {
        let roster = assemble(crowd(2_000));

        assert_eq!(roster.shown.len(), LISTED);
    }

    #[test]
    fn a_big_room_still_says_how_many_people_are_in_it() {
        // The bug this test exists to stop. A count that agrees with the list
        // rather than with the room: two thousand beside a hundred rows is
        // fine, a hundred beside a hundred rows when there are two thousand is
        // a lie.
        let roster = assemble(crowd(2_000));

        assert_eq!(roster.count, 2_000);
    }

    #[test]
    fn the_hundred_a_big_room_lists_are_the_first_hundred_in_order() {
        // The cap is applied after the sort rather than before it. Capping
        // first would list whichever hundred the store happened to hand over,
        // which is a different hundred every time.
        let mut backwards = crowd(2_000);
        backwards.reverse();

        let roster = assemble(backwards);

        assert_eq!(roster.shown[0].person.name, "Person 0000");
        assert_eq!(roster.shown[LISTED - 1].person.name, "Person 0099");
    }

    #[test]
    fn a_room_small_enough_to_list_lists_all_of_it() {
        let roster = assemble(crowd(7));

        assert_eq!(roster.count, 7);
        assert_eq!(roster.shown.len(), 7);
    }

    #[test]
    fn a_room_with_nobody_in_it_counts_nobody() {
        let roster = assemble(Vec::new());

        assert_eq!(roster.count, 0);
        assert!(roster.shown.is_empty());
    }

    #[test]
    fn a_member_carries_the_fields_the_interface_reads() {
        // The wire format is the contract with `app/src/lib/api.ts`.
        let json = serde_json::to_value(Member {
            person: Participant::named("@ada:example.org", "Ada"),
            naming: Some(Naming::Shared),
        })
        .unwrap();

        assert_eq!(json["person"]["id"], "@ada:example.org");
        assert_eq!(json["person"]["name"], "Ada");
        assert_eq!(json["naming"], "shared");
    }

    #[test]
    fn an_ordinary_name_says_nothing_about_itself() {
        // Left out rather than null, so the interface has one way of meaning
        // nothing rather than two.
        let json = serde_json::to_value(Member {
            person: Participant::named("@ada:example.org", "Ada"),
            naming: None,
        })
        .unwrap();

        assert!(json.get("naming").is_none(), "{json}");
    }

    #[test]
    fn a_missing_name_and_a_shared_one_are_different_words_on_the_wire() {
        // The interface draws them differently, so the two have to arrive
        // differently.
        assert_eq!(
            serde_json::to_string(&Naming::Absent).unwrap(),
            "\"absent\""
        );
        assert_eq!(
            serde_json::to_string(&Naming::Shared).unwrap(),
            "\"shared\""
        );
    }
}
