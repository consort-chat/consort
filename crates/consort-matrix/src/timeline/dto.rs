// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for a room's messages.
//!
//! One value describes the whole of what is currently loaded, on the same
//! terms as the room list: a reader handed one of these has everything it
//! needs to draw, and a late subscriber can be handed the same value rather
//! than a stream of patches it has to replay in order.
//!
//! None of the SDK's own types appear here, for the reason they do not appear
//! in the room or verification DTOs: this shape is a contract with
//! `app/src/lib/api.ts`, and pinning it to an upstream type means an SDK bump
//! can silently change what the webview receives.

use serde::{Deserialize, Serialize};

/// Everything currently loaded for one room, oldest message first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// Which room this is, so a reader can tell an arriving timeline apart
    /// from the one it is drawing.
    ///
    /// Load-bearing rather than informational. This channel keeps its latest
    /// value for a late subscriber, and somebody who changes room twice
    /// quickly has two watchers publishing for a moment; without this the
    /// second room would draw the first room's messages until the next one
    /// arrived.
    pub room_id: String,
    /// Oldest first, which is the order they are drawn in.
    pub messages: Vec<Message>,
    /// Whether there is more history to ask for.
    ///
    /// False at the start of the room, and also false before anything has been
    /// loaded at all, because "there might be more" is not something to offer
    /// until there is something to be more than.
    pub more_before: bool,
    /// Whether history is being fetched right now.
    ///
    /// Here rather than kept by the interface so that the spinner belongs to
    /// the room. A reader that owned this would have to clear it itself on a
    /// room change, and forgetting to is a spinner that never stops.
    pub loading: bool,
}

/// One message in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The event ID. Also the deduplication key: an event can arrive from a
    /// sync and again from a backfill page that overlaps it.
    pub id: String,
    /// Who sent it, as a Matrix user ID.
    ///
    /// Not a display name. A name is per room and changes under a message that
    /// has already been drawn, and the frontend already asks for names and
    /// avatars by user ID for the voice roster.
    pub sender: String,
    /// `origin_server_ts`, in milliseconds.
    ///
    /// The server's clock rather than the sender's, because the sender's is
    /// whatever their machine says and a room with one badly set clock in it
    /// would draw one person's messages in the wrong century.
    pub at: u64,
    /// What it says.
    pub body: String,
    pub kind: MessageKind,
}

/// What sort of message this is.
///
/// Only the three `m.room.message` types that are text, plus the two ways a
/// message can exist and have no text to draw. Images, files and everything
/// else are deliberately absent: they are not built, and a variant for one
/// would be a promise the interface cannot keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    /// `m.text`. The ordinary case.
    Text,
    /// `m.emote`, which is `/me`. Drawn as an action rather than as speech.
    Emote,
    /// `m.notice`, which is what bots and bridges send. Drawn quieter, because
    /// the whole point of the type is that it is not a person talking.
    Notice,
    /// Encrypted, and this session has no key for it.
    ///
    /// Drawn rather than skipped. A gap in a conversation that says nothing
    /// about itself is indistinguishable from a conversation that had a gap in
    /// it, and the difference matters: one is a key that has not arrived and
    /// the other is nobody talking.
    Undecryptable,
    /// A message body this build cannot render, such as an image or a file.
    ///
    /// Also drawn rather than skipped, and for the same reason. Somebody whose
    /// screenshot silently vanished has no way to know it was ever sent.
    Unsupported,
}
