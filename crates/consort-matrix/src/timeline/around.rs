// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The history either side of one message.
//!
//! What a room's own watcher cannot do. It loads a page at a time backwards
//! from the live end, so a message from last year is reachable only by asking
//! for every page between here and there: an unbounded number of round trips
//! and a room that scrolls itself for a minute. `/context` asks the question
//! directly, and answers with the message, a few either side of it, and a
//! token for each direction.
//!
//! ## Why this is not spliced into what is loaded
//!
//! Because the two pieces are not adjacent. Dropping a window from last year
//! in front of yesterday's messages would draw them as one conversation, with
//! nothing saying there is a year between them, and the reader has no way to
//! tell that from a quiet afternoon. So the window replaces what is loaded
//! rather than joining it, and going back to the present is its own ask: see
//! [`Watch::go_to`](super::Watch::go_to).

use matrix_sdk::Room;
use matrix_sdk::ruma::UInt;

use crate::error::{Error, Result};
use crate::timeline::dto::Message;
use crate::timeline::facts;

/// How many messages to ask for around the one being gone to.
///
/// The homeserver splits this either side, so it is half a screen in each
/// direction. Enough that the message lands in a conversation rather than
/// alone at the top of an empty room, and few enough that a jump is one small
/// request.
const CONTEXT: u32 = 24;

/// A window of history, and where it can be grown from.
pub struct Around {
    /// Oldest first, like everything else here.
    pub messages: Vec<Message>,
    /// Where a page older than this window starts, or `None` at the beginning
    /// of the room.
    pub back: Option<String>,
    /// Where a page newer than this window starts, or `None` when the window
    /// already reaches the live end.
    pub forward: Option<String>,
}

/// Read the history around `event_id`.
///
/// The event itself is not required to be drawable. A reply can name a
/// redacted message or one this session has no key for, and the window either
/// side of it is still where somebody asked to be taken; refusing the jump
/// would answer a press with nothing.
///
/// Fails only when the homeserver will not answer at all, which for a message
/// somebody was shown a reply to means it has been made unreadable to this
/// account since. The caller says so rather than moving.
pub async fn around(room: &Room, event_id: &str) -> Result<Around> {
    let parsed = super::event_id_of(event_id)?;

    let window = room
        .event_with_context(&parsed, false, UInt::from(CONTEXT), None)
        .await
        .map_err(|_| Error::NoSuchEvent {
            event_id: event_id.to_owned(),
        })?;

    // `events_before` comes back newest first, the way a backwards pagination
    // does, and has to be turned round. Getting this wrong reverses the half
    // above the message while leaving the half below it in order, which reads
    // as a conversation that almost makes sense.
    //
    // Read on the room's own terms, thread replies dropped, because this is
    // the room's timeline drawn at a different place in it. A window that drew
    // them would be the same conversation twice, once here and once in the
    // panel, which is the whole reason `facts` has two readings.
    let messages = window
        .events_before
        .iter()
        .rev()
        .chain(window.event.iter())
        .chain(window.events_after.iter())
        .filter_map(facts::message)
        .collect();

    Ok(Around {
        messages,
        back: window.prev_batch_token,
        forward: window.next_batch_token,
    })
}
