// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Fetching the picture or the clip hanging off one message.
//!
//! A timeline is re-sent in full whenever anything in it changes, so it
//! carries handles and not bytes. The SDK keeps what it fetches, so scrolling
//! past a picture a second time costs nothing.
//!
//! [`media`] draws and sniffs; [`bytes`] saves and does not. Both are bounded
//! by [`MAX_BYTES`], here rather than in the webview because nothing
//! downstream can decline what it has already been handed.

use matrix_sdk::Client;
use matrix_sdk::media::{MediaFormat, MediaRequestParameters};
use matrix_sdk::ruma::events::room::MediaSource;

use crate::error::{Error, Result};
use crate::media::{image_type, video_type};

/// The most attachment data worth holding at once.
///
/// matrix-sdk has no range-aware download, so one of these arrives whole and
/// is held whole while it is served or written. A ceiling on the absurd rather
/// than a judgement about what a clip weighs.
///
/// The same number bounds the send side. Exported so the shell can refuse a
/// file by its length before reading it, since a bound applied after the read
/// has already cost the memory it exists to protect.
pub const MAX_BYTES: usize = 512 * 1024 * 1024;

/// One attachment, and what its bytes actually are. The type is sniffed rather
/// than repeated off the event, so a sender who called their upload
/// `text/html` cannot have a page treat it as one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    /// A media type from the short list in [`crate::media`], never the
    /// sender's word for it.
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// One attachment as something to draw, by the handle its message carried.
///
/// The handle is the event's own `MediaSource`, which is why an encrypted room
/// needs nothing special: the key travelled inside the handle.
pub async fn media(client: &Client, handle: &str) -> Result<Attachment> {
    drawable(bytes(client, handle).await?)
}

/// One attachment's bytes, whatever they are.
///
/// No sniffing, because the only thing offered for a file is saving it and a
/// spreadsheet is a perfectly good thing to write to disk.
pub async fn bytes(client: &Client, handle: &str) -> Result<Vec<u8>> {
    let source: MediaSource = serde_json::from_str(handle).map_err(|error| {
        // Only reachable by handing back something this build never wrote.
        tracing::warn!(%error, "asked for an attachment by a handle that is not one");
        Error::UndrawableMedia
    })?;

    let bytes = client
        .media()
        .get_media_content(
            &MediaRequestParameters {
                source,
                format: MediaFormat::File,
            },
            true,
        )
        .await?;

    if bytes.len() > MAX_BYTES {
        return Err(Error::MediaTooLarge {
            bytes: bytes.len(),
            limit: MAX_BYTES,
        });
    }

    Ok(bytes)
}

/// The bytes as something to draw, or the refusal saying they are not.
///
/// Separate from the fetch so the rule can be driven without a homeserver,
/// which is the only part of the above worth testing.
fn drawable(bytes: Vec<u8>) -> Result<Attachment> {
    let Some(mime) = image_type(&bytes).or_else(|| video_type(&bytes)) else {
        tracing::warn!(
            bytes = bytes.len(),
            "an attachment came back as neither a picture nor a clip"
        );
        return Err(Error::UndrawableMedia);
    };

    Ok(Attachment { mime, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest thing the sniffer will call a picture.
    fn png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]
    }

    #[test]
    fn media_source_of_a_plain_mxc() {
        // The other half of `mxcUrl` in `app/src/lib/api.ts`, which builds
        // this exact string for a custom emoji. The literal is the contract:
        // if ruma changes how a plain source is written, this fails here
        // rather than becoming a broken picture in every message.
        let handle = r#"{"url":"mxc://example.org/abc"}"#;

        let source: MediaSource =
            serde_json::from_str(handle).expect("a plain source is a media source");

        assert!(
            matches!(&source, MediaSource::Plain(uri) if uri.as_str() == "mxc://example.org/abc"),
            "{source:?}"
        );
        assert_eq!(
            serde_json::to_string(&source).expect("a source serialises"),
            handle,
            "the frontend builds this string by hand"
        );
    }

    #[test]
    fn a_picture_comes_back_as_it_arrived() {
        // Byte for byte, under the type the bytes say it is rather than the
        // one the sender claimed.
        let bytes = png();

        let drawn = drawable(bytes.clone()).expect("a png is drawable");

        assert_eq!(drawn.bytes, bytes);
        assert_eq!(drawn.mime, "image/png");
    }

    #[test]
    fn a_clip_is_drawable_too() {
        let mp4 = b"\0\0\0\x20ftypisom\0\0\x02\0".to_vec();

        assert_eq!(drawable(mp4).expect("an mp4 is drawable").mime, "video/mp4");
    }

    #[test]
    fn something_that_is_neither_is_refused_rather_than_handed_over() {
        // What a homeserver returns when the media has been removed, and what
        // a sender who lied about the type of their upload produces. Either
        // way the interface is about to point a tag at it.
        let error = drawable(b"<!doctype html>".to_vec()).expect_err("html is not media");

        assert!(matches!(error, Error::UndrawableMedia));
    }

    #[test]
    fn nothing_at_all_is_not_an_attachment() {
        // A homeserver answering an empty body, which is what a media store
        // that has lost the file does.
        assert!(drawable(Vec::new()).is_err());
    }
}
