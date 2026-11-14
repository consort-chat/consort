// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for a room's messages.
//!
//! One value describes the whole of what is currently loaded, so a late
//! subscriber can be handed it rather than a stream of patches to replay in
//! order. None of the SDK's own types appear here: this shape is a contract
//! with `app/src/lib/api.ts`, and an SDK bump must not silently change what
//! the webview receives.

use serde::{Deserialize, Serialize};

/// Everything currently loaded for one room, oldest message first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// Which room this is. Load-bearing: changing room twice quickly leaves
    /// two watchers publishing for a moment, and the reader draws the one it
    /// asked for rather than whichever arrived.
    pub room_id: String,
    /// Oldest first, which is the order they are drawn in.
    pub messages: Vec<Message>,
    /// The messages a reply names that are not in `messages`, since a reply
    /// can name anything older than the loaded window. Only the ones the
    /// reader does not already have, and a message twenty replies name once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answered: Vec<Message>,
    /// Whether there is more history to ask for. False before anything has
    /// loaded at all, as well as at the start of the room.
    pub more_before: bool,
    /// Whether there are messages newer than these. True only in a window
    /// somebody jumped into; the live end has nothing after it.
    #[serde(default)]
    pub more_after: bool,
    /// Membership changes, drawn as system lines rather than as messages,
    /// oldest first. The interface merges them with `messages` by `at`, which
    /// is an approximation: see [`SystemMessage::at`] for what that costs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<SystemMessage>,
    /// The message this window was opened around; `None` at the live end. A
    /// reader looking at last March has to be told it is not the bottom of
    /// the room, because that is also what a broken connection looks like.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// Whether a page of older messages is being fetched, a jump included.
    /// Here rather than in the interface, which would have to clear it on a
    /// room change and is a spinner that never stops when it forgets.
    pub loading: bool,
    /// Whether a page of newer messages is being fetched. Its own flag rather
    /// than a direction on `loading`, because the notice is drawn at the end
    /// of the list the page is coming from.
    #[serde(default)]
    pub loading_after: bool,
    /// The last message this account had read when the room was opened, which
    /// the new-messages line is drawn under. Read from `m.fully_read` once and
    /// then held, because reading the room moves the marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_up_to: Option<String>,
}

/// One thread as it is currently loaded, oldest reply first. Its own value
/// rather than a field on [`Timeline`]: a room whose panel is shut should not
/// carry a copy of somebody's last conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The room the thread is in, on the same terms as [`Timeline::room_id`].
    pub room_id: String,
    /// The event ID the thread hangs from. Load-bearing like `room_id`:
    /// opening one thread and then another puts two of these in flight.
    pub root_id: String,
    /// The message it hangs from. `None` when the homeserver would not hand it
    /// over, which a redaction and a missing key both look like; the replies
    /// are still worth reading, so the thread is drawn rather than refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<Message>,
    /// The replies, oldest first, which is the order they are drawn in.
    pub messages: Vec<Message>,
    /// Whether there are older replies than the ones here. The homeserver is
    /// asked for the recent end of a long thread.
    pub more_before: bool,
}

/// Who is typing in one room, right now. Its own value rather than a field on
/// [`Timeline`], because republishing every loaded message on each keystroke
/// would cross the IPC boundary several times a sentence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typing {
    /// The room they are typing in, on the same terms as [`Timeline::room_id`].
    pub room_id: String,
    /// Matrix user IDs, with this session's own already taken out: nobody
    /// needs telling that they are typing.
    pub users: Vec<String>,
}

/// Who has read how far, for the room and for whatever thread is open.
///
/// Its own value rather than a field on [`Timeline`], because a receipt
/// arriving would otherwise redraw the whole room: the measurements are in
/// docs/PERFORMANCE.md. Both conversations in one value because the channel
/// keeps only its latest, so two values would mean one erasing the other.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Readers {
    /// The room this is about, on the same terms as [`Timeline::room_id`].
    pub room_id: String,
    /// Who has read how far in the room's own timeline.
    pub main: Vec<ReadOn>,
    /// The same for the thread somebody has open, when one is. A thread has
    /// receipts of its own and a receipt in the room does not answer for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadReaders>,
}

/// One thread's readers, named by the thread they are in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReaders {
    /// The message the thread hangs from, so a reader can tell an answer about
    /// the thread it has open from an answer about the one it just closed.
    pub root_id: String,
    /// Who has read how far inside it.
    pub on: Vec<ReadOn>,
}

