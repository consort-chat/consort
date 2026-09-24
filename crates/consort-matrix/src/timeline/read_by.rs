// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Where each person in a room has read up to.
//!
//! Kept beside [`crate::timeline::history::History`] on the same terms as
//! [`crate::timeline::reactions::Reactions`]: a receipt arrives for a message
//! that may not be loaded, may be loaded later by a page, or may never be.
//!
//! ## Keyed by the person, which is the whole of it
//!
//! A receipt names the newest event somebody has read, and everything before
//! it is implied. So the question this answers is "where is each person", not
//! "who has read this message", and the state is one entry per person.
//!
//! Holding it the other way round is the bug the shape exists to prevent. A
//! map of message to readers, added to as receipts arrive, never takes the old
//! entry away: somebody who reads on down a room accumulates an avatar against
//! every message they have passed, which is both wrong and unbounded. Keyed by
//! the person there is nowhere for the old answer to survive, because writing
//! the new one is what removes it.
//!
//! ## Two conversations, not one
//!
//! A thread has receipts of its own, and a receipt in one says nothing about
//! the other. `m.read` with no `thread_id` and `m.read` with `main` are both
//! about the room's own timeline, and every other value is about the thread it
//! names. Clients differ over which of the first two they send, so a room that
//! reads only one of them draws an empty answer for half the people in it.

use std::collections::HashMap;

use matrix_sdk::ruma::events::receipt::ReceiptThread;

use crate::timeline::dto::ReadOn;

/// How many faces are drawn against one message before the rest become a
/// count.
///
/// Five. This is a row of pictures beside a line of text rather than a list
/// somebody scrolls, so the cap that suits it is the number that fits without
/// pushing the conversation around, not the hundred a member list can afford.
/// Beyond it the row says how many more, which is the same bargain a long
/// member list makes and the same one Element makes here.
pub const SHOWN: usize = 5;

/// Which conversation a receipt is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum About {
    /// The room's own timeline.
    ///
    /// Both `ReceiptThread::Unthreaded` and `ReceiptThread::Main` land here.
    /// The distinction is about what the sending client knew, not about what
    /// was read: a client from before threads existed sends the first and a
    /// current one sends the second, and both mean the room.
    Room,
    /// One thread, named by the message it hangs from.
    Thread(String),
}

impl From<&ReceiptThread> for About {
    /// Which conversation a receipt off the wire is about.
    ///
    /// The one place the three wire values become the two that matter. See
    /// [`About::Room`] for why two of them collapse into one.
    fn from(thread: &ReceiptThread) -> Self {
        match thread {
            ReceiptThread::Unthreaded | ReceiptThread::Main => Self::Room,
            ReceiptThread::Thread(root) => Self::Thread(root.to_string()),
            // `ReceiptThread` is non-exhaustive: it is a wire enum and the
            // specification can add to it. Anything this build has not heard
            // of is not the room, because the room is the one value that is
            // already spelled two ways and adding a third to it would draw
            // somebody as having read a conversation they have not.
            unknown => Self::Thread(unknown.as_str().unwrap_or_default().to_owned()),
        }
    }
}

/// Where one person has read up to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Upto {
    event_id: String,
    about: About,
}

/// Where everybody has read up to, for one room.
#[derive(Debug)]
pub struct ReadBy {
    /// Keyed by the person. See the module docs: this is the collapse.
    ///
    /// One entry per person per conversation, because somebody reading in a
    /// thread has not moved where they are in the room.
    upto: HashMap<(String, About), Upto>,
    /// Whoever is signed in, whose own receipts are never drawn.
    ///
    /// Held rather than passed to the reader, so that the two places a receipt
    /// arrives from cannot disagree about it.
    me: Option<String>,
}

impl ReadBy {
    /// Nothing known yet.
    pub fn new(me: Option<String>) -> Self {
        Self {
            upto: HashMap::new(),
            me,
        }
    }

    /// Take note that `user` has read up to `event_id`. Says whether anything
    /// changed.
    ///
    /// The latest statement wins. A receipt is the sender's own claim about
    /// where they are, and there is no older one worth keeping beside it.
    ///
    /// This account's own receipts are dropped here rather than filtered on
    /// the way out. Nobody needs telling that they have read their own room,
    /// and every client that has ever shown you your own face against your own
    /// message looked broken.
    pub fn noted(&mut self, user: &str, event_id: &str, about: About) -> bool {
        if Some(user) == self.me.as_deref() {
            return false;
        }

        let seen = Upto {
            event_id: event_id.to_owned(),
            about: about.clone(),
        };
        let key = (user.to_owned(), about);
        if self.upto.get(&key) == Some(&seen) {
            return false;
        }
        self.upto.insert(key, seen);
        true
    }

