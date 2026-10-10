// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning one timeline event into one message, or into nothing.
//!
//! The only part of this module that has to know what an SDK event looks like,
//! and it needs no `Client`, no room and no network, so every rule below is
//! tested against the exact bytes a homeserver sends.
//!
//! Most of a room's timeline is not messages, so `None` is the ordinary answer
//! rather than a failure.

use matrix_sdk::deserialized_responses::{ShieldState, ShieldStateCode, TimelineEvent};
use matrix_sdk::ruma::UInt;
use matrix_sdk::ruma::events::relation::{BundledMessageLikeRelations, BundledThread};
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::room::member::{MembershipState, OriginalSyncRoomMemberEvent};
use matrix_sdk::ruma::events::room::message::sanitize::remove_plain_reply_fallback;
use matrix_sdk::ruma::events::room::message::{
    FormattedBody, MessageFormat, MessageType, RedactedRoomMessageEventContent, Relation,
    RoomMessageEventContentWithoutRelation,
};
use matrix_sdk::ruma::events::room::redaction::SyncRoomRedactionEvent;
use matrix_sdk::ruma::events::{
    AnySyncEphemeralRoomEvent, AnySyncMessageLikeEvent, AnySyncStateEvent, AnySyncTimelineEvent,
    RedactedSyncMessageLikeEvent, SyncMessageLikeEvent, SyncStateEvent,
};
use matrix_sdk::ruma::serde::Raw;
use serde::Deserialize;

use matrix_sdk::ruma::events::receipt::ReceiptType;

use crate::timeline::dto::{
    Media, Message, MessageKind, SenderTrust, SystemChange, SystemMessage, ThreadSummary,
};
use crate::timeline::read_by::About;

/// One `m.reaction` event, unpacked. Its own type rather than a tuple, because
/// three strings in a row the compiler cannot tell apart is three chances to
/// pass them in the wrong order.
pub struct Annotated {
    /// The reaction event's own ID, which is what a redaction names.
    pub event_id: String,
    /// The message it is on.
    pub target: String,
    /// What they reacted with.
    pub key: String,
    /// Who reacted.
    pub sender: String,
    /// What the sender called the image they reacted with, when they said.
    ///
    /// MSC2545 keys a custom emoji reaction by the image's own `mxc://` URI
    /// and says nothing about naming it, so the name is carried beside the
    /// relation as `shortcode`: that is what Cinny writes and reads, and
    /// without it a pill about an emoji from a pack this account cannot see
    /// has nothing to read out but a URI.
    pub shortcode: Option<String>,
}

/// One `m.replace` event, unpacked.
///
/// Its own type for the reason [`Annotated`] is one, and because `target` and
/// `event_id` are both event IDs: confusing them is the difference between
/// correcting a message and correcting the correction.
pub struct Replacement {
    /// The edit's own event ID, which is what a redaction of it names.
    pub event_id: String,
    /// The message it replaces.
    pub target: String,
    /// Who sent the edit. Carried rather than assumed, because matching it
    /// against the original's sender is the whole of what stops one person
    /// rewriting another's words, and the original is often not loaded here.
    pub sender: String,
    /// The edit's own `origin_server_ts`, in milliseconds: which of several
    /// edits wins, and nothing else. Deliberately not written onto the
    /// message, for the reason `Loaded::corrected` gives.
    pub at: u64,
    /// What the message now says, with no formatting.
    pub body: String,
    /// What it now says as HTML, when the edit carried formatting.
    ///
    /// `None` is a message edited down to plain text, and it has to overwrite
    /// rather than leave the old formatting standing, because
    /// `FormattedBody` draws the HTML whenever there is any.
    pub html: Option<String>,
    /// Why the device that sent the edit could not be vouched for.
    ///
    /// The edit's own: the words a reader sees are this event's.
    pub sender_trust: Option<SenderTrust>,
}

/// What this build says instead of an encrypted message it has no key for.
///
/// Short on purpose: a room that was busy while this session was away is a
/// screen full of these, and a screen full of sentences beginning "cannot"
/// reads as a broken client rather than as a key that has not arrived.
const NO_KEY: &str = "Waiting for the key to this message.";

/// What this build says instead of a message it cannot draw.
const NOT_SUPPORTED: &str = "A message Consort cannot draw.";

/// Where the message being read is going to be drawn.
///
/// The only thing it decides is what to do with a thread reply, which belongs
/// in exactly one of the two and would be a conversation drawn twice if it
/// were in both. A reply in the room is not a thread reply and is kept.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// The room's own timeline.
    Room,
    /// One thread inside it.
    Thread,
}

/// One event as a message in a room, or `None` when it is not one to draw.
pub fn message(event: &TimelineEvent) -> Option<Message> {
    read(event, Reading::Room)
}

/// One event as a message in a thread, or `None` when it is not one to draw.
pub fn in_thread(event: &TimelineEvent) -> Option<Message> {
    read(event, Reading::Thread)
}

/// One event as a message drawn on its own, or `None` when it is not one.
///
/// The same reading as [`in_thread`], named separately because the caller asks
/// a different question: a reply row draws the message it names as itself,
/// whether or not that message lives in a thread.
pub fn alone(event: &TimelineEvent) -> Option<Message> {
    read(event, Reading::Thread)
}

/// The message an event is a threaded reply to, when it is one. Deliberately
/// not a field on [`Message`]: the one caller is the watcher asking whether an
/// arriving event belongs to the thread somebody has open.
pub fn thread_root(event: &TimelineEvent) -> Option<String> {
    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
        SyncMessageLikeEvent::Original(said),
    )) = event.raw().deserialize().ok()?
    else {
        return None;
    };

    match said.content.relates_to {
        Some(Relation::Thread(thread)) => Some(thread.event_id.to_string()),
        _ => None,
    }
}

/// One person's claim to have read up to one message. Its own type rather than
/// a tuple, on the same terms as [`Annotated`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    /// Who read it, as a Matrix user ID.
    pub user: String,
    /// The newest message they have read. Everything before it is implied.
    pub event_id: String,
    /// Which conversation they read it in.
    pub about: About,
}

/// Who an `m.receipt` event says has read what, or `None` for anything else.
///
/// The whole batch every time, because that is what the event carries. An
/// empty answer is not `None`: it lets the caller tell a batch with no public
/// receipts in it from no batch at all.
///
/// `m.read` only, because it is the one receipt anybody else in the room can
/// see, and a private one reaching this client is its own.
///
/// A receipt with no `thread_id` and one naming `main` are both about the
/// room's own timeline, so both are read: the difference is what the sending
/// client knew, and reading one of the two draws nobody for half the room.
pub fn receipts(event: &Raw<AnySyncEphemeralRoomEvent>) -> Option<Vec<Read>> {
    let AnySyncEphemeralRoomEvent::Receipt(receipt) = event.deserialize().ok()? else {
        return None;
    };

    let mut said = Vec::new();
    for (event_id, kinds) in receipt.content.0 {
        let Some(public) = kinds.get(&ReceiptType::Read) else {
            continue;
        };
        for (user, one) in public {
            said.push(Read {
                user: user.to_string(),
                event_id: event_id.to_string(),
                about: About::from(&one.thread),
            });
        }
    }
    Some(said)
}

/// Who an `m.typing` event says is typing, or `None` for anything else.
///
/// The whole list every time, because the event is a statement of who is
/// typing now. A room where everybody stopped sends an empty one, and treating
/// that as nothing to say leaves the last name on screen for ever.
pub fn typing(event: &Raw<AnySyncEphemeralRoomEvent>) -> Option<Vec<String>> {
    let AnySyncEphemeralRoomEvent::Typing(typing) = event.deserialize().ok()? else {
        return None;
    };

    Some(
        typing
            .content
            .user_ids
            .into_iter()
            .map(|user| user.to_string())
            .collect(),
    )
}

/// One event as a reaction, or `None` when it is not one. Deliberately
/// separate from [`message`], which answers `None` for a reaction and should
/// keep doing so: a reaction is not a line in the conversation.
pub fn annotation(event: &TimelineEvent) -> Option<Annotated> {
    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::Reaction(
        SyncMessageLikeEvent::Original(reacted),
    )) = event.raw().deserialize().ok()?
    else {
        return None;
    };

    Some(Annotated {
        event_id: reacted.event_id.to_string(),
        target: reacted.content.relates_to.event_id.to_string(),
        key: reacted.content.relates_to.key,
        sender: reacted.sender.to_string(),
        shortcode: shortcode_of(event),
    })
}

/// The longest a shortcode may be, in bytes, which is MSC2545's own ceiling.
///
/// Applied to a reaction's name and not to a pack's. A pack is set by somebody
/// with power in a room, and the MSC asks that a malformed shortcode be shown
/// so they can see it and fix it. A reaction is sent by anybody in the room
/// and nobody can fix it, and the name becomes a pill's tooltip and the label
/// a screen reader reads out.
const LONGEST_SHORTCODE: usize = 100;

/// What a reaction's sender called the image, out of the raw event.
///
/// Read field by field rather than by deserialising the content, because the
/// field is not in the specification: a typed read of the whole content would
/// let a sender who wrote a number there cost the reaction itself. A blank one
/// is no name, which is the same rule a blank room name gets.
fn shortcode_of(event: &TimelineEvent) -> Option<String> {
    /// Only the one field, so nothing else in the content can refuse to parse.
    #[derive(Deserialize)]
    struct Named {
        shortcode: Option<String>,
    }

    event
        .raw()
        .get_field::<Named>("content")
        .ok()??
        .shortcode
        .filter(|name| !name.trim().is_empty() && name.len() <= LONGEST_SHORTCODE)
}

/// One `m.room.redaction`, unpacked. Its own type rather than the event ID
/// alone, because who did it is now drawn.
pub struct Redaction {
    /// The event it removes.
    pub event_id: String,
    /// Who removed it, which is not always whoever wrote the message: a
    /// moderator can redact somebody else's, and the mark has to say which of
    /// the two happened.
    pub sender: String,
}

/// The event a redaction removes, when the event is one.
///
/// Both fields are read for the ID, because which one carries it is the room
/// version's business: room 11 moved `redacts` into the content and older
/// rooms keep it at the top level. This module has no room version to ask.
pub fn redaction(event: &TimelineEvent) -> Option<Redaction> {
    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomRedaction(
        SyncRoomRedactionEvent::Original(redacted),
    )) = event.raw().deserialize().ok()?
    else {
        return None;
    };

    Some(Redaction {
        event_id: redacted
            .redacts
            .or(redacted.content.redacts)
            .map(|id| id.to_string())?,
        sender: redacted.sender.to_string(),
    })
}

