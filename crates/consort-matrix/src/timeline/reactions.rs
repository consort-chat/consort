// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Who has reacted to what, and with what.
//!
//! Kept beside [`crate::timeline::history::History`] rather than inside it,
//! because a reaction is not a message and the two lists do not line up. An
//! annotation arrives for a message that may not be loaded, may be loaded
//! later, or may never be; and a message can be replaced by a re-read without
//! anything having happened to what is on it.
//!
//! ## Why the annotations are held individually
//!
//! A count on its own cannot be undone. Taking a reaction back is redacting
//! the `m.reaction` event, and a redaction names only the event it removes: it
//! carries neither the key nor the message it was on. So each annotation is
//! held under its own event ID, and the redaction is a lookup.
//!
//! That is also what answers "have I reacted with this one", which is what
//! decides whether pressing a pill adds or removes.

use std::collections::HashMap;

use crate::timeline::dto::Reaction;

/// One `m.reaction` event, unpacked.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Annotation {
    /// The message it is on.
    target: String,
    /// What was reacted with.
    key: String,
    /// Who reacted.
    sender: String,
}

/// The annotations currently known, for one room.
#[derive(Debug, Default)]
pub struct Reactions {
    /// Every annotation held, by its own event ID, which is what a redaction
    /// names.
    held: HashMap<String, Annotation>,
    /// The annotations on each message, in the order they arrived.
    ///
    /// The order is what stops the pills under a message rearranging
    /// themselves every time somebody adds one. Arrival order is not the
    /// homeserver's opinion about anything, it is simply stable.
    on: HashMap<String, Vec<String>>,
}

impl Reactions {
    /// Nothing known yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Take note of one annotation. Says whether anything changed.
    ///
    /// A repeat of one already held changes nothing, which covers the event
    /// arriving from a sync and again inside a backfill page that overlaps the
    /// live edge. So does a second annotation of the same key by the same
    /// person, which the specification says to ignore and which a client that
    /// counted would draw as two.
    pub fn added(&mut self, event_id: &str, target: &str, key: &str, sender: &str) -> bool {
        if self.held.contains_key(event_id) {
            return false;
        }

        let already = self
            .ids_on(target)
            .any(|held| held.key == key && held.sender == sender);
        if already {
            return false;
        }

        self.held.insert(
            event_id.to_owned(),
            Annotation {
                target: target.to_owned(),
                key: key.to_owned(),
                sender: sender.to_owned(),
            },
        );
        self.on
            .entry(target.to_owned())
            .or_default()
            .push(event_id.to_owned());
        true
    }

    /// Forget the annotation `event_id` was, if it was one.
    ///
    /// Says whether anything changed. A redaction of something that is not an
    /// annotation is the ordinary case rather than an error: a room's
    /// redactions are mostly of messages.
    pub fn redacted(&mut self, event_id: &str) -> bool {
        let Some(gone) = self.held.remove(event_id) else {
            return false;
        };

        if let Some(ids) = self.on.get_mut(&gone.target) {
            ids.retain(|held| held != event_id);
            if ids.is_empty() {
                self.on.remove(&gone.target);
            }
        }
        true
    }

    /// What is on `target`, ready to draw, most recently started last.
    ///
    /// `me` is whoever is signed in, so that a pill can say whether pressing
    /// it would add or take away. `None` for a session with no user ID, which
    /// nothing signed in has.
    pub fn on(&self, target: &str, me: Option<&str>) -> Vec<Reaction> {
        let mut counted: Vec<Reaction> = Vec::new();

        for (event_id, held) in self.ids_and_annotations_on(target) {
            let mine = Some(held.sender.as_str()) == me;
            match counted.iter_mut().find(|had| had.key == held.key) {
                Some(had) => {
                    had.count = had.count.saturating_add(1);
                    if mine {
                        had.mine = Some(event_id.to_owned());
                    }
                }
                None => counted.push(Reaction {
                    key: held.key.clone(),
                    count: 1,
                    mine: mine.then(|| event_id.to_owned()),
                }),
            }
        }

        counted
    }

    fn ids_on(&self, target: &str) -> impl Iterator<Item = &Annotation> {
        self.ids_and_annotations_on(target).map(|(_, held)| held)
    }