    /// Who has read what, ready to draw, for one conversation.
    ///
    /// Grouped by message, because that is what it is drawn against, and by
    /// user ID within a message so that a row of faces does not rearrange
    /// itself every time somebody else reads. Arrival order would be the
    /// livelier answer and is not worth motion beside a message somebody is
    /// reading.
    ///
    /// Messages are in no particular order. The interface asks for one at a
    /// time by ID.
    pub fn on(&self, about: &About) -> Vec<ReadOn> {
        let mut by_event: HashMap<&str, Vec<&str>> = HashMap::new();
        for ((user, _), seen) in self.upto.iter().filter(|((_, at), _)| at == about) {
            by_event
                .entry(seen.event_id.as_str())
                .or_default()
                .push(user.as_str());
        }

        let mut drawn: Vec<ReadOn> = by_event
            .into_iter()
            .map(|(event_id, mut readers)| {
                readers.sort_unstable();
                let more = readers.len().saturating_sub(SHOWN);
                readers.truncate(SHOWN);
                ReadOn {
                    event_id: event_id.to_owned(),
                    readers: readers.into_iter().map(ToOwned::to_owned).collect(),
                    more: more as u32,
                }
            })
            .collect();
        drawn.sort_unstable_by(|one, two| one.event_id.cmp(&two.event_id));
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "@ada:example.org";
    const BOB: &str = "@bob:example.org";
    const ME: &str = "@me:example.org";
    const OLDER: &str = "$older:example.org";
    const NEWER: &str = "$newer:example.org";

    fn empty() -> ReadBy {
        ReadBy::new(Some(ME.to_owned()))
    }

    /// Who is drawn against one message, in the order the row draws them.
    fn against<'a>(drawn: &'a [ReadOn], event_id: &str) -> Vec<&'a str> {
        drawn
            .iter()
            .find(|one| one.event_id == event_id)
            .map(|one| one.readers.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    #[test]
    fn reading_further_down_moves_the_face_rather_than_adding_one() {
        // The whole of the collapse, and the thing a map of message to readers
        // gets wrong while still looking right on a small fixture: the new
        // avatar appears either way, and only the old one staying behind says
        // the state is keyed by the wrong thing.
        let mut read = empty();
        read.noted(ADA, OLDER, About::Room);

        read.noted(ADA, NEWER, About::Room);

        let drawn = read.on(&About::Room);
        assert_eq!(against(&drawn, NEWER), [ADA]);
        assert_eq!(against(&drawn, OLDER), [] as [&str; 0]);
    }

    #[test]
    fn a_receipt_said_twice_changes_nothing() {
        // What a backfill page overlapping the live edge delivers, and what a
        // homeserver repeating an unchanged ephemeral event does.
        let mut read = empty();
        assert!(read.noted(ADA, NEWER, About::Room));

        assert!(!read.noted(ADA, NEWER, About::Room));
    }

    #[test]
    fn everybody_who_has_read_a_message_is_drawn_against_it() {
        let mut read = empty();
        read.noted(BOB, NEWER, About::Room);
        read.noted(ADA, NEWER, About::Room);

        assert_eq!(against(&read.on(&About::Room), NEWER), [ADA, BOB]);
    }

    #[test]
    fn this_account_is_never_drawn_reading_its_own_room() {
        let mut read = empty();

        assert!(!read.noted(ME, NEWER, About::Room));

        assert!(read.on(&About::Room).is_empty());
    }

    #[test]
    fn a_thread_does_not_answer_for_the_room() {
        // Trap two. The panel and the room are asking different questions, and
        // a build that ignored the thread would show the room's readers inside
        // every thread hanging off it.
        let mut read = empty();
        read.noted(ADA, NEWER, About::Thread("$root:example.org".to_owned()));

        assert!(read.on(&About::Room).is_empty());
    }

    #[test]
    fn the_room_does_not_answer_for_a_thread() {
        let mut read = empty();
        read.noted(ADA, NEWER, About::Room);

        assert!(
            read.on(&About::Thread("$root:example.org".to_owned()))
                .is_empty()
        );
    }

    #[test]
    fn reading_in_a_thread_leaves_where_somebody_is_in_the_room_alone() {
        // The two are not one position. Somebody who answers a thread has not
        // caught up with the conversation it hangs off.
        let mut read = empty();
        read.noted(ADA, OLDER, About::Room);

        read.noted(ADA, NEWER, About::Thread("$root:example.org".to_owned()));

        assert_eq!(against(&read.on(&About::Room), OLDER), [ADA]);
    }

    #[test]
    fn a_crowd_is_capped_and_says_how_many_more() {
        // A room with fifty people in it has fifty receipts on the newest
        // message, and fifty faces beside one line is not a row.
        let mut read = empty();
        for who in 0..50 {
            read.noted(&format!("@person{who:02}:example.org"), NEWER, About::Room);
        }

        let drawn = read.on(&About::Room);
        let one = drawn.iter().find(|one| one.event_id == NEWER).unwrap();
        assert_eq!(one.readers.len(), SHOWN);
        assert_eq!(one.more, 50 - SHOWN as u32);
    }

    #[test]
    fn a_row_that_fits_says_nobody_is_missing() {
        let mut read = empty();
        read.noted(ADA, NEWER, About::Room);

        let drawn = read.on(&About::Room);
        assert_eq!(
            drawn.iter().find(|one| one.event_id == NEWER).unwrap().more,
            0
        );
    }
}
