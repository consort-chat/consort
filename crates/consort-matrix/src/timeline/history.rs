// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What is currently loaded for one room, and the rules for adding to it.
//!
//! A sync goes on the end, a backfill on the front, and each batch keeps the
//! order it came in. Deliberately not sorted by `origin_server_ts`: the server
//! already decided the order, and the timestamp is only close to it, because
//! events tie within a millisecond and a federated room carries clocks that
//! agree approximately.
//!
//! An event can arrive from a sync and again from a backfill page overlapping
//! the live edge, so the first occurrence wins and a message never moves.

use std::collections::HashSet;

use crate::timeline::dto::{Message, MessageKind, SystemMessage};

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

    /// Add what a sync just delivered, at the live end, which is also where a
    /// forwards page goes. Reports whether anything was new, so a sync
    /// carrying nothing for this room does not republish the timeline.
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
    /// already turned a backwards pagination round: which direction the
    /// homeserver was asked in is not this type's business.
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

    /// Swap a message already held for a new reading of the same event, which
    /// is what a room key arriving does. In place, so a message drawn as a
    /// wait becomes what it says without the conversation reordering.
    ///
    /// Reports whether anything changed. An unheld event is not an error: a
    /// key can arrive for a room while a different one is open.
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

    /// Empty a message the homeserver has redacted, leaving the mark.
    ///
    /// In place and not removed, because a message that vanished leaves the
    /// reply under it answering nothing. The envelope stays, since a redaction
    /// leaves it alone, and everything the content carried goes.
    ///
    /// `thread` stays, which is the one field where that is a decision: the
    /// replies under a deleted root were not redacted, and the control drawn
    /// from the summary is the only thing that opens them.
    ///
    /// `by` is whoever sent the redaction, which is not always whoever wrote
    /// the message. Reports whether anything changed; a redaction naming
    /// something not loaded is the ordinary case.
    pub fn redacted(&mut self, event_id: &str, by: Option<&str>) -> bool {
        let Some(held) = self.messages.iter_mut().find(|held| held.id == event_id) else {
            return false;
        };
        redact(held, by)
    }

    /// Stop drawing an event, without forgetting that it was seen.
    ///
    /// The other half of [`replace`](Self::replace), for a wait that turns out
    /// to have been a reaction or a thread reply. Not what a redaction does:
    /// that is [`redacted`](Self::redacted), which leaves a mark.
    ///
    /// The ID stays in `seen`, so a backfill carrying the event again does not
    /// draw it a second time.
    pub fn forget(&mut self, event_id: &str) -> bool {
        let before = self.messages.len();
        self.messages.retain(|held| held.id != event_id);
        self.messages.len() != before
    }
}

/// Empty one message in place, leaving the mark.
///
/// Free rather than a method, because the thread panel holds its root as one
/// `Message` beside a `History` of replies and the same redaction marks both.
///
/// Reports whether anything changed. A second redaction of the same event is
/// not news: an overlapping backfill page carries one already swept.
pub fn redact(message: &mut Message, by: Option<&str>) -> bool {
    if message.kind == MessageKind::Deleted {
        return false;
    }

    message.body = String::new();
    message.html = None;
    message.media = None;
    // `thread` is missing from this list on purpose, and the reason is on
    // `History::redacted`. It is the one thing here a redaction does not take.
    message.reply_to = None;
    message.mentions = Vec::new();
    message.edited = false;
    message.deleted_by = by.map(str::to_owned);
    message.kind = MessageKind::Deleted;
    true
}

/// The membership changes loaded for one room.
///
/// [`History`]'s smaller sibling, on the same arrival-order and dedup rules.
/// No [`History::replace`] or [`History::forget`], because a `SystemMessage`
/// is never held as a wait for a key.
#[derive(Debug, Default)]
pub struct SystemHistory {
    messages: Vec<SystemMessage>,
    seen: HashSet<String>,
}

impl SystemHistory {
    /// Nothing loaded yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// What is loaded, oldest first.
    pub fn messages(&self) -> &[SystemMessage] {
        &self.messages
    }

    /// Add what a sync just delivered, at the live end. See
    /// [`History::arrived`].
    pub fn arrived(&mut self, batch: Vec<SystemMessage>) -> bool {
        let fresh: Vec<SystemMessage> = batch
            .into_iter()
            .filter(|message| self.seen.insert(message.id.clone()))
            .collect();
        if fresh.is_empty() {
            return false;
        }

        self.messages.extend(fresh);
        true
    }

