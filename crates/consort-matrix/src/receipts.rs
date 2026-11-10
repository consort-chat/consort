// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Saying what has been read, and counting what has not.
//!
//! Two halves of one subject. A receipt is what this account tells the
//! homeserver it has seen; an unread count is what the homeserver and the SDK
//! make of that afterwards. Neither works without the other: without a receipt
//! every room is unread forever, and without a count a receipt is a courtesy to
//! other people's clients and nothing this one draws.
//!
//! ## Two receipts and a marker
//!
//! [`mark_read`] sends up to three things in one request, and they are not the
//! same thing said three ways.
//!
//! `m.read` is public. Everybody in the room can see it, which is how a client
//! draws the little faces beside a message, and there is no way to unsend one.
//! `m.read.private` is the same claim made to the homeserver alone: it resets
//! the unread counts and tells nobody. Which of the two goes out is a setting,
//! because it is a decision about being watched rather than a technical one,
//! and `app/src-tauri/src/settings.rs` is where the answer is kept.
//!
//! `m.fully_read` is neither. It is room account data rather than a receipt,
//! it is always private, and it is the one this build draws: it is what the
//! line across a conversation saying "you stopped here" is read from. It goes
//! out on every call whichever receipt accompanies it.
//!
//! ## Where the counts come from
//!
//! [`count_unread`] subscribes matrix-sdk's event cache, and until it is
//! called `Room::num_unread_messages` and `Room::num_unread_mentions` answer
//! zero for every room on the account, forever. They are not read off the
//! homeserver's own tally: the server cannot see inside an encrypted message,
//! so it can neither tell a message from a reaction nor find a mention, and
//! every room Consort cares about is encrypted. The SDK counts client-side
//! instead, from the events it has cached and the account's push rules, and
//! the event cache is the thing that holds those events.
//!
//! What that costs is one more consumer of the sync updates this client
//! already publishes, and the event store the sqlite configuration already
//! opens. What it does not cost is a second sync: the cache reads
//! `subscribe_to_all_room_updates`, which is the same broadcast
//! [`crate::rooms::watch`] and [`crate::timeline::watch`] are already on.
//!
//! Since matrix-sdk 0.19 the client builds and drives the cache itself, so
//! there is nothing here to start. See [`count_unread`] for what is left.

use matrix_sdk::room::Receipts;
use matrix_sdk::ruma::{EventId, OwnedEventId};
use matrix_sdk::{Client, Room};

use crate::error::{Error, Result};

/// Check that unread messages and mentions are being counted.
///
/// Called once per signed-in client, before the sync loop starts. It used to
/// subscribe the event cache; matrix-sdk 0.19 creates and drives the cache in
/// the client instead, and turns it off through `ClientBuilder::with_event_cache`
/// rather than by not subscribing. So what is left is the assertion, which was
/// always the point: this is here so that a session which starts without counts
/// is not silently one that will never have any.
///
/// The cache is on by default and nothing in this crate turns it off, so the
/// failure is unreachable today. Kept because it is one `with_event_cache(false)`
/// away from being reachable, and the symptom would otherwise be an unread mark
/// that never appears for anybody.
pub fn count_unread(client: &Client) -> Result<()> {
    if client.event_cache().is_none() {
        return Err(Error::NoEventCache);
    }
    Ok(())
}

/// Say that everything up to and including `event_id` has been read.
///
/// `public` decides which receipt rides along with the marker. False sends
/// `m.read.private`, which resets this account's counts and tells nobody; true
/// sends `m.read`, which every other person in the room can see and which
/// cannot be taken back once sent.
///
/// One request either way, because `send_multiple_receipts` sets the marker and
/// the receipt together. Sending them separately would be two round trips and a
/// window in which the two disagree.
///
/// Nothing here throttles or deduplicates. Both belong to the caller, which
/// knows what it last said and when: see [`crate::timeline::Watch::mark_read`],
/// which is the only caller and which already holds the room open.
pub(crate) async fn mark_read(room: &Room, event_id: &str, public: bool) -> Result<()> {
    let event_id = EventId::parse(event_id).map_err(|_| Error::NoSuchEvent {
        event_id: event_id.to_owned(),
    })?;

    room.send_multiple_receipts(receipts(event_id, public))
        .await?;
    Ok(())
}

/// What one "I have read this" is on the wire.
///
/// Pure, and split out for that reason: which of the two receipts a setting
/// selects is the whole of what this module decides, and it is worth being able
/// to check both answers without a homeserver.
fn receipts(event_id: OwnedEventId, public: bool) -> Receipts {
    let receipts = Receipts::new().fully_read_marker(event_id.clone());
    if public {
        receipts.public_read_receipt(event_id)
    } else {
        receipts.private_read_receipt(event_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> OwnedEventId {
        EventId::parse("$said:example.org").expect("the fixture is a valid event ID")
    }

    #[test]
    fn the_marker_goes_out_whichever_receipt_does() {
        // The marker is what the line across the conversation is drawn from,
        // and it is private in both directions. Somebody who turned public
        // receipts off has not asked to stop being told where they left off.
        for public in [true, false] {
            assert_eq!(receipts(event(), public).fully_read, Some(event()));
        }
    }

    #[test]
    fn a_private_read_tells_the_room_nothing() {
        let sent = receipts(event(), false);

        assert_eq!(sent.private_read_receipt, Some(event()));
        assert_eq!(sent.public_read_receipt, None);
    }

    #[test]
    fn a_public_read_is_the_one_everybody_can_see() {
        // And only that one. Sending both would publish the thing the private
        // receipt exists to avoid publishing, which is the whole setting.
        let sent = receipts(event(), true);

        assert_eq!(sent.public_read_receipt, Some(event()));
        assert_eq!(sent.private_read_receipt, None);
    }
}
