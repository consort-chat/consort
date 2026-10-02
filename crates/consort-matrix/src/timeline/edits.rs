// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Which correction a message is currently showing.
//!
//! Kept beside [`crate::timeline::history::History`] rather than inside it,
//! for the reason [`crate::timeline::reactions::Reactions`] is: an edit is not
//! a message and the two lists do not line up. An `m.replace` arrives for a
//! message that may not be loaded, may be loaded later by a page, or may never
//! be, and a message can be replaced by a re-read without anything having
//! happened to the corrections on it.
//!
//! That last one is why folding here rather than writing the new text into the
//! history. `History::replace` swaps one reading of an event for another
//! reading of the same event, which is what a room key arriving does. An edit
//! is a different event, it arrives and re-arrives, and a later re-read of the
//! original would silently undo it.
//!
//! ## Why the edits are held individually
//!
//! The same argument the annotations make, twice over. A redaction names only
//! the event it removes, so undoing an edit needs that edit under its own ID;
//! and which edit wins changes when a later one arrives, so collapsing on
//! arrival would throw away the one that a redaction falls back to.

use std::collections::HashMap;

use crate::timeline::dto::SenderTrust;
use crate::timeline::facts::Replacement;

/// One `m.replace` event, unpacked and held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// The message it replaces.
    target: String,
    /// Who sent it.
    sender: String,
    /// Its own `origin_server_ts`, in milliseconds. Which edit wins.
    at: u64,
    /// What the message now says, with no formatting.
    pub body: String,
    /// What it now says as HTML, or `None` for an edit that carried none.
    pub html: Option<String>,
    /// Why the device that sent the edit could not be vouched for.
    pub sender_trust: Option<SenderTrust>,
}

/// The corrections currently known, for one room.
#[derive(Debug, Default)]
pub struct Edits {
    /// Every edit held, by its own event ID, which is what a redaction names.
    held: HashMap<String, Edit>,
    /// The edits on each message, in the order they arrived.
    on: HashMap<String, Vec<String>>,
}

impl Edits {
    /// Nothing known yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Take note of one edit. Says whether anything changed.
    ///
    /// Taken unconditionally, including from somebody who did not write the
    /// message being replaced. Whether they are allowed to have sent it is a
    /// comparison against the original's sender, and the original is regularly
    /// not loaded when its edit arrives; [`Self::latest_on`] is where the
    /// comparison can actually be made. An edit that never matches is dead
    /// weight in a map, which is the same dead weight a reaction on an
    /// unloaded message already is.
    pub fn added(&mut self, one: Replacement) -> bool {
        let Replacement {
            event_id,
            target,
            sender,
            at,
            body,
            html,
            sender_trust,
        } = one;
        if self.held.contains_key(&event_id) {
            return false;
        }

        self.held.insert(
            event_id.clone(),
            Edit {
                target: target.clone(),
                sender,
                at,
                body,
                html,
                sender_trust,
            },
        );
        self.on.entry(target).or_default().push(event_id);
        true
    }

    /// Forget the edit `event_id` was, if it was one.
    ///
    /// Says whether anything changed. What the message falls back to is the
    /// newest edit still held, or the original when there are none, and that
    /// falls out of [`Self::latest_on`] rather than needing a branch here.
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

