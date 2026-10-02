// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Where each person in a room has read up to.
//!
//! Keyed by the person, which is the whole of it: a receipt names the newest
//! event somebody has read, so a map of message to readers never takes the old
//! entry away and accumulates a face against every message they have passed.
//!
//! A thread has receipts of its own, so a receipt in one says nothing about
//! the other: see [`About`].

use std::collections::HashMap;

use matrix_sdk::ruma::events::receipt::ReceiptThread;

use crate::timeline::dto::ReadOn;

/// How many faces are drawn against one message before the rest become a
/// count. A row of pictures beside a line of text rather than a list somebody
/// scrolls, so the cap is what fits without pushing the conversation around.
pub const SHOWN: usize = 5;

/// Which conversation a receipt is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum About {
    /// The room's own timeline.
    ///
    /// Both `ReceiptThread::Unthreaded` and `ReceiptThread::Main` land here:
    /// a client from before threads sends the first and a current one the
    /// second, and both mean the room.
    Room,
    /// One thread, named by the message it hangs from.
    Thread(String),
}

impl From<&ReceiptThread> for About {
    /// Which conversation a receipt off the wire is about: the one place the
    /// three wire values become the two that matter.
    fn from(thread: &ReceiptThread) -> Self {
        match thread {
            ReceiptThread::Unthreaded | ReceiptThread::Main => Self::Room,
            ReceiptThread::Thread(root) => Self::Thread(root.to_string()),
            // `ReceiptThread` is non-exhaustive, so anything this build has
            // not heard of is not the room: adding a third spelling would draw
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
    /// Keyed by the person, which is the collapse the module header describes.
    /// One entry per person per conversation, because somebody reading in a
    /// thread has not moved where they are in the room.
    upto: HashMap<(String, About), Upto>,
    /// Whoever is signed in, whose own receipts are never drawn. Held rather
    /// than passed in, so the two places a receipt arrives from agree.
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
    /// The latest statement wins, and this account's own receipts are dropped
    /// here rather than filtered on the way out.
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
    /// Grouped by message and sorted by user ID within one, so a row of faces
    /// does not rearrange itself every time somebody else reads. The messages
    /// themselves are in no particular order.
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
        // The whole of the collapse. A map of message to readers looks right
        // on a small fixture: the new avatar appears either way, and only the
        // old one staying behind says the state is keyed wrongly.
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