    fn ids_and_annotations_on(&self, target: &str) -> impl Iterator<Item = (&str, &Annotation)> {
        self.on
            .get(target)
            .into_iter()
            .flatten()
            .filter_map(|event_id| {
                self.held
                    .get(event_id)
                    .map(|held| (event_id.as_str(), held))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "@ada:example.org";
    const BOB: &str = "@bob:example.org";
    const SAID: &str = "$said";

    fn reacted(reactions: &mut Reactions, id: &str, key: &str, who: &str) -> bool {
        reactions.added(id, SAID, key, who)
    }

    fn keys(reactions: &Reactions, me: Option<&str>) -> Vec<(String, u32, bool)> {
        reactions
            .on(SAID, me)
            .into_iter()
            .map(|one| (one.key, one.count, one.mine.is_some()))
            .collect()
    }

    #[test]
    fn a_message_nobody_reacted_to_carries_nothing() {
        assert!(Reactions::new().on(SAID, Some(ADA)).is_empty());
    }

    #[test]
    fn two_people_on_one_key_are_counted_as_two() {
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);
        reacted(&mut reactions, "$b", "👍", BOB);

        assert_eq!(keys(&reactions, None), [("👍".to_owned(), 2, false)]);
    }

    #[test]
    fn one_person_on_two_keys_is_counted_once_each() {
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);
        reacted(&mut reactions, "$b", "🎉", ADA);

        assert_eq!(
            keys(&reactions, None),
            [("👍".to_owned(), 1, false), ("🎉".to_owned(), 1, false)]
        );
    }

    #[test]
    fn the_readers_own_reaction_carries_the_event_to_redact() {
        // The whole reason `mine` is an event ID rather than a flag: taking a
        // reaction back is redacting that exact event, and this is the only
        // place its ID is known.
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", BOB);
        reacted(&mut reactions, "$mine", "👍", ADA);

        let drawn = reactions.on(SAID, Some(ADA));

        assert_eq!(drawn[0].count, 2);
        assert_eq!(drawn[0].mine.as_deref(), Some("$mine"));
    }

    #[test]
    fn somebody_elses_reaction_is_not_mine_to_take_back() {
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", BOB);

        assert_eq!(reactions.on(SAID, Some(ADA))[0].mine, None);
    }

    #[test]
    fn a_redaction_takes_the_reaction_away() {
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);

        assert!(reactions.redacted("$a"));

        assert!(reactions.on(SAID, Some(ADA)).is_empty());
    }

    #[test]
    fn a_redaction_leaves_the_other_reactions_alone() {
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);
        reacted(&mut reactions, "$b", "👍", BOB);

        reactions.redacted("$a");

        assert_eq!(keys(&reactions, Some(ADA)), [("👍".to_owned(), 1, false)]);
    }

    #[test]
    fn redacting_something_that_was_never_a_reaction_is_not_news() {
        // Most of a room's redactions are of messages, and they arrive in the
        // same batch as everything else.
        let mut reactions = Reactions::new();

        assert!(!reactions.redacted("$a message"));
    }

    #[test]
    fn the_same_event_arriving_twice_is_counted_once() {
        // The ordinary case rather than an edge one: a backfill page overlaps
        // the live edge, so its newest events are ones a sync already brought.
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);

        assert!(!reacted(&mut reactions, "$a", "👍", ADA));

        assert_eq!(keys(&reactions, None), [("👍".to_owned(), 1, false)]);
    }

    #[test]
    fn one_person_reacting_twice_with_one_key_is_counted_once() {
        // Two events, not one, so the deduplication above does not catch it.
        // The specification says to ignore the second; a client that counted
        // it would draw somebody agreeing with themselves.
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "👍", ADA);

        assert!(!reacted(&mut reactions, "$b", "👍", ADA));

        assert_eq!(keys(&reactions, None), [("👍".to_owned(), 1, false)]);
    }

    #[test]
    fn keys_keep_the_order_they_first_arrived_in() {
        // Otherwise the pills under a message rearrange themselves every time
        // somebody adds one, under whoever is reading it.
        let mut reactions = Reactions::new();
        reacted(&mut reactions, "$a", "🎉", ADA);
        reacted(&mut reactions, "$b", "👍", BOB);
        reacted(&mut reactions, "$c", "🎉", BOB);

        assert_eq!(
            keys(&reactions, None),
            [("🎉".to_owned(), 2, false), ("👍".to_owned(), 1, false)]
        );
    }

    #[test]
    fn reactions_on_one_message_say_nothing_about_another() {
        let mut reactions = Reactions::new();
        reactions.added("$a", "$one", "👍", ADA);
        reactions.added("$b", "$two", "🎉", ADA);

        assert_eq!(reactions.on("$one", None).len(), 1);
        assert_eq!(reactions.on("$one", None)[0].key, "👍");
    }
}
