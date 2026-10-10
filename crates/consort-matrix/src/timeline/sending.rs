// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Putting a picture, a clip, a file or a voice note into a room.
//!
//! One `Room::send_attachment` call, which uploads, sends, and encrypts when
//! the room is encrypted, deciding that from the room's own state. A caption
//! and a reply ride on the same call as fields on [`AttachmentConfig`].
//!
//! What it is is decided by the bytes rather than by the file extension, and
//! the `msgtype` follows from that: see [`content_type_of`].

use matrix_sdk::attachment::{
    AttachmentConfig, AttachmentInfo, BaseFileInfo, BaseImageInfo, BaseVideoInfo,
};
use matrix_sdk::room::reply::{EnforceThread, Reply};
use matrix_sdk::ruma::UInt;
use matrix_sdk::ruma::events::room::message::{
    AddMentions, ReplyWithinThread, TextMessageEventContent,
};
use matrix_sdk::{Client, Room};

use crate::error::{Error, Result};
use crate::media::{audio_type, image_type, pixel_size, video_type};
use crate::timeline::media::MAX_BYTES;

/// What goes out as `m.file` when the bytes answer to nothing else: a pile of
/// bytes, which is what is known about it, and what every client turns into a
/// card that saves. A content type this build made up could be acted on.
const ANYTHING: &str = "application/octet-stream";

/// One attachment on its way out. A value rather than an argument list,
/// because the last two are optional and a call site passing `None, None`
/// says nothing about which of them it meant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attaching {
    /// What to call it in the room: the sender's own file name, which is the
    /// only name anybody has for it and is not read for anything else.
    pub filename: String,
    /// The whole of it, held once.
    pub bytes: Vec<u8>,
    /// What was typed beside it, read as markdown because it is typed into the
    /// same box as an ordinary message.
    pub caption: Option<String>,
    /// What it answers, which is also what decides where it is drawn.
    pub answering: Answering,
}

/// What an attachment answers, and so which conversation it lands in.
///
/// An event ID and nothing else in each case, unlike [`super::send_in_thread`]
/// which is handed the thread's root as well. The SDK reads the event named
/// here and takes the thread off it, and a thread with no replies yet is named
/// by its own root, which carries no thread relation and so becomes the root of
/// the one this writes. Both are the answer this wants, so there is nothing for
/// a caller to pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Answering {
    /// Nothing. It goes into the room on its own.
    #[default]
    Nothing,
    /// One message in the room. The event ID alone, unlike
    /// [`super::send_reply`]: the SDK resolves the event itself, so it knows
    /// who to mention.
    Message(String),
    /// A thread, named by the last thing said in it as far as the caller knows.
    /// Stale is harmless: the pointer is only the fallback a thread carries for
    /// clients that do not understand threads, and nothing about which thread
    /// this lands in depends on it.
    Thread(String),
    /// One reply in a thread, which is an answer somebody pressed Reply for
    /// rather than a fallback, so its author is mentioned.
    ReplyInThread(String),
}

/// Send one attachment to a room.
///
/// Nothing is returned and nothing is echoed: it appears when the sync brings
/// it back, and an upload takes long enough for that gap to be visible.
pub async fn send_attachment(client: &Client, room_id: &str, attaching: Attaching) -> Result<()> {
    let room = super::room_of(client, room_id)?;
    let reply = relation(&attaching.answering)?;

    if attaching.bytes.is_empty() {
        return Err(Error::EmptyMessage);
    }
    within_the_ceiling(attaching.bytes.len())?;
    within_the_servers_limit(attaching.bytes.len() as u64, upload_limit(client).await)?;

    let content_type = content_type_of(&attaching.bytes);
    let config = AttachmentConfig::new()
        .info(info_of(content_type, &attaching.bytes))
        .caption(caption(attaching.caption.as_deref()))
        .reply(reply);

    upload(
        &room,
        &attaching.filename,
        content_type,
        attaching.bytes,
        config,
    )
    .await
}

/// The call itself, split out so the arguments above stay readable.
///
/// The mime is parsed rather than held. Every string this can be handed comes
/// from [`content_type_of`]'s fixed list, so a parse failure is unreachable
/// without editing that list into something that is not a media type.
async fn upload(
    room: &Room,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
    config: AttachmentConfig,
) -> Result<()> {
    let mime = content_type
        .parse::<mime::Mime>()
        .expect("every content type this crate sniffs is a media type");

    room.send_attachment(filename, &mime, bytes, config).await?;
    Ok(())
}

