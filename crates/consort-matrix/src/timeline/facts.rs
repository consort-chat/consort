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
use matrix_sdk::ruma::events::room::message::{MessageType, Relation};
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
};

use crate::timeline::dto::{Message, MessageKind};

/// What this build says instead of an encrypted message it has no key for.
///
/// Written for a person, like every other user-facing string in this crate.
/// It says what happened and whose problem it is, because the two most likely
/// causes are a device that has not been verified and a sender who was offline
/// when the key went out, and neither is something the reader did.
const NO_KEY: &str =
    "This message cannot be read. The key for it has not reached this session yet.";

/// What this build says instead of a message it cannot draw.
const NOT_SUPPORTED: &str = "A file or image. Consort cannot show these yet.";

/// One event as a message, or `None` when it is not one to draw.
pub fn message(event: &TimelineEvent) -> Option<Message> {
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

    // Before the body is read, because a thread reply has one and drawing it
    // is the thing being avoided. `Reply` is deliberately not matched: see the
    // header.
    if matches!(
        said.content.relates_to,
        Some(Relation::Thread(_) | Relation::Replacement(_))
    ) {
        return None;
    }

    let (kind, body) = match said.content.msgtype {
        MessageType::Text(text) => (MessageKind::Text, text.body),
        MessageType::Emote(emote) => (MessageKind::Emote, emote.body),
        MessageType::Notice(notice) => (MessageKind::Notice, notice.body),
        _ => (MessageKind::Unsupported, NOT_SUPPORTED.to_owned()),
    };

    Some(Message {
        id: said.event_id.to_string(),
        sender: said.sender.to_string(),
        at: said.origin_server_ts.0.into(),
        body,
        kind,
    })
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
        kind: MessageKind::Undecryptable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::deserialized_responses::UnableToDecryptInfo;
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
    fn an_image_is_drawn_as_something_rather_than_vanishing() {
        // Somebody whose screenshot silently disappeared has no way to know it
        // was ever sent, and would send it again.
        let said = message(&sent(json!({
            "msgtype": "m.image",
            "body": "screenshot.png",
            "url": "mxc://example.org/abc",
        })))
        .expect("an image is still something to draw");

        assert_eq!(said.kind, MessageKind::Unsupported);
        assert!(!said.body.is_empty());
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
            "body": "> quoted\n\nagreed",
            "m.relates_to": {
                "m.in_reply_to": { "event_id": "$original:example.org" },
            },
        }));

        assert_eq!(
            message(&reply).map(|said| said.body),
            Some("> quoted\n\nagreed".to_owned())
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
    fn something_that_is_not_an_event_at_all_is_not_a_message() {
        // A homeserver sending something this build cannot parse must not take
        // the room's whole timeline with it.
        assert_eq!(message(&event(json!({ "type": "m.room.message" }))), None);
    }
}