/// Who has read one message, as far as this account can see. Somebody who has
/// turned public read receipts off is absent from every one of these and
/// cannot be told from somebody who has not read it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadOn {
    /// The message they have read up to.
    pub event_id: String,
    /// Up to [`crate::timeline::read_by::SHOWN`] of them, by Matrix user ID,
    /// in a stable order. This session's own is never here.
    pub readers: Vec<String>,
    /// How many more there are than the ones named. A room of fifty puts fifty
    /// receipts on its newest message, and the count keeps that a row.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more: u32,
}

/// Whether a count is nothing, for skipping it on the wire.
fn is_zero(count: &u32) -> bool {
    *count == 0
}

/// One key people have reacted to a message with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    /// What they reacted with. Deliberately not required to be an emoji:
    /// `m.annotation` carries free text and other clients send it.
    pub key: String,
    /// How many people have used it.
    pub count: u32,
    /// This session's own annotation, when there is one. The event ID rather
    /// than a flag, because taking a reaction back redacts that exact event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mine: Option<String>,
    /// What to call the key when it is a custom emoji, which MSC2545 keys by
    /// the image's own `mxc://` URI. Absent for an ordinary reaction, where
    /// the key is the character and needs no name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcode: Option<String>,
}

/// One message in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The event ID. Also the deduplication key: an event can arrive from a
    /// sync and again from a backfill page that overlaps it.
    pub id: String,
    /// Who sent it, as a Matrix user ID. Not a display name: a name is per
    /// room and changes under a message that has already been drawn.
    pub sender: String,
    /// `origin_server_ts`, in milliseconds. The server's clock, because one
    /// badly set machine would draw its messages in the wrong century.
    pub at: u64,
    /// What it says, with no formatting, and the only thing to draw when
    /// `html` is `None`. Empty for an attachment nobody captioned: the
    /// filename is on [`Media::name`] rather than here, above the picture.
    pub body: String,
    /// `formatted_body` off the wire, verbatim, and only when `format` said
    /// `org.matrix.custom.html`.
    ///
    /// Deliberately not sanitised here, and nothing downstream may put it in a
    /// document. `FormattedBody` in the webview rebuilds it from an allow-list
    /// of elements, and a second copy of that list would be the stale one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// Where the picture or the clip is, not the bytes themselves. Present for
    /// [`MessageKind::Image`] and [`MessageKind::Video`] and nothing else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    /// The thread hanging off it. `None` rather than a count of zero, because
    /// a message with no thread is not a thread with nothing in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<ThreadSummary>,
    /// What people have reacted with, in the order the keys first appeared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    /// The event this message is answering. The ID alone, since the interface
    /// already holds what it draws. `None` for the fallback pointer every
    /// threaded message carries, which names nobody's choice of reply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Who this message names: `m.mentions` off the wire and nothing else, so
    /// a client that does not send it names nobody. The `room` flag beside
    /// them is deliberately not read, because an @room is about everybody.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
    /// Whether what is drawn above is a correction. The flag alone: no
    /// interface here offers an edit history, so a sentence somebody took back
    /// is not put on the wire.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
    /// Who redacted it, for a [`MessageKind::Deleted`] and nothing else. Equal
    /// to `sender` ordinarily, and carried so that a moderator's removal is
    /// not drawn as the author taking their own message back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_by: Option<String>,
    /// Why this message's sender could not be vouched for, when they could not.
    ///
    /// `None` covers more than "verified": a sender nobody ever verified is
    /// also `None`, not having checked being no finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_trust: Option<SenderTrust>,
    pub kind: MessageKind,
}

/// What is known about a thread without opening it. The homeserver bundles it
/// onto the message the thread hangs from, and in an encrypted room it arrives
/// with that message and is decrypted alongside it.
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
    /// An opaque handle, handed back to `timeline::media` unread: the event's
    /// own `MediaSource` as JSON, carrying an encrypted file's key as well as
    /// its URI. It crosses the boundary the decrypted bytes then cross.
    pub source: String,
    /// The file's own name: `filename` where the sender wrote one and `body`
    /// where they did not, which is the rule the spec gives for captions.
    pub name: String,
    /// A second handle, for the still a sender uploaded beside a clip, since a
    /// clip is not fetched until somebody asks for it. Always absent for
    /// anything that is not a clip, and for senders who upload no still.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// What the sender said the bytes are, kept only when it names an image or
    /// a video: the webview is handed this for playback, and anything else
    /// would let a browser treat an attachment as a document. The bytes that
    /// actually arrive are sniffed in Rust.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// How many bytes the sender said it is, for telling somebody what they
    /// are about to wait for. The real limit is applied to what arrives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The pixel width the sender said it has, so the room can hold the space
    /// before the bytes land rather than shove the conversation downwards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    /// The pixel height the sender said it has, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
}