/// One event as an edit of another, or `None` when it is not one.
///
/// Read out of `m.new_content` and never out of the top-level body, which is
/// the `* `-prefixed fallback a client with no idea about edits draws. An edit
/// carrying no `m.new_content` has nothing to fold, and `None` says so.
///
/// Says nothing about whether the edit is allowed: that comparison needs the
/// original, so [`crate::timeline::Edits::latest_on`] makes it at fold time.
pub fn replacement(event: &TimelineEvent) -> Option<Replacement> {
    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
        SyncMessageLikeEvent::Original(said),
    )) = event.raw().deserialize().ok()?
    else {
        return None;
    };

    let Some(Relation::Replacement(replaced)) = said.content.relates_to else {
        return None;
    };

    // Text, and the two that are text wearing a different hat. An edit of an
    // attachment's caption arrives as the attachment's own type, and there is
    // no caption editing surface to fold it onto.
    let (body, formatted) = match replaced.new_content.msgtype {
        MessageType::Text(text) => (text.body, text.formatted),
        MessageType::Emote(emote) => (emote.body, emote.formatted),
        MessageType::Notice(notice) => (notice.body, notice.formatted),
        _ => return None,
    };

    Some(Replacement {
        event_id: said.event_id.to_string(),
        target: replaced.event_id.to_string(),
        sender: said.sender.to_string(),
        at: said.origin_server_ts.0.into(),
        sender_trust: trust(event),
        body,
        html: formatted
            .filter(|formatted| formatted.format == MessageFormat::Html)
            .map(|formatted| formatted.body),
    })
}

/// One event as something that happened to the room, or `None` when it is not
/// one to draw.
///
/// Membership, name, topic and avatar are the only state events read. A power
/// level change, a canonical alias, a join rule and the rest are still `None`:
/// a line not yet written rather than a line missing.
pub fn system(event: &TimelineEvent) -> Option<SystemMessage> {
    let AnySyncTimelineEvent::State(state) = event.raw().deserialize().ok()? else {
        // Every message-like event, and every reaction.
        return None;
    };

    // Each arm names its own event rather than binding one through the enum:
    // there is nothing common to hoist that would not be taken apart again.
    match state {
        AnySyncStateEvent::RoomMember(SyncStateEvent::Original(member)) => Some(SystemMessage {
            id: member.event_id.to_string(),
            at: member.origin_server_ts.0.into(),
            actor: member.sender.to_string(),
            change: membership(&member)?,
        }),
        AnySyncStateEvent::RoomName(SyncStateEvent::Original(named)) => Some(SystemMessage {
            id: named.event_id.to_string(),
            at: named.origin_server_ts.0.into(),
            actor: named.sender.to_string(),
            change: SystemChange::Renamed {
                name: or_cleared(named.content.name),
            },
        }),
        AnySyncStateEvent::RoomTopic(SyncStateEvent::Original(topic)) => Some(SystemMessage {
            id: topic.event_id.to_string(),
            at: topic.origin_server_ts.0.into(),
            actor: topic.sender.to_string(),
            change: SystemChange::TopicChanged {
                topic: or_cleared(topic.content.topic),
            },
        }),
        AnySyncStateEvent::RoomAvatar(SyncStateEvent::Original(avatar)) => Some(SystemMessage {
            id: avatar.event_id.to_string(),
            at: avatar.origin_server_ts.0.into(),
            actor: avatar.sender.to_string(),
            change: SystemChange::AvatarChanged {
                url: avatar.content.url.map(|url| url.to_string()),
            },
        }),
        // Every other state event, and a redacted one of the four above: a
        // redaction takes the content with it, so there is no name left to
        // report and no membership left to compare.
        _ => None,
    }
}

/// A state event's new value, or `None` when the event cleared it.
///
/// A room name or topic is removed by setting it to the empty string rather
/// than by redacting the event, so an empty one is a removal and not a name.
fn or_cleared(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// One `m.room.member` event as the change it makes, or `None` when it makes
/// none worth drawing.
fn membership(member: &OriginalSyncRoomMemberEvent) -> Option<SystemChange> {
    let previous = member
        .unsigned
        .prev_content
        .as_ref()
        .map(|prev| prev.membership.clone());
    let subject = || member.state_key.to_string();

    let change = match (previous, &member.content.membership) {
        // A rejoin reads the same as a first join: there is nothing loaded
        // here that remembers whether this account has seen this member
        // before, and the two sentences would say the same thing anyway.
        (prev, MembershipState::Join) if prev.as_ref() != Some(&MembershipState::Join) => {
            SystemChange::Joined { subject: subject() }
        }
        // Already a member, so this is a display name or avatar change, not
        // a membership change. Left undrawn on the same terms as every state
        // event this module still skips: see the header on [`system`].
        (Some(MembershipState::Join), MembershipState::Join) => return None,
        (_, MembershipState::Invite) => SystemChange::Invited { subject: subject() },
        (Some(MembershipState::Join), MembershipState::Leave) => {
            // Left on their own unless somebody else's hand is on the door:
            // a kick is also `membership: leave`, told apart only by whether
            // the sender is the member being removed.
            if member.sender == member.state_key {
                SystemChange::Left { subject: subject() }
            } else {
                SystemChange::Kicked { subject: subject() }
            }
        }
        (_, MembershipState::Ban) => SystemChange::Banned { subject: subject() },
        // A withdrawn or declined invite, a knock, a leave from anything
        // other than having joined, or a membership this build has no
        // variant for. None of them has a sentence yet.
        _ => return None,
    };

    Some(change)
}

fn read(event: &TimelineEvent, reading: Reading) -> Option<Message> {
    if event.kind.is_utd() {
        return undecryptable(event);
    }

    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(said)) =
        event.raw().deserialize().ok()?
    else {
        // Every state event and every reaction.
        return None;
    };

    let said = match said {
        SyncMessageLikeEvent::Original(said) => said,
        // Emptied by the homeserver. There is nothing left to read, which is
        // exactly what the mark says.
        SyncMessageLikeEvent::Redacted(gone) => return deleted(event, &gone),
    };

    // Before the body is read, because both of these have one and drawing it
    // is the thing being avoided. `Reply` is deliberately not matched: see the
    // header.
    let reply_to = answering(said.content.relates_to.as_ref());
    let mentions = said
        .content
        .mentions
        .map(|mentions| {
            mentions
                .user_ids
                .into_iter()
                .map(|user| user.to_string())
                .collect()
        })
        .unwrap_or_default();

    match said.content.relates_to {
        // An edit, wherever it turns up. It is a correction to a message
        // already drawn rather than a line of its own, and [`replacement`] is
        // what reads it.
        Some(Relation::Replacement(_)) => return None,
        // In the room it is half of a conversation happening elsewhere. In the
        // thread it is the conversation.
        Some(Relation::Thread(_)) if reading == Reading::Room => return None,
        _ => {}
    }

    // All three text types carry a `formatted_body`, and reading it for one of
    // them is how a bot's links arrive as literal angle brackets.
    let (kind, body, formatted, media) = match said.content.msgtype {
        MessageType::Text(text) => (MessageKind::Text, text.body, text.formatted, None),
        MessageType::Emote(emote) => (MessageKind::Emote, emote.body, emote.formatted, None),
        MessageType::Notice(notice) => (MessageKind::Notice, notice.body, notice.formatted, None),
        MessageType::Image(image) => {
            let info = image.info.as_deref();
            let media = attachment(
                &image.source,
                image.filename().to_owned(),
                None,
                info.and_then(|info| info.mimetype.as_deref()),
                info.and_then(|info| info.size),
                info.and_then(|info| info.width),
                info.and_then(|info| info.height),
            );
            drawable(
                MessageKind::Image,
                image.caption().unwrap_or_default().to_owned(),
                image.formatted_caption().cloned(),
                media,
            )
        }
        MessageType::Video(video) => {
            let info = video.info.as_deref();
            let media = attachment(
                &video.source,
                video.filename().to_owned(),
                info.and_then(|info| info.thumbnail_source.as_ref()),
                info.and_then(|info| info.mimetype.as_deref()),
                info.and_then(|info| info.size),
                info.and_then(|info| info.width),
                info.and_then(|info| info.height),
            );
            drawable(
                MessageKind::Video,
                video.caption().unwrap_or_default().to_owned(),
                video.formatted_caption().cloned(),
                media,
            )
        }
        // Neither is played and neither is looked at, so both hand over the
        // size and nothing about the type: what the sender claims a file is
        // has no business becoming a content type.
        MessageType::File(file) => {
            let media = attachment(
                &file.source,
                file.filename().to_owned(),
                None,
                None,
                file.info.as_deref().and_then(|info| info.size),
                None,
                None,
            );
            drawable(
                MessageKind::File,
                file.caption().unwrap_or_default().to_owned(),
                file.formatted_caption().cloned(),
                media,
            )
        }
        MessageType::Audio(audio) => {
            let media = attachment(
                &audio.source,
                audio.filename().to_owned(),
                None,
                None,
                audio.info.as_deref().and_then(|info| info.size),
                None,
                None,
            );
            drawable(
                MessageKind::Audio,
                audio.caption().unwrap_or_default().to_owned(),
                audio.formatted_caption().cloned(),
                media,
            )
        }
        _ => (
            MessageKind::Unsupported,
            NOT_SUPPORTED.to_owned(),
            None,
            None,
        ),
    };

    Some(Message {
        id: said.event_id.to_string(),
        sender: said.sender.to_string(),
        at: said.origin_server_ts.0.into(),
        // Neither of these is read off the event, because neither is on it.
        // What is on a message is put there when a timeline is published, out
        // of the annotations and the corrections the watcher is holding.
        reactions: Vec::new(),
        edited: false,
        // Only for a reply: the plaintext fallback and an ordinary markdown
        // quote are the same characters, so stripping unconditionally would
        // eat the first paragraph of anybody quoting somebody.
        body: if reply_to.is_some() {
            remove_plain_reply_fallback(&body).to_owned()
        } else {
            body
        },
        // `format` is an open string, and anything other than the one the
        // specification defines is somebody's extension that this build has no
        // way to read. The plaintext fallback is what it is for.
        html: formatted
            .filter(|formatted| formatted.format == MessageFormat::Html)
            .map(|formatted| without_quoted_reply(formatted.body)),
        media,
        thread: said.unsigned.relations.thread.as_deref().map(summary),
        reply_to,
        mentions,
        sender_trust: trust(event),
        // Nobody has, or this would not have deserialised as an original.
        deleted_by: None,
        kind,
    })
}

/// One homeserver's thread tally as the interface's.
///
/// Shared, because the two paths that build a message read the same bundle
/// from different places. Saturating rather than fallible: a badge that has
/// stopped counting beats a message that failed to draw.
fn summary(bundle: &BundledThread) -> ThreadSummary {
    ThreadSummary {
        count: u32::try_from(u64::from(bundle.count)).unwrap_or(u32::MAX),
        participated: bundle.current_user_participated,
    }
}