    /// Add a page of history, at the old end. See [`History::backfilled`].
    pub fn backfilled(&mut self, batch: Vec<SystemMessage>) -> bool {
        let fresh: Vec<SystemMessage> = batch
            .into_iter()
            .filter(|message| self.seen.insert(message.id.clone()))
            .collect();
        if fresh.is_empty() {
            return false;
        }

        self.messages.splice(..0, fresh);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::dto::ThreadSummary;

    fn said(id: &str, body: &str) -> Message {
        Message {
            id: id.to_owned(),
            sender: "@ada:example.org".to_owned(),
            at: 1_000,
            body: body.to_owned(),
            html: None,
            media: None,
            thread: None,
            reactions: Vec::new(),
            reply_to: None,
            mentions: Vec::new(),
            edited: false,
            deleted_by: None,
            sender_trust: None,
            kind: MessageKind::Text,
            effect: None,
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
    fn a_redaction_empties_a_message_where_it_stands() {
        // In place and not removed. The conversation keeps its shape, and the
        // reply under it is still answering something.
        let mut history = History::new();
        history.arrived(vec![
            said("$1", "one"),
            said("$2", "wrong number"),
            said("$3", "three"),
        ]);

        assert!(history.redacted("$2", Some("@ada:example.org")));

        assert_eq!(bodies(&history), ["one", "", "three"]);
        let gone = &history.messages()[1];
        assert_eq!(gone.kind, MessageKind::Deleted);
        assert_eq!(gone.deleted_by.as_deref(), Some("@ada:example.org"));
        // The envelope stays, because a redaction leaves it alone and the mark
        // is drawn under the name and the time it already had.
        assert_eq!(gone.sender, "@ada:example.org");
        assert_eq!(gone.at, 1_000);
    }

    #[test]
    fn a_redaction_takes_everything_the_content_carried() {
        // Each of these is a way the words could come back. A caption is a
        // body, an attachment is a picture still fetchable by its handle, and
        // a mention is a line in somebody's notifications.
        let mut history = History::new();
        let mut rich = said("$1", "look at this");
        rich.html = Some("<b>look at this</b>".to_owned());
        rich.mentions = vec!["@bob:example.org".to_owned()];
        rich.reply_to = Some("$0".to_owned());
        rich.edited = true;
        history.arrived(vec![rich]);

        assert!(history.redacted("$1", None));

        let gone = &history.messages()[0];
        assert_eq!(gone.body, "");
        assert_eq!(gone.html, None);
        assert_eq!(gone.media, None);
        assert_eq!(gone.mentions, Vec::<String>::new());
        assert_eq!(gone.reply_to, None);
        assert!(!gone.edited);
    }

    #[test]
    fn a_redaction_leaves_the_way_into_the_thread_hanging_from_it() {
        // The one field a redaction does not take. The replies were not
        // redacted, the homeserver still counts them, and this control is the
        // only thing in the room that opens them.
        //
        // Safe to keep because the count survives a reload too: see
        // `facts::deleted`, which reads the same tally off the raw `unsigned`.
        let mut history = History::new();
        let mut root = said("$1", "the question");
        root.thread = Some(ThreadSummary {
            count: 3,
            participated: true,
        });
        history.arrived(vec![root]);

        assert!(history.redacted("$1", None));

        let marked = &history.messages()[0];
        assert_eq!(marked.kind, MessageKind::Deleted);
        assert_eq!(
            marked.thread,
            Some(ThreadSummary {
                count: 3,
                participated: true,
            })
        );
    }

    #[test]
    fn redacting_the_same_event_twice_is_only_news_once() {
        // A backfill page overlapping the live edge carries a redaction that
        // has already been swept, which is the ordinary case rather than an
        // edge one.
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);
        history.redacted("$1", None);

        assert!(!history.redacted("$1", None));
    }

    #[test]
    fn redacting_something_not_loaded_is_not_news() {
        // A redaction names an event anywhere in the room and one window of it
        // is open, so this is ordinary rather than a failure. The message
        // arrives already emptied when somebody scrolls back to it.
        let mut history = History::new();
        history.arrived(vec![said("$1", "one")]);

        assert!(!history.redacted("$2", None));

        assert_eq!(bodies(&history), ["one"]);
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

#[cfg(test)]
mod system_history_tests {
    use super::*;
    use crate::timeline::dto::SystemChange;

    fn joined(id: &str, subject: &str) -> SystemMessage {
        SystemMessage {
            id: id.to_owned(),
            at: 1_000,
            actor: subject.to_owned(),
            change: SystemChange::Joined {
                subject: subject.to_owned(),
            },
        }
    }

    fn subjects(history: &SystemHistory) -> Vec<&str> {
        history
            .messages()
            .iter()
            .map(|message| match &message.change {
                SystemChange::Joined { subject } => subject.as_str(),
                other => panic!("these tests build joins and nothing else: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_fresh_system_history_holds_nothing() {
        assert!(SystemHistory::new().messages().is_empty());
    }

    #[test]
    fn what_arrives_goes_on_the_end() {
        let mut history = SystemHistory::new();

        history.arrived(vec![joined("$1", "ada"), joined("$2", "bragoodle")]);
        history.arrived(vec![joined("$3", "grace")]);

        assert_eq!(subjects(&history), vec!["ada", "bragoodle", "grace"]);
    }

    #[test]
    fn a_page_of_history_goes_on_the_front() {
        let mut history = SystemHistory::new();
        history.arrived(vec![joined("$3", "grace")]);

        history.backfilled(vec![joined("$1", "ada"), joined("$2", "bragoodle")]);

        assert_eq!(subjects(&history), vec!["ada", "bragoodle", "grace"]);
    }

    #[test]
    fn a_membership_change_that_arrives_twice_is_held_once() {
        let mut history = SystemHistory::new();
        history.arrived(vec![joined("$1", "ada")]);

        history.backfilled(vec![joined("$0", "grace"), joined("$1", "ada")]);

        assert_eq!(subjects(&history), vec!["grace", "ada"]);
    }

    #[test]
    fn a_batch_of_nothing_new_is_reported_as_nothing_new() {
        let mut history = SystemHistory::new();
        history.arrived(vec![joined("$1", "ada")]);

        assert!(!history.arrived(vec![joined("$1", "ada")]));
        assert!(!history.arrived(Vec::new()));
        assert!(!history.backfilled(Vec::new()));
    }
}
