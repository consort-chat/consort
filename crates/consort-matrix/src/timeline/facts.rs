// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning one timeline event into one message, or into nothing.
//!
//! The only part of this module that has to know what an SDK event looks like.
//! It needs no `Client`, no room and no network: a `TimelineEvent` can be built
//! from JSON, so every rule below is tested against the exact bytes a
//! homeserver sends rather than against a shape somebody assumed.
//!
//! ## Most events are not messages
//!
//! A room's timeline carries joins, leaves, renames, topic changes, avatar
//! changes, power level edits, reactions, receipts and call membership. None of
//! them is drawn here, and `None` is the ordinary answer rather than a failure.
//!
//! ## What is deliberately dropped
//!
//! Thread replies, until threads are built. They arrive in the main timeline
//! and every other client keeps them out of it, so drawing them inline would
//! put half of two conversations in one column with nothing to say which half
//! belonged to what.
//!
//! Edits, on the same terms. An `m.replace` carries the new text and the
//! interface has no way to attach it to the message it replaces, so drawing it
//! inline would show a room a second copy of a sentence somebody corrected.
//!
//! Replies are **not** dropped. A reply is a whole message that happens to
//! name another one, and it reads correctly on its own; a thread reply does
//! not.
//!
//! ## What is deliberately kept
//!
//! An event this session cannot decrypt, and a message body this build cannot
//! render. Both are drawn as themselves. A gap that says nothing about itself
//! is indistinguishable from nobody having spoken, and those are very different
//! things to be looking at.

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::ruma::UInt;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::room::message::sanitize::remove_plain_reply_fallback;
use matrix_sdk::ruma::events::room::message::{
    FormattedBody, MessageFormat, MessageType, Relation, RoomMessageEventContentWithoutRelation,
};
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
};

use crate::timeline::dto::{Media, Message, MessageKind, ThreadSummary};

/// What this build says instead of an encrypted message it has no key for.
///
/// Short on purpose. A room that was busy while this session was away is a
/// screen full of these, and a screen full of sentences beginning "cannot"
/// reads as a broken client rather than as what it is. A key that has not
/// arrived is a wait, so this says it is waiting and leaves it there; the
/// interface draws something turning beside it.
const NO_KEY: &str = "Waiting for the key to this message.";

/// What this build says instead of a message it cannot draw.
const NOT_SUPPORTED: &str = "A message Consort cannot draw.";

/// Where the message being read is going to be drawn.
///
/// The only thing it decides is what to do with a thread reply, which belongs
/// in exactly one of the two and would be a conversation happening twice if it
/// were drawn in both.
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

/// The message an event is a threaded reply to, when it is one.
///
/// Deliberately not a field on [`Message`]. A reply's own relation is of no
/// interest to anything drawing it, and the one caller is the watcher asking
/// whether an arriving event belongs to the thread somebody has open.
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

fn read(event: &TimelineEvent, reading: Reading) -> Option<Message> {
    if event.kind.is_utd() {
        return undecryptable(event);
    }

    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
        SyncMessageLikeEvent::Original(said),
    )) = event.raw().deserialize().ok()?
    else {
        // Every state event, every reaction, and a redacted message, which has
        // no content left to draw.
        return None;
    };

    // Before the body is read, because both of these have one and drawing it
    // is the thing being avoided. `Reply` is deliberately not matched: see the
    // header.
    let reply_to = answering(said.content.relates_to.as_ref());

    match said.content.relates_to {
        // An edit, wherever it turns up. It carries the replacement text and
        // nothing here can attach it to the message it replaces.
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
        // Neither is played and neither is looked at. Both are a card that
        // saves, so both hand over the size to say what it will cost and
        // nothing about the type: what the sender claims a file is has no
        // business becoming a content type, and the name already ends in the
        // extension that says it.
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
        // Only for a reply. The plaintext fallback and an ordinary markdown
        // quote are the same characters, and the relation is the only thing
        // that tells them apart, so stripping unconditionally would eat the
        // first paragraph of anybody quoting somebody.
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
        thread: said.unsigned.relations.thread.map(|bundle| ThreadSummary {
            // Saturating rather than fallible. The count is the homeserver's
            // own tally, and a thread long enough to overflow this is one
            // nobody is reaching the end of, so a badge that has stopped
            // counting beats a message that failed to draw.
            count: u32::try_from(u64::from(bundle.count)).unwrap_or(u32::MAX),
            participated: bundle.current_user_participated,
        }),
        reply_to,
        kind,
    })
}