/// Why a message's sender could not be vouched for.
///
/// One variant per `ShieldStateCode` the SDK can raise, and nothing else. A
/// code rather than a sentence: the words belong beside the icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SenderTrust {
    /// Not enough is known to check who sent it.
    AuthenticityNotGuaranteed,
    /// The sending device is one this session cannot find.
    UnknownDevice,
    /// The sender never signed the device they sent it from.
    UnsignedDevice,
    /// The sender is somebody this session has not verified.
    UnverifiedIdentity,
    /// The sender was verified once and their identity has changed since.
    VerificationViolation,
    /// The sender named on the event does not own the session it came in.
    MismatchedSender,
}

/// What sort of message this is.
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
    /// `m.file`. Drawn as a card that saves: Consort has no viewer for a
    /// spreadsheet and should not pretend to.
    File,
    /// `m.audio`. A card that saves, on the same terms as a file, and separate
    /// only because the interface says "voice note" when it knows.
    Audio,
    /// `m.video`. Separate from an image because the two are not fetched on
    /// the same terms: a clip waits to be asked for.
    Video,
    /// Encrypted, and this session has no key for it. Drawn rather than
    /// skipped: a silent gap is indistinguishable from nobody talking.
    Undecryptable,
    /// A message body this build cannot render, such as a location. Also drawn
    /// rather than skipped, so nothing vanishes without saying so.
    Unsupported,
    /// Redacted. A mark where it was rather than a gap, because every other
    /// client in the room draws one and a reply under a vanished message
    /// answers nothing. Redacted is not erased: the envelope survives, with
    /// its sender and timestamp, which is what this is built from.
    Deleted,
}

/// One thing that happened to a room rather than in it: a membership, name,
/// topic or picture changing. Carries bare Matrix IDs rather than a composed
/// sentence, so the interface writes the English in the locale it draws in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMessage {
    /// The event ID. The React key, on the same terms as [`Message::id`].
    pub id: String,
    /// `origin_server_ts`, in milliseconds, used only to place this line among
    /// the messages. An approximation rather than the room's own order: see
    /// [`History`](super::History) for why messages are not sorted by it.
    pub at: u64,
    /// Who made the change: the sender of the state event. For a join this is
    /// also the subject; for an invite, a kick or a ban it is the other party.
    pub actor: String,
    /// What was done. Flattened, so the wire carries `kind` beside `actor` and
    /// the interface switches on one discriminant.
    #[serde(flatten)]
    pub change: SystemChange,
}

/// What a [`SystemMessage`] reports. Tagged by `kind` so TypeScript reads it
/// as a discriminated union: one `subject` string for both a person and a room
/// name would make "renamed to @ada:example.org" a value this type could hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SystemChange {
    /// `subject` joined the room, for the first time or again after having
    /// left it.
    Joined {
        /// Who joined, which for a join is also the actor.
        subject: String,
    },
    /// The actor invited `subject`.
    Invited {
        /// Who was invited.
        subject: String,
    },
    /// `subject` left the room on their own.
    Left {
        /// Who left, which for a self-leave is also the actor.
        subject: String,
    },
    /// The actor removed `subject` from the room.
    Kicked {
        /// Who was removed.
        subject: String,
    },
    /// The actor banned `subject`.
    Banned {
        /// Who was banned.
        subject: String,
    },
    /// The actor changed the room's name.
    Renamed {
        /// The new name, or `None` when it was removed. A room name is cleared
        /// by setting it empty rather than by redacting: see
        /// `facts::or_cleared`.
        name: Option<String>,
    },
    /// The actor changed the room's topic.
    TopicChanged {
        /// The new topic, or `None` when it was removed, on the same terms as
        /// [`SystemChange::Renamed`]. The plain-text one only: `m.topic`
        /// carries the same thing in several mimetypes.
        topic: Option<String>,
    },
    /// The actor changed the room's picture.
    AvatarChanged {
        /// The new picture as an `mxc:` URI, or `None` when it was removed.
        /// `m.room.avatar` clears itself by omitting `url` rather than by
        /// emptying it, so unlike a name this arrives as an `Option` already.
        url: Option<String>,
    },
}
