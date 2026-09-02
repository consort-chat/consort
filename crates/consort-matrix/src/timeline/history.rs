// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What is currently loaded for one room, and the rules for adding to it.
//!
//! Two sources feed one list and they arrive at opposite ends. A sync delivers
//! what was just said, which goes on the end; a backfill delivers a page of
//! what was said before, which goes on the front. Neither is sorted here.
//!
//! ## Why not sorted by timestamp
//!
//! Because the server already decided the order, and its decision is the one
//! every other client draws. `origin_server_ts` is close to that order and is
//! not it: two events written in the same millisecond tie, a homeserver under
//! load can stamp them out of order, and a federated room carries timestamps
//! from several machines that agree with each other only approximately. Sorting
//! by it would reorder a conversation that arrived correct.
//!
//! So each batch keeps the order it came in, and the only decision here is
//! which end it goes on.
//!
//! ## Deduplication
//!
//! An event can arrive twice: once from a sync and again inside a backfill
//! page that overlaps the live edge, which is the ordinary case rather than an
//! edge one. First occurrence wins, so a message never moves once it has been
//! drawn.

use std::collections::HashSet;

use crate::timeline::dto::Message;

/// The messages loaded for one room.
#[derive(Debug, Default)]
pub struct History {
    messages: Vec<Message>,
    /// Event IDs already held, so a duplicate is a hash lookup rather than a
    /// scan of everything loaded.
    seen: HashSet<String>,
}

impl History {
    /// Nothing loaded yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What is loaded, oldest first.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Add what a sync just delivered, at the live end.
    ///
    /// Reports whether anything was new, so a sync carrying nothing for this
    /// room does not republish a timeline nobody's copy differs from.
    pub fn arrived(&mut self, batch: Vec<Message>) -> bool {
        let fresh: Vec<Message> = batch
            .into_iter()
            .filter(|message| self.seen.insert(message.id.clone()))
            .collect();
        if fresh.is_empty() {
            return false;
        }

        self.messages.extend(fresh);
        true
    }

    /// Add a page of history, at the old end.
    ///
    /// `batch` is oldest first, like everything else here, so the caller has
    /// already put a backwards pagination the right way round. Doing it here
    /// instead would mean this type had an opinion about which direction a
    /// homeserver was asked in, which is not its business.
    pub fn backfilled(&mut self, batch: Vec<Message>) -> bool {
        let fresh: Vec<Message> = batch
            .into_iter()
            .filter(|message| self.seen.insert(message.id.clone()))
            .collect();
        if fresh.is_empty() {
            return false;
        }

        // Spliced rather than pushed and re-sorted. The page belongs before
        // everything loaded, in the order the server gave it, and this is the
        // only arrangement that says so.
        self.messages.splice(..0, fresh);
        true
    }

    /// Swap a message already held for a new reading of the same event.
    ///
    /// What a room key arriving does to the messages this session could not
    /// read when they came in. In place, so a message that was drawn as a wait
    /// becomes what it says without moving: the conversation around it is
    /// already on somebody's screen, and reordering under them would be worse
    /// than the wait.
    ///
    /// Reports whether anything changed, so a key for a session none of these
    /// messages used does not republish a timeline nobody's copy differs from.
    /// An unheld event is not an error: a key can arrive for a room while a
    /// different one is open.
    pub fn replace(&mut self, message: Message) -> bool {
        let Some(held) = self.messages.iter_mut().find(|held| held.id == message.id) else {
            return false;
        };
        if *held == message {
            return false;
        }

        *held = message;
        true
    }

    /// Stop drawing an event, without forgetting that it was seen.
    ///
    /// The other half of [`replace`](Self::replace). An event this session
    /// could not read is drawn as a wait, and when the key arrives some of
    /// them turn out to be reactions or thread replies, which are not drawn at
    /// all. Leaving the wait there would keep a placeholder for something that
    /// was never a message.
    ///
    /// The ID stays in `seen`, so a backfill that carries the event again does
    /// not draw it a second time.
    pub fn forget(&mut self, event_id: &str) -> bool {
        let before = self.messages.len();
        self.messages.retain(|held| held.id != event_id);
        self.messages.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::dto::MessageKind;

    fn said(id: &str, body: &str) -> Message {
        Message {
            id: id.to_owned(),
            sender: "@ada:example.org".to_owned(),
            at: 1_000,
            body: body.to_owned(),
            html: None,
            media: None,
            kind: MessageKind::Text,
        }
    }

    fn bodies(history: &History) -> Vec<&str> {
        history
            .messages()
            .iter()
            .map(|message| message.body.as_str())
            .collect()
    }

    #[test]
    fn a_message_re_read_takes_the_place_of_the_old_one() {
        // What a room key arriving does. The wait becomes the sentence,
        // without the conversation around it moving.
        let mut history = History::new();
        history.arrived(vec![
            said("$1", "one"),
            said("$2", "waiting"),
            said("$3", "three"),
        ]);

        assert!(history.replace(said("$2", "the actual message")));

        assert_eq!(bodies(&history), ["one", "the actual message", "three"]);
    }

    #[test]
    fn re_reading_an_event_nobody_holds_changes_nothing() {
        // Ordinary rather than an error: a key can arrive for a room while a
        // different one is open.
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);

        assert!(!history.replace(said("$2", "two")));

        assert_eq!(bodies(&history), ["one"]);
    }

