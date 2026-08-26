// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Room avatars, one at a time.
//!
//! Deliberately not part of the snapshot. A snapshot carries `mxc://` URIs and
//! no bytes, because it is re-sent in full every time any room changes, and an
//! account with a hundred rooms would then push every avatar it has across the
//! IPC boundary because somebody renamed one channel.
//!
//! So the interface asks for the ones it is about to draw. The second ask is
//! nearly free: the SDK keeps fetched media in the same SQLite directory as
//! everything else, so a restart re-reads from disk rather than from the
//! homeserver.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use matrix_sdk::Client;
use matrix_sdk::media::{MediaFormat, MediaThumbnailSettings};
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::api::client::media::get_content_thumbnail::v3::Method;

/// How big a thumbnail to ask for, in pixels.
///
/// Twice the largest size anything draws an avatar at, so it still looks right
/// on a display that is not one device pixel per CSS pixel. It is also one of
/// Synapse's default thumbnail sizes, which matters: a homeserver only
/// generates the sizes it was configured for and scales from the nearest, so
/// asking for a size on that list is the difference between a cached thumbnail
/// and work on every request.
const SIZE: u16 = 96;

/// One room's avatar, as a data URL, or `None` when it has none.
///
/// `None` rather than an error for every failure that is not worth
/// interrupting somebody about, which is all of them: an avatar that will not
/// load falls back to initials, and a dialog about it would be worse than the
/// initials. Every such case is logged.
pub async fn avatar(client: &Client, room_id: &str) -> Option<String> {
    let room_id = match RoomId::parse(room_id) {
        Ok(room_id) => room_id,
        Err(error) => {
            tracing::warn!(%error, room_id, "asked for the avatar of something that is not a room");
            return None;
        }
    };

    // Home is a rail entry rather than a room, and a room the account has left
    // between the snapshot and the request is gone. Neither is a fault.
    let room = client.get_room(&room_id)?;
    // Cheap and local, and it saves a request for the four rooms in ten that
    // have no avatar at all.
    room.avatar_url()?;

    // Crop rather than scale. An avatar is drawn in a circle, and a scaled
    // thumbnail of a wide image is letterboxed inside it with the subject
    // shrunk into the middle.
    let settings = MediaThumbnailSettings::with_method(Method::Crop, SIZE.into(), SIZE.into());
    let bytes = match room.avatar(MediaFormat::Thumbnail(settings)).await {
        Ok(bytes) => bytes?,
        Err(error) => {
            tracing::warn!(%error, %room_id, "could not fetch a room avatar");
            return None;
        }
    };

    let Some(mime) = image_type(&bytes) else {
        tracing::warn!(
            %room_id,
            bytes = bytes.len(),
            "a room avatar came back as something that is not an image this can name"
        );
        return None;
    };

    Some(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)))
}

/// What kind of image these bytes are, by their magic number.
///
/// Sniffed rather than taken from a header, because the SDK's media API hands
/// back bytes and not a content type. Four formats, which is every format a
/// homeserver produces a thumbnail in plus the two it may pass through
/// untouched.
///
/// `None` for anything else, which the caller turns into initials. Guessing
/// would put a broken image icon on screen instead.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // RIFF is a container. Only the ones whose fourth chunk word is WEBP are
    // images; the rest are audio and video that no browser will draw.
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_is_recognised_by_its_magic_number() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

        assert_eq!(image_type(&png), Some("image/png"));
    }

    #[test]
    fn a_jpeg_is_recognised_by_its_magic_number() {
        assert_eq!(
            image_type(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
            Some("image/jpeg")
        );
    }

    #[test]
    fn both_gif_versions_are_recognised() {
        assert_eq!(image_type(b"GIF87a....."), Some("image/gif"));
        assert_eq!(image_type(b"GIF89a....."), Some("image/gif"));
    }

    #[test]
    fn a_webp_is_recognised_by_its_riff_chunk() {
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    }

    #[test]
    fn a_riff_container_that_is_not_an_image_is_not_one() {
        // A WAV file starts with the same four bytes. Calling it an image
        // would put a broken image icon in the rail rather than initials.
        assert_eq!(image_type(b"RIFF\0\0\0\0WAVEfmt "), None);
    }

    #[test]
    fn something_that_is_not_an_image_at_all_is_refused() {
        assert_eq!(image_type(b"<html>"), None);
        assert_eq!(image_type(b""), None);
    }

    #[test]
    fn a_truncated_magic_number_is_not_a_match() {
        // Short reads are what a failed download looks like.
        assert_eq!(image_type(&[0x89, b'P']), None);
        assert_eq!(image_type(b"RIFF"), None);
    }
}
