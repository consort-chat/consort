// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The history either side of one message, which `/context` answers directly
//! rather than by paging back from the live end.
//!
//! Not spliced into what is loaded, because the two pieces are not adjacent: a
//! window from last year in front of yesterday's messages reads as one
//! conversation. It replaces what is loaded, and
//! [`Watch::go_to`](super::Watch::go_to) is the way back.

use matrix_sdk::Room;
use matrix_sdk::ruma::UInt;

use crate::error::{Error, Result};
use crate::timeline::dto::{Message, SystemMessage};
use crate::timeline::facts;

/// How many messages to ask for around the one being gone to. The homeserver
/// splits this either side, so the message lands in a conversation rather than
/// alone at the top of an empty room.
const CONTEXT: u32 = 24;

/// A window of history, and where it can be grown from.
pub struct Around {
    /// Oldest first, like everything else here.
    pub messages: Vec<Message>,
    /// The membership changes in this same window, on the same terms as
    /// `messages`.
    pub system: Vec<SystemMessage>,
    /// Where a page older than this window starts, or `None` at the beginning
    /// of the room.
    pub back: Option<String>,
    /// Where a page newer than this window starts, or `None` when the window
    /// already reaches the live end.
    pub forward: Option<String>,
}

/// Read the history around `event_id`.
///
/// The event itself is not required to be drawable: a reply can name a
/// redacted message, and the window either side of it is still where somebody
/// asked to be taken.
///
/// Fails only when the homeserver will not answer at all, which means the
/// message has been made unreadable to this account since.
pub async fn around(room: &Room, event_id: &str) -> Result<Around> {
    let parsed = super::event_id_of(event_id)?;

    let window = room
        .event_with_context(&parsed, false, UInt::from(CONTEXT), None)
        .await
        .map_err(|_| Error::NoSuchEvent {
            event_id: event_id.to_owned(),
        })?;

    // `events_before` comes back newest first, the way a backwards pagination
    // does, and has to be turned round.
    //
    // Read on the room's own terms, thread replies dropped, because this is
    // the room's timeline drawn at a different place in it.
    let events: Vec<&matrix_sdk::deserialized_responses::TimelineEvent> = window
        .events_before
        .iter()
        .rev()
        .chain(window.event.iter())
        .chain(window.events_after.iter())
        .collect();

    let messages = events.iter().copied().filter_map(facts::message).collect();
    let system = events.iter().copied().filter_map(facts::system).collect();

    Ok(Around {
        messages,
        system,
        back: window.prev_batch_token,
        forward: window.next_batch_token,
    })
}