    #[test]
    fn re_reading_an_event_to_the_same_answer_is_not_news() {
        // A key that opens nothing new is the ordinary case for every key
        // after the first, and republishing on each one would redraw the room
        // for nothing.
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);

        assert!(!history.replace(said("$1", "one")));
    }

    #[test]
    fn an_event_that_turns_out_not_to_be_a_message_stops_being_drawn() {
        let mut history = History::new();
        history.arrived(vec![said("$1", "one"), said("$2", "waiting")]);

        assert!(history.forget("$2"));

        assert_eq!(bodies(&history), ["one"]);
    }

    #[test]
    fn forgetting_an_event_twice_is_only_news_once() {
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);
        history.forget("$1");

        assert!(!history.forget("$1"));
    }

    #[test]
    fn a_forgotten_event_is_not_drawn_again_by_a_backfill() {
        // It was dropped because it is not a message, and a page of history
        // that carries it again has not changed that.
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);
        history.forget("$1");

        history.backfilled(vec![said("$1", "one")]);

        assert!(history.messages().is_empty());
    }

    #[test]
    fn a_fresh_history_holds_nothing() {
        assert!(History::new().messages().is_empty());
    }

    #[test]
    fn what_arrives_goes_on_the_end() {
        let mut history = History::new();

        history.arrived(vec![said("$1", "first"), said("$2", "second")]);
        history.arrived(vec![said("$3", "third")]);

        assert_eq!(bodies(&history), vec!["first", "second", "third"]);
    }

    #[test]
    fn a_page_of_history_goes_on_the_front() {
        let mut history = History::new();
        history.arrived(vec![said("$3", "third")]);

        history.backfilled(vec![said("$1", "first"), said("$2", "second")]);

        assert_eq!(bodies(&history), vec!["first", "second", "third"]);
    }

    #[test]
    fn two_pages_of_history_stack_in_the_right_order() {
        // Each page is older than the one before it, so the second goes in
        // front of the first rather than behind it.
        let mut history = History::new();
        history.arrived(vec![said("$5", "fifth")]);

        history.backfilled(vec![said("$3", "third"), said("$4", "fourth")]);
        history.backfilled(vec![said("$1", "first"), said("$2", "second")]);

        assert_eq!(
            bodies(&history),
            vec!["first", "second", "third", "fourth", "fifth"]
        );
    }

    #[test]
    fn the_server_s_order_is_kept_even_when_the_clocks_disagree() {
        // A federated room carries timestamps from several machines and they
        // agree only approximately. Sorting by them would reorder a
        // conversation that arrived correct.
        let mut history = History::new();
        let mut later = said("$1", "first");
        later.at = 9_000;
        let mut earlier = said("$2", "second");
        earlier.at = 1_000;

        history.arrived(vec![later, earlier]);

        assert_eq!(bodies(&history), vec!["first", "second"]);
    }

    #[test]
    fn a_message_that_arrives_twice_is_held_once() {
        // The ordinary case rather than an edge one: a backfill page overlaps
        // the live edge, so its newest events are ones a sync already
        // delivered.
        let mut history = History::new();
        history.arrived(vec![said("$1", "first")]);

        history.backfilled(vec![said("$0", "zeroth"), said("$1", "first")]);

        assert_eq!(bodies(&history), vec!["zeroth", "first"]);
    }

    #[test]
    fn a_duplicate_does_not_move_a_message_that_is_already_drawn() {
        // First occurrence wins. Letting the second one win would move a
        // message somebody is reading to a different place in the list.
        let mut history = History::new();
        history.arrived(vec![said("$1", "first"), said("$2", "second")]);

        history.arrived(vec![said("$1", "first")]);

        assert_eq!(bodies(&history), vec!["first", "second"]);
    }

    #[test]
    fn a_batch_of_nothing_new_is_reported_as_nothing_new() {
        // The sync loop delivers an update per sync whether or not this room
        // was in it. Republishing the timeline for every one of them would
        // wake the webview twice a minute to hand it what it has.
        let mut history = History::new();
        history.arrived(vec![said("$1", "first")]);

        assert!(!history.arrived(vec![said("$1", "first")]));
        assert!(!history.arrived(Vec::new()));
        assert!(!history.backfilled(Vec::new()));
    }

    #[test]
    fn a_batch_with_anything_new_in_it_is_reported_as_new() {
        let mut history = History::new();
        history.arrived(vec![said("$1", "first")]);

        assert!(history.arrived(vec![said("$1", "first"), said("$2", "second")]));
        assert_eq!(bodies(&history), vec!["first", "second"]);
    }
}
