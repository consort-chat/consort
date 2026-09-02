// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Fetching the picture or the clip hanging off one message.
//!
//! On the same terms as an avatar and for the same reason: a timeline is
//! re-sent in full whenever anything in it changes, so it carries handles and
//! not bytes, and the interface asks for the ones it is about to draw. The
//! SDK keeps what it fetches in the same SQLite directory as everything else,
//! so scrolling back past a picture a second time costs nothing.
//!
//! ## Why the bytes are not encoded here
//!
//! An avatar comes back as a data URL because it is a few kilobytes and it
//! lands in an `img` tag. These are not: a phone photograph is megabytes and a
//! clip is tens of them, and base64 adds a third to that before the webview
//! has to hold it as a string. So this hands back the bytes as they are, the
//! command wraps them in a `tauri::ipc::Response`, and the interface makes a
//! blob out of what arrives.
//!
//! ## What is refused, and why here rather than there
//!
//! Anything past [`MAX_BYTES`], and anything whose bytes are neither a picture
//! nor a clip. Both checks are on this side of the boundary because this is
//! where the bytes are: the webview cannot decline something it has already
//! been handed, and the type an event claims is written by whoever sent it.

use matrix_sdk::Client;
use matrix_sdk::media::{MediaFormat, MediaRequestParameters};
use matrix_sdk::ruma::events::room::MediaSource;

use crate::error::{Error, Result};
use crate::media::{image_type, video_type};

/// The most attachment data worth carrying into the webview.
///
/// One of these is held in memory whole, twice over for a moment: once in Rust
/// and once as the blob the interface builds from it. That is affordable for
/// the photographs and short clips people paste into a conversation and it is
/// not affordable without a bound, which is what this is.
///
/// Generous rather than tuned. It is a ceiling on the absurd, and the thing
/// that keeps it from being reached casually is that clips are fetched only
/// when somebody asks for one.
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// The bytes of one attachment, by the handle its message carried.
///
/// The handle is the event's own `MediaSource`, which is why this works the
/// same for an encrypted room: the SDK decrypts the file with the key that
/// travelled inside the handle, and nothing here has to know which kind it
/// was holding.
pub async fn media(client: &Client, handle: &str) -> Result<Vec<u8>> {
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

    drawable(bytes)
}

/// The bytes, if they are something the interface can be handed.
///
/// Separate from the fetch so that both rules can be driven without a
/// homeserver, which is the only part of the above worth testing.
fn drawable(bytes: Vec<u8>) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::MediaTooLarge {
            bytes: bytes.len(),
            limit: MAX_BYTES,
        });
    }

    if image_type(&bytes).is_none() && video_type(&bytes).is_none() {
        tracing::warn!(
            bytes = bytes.len(),
            "an attachment came back as neither a picture nor a clip"
        );
        return Err(Error::UndrawableMedia);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest thing the sniffer will call a picture.
    fn png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0]
    }

    #[test]
    fn a_picture_comes_back_as_it_arrived() {
        // Byte for byte. Anything else here would be a second encoding of
        // something the webview is about to decode again.
        let bytes = png();

        assert_eq!(drawable(bytes.clone()).expect("a png is drawable"), bytes);
    }

    #[test]
    fn a_clip_is_drawable_too() {
        let mp4 = b"\0\0\0\x20ftypisom\0\0\x02\0".to_vec();

        assert!(drawable(mp4).is_ok());
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
    fn an_attachment_past_the_cap_is_refused_before_it_is_looked_at() {
        // Refused on size, so the sniffing below it never runs on something
        // this large.
        let mut huge = vec![0u8; MAX_BYTES + 1];
        huge[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        let error = drawable(huge).expect_err("too large is too large");

        assert!(matches!(error, Error::MediaTooLarge { .. }));
    }

    #[test]
    fn an_attachment_at_exactly_the_cap_is_still_drawn() {
        let mut big = vec![0u8; MAX_BYTES];
        big[..8].copy_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        assert!(drawable(big).is_ok());
    }

    #[test]
    fn nothing_at_all_is_not_an_attachment() {
        // A homeserver answering an empty body, which is what a media store
        // that has lost the file does.
        assert!(drawable(Vec::new()).is_err());
    }
}