/// The part of `unsigned` that ruma's redacted view does not offer.
///
/// Deserialised by hand because [`RedactedUnsigned`] models `redacted_because`
/// and nothing else, while the aggregations are still on the wire: see
/// [`deleted`]. The field below is [`MessageLikeUnsigned`]'s own declaration
/// copied, same name, same `default`, same type.
///
/// [`RedactedUnsigned`]: matrix_sdk::ruma::events::RedactedUnsigned
/// [`MessageLikeUnsigned`]: matrix_sdk::ruma::events::MessageLikeUnsigned
#[derive(Deserialize)]
struct StillBundled {
    #[serde(rename = "m.relations", default)]
    relations: BundledMessageLikeRelations<AnySyncMessageLikeEvent>,
}

/// Which event a message is answering, if it chose one.
///
/// Not every `m.in_reply_to` is somebody answering: a threaded message carries
/// one as a fallback for clients that do not understand threads, and
/// `is_falling_back` is the flag that says which kind it is.
fn answering(
    relation: Option<&Relation<RoomMessageEventContentWithoutRelation>>,
) -> Option<String> {
    match relation? {
        Relation::Reply(reply) => Some(reply.in_reply_to.event_id.to_string()),
        Relation::Thread(thread) if !thread.is_falling_back => Some(
            thread
                .in_reply_to
                .as_ref()
                .map(|in_reply_to| in_reply_to.event_id.to_string())?,
        ),
        _ => None,
    }
}

/// A `formatted_body` with the rich reply fallback taken off the front.
///
/// The specification puts `<mx-reply>` at the very start and nowhere else, so
/// this is a slice rather than a parse. ruma's HTML half needs its own
/// sanitiser turned on, which would be a second allow-list to keep in step
/// with `FormattedBody`'s in the webview.
fn without_quoted_reply(html: String) -> String {
    const CLOSE: &str = "</mx-reply>";

    if !html.starts_with("<mx-reply") {
        return html;
    }
    match html.find(CLOSE) {
        Some(at) => html[at + CLOSE.len()..].to_owned(),
        None => html,
    }
}

/// What the interface needs to fetch, name and place one attachment.
///
/// `None` only when the source cannot be written down, which nothing a
/// homeserver sends produces. The caller then falls back to the line that says
/// this build cannot draw it.
fn attachment(
    source: &MediaSource,
    name: String,
    thumbnail: Option<&MediaSource>,
    mime: Option<&str>,
    size: Option<UInt>,
    width: Option<UInt>,
    height: Option<UInt>,
) -> Option<Media> {
    Some(Media {
        source: serde_json::to_string(source).ok()?,
        name,
        // A handle like the one above, so a thumbnail in an encrypted room
        // carries its own key. Dropped rather than raised if it will not
        // serialise: the card falls back to the filename.
        thumbnail: thumbnail.and_then(|source| serde_json::to_string(source).ok()),
        // The sender writes this and nothing checks it, and it is the type the
        // webview is handed for playback. Anything that is not one of the two
        // kinds this plays is dropped rather than repeated.
        mime: mime
            .filter(|mime| mime.starts_with("image/") || mime.starts_with("video/"))
            .map(str::to_owned),
        size: size.map(Into::into),
        width: width.map(Into::into),
        height: height.map(Into::into),
    })
}

/// An attachment as something to draw, or as the line that says otherwise.
fn drawable(
    kind: MessageKind,
    caption: String,
    formatted: Option<FormattedBody>,
    media: Option<Media>,
) -> (MessageKind, String, Option<FormattedBody>, Option<Media>) {
    match media {
        Some(media) => (kind, caption, formatted, Some(media)),
        None => (
            MessageKind::Unsupported,
            NOT_SUPPORTED.to_owned(),
            None,
            None,
        ),
    }
}

/// One `ShieldStateCode` under this crate's own name for it.
///
/// Here rather than beside the enum: `dto` names no SDK type on purpose.
impl From<ShieldStateCode> for SenderTrust {
    fn from(code: ShieldStateCode) -> Self {
        match code {
            ShieldStateCode::AuthenticityNotGuaranteed => Self::AuthenticityNotGuaranteed,
            ShieldStateCode::UnknownDevice => Self::UnknownDevice,
            ShieldStateCode::UnsignedDevice => Self::UnsignedDevice,
            ShieldStateCode::UnverifiedIdentity => Self::UnverifiedIdentity,
            ShieldStateCode::VerificationViolation => Self::VerificationViolation,
            ShieldStateCode::MismatchedSender => Self::MismatchedSender,
        }
    }
}

/// Why this message's sender could not be vouched for, when they could not.
///
/// Lax, not strict: strict reddens a sender merely nobody has verified, which
/// is most of them. Grey goes with it, being mostly key backup.
fn trust(event: &TimelineEvent) -> Option<SenderTrust> {
    match event
        .encryption_info()?
        .verification_state
        .to_shield_state_lax()
    {
        ShieldState::Red { code, .. } => Some(code.into()),
        ShieldState::Grey { .. } | ShieldState::None => None,
    }
}

/// An encrypted event with no key for it, as something to draw.
///
/// Read out of the raw JSON field by field rather than deserialised, because
/// there is nothing to deserialise it into: the content is ciphertext, and the
/// only things outside it are the envelope fields below.
fn undecryptable(event: &TimelineEvent) -> Option<Message> {
    Some(Message {
        id: event.kind.parse_event_id()?.to_string(),
        sender: event.kind.parse_sender()?.to_string(),
        // Not `TimelineEvent::timestamp`, which is `None` for an event that
        // was stored before the SDK started keeping one. Reading the envelope
        // is the same answer without the hole.
        at: event
            .raw()
            .get_field::<u64>("origin_server_ts")
            .ok()
            .flatten()?,
        body: NO_KEY.to_owned(),
        html: None,
        media: None,
        thread: None,
        edited: false,
        reactions: Vec::new(),
        reply_to: None,
        mentions: Vec::new(),
        deleted_by: None,
        // No `EncryptionInfo` on an event that did not decrypt, and the mark
        // this draws already says the session has no key for it.
        sender_trust: None,
        kind: MessageKind::Undecryptable,
    })
}