/// Which event a message is answering, if it chose one.
///
/// Not every `m.in_reply_to` is somebody answering. A threaded message carries
/// one pointing at the last thing said in the thread, purely so that a client
/// with no idea about threads draws the conversation in some order, and
/// `is_falling_back` is the flag that says which kind it is. Reading them all
/// would put a reply row on every message in a thread panel.
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
/// this is a slice rather than a parse. ruma has the plaintext half of this
/// and not the HTML half without turning on its own sanitiser, which would
/// mean a second allow-list of elements to keep in step with `FormattedBody`'s
/// in the webview, and the one that is not the renderer is the one that goes
/// stale.
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
/// homeserver sends produces: it is a URI or a key, and both are JSON already.
/// The caller falls back to the line that says this build cannot draw it.
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
        // A handle like the one above and on the same terms, so a thumbnail in
        // an encrypted room carries its own key. Dropped rather than raised if
        // it will not serialise: the card falls back to the filename, which is
        // what it did before there was a thumbnail at all.
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
        reply_to: None,
        kind: MessageKind::Undecryptable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::deserialized_responses::UnableToDecryptInfo;
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
        // value the specification gives. Anything else is somebody's
        // extension, and `body` is what the specification says to draw
        // instead.
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
        // What the Lampshade bot does with a link: it uploads the clip and
        // puts the quoted post in the same event as a caption. Reading `body`
        // as the filename threw the post away and put it on the card as a
        // name.
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
        // It carries the replacement text, and a panel that drew it would show
        // a second copy of a sentence somebody corrected.
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
        // It carries the new text, and there is nothing here to attach it to
        // the message it replaces, so drawing it inline would show the room a
        // second copy of a sentence somebody corrected.
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
        // The fallback exists for clients that draw no reply of their own.
        // Consort draws one, so leaving it in would show the quoted message
        // twice, once as a blockquote nobody can click and once as the row
        // above it.
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
        // last thing said, so that a client with no idea about threads draws
        // something. `is_falling_back` is what says it is that rather than
        // somebody actually answering a particular message.
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

    #[test]
    fn a_redacted_message_is_not_drawn() {
        // Its content is gone, so there is nothing to show. A tombstone in its
        // place is a thing to build once there is a reason to.
        let redacted = event(json!({
            "type": "m.room.message",
            "event_id": "$gone:example.org",
            "sender": "@ada:example.org",
            "origin_server_ts": 1_700_000_000_000u64,
            "content": {},
            "unsigned": {
                "redacted_because": {
                    "type": "m.room.redaction",
                    "event_id": "$redaction:example.org",
                    "sender": "@ada:example.org",
                    "origin_server_ts": 1_700_000_000_001u64,
                    "content": {},
                },
            },
        }));

        assert_eq!(message(&redacted), None);
    }

    #[test]
    fn an_event_this_session_has_no_key_for_is_still_drawn() {
        // The one that matters most. A gap that says nothing about itself is
        // indistinguishable from nobody having spoken, and the two are very
        // different things to be looking at: one is a key that has not
        // arrived, the other is a quiet room.
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
        // of these, and a screen full of sentences beginning "cannot" reads as
        // a broken client. What it is is a key that has not arrived yet, and
        // one short line says so without filling the room with it.
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
}
