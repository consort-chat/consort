// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The message a reply is answering, when it is not one of the loaded ones:
//! one event, by ID, read as a message.
//!
//! Not the whole window, because drawing the row and going to the message are
//! different asks. [`around`](super::around) is the second one and moves the
//! reader; this answers who wrote it and what it said, and for a message the
//! SDK has already stored it costs no request at all.

use matrix_sdk::Room;

use crate::timeline::dto::Message;
use crate::timeline::facts;

/// Read one event as a message, or `None` when there is nothing to draw.
///
/// `None` covers a redaction, a message with no key, and a homeserver that
/// will not hand the event over, which is what an account that was not in the
/// room at the time gets. All three are the same answer to the row.
pub async fn answered(room: &Room, event_id: &str) -> Option<Message> {
    let parsed = super::event_id_of(event_id).ok()?;

    room.load_or_fetch_event(&parsed, None)
        .await
        .ok()
        .as_ref()
        // Drawn on its own rather than as part of the room, so a reply naming
        // a message that lives in a thread still says who wrote it and what it
        // said. The row is beside the reply either way.
        .and_then(facts::alone)
}