/// The mark left where a redacted message was.
///
/// Built out of the envelope, which a redaction leaves alone: the event ID,
/// who wrote it and when. The body is empty and the words on screen are the
/// interface's, which is what lets the mark name a moderator.
///
/// `thread` is read off the raw JSON, because a redaction strips `content` and
/// leaves `unsigned` alone, so the homeserver goes on counting replies under a
/// deleted root while ruma's typed view drops the field: see [`StillBundled`].
///
/// Which conversation it was in is not recoverable. `m.relates_to` goes with
/// the content, so a deleted thread reply paged back in is drawn in the room.
fn deleted(
    event: &TimelineEvent,
    gone: &RedactedSyncMessageLikeEvent<RedactedRoomMessageEventContent>,
) -> Option<Message> {
    Some(Message {
        id: gone.event_id.to_string(),
        sender: gone.sender.to_string(),
        at: gone.origin_server_ts.0.into(),
        body: String::new(),
        html: None,
        media: None,
        // Absent for almost every mark, because a homeserver bundles nothing
        // onto a message nobody replied to. Unreadable `unsigned` is read the
        // same way: no door rather than a guess at one.
        thread: event
            .raw()
            .get_field::<StillBundled>("unsigned")
            .ok()
            .flatten()
            .and_then(|unsigned| unsigned.relations.thread)
            .as_deref()
            .map(summary),
        edited: false,
        reactions: Vec::new(),
        reply_to: None,
        mentions: Vec::new(),
        // Nothing survives to vouch for. The content is gone, so a shield on
        // the mark left behind would be a claim about an empty box.
        sender_trust: None,
        // One field off the raw redaction rather than a match over
        // `AnyRedactionEvent`, which is non-exhaustive. A homeserver that sent
        // no sender leaves it `None`, and the mark then names nobody.
        deleted_by: gone
            .unsigned
            .redacted_because
            .get_field::<String>("sender")
            .ok()
            .flatten(),
        kind: MessageKind::Deleted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::deserialized_responses::{
        AlgorithmInfo, EncryptionInfo, UnableToDecryptInfo, VerificationLevel, VerificationState,
    };
    use matrix_sdk::ruma::events::room::MediaSource;
    use matrix_sdk::ruma::serde::Raw;
    use serde_json::{Value, json};

    /// One event as a homeserver sends it.
    fn event(value: Value) -> TimelineEvent {
        TimelineEvent::from_plaintext(
            Raw::new(&value)
                .expect("the fixture is valid JSON")
                .cast_unchecked(),
        )
    }

    /// An ordinary message with `content` as its content.
    fn sent(content: Value) -> TimelineEvent {
        event(json!({
            "type": "m.room.message",
            "event_id": "$one:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": content,
        }))
    }

    /// The same, with the homeserver's own aggregations attached.
    fn sent_with_unsigned(content: Value, unsigned: Value) -> TimelineEvent {
        event(json!({
            "type": "m.room.message",
            "event_id": "$one:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": content,
            "unsigned": unsigned,
        }))
    }

    /// What a homeserver bundles onto a message somebody has replied to.
    fn thread_bundle(count: u64, participated: bool) -> Value {
        json!({
            "m.relations": {
                "m.thread": {
                    "latest_event": {
                        "type": "m.room.message",
                        "event_id": "$last:example.org",
                        "sender": "@grace:example.org",
                        "origin_server_ts": 1_700_000_100_000u64,
                        "content": { "msgtype": "m.text", "body": "the last word" },
                    },
                    "count": count,
                    "current_user_participated": participated,
                }
            }
        })
    }

    fn text(body: &str) -> Value {
        json!({ "msgtype": "m.text", "body": body })
    }

    /// One decrypted message, carrying the trust the crypto machine put on it.
    fn decrypted(state: VerificationState) -> TimelineEvent {
        TimelineEvent::from_decrypted(
            matrix_sdk::deserialized_responses::DecryptedRoomEvent {
                event: Raw::new(&json!({
                    "type": "m.room.message",
                    "event_id": "$one:example.org",
                    "room_id": "!room:example.org",
                    "sender": "@ada:example.org",
                    "origin_server_ts": 1_700_000_000_000u64,
                    "content": text("hello"),
                }))
                .expect("the fixture is valid JSON")
                .cast_unchecked(),
                encryption_info: std::sync::Arc::new(EncryptionInfo {
                    sender: matrix_sdk::ruma::user_id!("@ada:example.org").to_owned(),
                    sender_device: None,
                    forwarder: None,
                    algorithm_info: AlgorithmInfo::MegolmV1AesSha2 {
                        curve25519_key: "curve".to_owned(),
                        sender_claimed_keys: std::collections::BTreeMap::new(),
                        session_id: None,
                    },
                    verification_state: state,
                }),
                unsigned_encryption_info: None,
            },
            None,
        )
    }

    #[test]
    fn an_ordinary_message_comes_through_whole() {
        let said = message(&sent(text("hello"))).expect("a text message is a message");

        assert_eq!(said.id, "$one:example.org");
        assert_eq!(said.sender, "@ada:example.org");
        assert_eq!(said.body, "hello");
        assert_eq!(said.at, 1_700_000_000_000);
        assert_eq!(said.kind, MessageKind::Text);
    }

    #[test]
    fn a_formatted_message_keeps_the_html_the_sender_meant() {
        // The whole of what markdown is for. `body` is the plaintext fallback
        // and carries the hashes somebody typed; a client that drew only that
        // shows them their own syntax back.
        let said = message(&sent(json!({
            "msgtype": "m.text",
            "body": "### Heading",
            "format": "org.matrix.custom.html",
            "formatted_body": "<h3>Heading</h3>",
        })))
        .expect("a formatted message is a message");

        assert_eq!(said.body, "### Heading");
        assert_eq!(said.html.as_deref(), Some("<h3>Heading</h3>"));
    }

    #[test]
    fn a_message_nobody_formatted_carries_no_html() {
        assert_eq!(
            message(&sent(text("hello"))).and_then(|said| said.html),
            None
        );
    }

    #[test]
    fn a_format_this_build_does_not_know_is_left_for_the_fallback() {
        // `format` is an open string and `org.matrix.custom.html` is the only
        // value the specification gives, so anything else draws `body`.
        let said = message(&sent(json!({
            "msgtype": "m.text",
            "body": "plain enough",
            "format": "org.example.rtf",
            "formatted_body": "{\\rtf1}",
        })))
        .expect("an unknown format is still a message");

        assert_eq!(said.html, None);
        assert_eq!(said.body, "plain enough");
    }

    #[test]
    fn an_emote_and_a_notice_carry_their_formatting_too() {
        // Three message types have a `formatted_body` and reading it for one
        // of them is how a bot's links end up as literal angle brackets.
        let emote = message(&sent(json!({
            "msgtype": "m.emote",
            "body": "waves *slowly*",
            "format": "org.matrix.custom.html",
            "formatted_body": "waves <em>slowly</em>",
        })))
        .expect("an emote is a message");
        let notice = message(&sent(json!({
            "msgtype": "m.notice",
            "body": "build **failed**",
            "format": "org.matrix.custom.html",
            "formatted_body": "build <strong>failed</strong>",
        })))
        .expect("a notice is a message");

        assert_eq!(emote.html.as_deref(), Some("waves <em>slowly</em>"));
        assert_eq!(
            notice.html.as_deref(),
            Some("build <strong>failed</strong>")
        );
    }

    #[test]
    fn an_emote_is_kept_and_marked_as_one() {
        // `/me waves`. Drawn as an action rather than as speech, which the
        // interface can only do if the difference survives this far.
        let said = message(&sent(json!({ "msgtype": "m.emote", "body": "waves" })))
            .expect("an emote is a message");

        assert_eq!(said.kind, MessageKind::Emote);
        assert_eq!(said.body, "waves");
    }

    #[test]
    fn a_notice_is_kept_and_marked_as_one() {
        // What bots and bridges send, and the whole point of the type is that
        // it is not a person talking.
        let said = message(&sent(
            json!({ "msgtype": "m.notice", "body": "build failed" }),
        ))
        .expect("a notice is a message");

        assert_eq!(said.kind, MessageKind::Notice);
    }

    #[test]
    fn an_image_arrives_with_a_handle_to_fetch_it_by() {
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is a message");

        assert_eq!(said.kind, MessageKind::Image);
        let media = said.media.expect("an image has something to fetch");
        // The filename, which is what a reader is told when the picture will
        // not load and what a screen reader is told either way.
        assert_eq!(media.name, "screenshot.png");
        let source =
            serde_json::from_str::<MediaSource>(&media.source).expect("a handle is a source");
        assert!(
            matches!(source, MediaSource::Plain(uri) if uri == "mxc://example.org/abc"),
            "the handle must name the picture it was made from"
        );
    }

    #[test]
    fn an_image_carries_the_shape_it_will_be_drawn_at() {
        // So the room can hold the space before the bytes arrive. Without it
        // every picture that loads shoves the conversation under it downwards,
        // which in a room that follows the bottom is the whole view moving.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
            "info": { "mimetype": "image/png", "size": 1234, "w": 800, "h": 600 },
        })))
        .expect("an image is a message");

        let media = said.media.expect("an image has something to fetch");
        assert_eq!(media.mime.as_deref(), Some("image/png"));
        assert_eq!(media.size, Some(1234));
        assert_eq!(media.width, Some(800));
        assert_eq!(media.height, Some(600));
    }

    #[test]
    fn an_image_that_says_nothing_about_itself_is_still_an_image() {
        // `info` is optional, and a bridge that omits it is common.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is a message");

        let media = said.media.expect("an image has something to fetch");
        assert_eq!(media.mime, None);
        assert_eq!(media.width, None);
    }

    #[test]
    fn an_encrypted_image_carries_what_it_takes_to_open_it() {
        // The ordinary case in this client, because the rooms are encrypted.
        // The handle is the whole `MediaSource` rather than a URI precisely so
        // that this works without a second shape for it.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "file": {
                "url": "mxc://example.org/sealed",
                "key": {
                    "kty": "oct",
                    "key_ops": ["encrypt", "decrypt"],
                    "alg": "A256CTR",
                    "k": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    "ext": true,
                },
                "iv": "AAAAAAAAAAAAAAAAAAAAAA",
                "hashes": { "sha256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" },
                "v": "v2",
            },
        })))
        .expect("an encrypted image is a message");

        let media = said
            .media
            .expect("an encrypted image has something to fetch");
        let source =
            serde_json::from_str::<MediaSource>(&media.source).expect("a handle is a source");
        assert!(
            matches!(source, MediaSource::Encrypted(_)),
            "an encrypted image must not lose its key on the way to the interface"
        );
    }

    #[test]
    fn a_clip_carries_its_thumbnail_as_a_second_handle() {
        // A clip is not fetched until somebody asks for one, so this is the
        // only thing there is to draw in its place. Without it the card is a
        // black rectangle and a filename.
        let said = message(&sent(json!({
            "msgtype": "m.video",
            "body": "clip.mp4",
            "url": "mxc://example.org/reel",
            "info": {
                "mimetype": "video/mp4",
                "thumbnail_url": "mxc://example.org/still",
                "thumbnail_info": { "mimetype": "image/jpeg", "w": 320, "h": 180 },
            },
        })))
        .expect("a video is a message");

        let thumbnail = said
            .media
            .expect("a video has something to fetch")
            .thumbnail
            .expect("this one has a still as well");

        assert!(
            thumbnail.contains("mxc://example.org/still"),
            "the thumbnail handle must name the still it was made from"
        );
    }

    #[test]
    fn a_clip_with_no_thumbnail_carries_none() {
        // Plenty of senders upload no still, and inventing one would mean
        // fetching the clip to make it, which is the download the card exists
        // to postpone.
        let said = message(&sent(json!({
            "msgtype": "m.video",
            "body": "clip.mp4",
            "url": "mxc://example.org/reel",
        })))
        .expect("a video is a message");

        assert_eq!(said.media.expect("still a video").thumbnail, None);
    }

    #[test]
    fn a_picture_is_its_own_thumbnail() {
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is a message");

        assert_eq!(said.media.expect("still an image").thumbnail, None);
    }

    #[test]
    fn a_video_is_an_attachment_on_the_same_terms() {
        let said = message(&sent(json!({
            "msgtype": "m.video",
            "body": "clip.mp4",
            "url": "mxc://example.org/reel",
            "info": { "mimetype": "video/mp4", "size": 9_000, "w": 1920, "h": 1080 },
        })))
        .expect("a video is a message");

        assert_eq!(said.kind, MessageKind::Video);
        let media = said.media.expect("a video has something to fetch");
        assert_eq!(media.mime.as_deref(), Some("video/mp4"));
        assert_eq!(media.height, Some(1080));
    }

    #[test]
    fn a_type_the_sender_claims_that_is_neither_is_left_off() {
        // The sender writes this field and nothing checks it. It reaches the
        // webview as the type of a blob, so the one thing it must never say is
        // something the browser would treat as a document.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
            "info": { "mimetype": "text/html" },
        })))
        .expect("an image is a message");

        assert_eq!(said.media.expect("still an image").mime, None);
    }

    #[test]
    fn a_file_is_something_to_save() {
        let said = message(&sent(json!({
            "msgtype": "m.file",
            "body": "accounts.ods",
            "url": "mxc://example.org/sheet",
        })))
        .expect("a file is something to draw");

        assert_eq!(said.kind, MessageKind::File);
        let media = said.media.expect("a file has something to fetch");
        assert_eq!(media.name, "accounts.ods");
    }

    #[test]
    fn a_voice_note_is_a_card_on_the_same_terms() {
        let said = message(&sent(json!({
            "msgtype": "m.audio",
            "body": "voice-message.ogg",
            "url": "mxc://example.org/spoken",
            "info": { "mimetype": "audio/ogg", "size": 40_000 },
        })))
        .expect("an audio message is something to draw");

        assert_eq!(said.kind, MessageKind::Audio);
        let media = said.media.expect("audio has something to fetch");
        assert_eq!(media.name, "voice-message.ogg");
        assert_eq!(media.size, Some(40_000));
    }

    #[test]
    fn a_message_type_this_build_draws_nothing_for_is_still_the_line_that_says_so() {
        // A location, a widget, somebody's extension. Drawn as itself rather
        // than vanishing: somebody whose message silently disappeared would
        // send it again.
        let said = message(&sent(json!({
            "msgtype": "m.location",
            "body": "Here",
            "geo_uri": "geo:51.5,-0.12",
        })))
        .expect("a location is still something to draw");

        assert_eq!(said.kind, MessageKind::Unsupported);
        assert_eq!(said.media, None);
        assert!(!said.body.is_empty());
    }

    #[test]
    fn the_words_sent_with_a_picture_are_kept() {
        // What the Lampshade bot does with a link: the clip and the quoted
        // post arrive in one event, so reading `body` as the filename threw
        // the post away and put it on the card as a name.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "Look at this",
            "filename": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("a captioned image is a message");

        assert_eq!(said.body, "Look at this");
        assert_eq!(said.media.expect("still an image").name, "screenshot.png");
    }

    #[test]
    fn a_picture_sent_with_no_words_carries_none() {
        // `filename` absent means `body` is the filename, which is not a
        // caption and must not be drawn as one.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is a message");

        assert!(said.body.is_empty());
    }

    #[test]
    fn a_filename_that_repeats_the_body_is_not_a_caption() {
        // The other half of the same rule. A client that writes both fields to
        // the same string has said nothing, and drawing the filename twice is
        // worse than drawing it once.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "filename": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is a message");

        assert!(said.body.is_empty());
    }

    #[test]
    fn a_caption_keeps_the_formatting_it_was_sent_with() {
        let said = message(&sent(json!({
            "msgtype": "m.video",
            "body": "Watch **this**",
            "filename": "clip.mp4",
            "format": "org.matrix.custom.html",
            "formatted_body": "Watch <strong>this</strong>",
            "url": "mxc://example.org/reel",
        })))
        .expect("a captioned video is a message");

        assert_eq!(said.html.as_deref(), Some("Watch <strong>this</strong>"));
    }

    #[test]
    fn a_message_with_replies_says_how_many() {
        // Counted by the homeserver and bundled onto the message, so a room
        // knows which of its messages are threads without asking about any of
        // them one at a time.
        let said = message(&sent_with_unsigned(
            text("what shall we call it"),
            thread_bundle(3, true),
        ))
        .expect("a text message is a message");

        let thread = said.thread.expect("the bundle names a thread");
        assert_eq!(thread.count, 3);
        assert!(thread.participated);
    }

    #[test]
    fn a_thread_nobody_here_joined_says_so() {
        let said = message(&sent_with_unsigned(
            text("what shall we call it"),
            thread_bundle(1, false),
        ))
        .expect("a text message is a message");

        assert!(!said.thread.expect("the bundle names a thread").participated);
    }

    #[test]
    fn a_message_nobody_replied_to_carries_no_thread() {
        // Absent rather than a count of zero. A message with no thread is not
        // a thread with nothing in it, and drawing "0 replies" under every
        // line in a room would say so on every one of them.
        assert_eq!(
            message(&sent(text("hello"))).expect("a message").thread,
            None
        );
    }

    #[test]
    fn a_thread_reply_is_kept_when_the_thread_is_what_is_being_read() {
        let threaded = sent(json!({
            "msgtype": "m.text",
            "body": "in the thread",
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": "$root:example.org",
            },
        }));

        assert_eq!(
            in_thread(&threaded).expect("a reply is the thread").body,
            "in the thread"
        );
    }

    #[test]
    fn an_edit_is_left_out_of_a_thread_too() {
        // Not a line in the panel, on the same terms as in the room. It is
        // read by `replacement` and folded onto the message it replaces.
        let edit = sent(json!({
            "msgtype": "m.text",
            "body": "* corrected",
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": "$earlier:example.org",
            },
        }));

        assert_eq!(in_thread(&edit), None);
    }

    #[test]
    fn a_thread_reply_is_left_out() {
        // Threads are not built. Drawing these inline would put half of two
        // conversations in one column with nothing to say which half belonged
        // to what.
        let threaded = sent(json!({
            "msgtype": "m.text",
            "body": "in the thread",
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": "$root:example.org",
            },
        }));

        assert_eq!(message(&threaded), None);
    }

    #[test]
    fn an_edit_is_left_out() {
        // Still not a line in the room. It carries a correction to something
        // already drawn, and a second copy of a corrected sentence is not what
        // the correction was for. [`replacement`] is what reads it.
        let edit = sent(json!({
            "msgtype": "m.text",
            "body": "* corrected",
            "m.new_content": { "msgtype": "m.text", "body": "corrected" },
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": "$original:example.org",
            },
        }));

        assert_eq!(message(&edit), None);
    }

    /// An edit of `target`, as a homeserver sends one.
    fn edit_of(target: &str, new_content: Value) -> TimelineEvent {
        sent(json!({
            "msgtype": "m.text",
            "body": "* the fallback nothing should read",
            "m.new_content": new_content,
            "m.relates_to": { "rel_type": "m.replace", "event_id": target },
        }))
    }

    #[test]
    fn a_replacement_carries_the_new_text() {
        let edit = replacement(&edit_of("$original:example.org", text("corrected")))
            .expect("an m.replace is a replacement");

        assert_eq!(edit.event_id, "$one:example.org");
        assert_eq!(edit.target, "$original:example.org");
        assert_eq!(edit.sender, "@ada:example.org");
        assert_eq!(edit.at, 1_700_000_000_000);
        assert_eq!(edit.body, "corrected");
    }

    #[test]
    fn a_replacement_never_reads_the_asterisk_fallback() {
        // The top-level body is the copy a client that knows nothing about
        // edits draws, and convention prefixes it with `* `. Reading it is how
        // a client ends up with a stray asterisk in front of every correction.
        let edit = replacement(&edit_of("$original:example.org", text("corrected")))
            .expect("an m.replace is a replacement");

        assert_eq!(edit.body, "corrected");
    }

    #[test]
    fn a_replacement_carries_the_new_formatting() {
        let edit = replacement(&edit_of(
            "$original:example.org",
            json!({
                "msgtype": "m.text",
                "body": "corrected",
                "format": "org.matrix.custom.html",
                "formatted_body": "<em>corrected</em>",
            }),
        ))
        .expect("an m.replace is a replacement");

        assert_eq!(edit.html.as_deref(), Some("<em>corrected</em>"));
    }

    #[test]
    fn a_replacement_that_dropped_the_formatting_says_so() {
        // `None` rather than absent-and-therefore-unchanged. Somebody who
        // edits a formatted message down to plain text gets the old HTML drawn
        // for ever if the fold merges instead of replacing.
        let edit = replacement(&edit_of("$original:example.org", text("plain now")))
            .expect("an m.replace is a replacement");

        assert_eq!(edit.html, None);
    }

    #[test]
    fn a_replacement_of_a_threaded_message_is_still_a_replacement() {
        // An edit inside a thread relates by `m.replace`, not by `m.thread`,
        // because the relation slot holds one thing. A reader that looked for
        // the thread relation would drop every correction made in a panel.
        let edit = replacement(&edit_of("$in a thread:example.org", text("corrected")))
            .expect("an m.replace is a replacement");

        assert_eq!(edit.target, "$in a thread:example.org");
    }

    #[test]
    fn an_edit_of_an_attachments_caption_is_left_alone_for_now() {
        // Deliberate rather than missed: nothing in this build offers caption
        // editing, so a caption stays as it was sent rather than half
        // following an edit from another client. See #74's out of scope.
        let edit = sent(json!({
            "msgtype": "m.image",
            "body": "* a new caption",
            "url": "mxc://example.org/one",
            "m.new_content": {
                "msgtype": "m.image",
                "body": "a new caption",
                "url": "mxc://example.org/one",
            },
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": "$original:example.org",
            },
        }));

        assert!(replacement(&edit).is_none());
    }

    /// One `m.reaction` on `$one`, with `content` as its content.
    fn reaction(content: Value) -> TimelineEvent {
        event(json!({
            "type": "m.reaction",
            "event_id": "$reacted:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": content,
        }))
    }

    /// An annotation of `$one` with `key`, plus whatever else `extra` holds.
    fn annotating(key: &str, extra: Value) -> Value {
        let mut content = json!({
            "m.relates_to": {
                "rel_type": "m.annotation",
                "event_id": "$one:example.org",
                "key": key,
            },
        });
        for (name, value) in extra.as_object().expect("the fixture is an object") {
            content[name] = value.clone();
        }
        content
    }

    #[test]
    fn an_ordinary_reaction_carries_no_shortcode() {
        // Every reaction anybody has ever sent with a keyboard. There is
        // nothing to name: the key is the character.
        let read = annotation(&reaction(annotating("\u{1f44d}", json!({}))))
            .expect("a reaction is an annotation");

        assert_eq!(read.key, "\u{1f44d}");
        assert_eq!(read.shortcode, None);
    }

    #[test]
    fn a_custom_emoji_reaction_carries_the_name_its_sender_wrote() {
        // MSC2545 keys one by the image's own URI, which reads out as a URI
        // unless whoever sent it also said what it is called. Cinny writes
        // this field and it is the only local answer there is.
        let read = annotation(&reaction(annotating(
            "mxc://example.org/cat",
            json!({ "shortcode": "blobcat" }),
        )))
        .expect("a reaction is an annotation");

        assert_eq!(read.shortcode.as_deref(), Some("blobcat"));
    }

    #[test]
    fn a_blank_shortcode_is_no_shortcode_at_all() {
        // Worse than absent: an empty label on a pill reads as a pill with
        // nothing in it rather than falling back to saying what it is.
        let read = annotation(&reaction(annotating(
            "mxc://example.org/cat",
            json!({ "shortcode": "   " }),
        )))
        .expect("a reaction is an annotation");

        assert_eq!(read.shortcode, None);
    }

    #[test]
    fn a_shortcode_longer_than_a_shortcode_may_be_is_refused() {
        // Anybody in the room can send a reaction, and its content is theirs
        // to write. The name becomes a pill's label and its tooltip, so a
        // kilobyte of it is a kilobyte read out to whoever is listening.
        // MSC2545's own ceiling is 100 bytes.
        let read = annotation(&reaction(annotating(
            "mxc://example.org/cat",
            json!({ "shortcode": "a".repeat(101) }),
        )))
        .expect("the reaction is still an annotation");

        assert_eq!(read.shortcode, None);
    }

    #[test]
    fn a_shortcode_of_exactly_the_longest_allowed_is_kept() {
        // The fixture guard for the one above: without it that test passes
        // whether the bound is 100 or zero.
        let read = annotation(&reaction(annotating(
            "mxc://example.org/cat",
            json!({ "shortcode": "a".repeat(100) }),
        )))
        .expect("the reaction is still an annotation");

        assert_eq!(read.shortcode.as_deref(), Some("a".repeat(100).as_str()));
    }

    #[test]
    fn a_shortcode_that_is_not_a_name_is_ignored_rather_than_refused() {
        // Anybody can put anything in an event. A number there must not cost
        // the reaction itself, which is what reading the whole content as a
        // typed shape would do.
        let read = annotation(&reaction(annotating(
            "mxc://example.org/cat",
            json!({ "shortcode": 7 }),
        )))
        .expect("the reaction is still an annotation");

        assert_eq!(read.shortcode, None);
    }

    #[test]
    fn an_ordinary_message_is_not_a_replacement() {
        assert!(replacement(&sent(text("hello"))).is_none());
    }

    #[test]
    fn a_reaction_is_not_a_replacement() {
        let reacted = event(json!({
            "type": "m.reaction",
            "event_id": "$reacted:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$one:example.org",
                    "key": "\u{1f44d}",
                },
            },
        }));

        assert!(replacement(&reacted).is_none());
    }

    #[test]
    fn an_edit_with_no_new_content_is_nothing_to_fold() {
        // A client that sent only the `* ` fallback. There is no replacement
        // text in it that is not the fallback, and guessing at one would draw
        // the asterisk.
        let edit = sent(json!({
            "msgtype": "m.text",
            "body": "* corrected",
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": "$original:example.org",
            },
        }));

        assert!(replacement(&edit).is_none());
    }

    #[test]
    fn a_plain_reply_is_kept() {
        // Unlike a thread reply, a reply is a whole message that happens to
        // name another one, and it reads correctly on its own.
        let reply = sent(json!({
            "msgtype": "m.text",
            "body": "> <@ada:example.org> quoted\n\nagreed",
            "m.relates_to": {
                "m.in_reply_to": { "event_id": "$original:example.org" },
            },
        }));

        assert_eq!(
            message(&reply).map(|said| said.body),
            Some("agreed".to_owned())
        );
    }

    #[test]
    fn a_reply_says_what_it_is_answering() {
        let reply = sent(json!({
            "msgtype": "m.text",
            "body": "agreed",
            "m.relates_to": {
                "m.in_reply_to": { "event_id": "$original:example.org" },
            },
        }));

        assert_eq!(
            message(&reply).and_then(|said| said.reply_to),
            Some("$original:example.org".to_owned())
        );
    }

    #[test]
    fn a_reply_loses_the_quote_it_was_sent_with() {
        // The fallback exists for clients that draw no reply of their own, so
        // leaving it in would show the quoted message twice.
        let reply = sent(json!({
            "msgtype": "m.text",
            "body": "> <@ada:example.org> the original\n\nagreed",
            "format": "org.matrix.custom.html",
            "formatted_body": "<mx-reply><blockquote>\
        <a href=\"https://matrix.to/#/!room:example.org/$original:example.org\">In reply to</a> \
        <a href=\"https://matrix.to/#/@ada:example.org\">@ada:example.org</a><br>the original\
        </blockquote></mx-reply>agreed",
            "m.relates_to": {
                "m.in_reply_to": { "event_id": "$original:example.org" },
            },
        }));

        let said = message(&reply).expect("a reply is a message");
        assert_eq!(said.body, "agreed");
        assert_eq!(said.html.as_deref(), Some("agreed"));
    }

    #[test]
    fn a_quote_that_is_not_a_fallback_is_left_alone() {
        // The plaintext fallback and a markdown quote are the same characters,
        // and only the relation tells them apart. Stripping one that is not a
        // reply would eat the first paragraph of anybody quoting somebody.
        let quoting = sent(json!({
            "msgtype": "m.text",
            "body": "> <@ada:example.org> said this\n\nand I agree",
        }));

        assert_eq!(
            message(&quoting).map(|said| said.body),
            Some("> <@ada:example.org> said this\n\nand I agree".to_owned())
        );
    }

    #[test]
    fn a_thread_reply_answering_nothing_in_particular_says_so() {
        // Every threaded message carries an `m.in_reply_to` pointing at the
        // last thing said, and `is_falling_back` is what says it is that
        // rather than somebody answering a particular message.
        let threaded = sent(json!({
            "msgtype": "m.text",
            "body": "in the thread",
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": "$root:example.org",
                "is_falling_back": true,
                "m.in_reply_to": { "event_id": "$latest:example.org" },
            },
        }));

        assert_eq!(in_thread(&threaded).and_then(|said| said.reply_to), None);
    }

    #[test]
    fn a_thread_reply_answering_a_particular_message_keeps_it() {
        let threaded = sent(json!({
            "msgtype": "m.text",
            "body": "answering you",
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": "$root:example.org",
                "is_falling_back": false,
                "m.in_reply_to": { "event_id": "$earlier:example.org" },
            },
        }));

        assert_eq!(
            in_thread(&threaded).and_then(|said| said.reply_to),
            Some("$earlier:example.org".to_owned())
        );
    }

    #[test]
    fn a_state_event_is_not_a_message() {
        // Most of a timeline is these: joins, leaves, renames, topic changes,
        // and every call membership Consort writes itself.
        let joined = event(json!({
            "type": "m.room.member",
            "event_id": "$join:example.org",
            "sender": "@ada:example.org",
            "state_key": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": { "membership": "join" },
        }));

        assert_eq!(message(&joined), None);
    }

    #[test]
    fn a_reaction_is_not_a_message() {
        let reacted = event(json!({
            "type": "m.reaction",
            "event_id": "$react:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$one:example.org",
                    "key": "👍",
                },
            },
        }));

        assert_eq!(message(&reacted), None);
    }

    /// A message the homeserver has emptied, redacted by `by`.
    fn emptied(by: Value) -> TimelineEvent {
        event(json!({
            "type": "m.room.message",
            "event_id": "$gone:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": {},
            "unsigned": { "redacted_because": by },
        }))
    }

    /// The same, with the thread summary a homeserver still bundles onto it.
    ///
    /// Synapse computes the aggregation when it serialises the event, so a
    /// deleted root arrives looking exactly like this. Checked against a real
    /// one in `a_redacted_thread_root_still_carries_its_count`.
    fn emptied_in_a_thread(by: Value, count: u64, participated: bool) -> TimelineEvent {
        let mut unsigned = thread_bundle(count, participated);
        unsigned["redacted_because"] = by;

        event(json!({
            "type": "m.room.message",
            "event_id": "$gone:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": {},
            "unsigned": unsigned,
        }))
    }

    /// One `m.room.redaction` as it appears in `redacted_because`.
    fn because(sender: &str) -> Value {
        json!({
            "type": "m.room.redaction",
            "event_id": "$redaction:example.org",
            "sender": sender,
            "origin_server_ts": 1_700_000_000_001u64,
            "content": {},
        })
    }

    #[test]
    fn a_redacted_message_is_drawn_as_a_mark() {
        // The gap is the thing being avoided, here and in the two tests below.
        // Every other client draws a mark, so closing over it would make one
        // room look like two depending on what it was opened in.
        let gone = message(&emptied(because("@ada:example.org")))
            .expect("a redacted message is still drawn");

        assert_eq!(gone.kind, MessageKind::Deleted);
        // The envelope survives a redaction, which is what this is built from.
        assert_eq!(gone.sender, "@ada:example.org");
        assert_eq!(gone.at, 1_700_000_000_000);
        // And nothing else is invented. The words on screen belong to the
        // interface, which is also what lets it name a moderator by the
        // display name this side has never heard of.
        assert_eq!(gone.body, "");
        assert_eq!(gone.html, None);
        assert_eq!(gone.media, None);
    }

    #[test]
    fn a_mark_carries_who_did_it() {
        // A moderator removing somebody's message and that person removing
        // their own are one event type with a different sender on it.
        let gone = message(&emptied(because("@mod:example.org"))).unwrap();

        assert_eq!(gone.deleted_by.as_deref(), Some("@mod:example.org"));
        assert_eq!(gone.sender, "@ada:example.org");
    }

    #[test]
    fn a_mark_names_nobody_when_the_redaction_does_not() {
        // `redacted_because` is the homeserver's to fill in. Unknown is read
        // as unknown rather than as the author, because of the two readings
        // only one can say something untrue.
        let gone = message(&emptied(json!({ "type": "m.room.redaction" }))).unwrap();

        assert_eq!(gone.deleted_by, None);
        assert_eq!(gone.kind, MessageKind::Deleted);
    }

    #[test]
    fn a_mark_keeps_the_way_into_the_thread_hanging_from_it() {
        // The replies are not redacted and the homeserver still counts them,
        // so the count is on the wire and the room keeps the one control that
        // opens the panel.
        let gone = message(&emptied_in_a_thread(because("@ada:example.org"), 3, true)).unwrap();

        assert_eq!(gone.kind, MessageKind::Deleted);
        assert_eq!(
            gone.thread,
            Some(ThreadSummary {
                count: 3,
                participated: true,
            })
        );
    }

    #[test]
    fn a_mark_with_no_replies_under_it_has_no_door_to_draw() {
        // A homeserver bundles nothing onto a message nobody replied to, so
        // absence here is the ordinary case rather than a read that failed.
        let gone = message(&emptied(because("@ada:example.org"))).unwrap();

        assert_eq!(gone.thread, None);
    }

    #[test]
    fn a_count_too_large_to_draw_stops_at_the_largest_one_that_is() {
        // The same saturating read as the path an unredacted root takes, and
        // the same reason: a badge that has stopped counting beats a mark
        // that failed to draw. One rule, used from both ends.
        let gone = message(&emptied_in_a_thread(
            because("@ada:example.org"),
            u64::from(u32::MAX) + 1,
            false,
        ))
        .unwrap();

        assert_eq!(gone.thread.map(|thread| thread.count), Some(u32::MAX));
    }

    #[test]
    fn a_redaction_says_what_it_removes_and_who_removed_it() {
        let removal = event(json!({
            "type": "m.room.redaction",
            "event_id": "$redaction:example.org",
            "sender": "@mod:example.org",
            "origin_server_ts": 1_700_000_000_001u64,
            "redacts": "$gone:example.org",
            "content": {},
        }));

        let read = redaction(&removal).expect("a redaction is one");
        assert_eq!(read.event_id, "$gone:example.org");
        assert_eq!(read.sender, "@mod:example.org");
    }

    #[test]
    fn a_redaction_that_names_its_target_in_the_content_is_read_too() {
        // Room 11 moved `redacts` inside the content. Reading only the top
        // level would leave every redaction in a modern room unswept.
        let removal = event(json!({
            "type": "m.room.redaction",
            "event_id": "$redaction:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_001u64,
            "content": { "redacts": "$gone:example.org" },
        }));

        assert_eq!(
            redaction(&removal).map(|read| read.event_id),
            Some("$gone:example.org".to_owned())
        );
    }

    #[test]
    fn an_event_this_session_has_no_key_for_is_still_drawn() {
        // The one that matters most: a gap that says nothing about itself is
        // indistinguishable from a quiet room.
        let encrypted = TimelineEvent::from_utd(
            Raw::new(&json!({
                "type": "m.room.encrypted",
                "event_id": "$sealed:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "ciphertext": "AwgAEnB...",
                    "session_id": "session",
                },
            }))
            .expect("the fixture is valid JSON")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("session".to_owned()),
                reason: matrix_sdk::deserialized_responses::UnableToDecryptReason::MissingMegolmSession {
                    withheld_code: None,
                },
            },
        );

        let said = message(&encrypted).expect("an unreadable message is still a message");

        assert_eq!(said.kind, MessageKind::Undecryptable);
        assert_eq!(said.sender, "@bob:example.org");
        assert_eq!(said.at, 1_700_000_000_000);
        assert!(!said.body.is_empty());
    }

    #[test]
    fn an_unreadable_message_reads_as_a_wait_rather_than_a_failure() {
        // A room that was quiet while this session was away is a screen full
        // of these, and a screen of sentences beginning "cannot" reads as a
        // broken client rather than as a key that has not arrived.
        let encrypted = TimelineEvent::from_utd(
            Raw::new(&json!({
                "type": "m.room.encrypted",
                "event_id": "$sealed:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "ciphertext": "AwgAEnB...",
                    "session_id": "session",
                },
            }))
            .expect("the fixture is valid JSON")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("session".to_owned()),
                reason: matrix_sdk::deserialized_responses::UnableToDecryptReason::MissingMegolmSession {
                    withheld_code: None,
                },
            },
        );

        let body = message(&encrypted)
            .expect("an unreadable message is still a message")
            .body;

        assert!(body.contains("Waiting"), "{body}");
        assert!(!body.to_lowercase().contains("cannot"), "{body}");
        assert!(body.len() < 40, "{body}");
    }

    #[test]
    fn something_that_is_not_an_event_at_all_is_not_a_message() {
        // A homeserver sending something this build cannot parse must not take
        // the room's whole timeline with it.
        assert_eq!(message(&event(json!({ "type": "m.room.message" }))), None);
    }

    #[test]
    fn a_message_says_who_it_names() {
        let said = sent(json!({
            "msgtype": "m.text",
            "body": "bragoodle: have a look",
            "m.mentions": { "user_ids": ["@bragoodle:example.org"] },
        }));

        assert_eq!(
            message(&said).map(|said| said.mentions),
            Some(vec!["@bragoodle:example.org".to_owned()])
        );
    }

    #[test]
    fn a_message_naming_nobody_names_nobody() {
        let said = sent(json!({ "msgtype": "m.text", "body": "morning" }));

        assert_eq!(message(&said).map(|said| said.mentions), Some(vec![]));
    }

    #[test]
    fn a_room_mention_is_not_a_person() {
        // `m.mentions` carries a `room` flag beside the user IDs, and an @room
        // is about everybody, so nothing here invents a user ID for it.
        let said = sent(json!({
            "msgtype": "m.text",
            "body": "@room the server is going down",
            "m.mentions": { "room": true },
        }));

        assert_eq!(message(&said).map(|said| said.mentions), Some(vec![]));
    }

    /// One `m.room.member` event, with an optional `prev_content` on
    /// `unsigned` for the cases that turn on what came before.
    fn membership(
        sender: &str,
        state_key: &str,
        membership: &str,
        prev_membership: Option<&str>,
    ) -> TimelineEvent {
        let mut value = json!({
            "type": "m.room.member",
            "event_id": "$member:example.org",
            "sender": sender,
            "state_key": state_key,
            "origin_server_ts": 1_700_000_000_000u64,
            "content": { "membership": membership },
        });
        if let Some(prev) = prev_membership {
            value["unsigned"] = json!({
                "prev_content": { "membership": prev },
            });
        }
        event(value)
    }

    #[test]
    fn a_first_join_is_a_join() {
        let said = system(&membership(
            "@ada:example.org",
            "@ada:example.org",
            "join",
            None,
        ))
        .expect("a first join is drawn");

        assert_eq!(said.id, "$member:example.org");
        assert_eq!(said.at, 1_700_000_000_000);
        assert_eq!(said.actor, "@ada:example.org");
        assert_eq!(
            said.change,
            SystemChange::Joined {
                subject: "@ada:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn a_rejoin_after_leaving_is_also_a_join() {
        // Nothing loaded here remembers whether this account has seen this
        // member before, and the two sentences would say the same thing
        // anyway.
        let said = system(&membership(
            "@ada:example.org",
            "@ada:example.org",
            "join",
            Some("leave"),
        ))
        .expect("a rejoin is drawn as a join");

        assert_eq!(
            said.change,
            SystemChange::Joined {
                subject: "@ada:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn a_display_name_change_is_not_a_membership_change() {
        // Already a member on both sides of this event: the only thing that
        // changed is the profile riding along on the membership event.
        assert!(
            system(&membership(
                "@ada:example.org",
                "@ada:example.org",
                "join",
                Some("join"),
            ))
            .is_none()
        );
    }

    #[test]
    fn being_invited_names_who_sent_it() {
        let said = system(&membership(
            "@ada:example.org",
            "@bragoodle:example.org",
            "invite",
            None,
        ))
        .expect("an invite is drawn");

        assert_eq!(said.actor, "@ada:example.org");
        assert_eq!(
            said.change,
            SystemChange::Invited {
                subject: "@bragoodle:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn leaving_your_own_way_out_is_a_leave() {
        let said = system(&membership(
            "@ada:example.org",
            "@ada:example.org",
            "leave",
            Some("join"),
        ))
        .expect("a self-leave is drawn");

        assert_eq!(
            said.change,
            SystemChange::Left {
                subject: "@ada:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn being_shown_the_door_by_somebody_else_is_a_kick() {
        let said = system(&membership(
            "@ada:example.org",
            "@bragoodle:example.org",
            "leave",
            Some("join"),
        ))
        .expect("a kick is drawn");

        assert_eq!(said.actor, "@ada:example.org");
        assert_eq!(
            said.change,
            SystemChange::Kicked {
                subject: "@bragoodle:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn a_ban_is_a_ban_whoever_it_was_aimed_at() {
        let said = system(&membership(
            "@ada:example.org",
            "@bragoodle:example.org",
            "ban",
            Some("join"),
        ))
        .expect("a ban is drawn");

        assert_eq!(
            said.change,
            SystemChange::Banned {
                subject: "@bragoodle:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn a_withdrawn_invite_has_no_sentence_yet() {
        // `leave` from `invite` rather than from `join`: nobody was ever a
        // member, so neither "left" nor "kicked" is true of them.
        assert!(
            system(&membership(
                "@ada:example.org",
                "@bragoodle:example.org",
                "leave",
                Some("invite"),
            ))
            .is_none()
        );
    }

    #[test]
    fn an_ordinary_message_is_not_a_membership_change() {
        assert!(system(&sent(text("hello"))).is_none());
    }

    /// One room state event, of whatever type, with whatever content.
    fn changed(kind: &str, content: serde_json::Value) -> TimelineEvent {
        event(json!({
            "type": kind,
            "event_id": "$changed:example.org",
            "sender": "@ada:example.org",
            "state_key": "",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": content,
        }))
    }

    #[test]
    fn renaming_the_room_says_what_it_is_called_now() {
        // The line issue #84 asked for by name: "tominal has updated the
        // room" is a room change, not a membership one.
        let said =
            system(&changed("m.room.name", json!({ "name": "tech" }))).expect("a rename is drawn");

        assert_eq!(said.id, "$changed:example.org");
        assert_eq!(said.at, 1_700_000_000_000);
        assert_eq!(said.actor, "@ada:example.org");
        assert_eq!(
            said.change,
            SystemChange::Renamed {
                name: Some("tech".to_owned()),
            }
        );
    }

    #[test]
    fn a_room_name_emptied_is_a_name_removed() {
        // A name is cleared by being set to "", not by the event being
        // redacted, so an empty one is a removal rather than a room called
        // nothing.
        let said = system(&changed("m.room.name", json!({ "name": "" })))
            .expect("a cleared name is drawn");

        assert_eq!(said.change, SystemChange::Renamed { name: None });
    }

    #[test]
    fn changing_the_topic_says_what_it_is_now() {
        let said = system(&changed("m.room.topic", json!({ "topic": "the build" })))
            .expect("a topic change is drawn");

        assert_eq!(
            said.change,
            SystemChange::TopicChanged {
                topic: Some("the build".to_owned()),
            }
        );
    }

    #[test]
    fn a_topic_emptied_is_a_topic_removed() {
        let said = system(&changed("m.room.topic", json!({ "topic": "" })))
            .expect("a cleared topic is drawn");

        assert_eq!(said.change, SystemChange::TopicChanged { topic: None });
    }

    #[test]
    fn changing_the_picture_carries_the_new_one() {
        let said = system(&changed(
            "m.room.avatar",
            json!({ "url": "mxc://example.org/tech" }),
        ))
        .expect("an avatar change is drawn");

        assert_eq!(
            said.change,
            SystemChange::AvatarChanged {
                url: Some("mxc://example.org/tech".to_owned()),
            }
        );
    }

    #[test]
    fn a_picture_removed_carries_no_url() {
        // `m.room.avatar` clears itself by leaving `url` out rather than by
        // emptying it, which is why this one variant needs no `or_cleared`.
        let said = system(&changed("m.room.avatar", json!({}))).expect("a removed avatar is drawn");

        assert_eq!(said.change, SystemChange::AvatarChanged { url: None });
    }

    #[test]
    fn the_wire_carries_the_kind_beside_the_actor_rather_than_under_it() {
        // `SystemChange` is flattened into `SystemMessage`, which is what lets
        // the TypeScript mirror discriminate on `kind`. Pinned because a
        // nested `change` would typecheck on both sides and draw nothing.
        let said =
            system(&changed("m.room.name", json!({ "name": "tech" }))).expect("a rename is drawn");

        assert_eq!(
            serde_json::to_value(&said).expect("a system message serialises"),
            json!({
                "id": "$changed:example.org",
                "at": 1_700_000_000_000u64,
                "actor": "@ada:example.org",
                "kind": "renamed",
                "name": "tech",
            })
        );
    }

    #[test]
    fn a_membership_change_still_carries_a_subject_on_the_wire() {
        let said = system(&membership(
            "@ada:example.org",
            "@bragoodle:example.org",
            "invite",
            None,
        ))
        .expect("an invite is drawn");

        assert_eq!(
            serde_json::to_value(&said).expect("a system message serialises"),
            json!({
                "id": "$member:example.org",
                "at": 1_700_000_000_000u64,
                "actor": "@ada:example.org",
                "kind": "invited",
                "subject": "@bragoodle:example.org",
            })
        );
    }

    #[test]
    fn a_state_event_with_no_sentence_yet_is_still_nothing() {
        // The set #84's "or otherwise" gestures at and this change does not
        // build. Undrawn on purpose, and this pins that they stay that way.
        for (kind, content) in [
            (
                "m.room.canonical_alias",
                json!({ "alias": "#tech:example.org" }),
            ),
            ("m.room.join_rules", json!({ "join_rule": "public" })),
            (
                "m.room.history_visibility",
                json!({ "history_visibility": "shared" }),
            ),
            ("m.room.power_levels", json!({ "users_default": 0 })),
        ] {
            assert!(
                system(&changed(kind, content)).is_none(),
                "{kind} should still be undrawn"
            );
        }
    }

    /// An `m.receipt` as a homeserver sends it, with `content` as its content.
    fn receipt_event(content: Value) -> Raw<AnySyncEphemeralRoomEvent> {
        Raw::new(&json!({ "type": "m.receipt", "content": content }))
            .expect("the fixture is valid JSON")
            .cast_unchecked()
    }

    /// One person's public receipt on one message, with whatever thread field.
    fn read_by(user: &str, event_id: &str, thread: Option<&str>) -> Value {
        let mut receipt = json!({ "ts": 1_700_000_000_000u64 });
        if let Some(thread) = thread {
            receipt["thread_id"] = json!(thread);
        }
        json!({ event_id: { "m.read": { user: receipt } } })
    }

    #[test]
    fn a_receipt_with_no_thread_is_about_the_room() {
        // The one almost every client sends, and the one this build sends.
        // A room that only understood "main" would draw nobody.
        let said = receipts(&receipt_event(read_by(
            "@ada:example.org",
            "$said:example.org",
            None,
        )));

        assert_eq!(
            said,
            Some(vec![Read {
                user: "@ada:example.org".to_owned(),
                event_id: "$said:example.org".to_owned(),
                about: About::Room,
            }])
        );
    }

    #[test]
    fn a_receipt_naming_the_main_timeline_is_also_about_the_room() {
        let said = receipts(&receipt_event(read_by(
            "@ada:example.org",
            "$said:example.org",
            Some("main"),
        )));

        assert_eq!(said.unwrap()[0].about, About::Room);
    }

    #[test]
    fn a_receipt_naming_a_thread_is_about_that_thread() {
        let said = receipts(&receipt_event(read_by(
            "@ada:example.org",
            "$said:example.org",
            Some("$root:example.org"),
        )));

        assert_eq!(
            said.unwrap()[0].about,
            About::Thread("$root:example.org".to_owned())
        );
    }

    #[test]
    fn a_thread_this_build_cannot_name_is_still_not_the_room() {
        // `thread_id` is an arbitrary string on the wire, and ruma hands back
        // anything that is neither "main" nor an event ID as a value of its
        // own. Read as the room, it puts a stranger's face against a message
        // they have not seen, which is the failure worth avoiding.
        let said = receipts(&receipt_event(read_by(
            "@ada:example.org",
            "$said:example.org",
            Some("something-from-a-later-specification"),
        )));

        assert_ne!(said.unwrap()[0].about, About::Room);
    }

    #[test]
    fn a_private_receipt_is_not_something_to_draw() {
        // Only `m.read` is visible to the room, so only `m.read` is drawn. The
        // private ones that reach this client are its own, and drawing one
        // would put this account's own face against its own message.
        let said = receipts(&receipt_event(json!({
            "$said:example.org": {
                "m.read.private": { "@ada:example.org": { "ts": 1_700_000_000_000u64 } }
            }
        })));

        assert_eq!(said, Some(Vec::new()));
    }

    #[test]
    fn a_fully_read_marker_riding_along_is_not_a_receipt() {
        // `m.fully_read` shares the event's shape and is not a claim about
        // anybody but the account that sent it.
        let said = receipts(&receipt_event(json!({
            "$said:example.org": {
                "m.fully_read": { "@ada:example.org": { "ts": 1_700_000_000_000u64 } }
            }
        })));

        assert_eq!(said, Some(Vec::new()));
    }

    #[test]
    fn everybody_named_in_one_receipt_event_is_reported() {
        // One event carries the whole batch: several people, several messages.
        let said = receipts(&receipt_event(json!({
            "$one:example.org": {
                "m.read": {
                    "@ada:example.org": { "ts": 1_700_000_000_000u64 },
                    "@bob:example.org": { "ts": 1_700_000_000_001u64 }
                }
            },
            "$two:example.org": {
                "m.read": { "@cleo:example.org": { "ts": 1_700_000_000_002u64 } }
            }
        })))
        .unwrap();

        let mut seen: Vec<(String, String)> = said
            .into_iter()
            .map(|one| (one.user, one.event_id))
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                ("@ada:example.org".to_owned(), "$one:example.org".to_owned()),
                ("@bob:example.org".to_owned(), "$one:example.org".to_owned()),
                (
                    "@cleo:example.org".to_owned(),
                    "$two:example.org".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn an_ephemeral_event_that_is_not_a_receipt_says_nothing() {
        let said = receipts(&receipt_event_of_kind("m.typing"));

        assert_eq!(said, None);
    }

    /// An ephemeral event of some other kind.
    fn receipt_event_of_kind(kind: &str) -> Raw<AnySyncEphemeralRoomEvent> {
        Raw::new(&json!({
            "type": kind,
            "content": { "user_ids": ["@ada:example.org"] },
        }))
        .expect("the fixture is valid JSON")
        .cast_unchecked()
    }

    #[test]
    fn a_message_from_a_device_its_owner_never_signed_says_so() {
        // Issue #133's case, and the only one the SDK's own wording calls a
        // client rather than a person: the sender never signed this device.
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::UnsignedDevice,
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, Some(SenderTrust::UnsignedDevice));
    }

    #[test]
    fn a_message_from_a_verified_device_says_nothing() {
        let said = message(&decrypted(VerificationState::Verified))
            .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, None);
    }

    #[test]
    fn a_sender_we_simply_never_verified_is_not_called_untrusted() {
        // The state that means we have not checked, not that anything is
        // wrong. Marking it would put a warning on nearly every message in a
        // room and say something false about all of them.
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::UnverifiedIdentity,
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, None);
    }

    #[test]
    fn a_key_from_an_insecure_source_is_not_called_untrusted_either() {
        // Grey in the SDK's mapping, not red: a key out of backup or an
        // unsafe forward is common and says nothing about the sender.
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::None(
                matrix_sdk::deserialized_responses::DeviceLinkProblem::InsecureSource,
            ),
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, None);
    }

    #[test]
    fn a_device_this_session_cannot_find_says_so() {
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::None(
                matrix_sdk::deserialized_responses::DeviceLinkProblem::MissingDevice,
            ),
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, Some(SenderTrust::UnknownDevice));
    }

    #[test]
    fn a_sender_who_was_verified_and_changed_identity_says_so() {
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::VerificationViolation,
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, Some(SenderTrust::VerificationViolation));
    }

    #[test]
    fn a_sender_who_does_not_own_the_session_says_so() {
        let said = message(&decrypted(VerificationState::Unverified(
            VerificationLevel::MismatchedSender,
        )))
        .expect("a decrypted message is a message");

        assert_eq!(said.sender_trust, Some(SenderTrust::MismatchedSender));
    }

    #[test]
    fn an_unencrypted_message_carries_no_trust_either_way() {
        // Nothing here knows whether the room was encrypted, so there is no
        // honest warning to give. See docs/ADR on this if it ever gains one.
        let said = message(&sent(text("hello"))).expect("a text message is a message");

        assert_eq!(said.sender_trust, None);
    }

    #[test]
    fn an_undecryptable_message_carries_no_trust() {
        // It already draws as `Undecryptable`, and a shield beside that mark
        // would be the same fact twice.
        let encrypted = TimelineEvent::from_utd(
            Raw::new(&json!({
                "type": "m.room.encrypted",
                "event_id": "$sealed:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "ciphertext": "AwgAEnB...",
                    "session_id": "session",
                },
            }))
            .expect("the fixture is valid JSON")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("session".to_owned()),
                reason:
                    matrix_sdk::deserialized_responses::UnableToDecryptReason::MissingMegolmSession {
                        withheld_code: None,
                    },
            },
        );

        let said = message(&encrypted).expect("an unreadable message is still a message");

        assert_eq!(said.kind, MessageKind::Undecryptable);
        assert_eq!(said.sender_trust, None);
    }

    #[test]
    fn an_edit_carries_the_trust_of_the_device_that_sent_the_edit() {
        // The words on screen come from the edit, so the trust shown has to as
        // well. An edit from an unsigned device onto a message from a verified
        // one is the case that would otherwise draw nothing.
        let decrypted_edit = TimelineEvent::from_decrypted(
            matrix_sdk::deserialized_responses::DecryptedRoomEvent {
                event: Raw::new(&json!({
                    "type": "m.room.message",
                    "event_id": "$edit:example.org",
                    "room_id": "!room:example.org",
                    "sender": "@ada:example.org",
                    "origin_server_ts": 1_700_000_100_000u64,
                    "content": {
                        "msgtype": "m.text",
                        "body": "* corrected",
                        "m.new_content": text("corrected"),
                        "m.relates_to": {
                            "rel_type": "m.replace",
                            "event_id": "$original:example.org",
                        },
                    },
                }))
                .expect("the fixture is valid JSON")
                .cast_unchecked(),
                encryption_info: std::sync::Arc::new(EncryptionInfo {
                    sender: matrix_sdk::ruma::user_id!("@ada:example.org").to_owned(),
                    sender_device: None,
                    forwarder: None,
                    algorithm_info: AlgorithmInfo::MegolmV1AesSha2 {
                        curve25519_key: "curve".to_owned(),
                        sender_claimed_keys: std::collections::BTreeMap::new(),
                        session_id: None,
                    },
                    verification_state: VerificationState::Unverified(
                        VerificationLevel::UnsignedDevice,
                    ),
                }),
                unsigned_encryption_info: None,
            },
            None,
        );

        let edit = replacement(&decrypted_edit).expect("that is a replacement");

        assert_eq!(edit.sender_trust, Some(SenderTrust::UnsignedDevice));
    }

    #[test]
    fn an_edit_from_a_device_nothing_is_wrong_with_carries_no_trust() {
        let edit = replacement(&edit_of("$original:example.org", text("corrected")))
            .expect("that is a replacement");

        assert_eq!(edit.sender_trust, None);
    }

    #[test]
    fn every_code_the_sdk_can_raise_has_a_name_here() {
        // Total over `ShieldStateCode` on purpose. An SDK bump that adds a
        // code fails to compile here rather than drawing nothing.
        for (code, named) in [
            (
                ShieldStateCode::AuthenticityNotGuaranteed,
                SenderTrust::AuthenticityNotGuaranteed,
            ),
            (ShieldStateCode::UnknownDevice, SenderTrust::UnknownDevice),
            (ShieldStateCode::UnsignedDevice, SenderTrust::UnsignedDevice),
            (
                ShieldStateCode::UnverifiedIdentity,
                SenderTrust::UnverifiedIdentity,
            ),
            (
                ShieldStateCode::VerificationViolation,
                SenderTrust::VerificationViolation,
            ),
            (
                ShieldStateCode::MismatchedSender,
                SenderTrust::MismatchedSender,
            ),
        ] {
            assert_eq!(SenderTrust::from(code), named);
        }
    }
}