    /// The edit that wins on `target`, if `author` is allowed to have made it.
    ///
    /// `author` is who sent the message being replaced. An `m.replace` from
    /// anybody else is ignored, which is the whole of what stops one person in
    /// a room rewriting another person's words in Consort.
    pub fn latest_on(&self, target: &str, author: &str) -> Option<&Edit> {
        self.on
            .get(target)
            .into_iter()
            .flatten()
            .filter_map(|event_id| Some((event_id.as_str(), self.held.get(event_id)?)))
            .filter(|(_, edit)| edit.sender == author)
            // The event ID breaks a tie, and it is the stability rather than
            // the answer that matters: two edits stamped the same millisecond
            // is a homeserver under load, which of them wins cannot be known,
            // and an unstable comparison would have the room draw one and
            // redraw the other on the next publish for no reason.
            .max_by(|(one, left), (other, right)| {
                left.at.cmp(&right.at).then_with(|| one.cmp(other))
            })
            .map(|(_, edit)| edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADA: &str = "@ada:example.org";
    const BOB: &str = "@bob:example.org";
    const SAID: &str = "$said";

    /// One edit on `SAID` from `ADA`, which is the ordinary case.
    fn edited(edits: &mut Edits, id: &str, at: u64, body: &str) -> bool {
        edits.added(replacing(id, SAID, ADA, at, body))
    }

    /// One `m.replace` as `facts::replacement` would have unpacked it.
    fn replacing(id: &str, target: &str, sender: &str, at: u64, body: &str) -> Replacement {
        Replacement {
            event_id: id.to_owned(),
            target: target.to_owned(),
            sender: sender.to_owned(),
            at,
            body: body.to_owned(),
            html: None,
            sender_trust: None,
        }
    }

    fn showing(edits: &Edits) -> Option<&str> {
        edits.latest_on(SAID, ADA).map(|edit| edit.body.as_str())
    }

    #[test]
    fn a_message_nobody_edited_shows_no_correction() {
        assert_eq!(Edits::new().latest_on(SAID, ADA), None);
    }

    #[test]
    fn a_message_shows_the_edit_that_was_made_to_it() {
        let mut edits = Edits::new();

        assert!(edited(&mut edits, "$a", 2_000, "corrected"));

        assert_eq!(showing(&edits), Some("corrected"));
    }

    #[test]
    fn an_edit_is_held_for_a_message_that_is_not_loaded() {
        // The ordinary case rather than an edge one: an edit arrives in a sync
        // for a message a hundred lines above the window, and the message is
        // loaded later by a page or never. Nothing here knows which, so
        // everything is held and the fold asks.
        let mut edits = Edits::new();

        assert!(edits.added(replacing(
            "$a",
            "$nothing has heard of this",
            ADA,
            2_000,
            "corrected",
        )));

        assert_eq!(
            edits
                .latest_on("$nothing has heard of this", ADA)
                .map(|edit| edit.body.as_str()),
            Some("corrected")
        );
    }

    #[test]
    fn the_newest_of_two_edits_is_the_one_shown() {
        let mut edits = Edits::new();
        edited(&mut edits, "$a", 2_000, "first try");
        edited(&mut edits, "$b", 3_000, "second try");

        assert_eq!(showing(&edits), Some("second try"));
    }

    #[test]
    fn an_older_edit_arriving_afterwards_does_not_win() {
        // What paging backwards does. The live edge brought the newest edit
        // already, and the page behind it brings the one it replaced.
        let mut edits = Edits::new();
        edited(&mut edits, "$b", 3_000, "second try");
        edited(&mut edits, "$a", 2_000, "first try");

        assert_eq!(showing(&edits), Some("second try"));
    }

    #[test]
    fn an_edit_by_somebody_who_did_not_write_the_message_is_ignored() {
        // Without this, anybody in a room can rewrite anybody else's words in
        // Consort and every other client in the room shows the original.
        let mut edits = Edits::new();
        edits.added(replacing("$a", SAID, BOB, 2_000, "not what was said"));

        assert_eq!(edits.latest_on(SAID, ADA), None);
    }

    #[test]
    fn a_forgery_does_not_win_over_a_real_edit_either() {
        // The dangerous shape: it is newer, so a check made anywhere but here
        // would have let it take the fold.
        let mut edits = Edits::new();
        edited(&mut edits, "$real", 2_000, "corrected");
        edits.added(replacing("$forged", SAID, BOB, 9_000, "rewritten"));

        assert_eq!(showing(&edits), Some("corrected"));
    }

    #[test]
    fn a_redacted_edit_falls_back_to_the_one_before_it() {
        let mut edits = Edits::new();
        edited(&mut edits, "$a", 2_000, "first try");
        edited(&mut edits, "$b", 3_000, "second try");

        assert!(edits.redacted("$b"));

        assert_eq!(showing(&edits), Some("first try"));
    }

    #[test]
    fn redacting_the_only_edit_puts_the_original_back() {
        // Somebody corrected a message and then took the correction back. What
        // they meant is the sentence they originally sent, which is what the
        // history is still holding.
        let mut edits = Edits::new();
        edited(&mut edits, "$a", 2_000, "corrected");

        edits.redacted("$a");

        assert_eq!(showing(&edits), None);
    }

    #[test]
    fn redacting_something_that_was_never_an_edit_is_not_news() {
        // Most of a room's redactions are of messages, and they arrive in the
        // same batch as everything else.
        assert!(!Edits::new().redacted("$a message"));
    }

    #[test]
    fn the_same_edit_arriving_twice_changes_nothing() {
        // A backfill page overlaps the live edge, so its newest events are
        // ones a sync already brought.
        let mut edits = Edits::new();
        edited(&mut edits, "$a", 2_000, "corrected");

        assert!(!edited(&mut edits, "$a", 2_000, "corrected"));

        assert_eq!(showing(&edits), Some("corrected"));
    }

    #[test]
    fn two_edits_stamped_the_same_millisecond_settle_the_same_way_twice() {
        // A homeserver under load can stamp two events identically. Which of
        // them wins does not matter and cannot be known; that the answer does
        // not change between one publish and the next does, or the room
        // redraws itself for no reason.
        let mut one = Edits::new();
        edited(&mut one, "$a", 2_000, "one way");
        edited(&mut one, "$b", 2_000, "the other");

        let mut other = Edits::new();
        edited(&mut other, "$b", 2_000, "the other");
        edited(&mut other, "$a", 2_000, "one way");

        assert_eq!(showing(&one), showing(&other));
    }

    #[test]
    fn an_edit_of_one_message_says_nothing_about_another() {
        let mut edits = Edits::new();
        edits.added(replacing("$a", "$one", ADA, 2_000, "corrected"));

        assert_eq!(edits.latest_on("$two", ADA), None);
    }

    #[test]
    fn an_edit_that_cleared_the_formatting_says_so() {
        // Not absent-and-therefore-unchanged. A message edited down to plain
        // text has to lose its HTML, or the fold draws the sentence that was
        // corrected.
        let mut edits = Edits::new();
        edits.added(Replacement {
            html: Some("<em>formatted</em>".to_owned()),
            ..replacing("$a", SAID, ADA, 2_000, "plain now")
        });
        edits.added(replacing("$b", SAID, ADA, 3_000, "plain now"));

        assert_eq!(edits.latest_on(SAID, ADA).expect("an edit").html, None);
    }
}
