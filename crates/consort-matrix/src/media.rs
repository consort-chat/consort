// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a pile of bytes actually is.
//!
//! Sniffed rather than believed. Two callers need this and neither has a
//! trustworthy answer to hand: the SDK's media API returns bytes and no
//! content type, and the type an event claims is written by whoever sent it.
//! Both are about to point an `img` or a `video` at the result, so the
//! question that matters is what the bytes are, not what anybody said.
//!
//! Deliberately narrow. Every format here is one a browser draws or plays, and
//! anything else answers `None`, which the callers turn into an initial or
//! into a line saying so. Guessing would put a broken image icon on screen
//! instead.

/// What kind of image these bytes are, by their magic number.
///
/// Four formats: every one a homeserver produces a thumbnail in, plus the two
/// it may pass through untouched.
pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
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

/// What kind of video these bytes are, by their magic number.
///
/// Three containers, which is what people actually send and what a webview
/// will play given the codecs. AVI and the rest answer `None`: naming a
/// container the browser refuses only replaces a line that says so with a
/// black rectangle that says nothing.
pub(crate) fn video_type(bytes: &[u8]) -> Option<&'static str> {
    // ISO base media, which is mp4 and QuickTime both. The box length comes
    // first, so the name is at four rather than at zero.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some(if &bytes[8..12] == b"qt  " {
            "video/quicktime"
        } else {
            "video/mp4"
        });
    }
    // Matroska, of which WebM is a profile. Which one it is lives in a
    // DocType element rather than in the header, so it is read by looking for
    // the word in the space a DocType can occupy.
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let head = &bytes[..bytes.len().min(64)];
        return Some(if head.windows(4).any(|word| word == b"webm") {
            "video/webm"
        } else {
            "video/x-matroska"
        });
    }
    if bytes.starts_with(b"OggS") {
        return Some("video/ogg");
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

    #[test]
    fn an_mp4_is_recognised_by_the_box_after_its_length() {
        assert_eq!(
            video_type(b"\0\0\0\x20ftypisom\0\0\x02\0"),
            Some("video/mp4")
        );
    }

    #[test]
    fn a_quicktime_file_is_named_as_one() {
        // What a Mac and an iPhone produce, and the one ISO brand a browser
        // treats differently from the rest.
        assert_eq!(
            video_type(b"\0\0\0\x14ftypqt  \0\0\x02\0"),
            Some("video/quicktime")
        );
    }

    #[test]
    fn a_webm_is_told_apart_from_the_matroska_it_is_a_profile_of() {
        let webm =
            b"\x1a\x45\xdf\xa3\x01\x00\x00\x00\x00\x00\x00\x23\x42\x86\x81\x01\x42\x82\x84webm";
        let mkv =
            b"\x1a\x45\xdf\xa3\x01\x00\x00\x00\x00\x00\x00\x23\x42\x86\x81\x01\x42\x82\x88matroska";

        assert_eq!(video_type(webm), Some("video/webm"));
        assert_eq!(video_type(mkv), Some("video/x-matroska"));
    }

    #[test]
    fn an_ogg_stream_is_recognised() {
        assert_eq!(video_type(b"OggS\0\x02\0\0"), Some("video/ogg"));
    }

    #[test]
    fn a_container_no_browser_plays_is_refused() {
        // AVI. Naming it would replace a line saying Consort cannot show this
        // with a black rectangle saying nothing at all.
        assert_eq!(video_type(b"RIFF\0\0\0\0AVI LIST"), None);
    }

    #[test]
    fn an_image_is_not_a_video_and_a_video_is_not_an_image() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

        assert_eq!(video_type(&png), None);
        assert_eq!(image_type(b"\0\0\0\x20ftypisom\0\0\x02\0"), None);
    }

    #[test]
    fn a_truncated_video_header_is_not_a_match() {
        assert_eq!(video_type(b"\0\0\0\x20ftyp"), None);
        assert_eq!(video_type(b""), None);
    }
}
