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
    /// What it says, with no formatting.
    ///
    /// The plaintext fallback every message carries, and the only thing to
    /// draw when `html` is `None`.
    ///
    /// Empty for an attachment nobody captioned, which is most of them. The
    /// filename lives on [`Media::name`] and is deliberately not here: a line
    /// reading "screenshot.png" above the screenshot is the thing somebody
    /// sent a picture to avoid.
    pub body: String,
    /// What it says as HTML, when the sender sent formatting.
    ///
    /// `formatted_body` off the wire, verbatim, and only when `format` said
    /// `org.matrix.custom.html`. `None` for the messages nobody formatted,
    /// which is most of them.
    ///
    /// Deliberately not sanitised here, and nothing downstream may put it in a
    /// document. `FormattedBody` in the webview parses it into an inert
    /// document and rebuilds it out of an allow-list of elements it knows, so
    /// a tag that is not on that list is dropped rather than trusted.
    /// Sanitising here as well would be a second copy of that list to keep in
    /// step with the first, and the one that is not the renderer is the one
    /// that would go stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// The picture or the clip hanging off it, when there is one.
    ///
    /// Present for [`MessageKind::Image`] and [`MessageKind::Video`] and for
    /// nothing else. The bytes are not here: this says where they are and what
    /// shape they will be, and the interface asks for them one at a time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    /// The thread hanging off it, when anybody has replied in one.
    ///
    /// `None` rather than a count of zero for a message nobody has replied to,
    /// because a message with no thread is not a thread with nothing in it and
    /// the interface has to be able to tell the two apart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadSummary>,
    pub kind: MessageKind,
}

/// What is known about a thread without opening it.
///
/// The homeserver counts this and bundles it onto the message the thread hangs
/// from, so a room learns which of its messages are threads while it is being
/// drawn rather than by asking about each one. In an encrypted room the bundle
/// arrives with the encrypted message and is decrypted alongside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    /// How many replies are in it.
    pub count: u32,
    /// Whether the person signed in here has said anything in it.
    pub participated: bool,
}

/// Where an attachment's bytes are, and what shape they will be drawn at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// An opaque handle, to be handed back to `timeline::media` unread.
    ///
    /// It is the event's own `MediaSource` as JSON, which for an encrypted
    /// room carries the file's key as well as its URI. That is deliberate and
    /// it is the reason this is one field rather than two: half the shapes an
    /// attachment can take are encrypted, and a handle that held only a URI
    /// would need a second path beside it for the half that matters here.
    ///
    /// It crosses the IPC boundary, which is the same boundary the decrypted
    /// bytes cross a moment later, so the key adds nothing to what the webview
    /// already holds. Nothing reads it on that side: it goes back to Rust as
    /// it arrived.
    pub source: String,
    /// The file's own name.
    ///
    /// `filename` where the sender wrote one and `body` where they did not,
    /// which is the rule the specification gives for media captions. It is
    /// what a card is labelled with, what a save dialog opens on, and what a
    /// screen reader is told when the picture will not load.
    pub name: String,
    /// A second handle, for the still the sender uploaded beside a clip.
    ///
    /// A clip is not fetched until somebody asks for one, so without this
    /// there is nothing to draw where it will be: a black rectangle and a
    /// filename, which says almost nothing about what is in it. The thumbnail
    /// is a few kilobytes and is drawn straight away, so what somebody decides
    /// on is the picture rather than the name.
    ///
    /// Absent for the senders who upload no thumbnail, which is plenty of
    /// them, and always absent for anything that is not a clip: a picture is
    /// its own thumbnail, and there is nothing to look at in a spreadsheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// What the sender said the bytes are, when they said something this build
    /// would repeat.
    ///
    /// Kept only when it names an image or a video, because it is the type
    /// the webview is handed for playback and the sender writes it: anything
    /// else here would be a way to have a browser treat somebody's attachment
    /// as a document. It is a hint rather than a fact, and what actually
    /// arrives is sniffed in Rust.
    ///
    /// So a file and a voice note carry none. Neither is played, only saved,
    /// and the name already ends in the extension that says what it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// How many bytes the sender said it is.
    ///
    /// For telling somebody what they are about to wait for, and for nothing
    /// else. The real limit is applied to what arrives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The pixel width the sender said it has, if any.
    ///
    /// Here so the room can hold the space before the bytes land. Without it
    /// every picture that loads shoves the conversation below it downwards,
    /// which in a room that follows the bottom is the whole view moving.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    /// The pixel height the sender said it has, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
}

/// What sort of message this is.
///
/// The three `m.room.message` types that are text, the two that carry
/// something to look at, the two that carry something to save, and the two
/// ways a message can exist with nothing to draw at all.
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
    /// `m.image`. Its `media` says where the picture is.
    Image,
    /// `m.file`. Its `media` says where the file is.
    ///
    /// Drawn as a card that saves rather than as anything to look at. Consort
    /// has no viewer for a spreadsheet and should not pretend to.
    File,
    /// `m.audio`. A card that saves, on exactly the same terms as a file.
    ///
    /// Separate from one only because the interface says "voice note" rather
    /// than "file" when it knows, and because playing one is the obvious next
    /// thing and will want its own variant when it lands.
    Audio,
    /// `m.video`. Its `media` says where the clip is.
    ///
    /// Separate from an image rather than folded in with it, because the two
    /// are not fetched on the same terms: a picture is drawn as soon as the
    /// room is, and a clip waits to be asked for.
    Video,
    /// Encrypted, and this session has no key for it.
    ///
    /// Drawn rather than skipped. A gap in a conversation that says nothing
    /// about itself is indistinguishable from a conversation that had a gap in
    /// it, and the difference matters: one is a key that has not arrived and
    /// the other is nobody talking.
    Undecryptable,
    /// A message body this build cannot render, such as a location.
    ///
    /// Also drawn rather than skipped, and for the same reason. Somebody whose
    /// message silently vanished has no way to know it was ever sent.
    Unsupported,
}
