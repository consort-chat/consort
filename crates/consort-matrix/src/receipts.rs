// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Saying what has been read, and counting what has not.
//!
//! [`mark_read`] sends two receipts and a marker in one request, and which
//! receipt goes out is a setting. The counts are the SDK's own client-side
//! tally, because the homeserver cannot see inside an encrypted message: see
//! [`count_unread`].

use matrix_sdk::room::Receipts;
use matrix_sdk::ruma::{EventId, OwnedEventId};
use matrix_sdk::{Client, Room};

use crate::error::{Error, Result};

/// Check that unread messages and mentions are being counted.
///
/// Called once per signed-in client, before the sync loop. This is an
/// assertion: a session that starts without counts must not silently be one
/// that never has any. The cache is on by default, so the failure is one
/// `with_event_cache(false)` away from reachable, and the symptom would be an
/// unread mark that never appears for anybody.
pub fn count_unread(client: &Client) -> Result<()> {
    if client.event_cache().is_none() {
        return Err(Error::NoEventCache);
    }
    Ok(())
}

/// Say that everything up to and including `event_id` has been read.
///
/// `public` decides which receipt rides with the marker, and `m.read` cannot be
/// taken back once sent. One request either way, because
/// `send_multiple_receipts` sets both together. Throttling and deduplication
/// belong to the caller, [`crate::timeline::Watch::mark_read`].
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
