// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The rooms this account has opened, most recently first.
//!
//! A section of the settings file rather than a store of its own. It is the
//! same kind of fact as the per-person volumes beside it: something somebody's
//! use of the application produced, of no interest to anybody else, and worth
//! keeping only because it has to survive being shut down.
//!
//! Whose it is, is kept with it. Signing out and in as somebody else would
//! otherwise leave one account's room IDs in the other's opening screen, where
//! they resolve against nothing and stay in the file for good. An end-to-end
//! encrypted client holding a list of rooms the signed-in account has no part
//! in is not a trade worth making to save a field.

use serde::{Deserialize, Serialize};

/// How many rooms are kept.
///
/// More than the opening screen draws, deliberately. A room that has been left
/// is still in here and resolves against nothing, so remembering exactly as
/// many as are shown would leave the screen a row shorter for every room
/// somebody leaves.
///
/// A cap at all because a list of every room ever opened is a history rather
/// than a recent list, and nothing would ever shorten it.
const REMEMBERED: usize = 12;

/// What an account has opened, most recently first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentRooms {
    /// Whose these are, by Matrix user ID. Absent until something is opened.
    account: Option<String>,
    /// Room IDs, most recently opened first.
    rooms: Vec<String>,
}

impl RecentRooms {
    /// Record that `account` opened `room_id`.
    ///
    /// Opening something already in the list moves it rather than adding it.
    /// A recent list that can hold the same room twice would spend half of
    /// itself on the two rooms somebody is going between.
    pub fn opened(&mut self, account: &str, room_id: &str) {
        if self.account.as_deref() != Some(account) {
            self.account = Some(account.to_owned());
            self.rooms.clear();
        }

        self.rooms.retain(|held| held != room_id);
        self.rooms.insert(0, room_id.to_owned());
        self.rooms.truncate(REMEMBERED);
    }

    /// What `account` has opened, most recently first.
    ///
    /// Empty for anybody else, rather than the list it would otherwise hand
    /// over. The file can only be one account's at a time, and the account it
    /// belongs to is the one that wrote it.
    pub fn of(&self, account: &str) -> &[String] {
        if self.account.as_deref() == Some(account) {
            &self.rooms
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "@ada:example.org";
    const GRACE: &str = "@grace:example.org";

    #[test]
    fn nothing_has_been_opened_to_begin_with() {
        assert!(RecentRooms::default().of(ADA).is_empty());
    }

    #[test]
    fn a_room_that_was_opened_comes_back() {
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!lounge:example.org");

        assert_eq!(recent.of(ADA), ["!lounge:example.org"]);
    }

    #[test]
    fn the_most_recently_opened_room_is_first() {
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!first:example.org");
        recent.opened(ADA, "!second:example.org");

        assert_eq!(
            recent.of(ADA),
            ["!second:example.org", "!first:example.org"]
        );
    }

    #[test]
    fn opening_a_room_again_moves_it_rather_than_listing_it_twice() {
        // Somebody going between two rooms is the ordinary case, and a list
        // that grew an entry per switch would be two rooms six times over.
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!one:example.org");
        recent.opened(ADA, "!two:example.org");
        recent.opened(ADA, "!one:example.org");

        assert_eq!(recent.of(ADA), ["!one:example.org", "!two:example.org"]);
    }

    #[test]
    fn the_list_stops_growing_at_the_cap() {
        let mut recent = RecentRooms::default();

        for n in 0..REMEMBERED + 5 {
            recent.opened(ADA, &format!("!room{n}:example.org"));
        }

        assert_eq!(recent.of(ADA).len(), REMEMBERED);
    }

    #[test]
    fn the_rooms_dropped_at_the_cap_are_the_oldest() {
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!oldest:example.org");
        for n in 0..REMEMBERED {
            recent.opened(ADA, &format!("!room{n}:example.org"));
        }

        assert!(!recent.of(ADA).contains(&"!oldest:example.org".to_owned()));
    }

    #[test]
    fn another_accounts_rooms_are_not_handed_over() {
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!lounge:example.org");

        assert!(recent.of(GRACE).is_empty());
    }

    #[test]
    fn opening_something_as_somebody_else_replaces_the_list() {
        // Not merges it. Two accounts' rooms in one list would be a screen
        // offering rooms the signed-in account cannot open.
        let mut recent = RecentRooms::default();

        recent.opened(ADA, "!hers:example.org");
        recent.opened(GRACE, "!his:example.org");

        assert_eq!(recent.of(GRACE), ["!his:example.org"]);
        assert!(recent.of(ADA).is_empty());
    }
}