/// What these bytes are, or `application/octet-stream` when they are nothing
/// this build recognises.
///
/// Audio before video on purpose: an m4a and an mp4 share a container, and so
/// do the two kinds of Ogg, so asking the video sniffer first would send every
/// voice note as a clip.
fn content_type_of(bytes: &[u8]) -> &'static str {
    image_type(bytes)
        .or_else(|| audio_type(bytes))
        .or_else(|| video_type(bytes))
        .unwrap_or(ANYTHING)
}

/// The metadata that rides beside the upload.
///
/// A picture is measured, so a receiving room can hold the space before the
/// bytes land. Nothing else is: a clip's dimensions and duration need a
/// decoder, and the size is filled in by the SDK from the bytes.
fn info_of(content_type: &str, bytes: &[u8]) -> AttachmentInfo {
    let size = UInt::new(bytes.len() as u64);

    match content_type.split('/').next() {
        Some("image") => {
            let (width, height) = pixel_size(bytes).unzip();
            AttachmentInfo::Image(BaseImageInfo {
                width: width.and_then(|width| UInt::new(width.into())),
                height: height.and_then(|height| UInt::new(height.into())),
                size,
                ..BaseImageInfo::default()
            })
        }
        Some("video") => AttachmentInfo::Video(BaseVideoInfo {
            size,
            ..BaseVideoInfo::default()
        }),
        // Audio and everything else. A voice note's info holds a duration and
        // a waveform, both of which need the file decoded, so what is left is
        // the size, which is what a file carries too.
        _ => AttachmentInfo::File(BaseFileInfo { size }),
    }
}

/// The caption, as something to send, or `None` when nobody typed one.
///
/// Markdown, because it is typed into the same box as a message. Trimmed
/// before it is judged empty, so a box somebody tabbed through does not put an
/// empty line under their picture.
fn caption(caption: Option<&str>) -> Option<TextMessageEventContent> {
    let caption = caption?;
    if caption.trim().is_empty() {
        return None;
    }
    Some(TextMessageEventContent::markdown(caption))
}

/// The relation this attachment carries, or `None` when it answers nothing.
///
/// A message in the room gets `Unthreaded` rather than `MaybeThreaded`,
/// matching [`super::send_reply`]: a message in a thread is not drawn in the
/// room at all, so forwarding a thread relation found on the answered message
/// would put the picture somewhere nobody is looking.
fn relation(answering: &Answering) -> Result<Option<Reply>> {
    let (event_id, enforce_thread) = match answering {
        Answering::Nothing => return Ok(None),
        Answering::Message(event_id) => (event_id, EnforceThread::Unthreaded),
        Answering::Thread(event_id) => (event_id, EnforceThread::Threaded(ReplyWithinThread::No)),
        Answering::ReplyInThread(event_id) => {
            (event_id, EnforceThread::Threaded(ReplyWithinThread::Yes))
        }
    };

    Ok(Some(Reply {
        event_id: super::event_id_of(event_id)?,
        enforce_thread,
        add_mentions: AddMentions::Yes,
    }))
}

/// Whether this many bytes is more than this build will hold at once.
///
/// [`MAX_BYTES`] is about memory: an attachment is held whole while it is
/// uploaded. The shell refuses a file by its length first, so this ordinarily
/// does not trip, and it stays because the shell is one caller of many.
///
/// Takes the length rather than the bytes so a test can drive it: the ceiling
/// is half a gigabyte, which is a lot to allocate per run.
fn within_the_ceiling(bytes: usize) -> Result<()> {
    if bytes <= MAX_BYTES {
        return Ok(());
    }

    Err(Error::MediaTooLarge {
        bytes,
        limit: MAX_BYTES,
    })
}

/// What this homeserver takes in one upload, or `None` when it will not say.
///
/// Asked for apart from the comparison below, so that a caller holding a
/// length and no bytes can have the answer before it reads anything: the SDK
/// fetches this once per session and holds it, which is what makes asking
/// while a file picker is open free and the refusal afterwards immediate.
///
/// A homeserver that will not say is nobody's refusal to make here. The send
/// goes ahead and the SDK asks the same question a moment later, answering it
/// with an error written for a log.
pub async fn upload_limit(client: &Client) -> Option<u64> {
    match client.load_or_fetch_max_upload_size().await {
        Ok(limit) => Some(u64::from(limit)),
        Err(error) => {
            tracing::debug!(%error, "the homeserver would not say how large an upload it takes");
            None
        }
    }
}

