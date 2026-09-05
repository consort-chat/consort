// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The message a reply is answering, when it is not one of the loaded ones.
//!
//! A room draws a window of history and a reply can name anything older than
//! it, so the row above a reply is regularly pointing at something that is not
//! on screen and cannot be looked up in what is. This is the lookup for that
//! case: one event, by ID, read as a message.
//!
//! ## Why not the whole window
//!
//! Because drawing the row and going to the message are different asks.
//! [`around`](super::around) is the second one and costs a `/context` request
//! and a timeline the reader did not ask to be moved to. This one answers who
//! wrote it and what it said, which is all a reply row draws, and for a
//! message the SDK has already stored it costs no request at all.

use matrix_sdk::Room;

use crate::timeline::dto::Message;
use crate::timeline::facts;

/// Read one event as a message, or `None` when there is nothing to draw.
///
/// `None` covers a redaction, a message this session has no key for, and a
/// homeserver that will not hand the event over, which is what an account that
/// was not in the room at the time gets. All three are the same answer to the
/// only question being asked, and the row says so rather than guessing.
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