/// Refuse an upload the homeserver would refuse, rather than discovering it as
/// a 413 once the bytes have been read and sent.
///
/// Takes the length rather than the bytes because the shell applies this to a
/// file it has not opened, which is the whole point: the two numbers in the
/// refusal are known before anything is read.
pub fn within_the_servers_limit(bytes: u64, limit: Option<u64>) -> Result<()> {
    let Some(limit) = limit else {
        return Ok(());
    };

    if bytes > limit {
        return Err(Error::UploadTooLarge { bytes, limit });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest thing the sniffer will call a picture, with a size in it.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&[0, 0, 0, 0x0D]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn a_picture_is_named_as_one() {
        assert_eq!(content_type_of(&png(1, 1)), "image/png");
    }

    #[test]
    fn a_clip_is_named_as_one() {
        assert_eq!(
            content_type_of(b"\0\0\0\x20ftypisom\0\0\x02\0"),
            "video/mp4"
        );
    }

    #[test]
    fn a_voice_note_is_not_sent_as_a_clip() {
        // An m4a and an mp4 are the same container, and Ogg carries both
        // kinds. Asking the video sniffer first sends every one of these as
        // something that draws a black rectangle.
        assert_eq!(
            content_type_of(b"\0\0\0\x20ftypM4A \0\0\x02\0"),
            "audio/mp4"
        );
        assert_eq!(content_type_of(b"ID3\x04\0\0\0\0\0\x23"), "audio/mpeg");
    }

    #[test]
    fn anything_the_sniffer_will_not_name_goes_as_a_file() {
        // A spreadsheet, and also the case that matters: a content type this
        // build invented is one a receiving client might act on.
        assert_eq!(content_type_of(b"PK\x03\x04\x14\0\x06\0"), ANYTHING);
        assert_eq!(content_type_of(b"<!doctype html>"), ANYTHING);
    }

    #[test]
    fn the_extension_is_not_what_decides() {
        // The whole reason this sniffs. A file somebody renamed is the
        // ordinary case, not a hostile one, and it should arrive as the
        // picture it is.
        let renamed = Attaching {
            filename: "holiday.mp4".to_owned(),
            bytes: png(1, 1),
            ..Attaching::default()
        };

        assert_eq!(content_type_of(&renamed.bytes), "image/png");
    }

    #[test]
    fn a_picture_is_measured_before_it_is_sent() {
        // Without these a receiving room cannot hold the space, and every
        // picture that loads shoves the conversation below it downwards.
        let info = info_of("image/png", &png(640, 480));

        match info {
            AttachmentInfo::Image(image) => {
                assert_eq!(image.width, UInt::new(640));
                assert_eq!(image.height, UInt::new(480));
                assert_eq!(image.size, UInt::new(24));
            }
            other => panic!("expected image info, got {other:?}"),
        }
    }

    #[test]
    fn a_picture_this_build_cannot_measure_still_goes_out() {
        // A truncated header, which is what a half-written file looks like.
        // No dimensions is something every client copes with; refusing to send
        // is not.
        let info = info_of("image/png", &png(640, 480)[..20]);

        match info {
            AttachmentInfo::Image(image) => {
                assert_eq!(image.width, None);
                assert_eq!(image.height, None);
            }
            other => panic!("expected image info, got {other:?}"),
        }
    }

    #[test]
    fn a_clip_carries_its_size_and_no_invented_dimensions() {
        // Measuring one needs a decoder this build does not have, and a
        // guessed shape is a hole of the wrong size in everybody's room.
        let info = info_of("video/mp4", b"\0\0\0\x20ftypisom\0\0\x02\0");

        match info {
            AttachmentInfo::Video(video) => {
                assert_eq!(video.size, UInt::new(16));
                assert_eq!(video.width, None);
                assert_eq!(video.height, None);
            }
            other => panic!("expected video info, got {other:?}"),
        }
    }

    #[test]
    fn a_file_and_a_voice_note_carry_their_size() {
        for content_type in [ANYTHING, "audio/mpeg"] {
            match info_of(content_type, b"0123456789") {
                AttachmentInfo::File(file) => assert_eq!(file.size, UInt::new(10)),
                other => panic!("expected file info for {content_type}, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_attachment_past_this_builds_own_ceiling_is_refused() {
        // A different ceiling from the homeserver's and for a different
        // reason: this one is about holding the whole thing in memory while
        // it uploads, and it is the same everywhere.
        let refused = within_the_ceiling(MAX_BYTES + 1).expect_err("past the ceiling");

        assert!(matches!(refused, Error::MediaTooLarge { .. }));
    }

    #[test]
    fn an_attachment_at_exactly_the_ceiling_is_still_sendable() {
        assert!(within_the_ceiling(MAX_BYTES).is_ok());
    }

    #[test]
    fn an_attachment_past_the_homeservers_ceiling_is_refused_with_both_numbers() {
        // Both, because the message built from them is the only useful thing
        // to say: the fix is on a server, and which server is the point.
        let refused = within_the_servers_limit(60 * 1024 * 1024, Some(50 * 1024 * 1024))
            .expect_err("past what the server takes");

        assert!(
            matches!(
                refused,
                Error::UploadTooLarge {
                    bytes: 62_914_560,
                    limit: 52_428_800
                }
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn an_attachment_at_exactly_the_homeservers_ceiling_is_still_sendable() {
        assert!(within_the_servers_limit(50 * 1024 * 1024, Some(50 * 1024 * 1024)).is_ok());
    }

    #[test]
    fn a_homeserver_that_will_not_say_its_limit_refuses_nothing_here() {
        // Guessing one would refuse files a server would have taken. The send
        // goes ahead and the server gets to answer for itself.
        assert!(within_the_servers_limit(u64::MAX, None).is_ok());
    }

    #[test]
    fn a_caption_nobody_typed_is_not_a_caption() {
        // An empty one would put a blank line under the picture, and a box
        // somebody tabbed through is the ordinary way to produce one.
        assert!(caption(None).is_none());
        assert!(caption(Some("")).is_none());
        assert!(caption(Some("  \n ")).is_none());
    }

    #[test]
    fn a_caption_is_read_as_markdown() {
        // The same box as a message, so the same asterisks have to mean the
        // same thing.
        let caption = caption(Some("*look*")).expect("a caption with words in it");

        assert_eq!(caption.body, "*look*");
        assert!(caption.formatted.is_some());
    }

    /// The relation an [`Answering`] comes to, for the four cases below.
    fn related(answering: Answering) -> Option<Reply> {
        relation(&answering).expect("a valid event ID")
    }

    #[test]
    fn an_attachment_that_answers_nothing_carries_no_relation() {
        assert!(related(Answering::Nothing).is_none());
    }

    #[test]
    fn an_attachment_answering_a_message_names_it_and_mentions_its_author() {
        let reply = related(Answering::Message("$said:example.org".to_owned())).expect("a reply");

        assert_eq!(reply.event_id.as_str(), "$said:example.org");
        assert_eq!(reply.enforce_thread, EnforceThread::Unthreaded);
        assert!(matches!(reply.add_mentions, AddMentions::Yes));
    }

    #[test]
    fn an_attachment_in_a_thread_is_threaded_and_only_falls_back_to_the_reply() {
        // #134. `Unthreaded` here is the bug the thread's composer had no way
        // to hit, because it had no way to attach anything at all: it strips
        // the thread relation, and the picture lands in the room instead of in
        // the conversation somebody is having.
        let reply = related(Answering::Thread("$latest:example.org".to_owned())).expect("a reply");

        assert_eq!(reply.event_id.as_str(), "$latest:example.org");
        assert_eq!(
            reply.enforce_thread,
            EnforceThread::Threaded(ReplyWithinThread::No)
        );
    }

    #[test]
    fn an_attachment_answering_one_reply_in_a_thread_is_not_a_fallback() {
        // The difference between the two threaded cases is one flag on the
        // wire, and it decides whether every client draws the picture as an
        // answer to that reply or as the next thing said in the thread.
        let reply =
            related(Answering::ReplyInThread("$said:example.org".to_owned())).expect("a reply");

        assert_eq!(reply.event_id.as_str(), "$said:example.org");
        assert_eq!(
            reply.enforce_thread,
            EnforceThread::Threaded(ReplyWithinThread::Yes)
        );
    }

    #[test]
    fn an_event_id_that_is_not_one_is_refused_before_anything_is_uploaded() {
        for answering in [
            Answering::Message("not an event".to_owned()),
            Answering::Thread("not an event".to_owned()),
            Answering::ReplyInThread("not an event".to_owned()),
        ] {
            let refused = relation(&answering).expect_err("not an event ID");

            assert!(matches!(refused, Error::NoSuchEvent { .. }));
        }
    }
}
