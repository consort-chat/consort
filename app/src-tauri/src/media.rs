// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Serving one attachment to the webview over a scheme it can seek in.
//!
//! ## Why not a blob
//!
//! 0.1.3 fetched an attachment's bytes over the IPC boundary and wrapped them
//! in a blob. That works for a picture and is wrong for a clip in three ways
//! at once. A blob URL answers no range request, so the element cannot begin
//! until every byte is in JavaScript's hands and cannot seek once it has them.
//! The whole file is then held twice, once in Rust and once in the webview.
//! And an `ArrayBuffer` of a hundred megabytes crosses the boundary as one
//! message.
//!
//! A URI scheme answers ranges, so the element asks for the part it needs,
//! seeks by asking for a different part, and holds none of it. What Rust holds
//! is [`Cache`], which is bounded.
//!
//! What this does not do is make the first play progressive. matrix-sdk hands
//! back one `Vec<u8>`: there is no range-aware download in it, so the first
//! request for an attachment still waits for the whole thing. A passthrough to
//! the homeserver would fix that for a plain room and would do nothing for an
//! encrypted one, which is every room this is used in.
//!
//! ## What is in the URL
//!
//! The same opaque handle the timeline already carries, base64 encoded so it
//! survives a path. That handle is the event's own media source, which in an
//! encrypted room holds the file's key, and it is already in the document:
//! `consort_matrix::Media::source` says why that is the same trust boundary as
//! the decrypted bytes themselves. Encoding it rather than keeping a table of
//! them means there is no registry to evict from and no lifetime to get wrong.

use std::collections::VecDeque;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tauri::http::{Response, StatusCode, header};

/// The scheme attachments are served on.
///
/// On Linux and macOS this makes an origin of `consortmedia://localhost`; on
/// Windows it would be `http://consortmedia.localhost`. Both are in the
/// content security policy in `tauri.conf.json`, which is the other half of
/// this working at all.
pub const SCHEME: &str = "consortmedia";

/// The most attachment bytes to keep in memory at once.
///
/// Generous, because the thing it is protecting against is not a large file
/// but a room full of them: an attachment is dropped from here the moment a
/// third one pushes past the cap, and the only cost of dropping one is
/// fetching it again.
const MAX_CACHED_BYTES: usize = 256 * 1024 * 1024;

/// How many attachments to keep regardless of size.
///
/// Small on purpose. What this exists for is the file being played right now
/// and the picture beside it, and a range request arriving for something
/// nobody is looking at is not a case worth holding a hundred megabytes for.
const MAX_CACHED: usize = 8;

/// The handle a request's path names, or `None` when it is not one.
///
/// The other half of this is `mediaUrl` in `app/src/lib/api.ts`, which builds
/// the URL. It is on that side rather than this one so that pointing an `img`
/// at an attachment is a string rather than a round trip, and the two are
/// pinned to each other by a test on each side agreeing on one literal.
pub fn handle(path: &str) -> Option<String> {
    let encoded = path.strip_prefix('/').unwrap_or(path);
    String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()
}

/// What a `Range` header asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    /// All of it, which is also the answer to a header nothing here can read.
    Whole,
    /// Bytes `start` to `end`, both inclusive and both inside the file.
    Part { start: u64, end: u64 },
    /// A range that starts past the end of the file.
    Unsatisfiable,
}

/// Read one `Range` header against a file of `len` bytes.
///
/// Only the single-range forms, which is every range a media element sends.
/// Anything else, including a multipart request and a header this cannot
/// parse, is answered with the whole file: the specification allows ignoring a
/// `Range` entirely, and a wrong 206 is worse than a right 200.
pub fn wanted(header: Option<&str>, len: u64) -> Wanted {
    // Nothing can be a range of nothing, and answering 416 for an empty file
    // would turn "the homeserver has lost it" into a confusing error.
    if len == 0 {
        return Wanted::Whole;
    }

    let Some(spec) = header.and_then(|header| header.trim().strip_prefix("bytes=")) else {
        return Wanted::Whole;
    };
    // One range only. A media element never asks for two, and answering a
    // multipart request with the first part would be a lie.
    if spec.contains(',') {
        return Wanted::Whole;
    }

    let Some((first, last)) = spec.split_once('-') else {
        return Wanted::Whole;
    };
    let (first, last) = (first.trim(), last.trim());

    // `bytes=-500` is the last 500 bytes, not a range starting below zero.
    // Getting this backwards serves the beginning of a file to something
    // looking for the end of it, which for an mp4 whose index is at the back
    // is a clip that will not start.
    if first.is_empty() {
        let Ok(from_end) = last.parse::<u64>() else {
            return Wanted::Whole;
        };
        if from_end == 0 {
            return Wanted::Unsatisfiable;
        }
        return Wanted::Part {
            start: len.saturating_sub(from_end),
            end: len - 1,
        };
    }

    let Ok(start) = first.parse::<u64>() else {
        return Wanted::Whole;
    };
    if start >= len {
        return Wanted::Unsatisfiable;
    }

    let end = if last.is_empty() {
        len - 1
    } else {
        match last.parse::<u64>() {
            // Clamped rather than refused. A client asking for more than there
            // is has asked for the rest of it.
            Ok(end) => end.min(len - 1),
            Err(_) => return Wanted::Whole,
        }
    };
    if end < start {
        return Wanted::Unsatisfiable;
    }

    Wanted::Part { start, end }
}

/// One attachment, or the part of it that was asked for.
///
/// `Accept-Ranges` on every answer, including the 200, because that is what
/// tells a media element it may seek at all: without it WebKit downloads the
/// whole thing before it will start and refuses to move the scrub bar.
pub fn respond(bytes: &[u8], mime: &str, wanted: Wanted) -> Response<Vec<u8>> {
    let len = bytes.len() as u64;
    let build = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, mime);

    match wanted {
        Wanted::Whole => build
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, len)
            .body(bytes.to_vec()),
        Wanted::Part { start, end } => {
            let part = bytes[start as usize..=end as usize].to_vec();
            build
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                .header(header::CONTENT_LENGTH, part.len())
                .body(part)
        }
        Wanted::Unsatisfiable => build
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Vec::new()),
    }
    // Every header above is built from a number or a constant, so there is
    // nothing here that can be rejected.
    .expect("a response built from numbers and constants is a valid response")
}

/// What to say when there is no attachment to serve.
///
/// Plain text rather than an empty body, because this is what ends up in the
/// devtools network pane when something is wrong, and "404" on its own starts
/// a search rather than ending one.
pub fn refuse(status: StatusCode, why: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(why.as_bytes().to_vec())
        .expect("a plain text response is a valid response")
}

/// One attachment, held.
///
/// The type comes with the bytes because it was sniffed from them: serving a
/// cached file has to answer with the same content type the first request did,
/// and re-sniffing on every range would be the sniffing done a hundred times
/// for one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// The attachments held in memory, most recently used last.
///
/// A range request is one HTTP request per few hundred kilobytes, and every
/// one of them would otherwise be a fetch and a decryption of the whole file.
#[derive(Debug, Default)]
pub struct Cache {
    held: VecDeque<(String, Arc<Held>)>,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// What is held for `handle`, marking it as the one in use.
    ///
    /// Promoting on a hit rather than only on insertion is what stops a room
    /// of pictures scrolling past evicting the clip somebody is watching.
    pub fn get(&mut self, handle: &str) -> Option<Arc<Held>> {
        let at = self.held.iter().position(|(held, _)| held == handle)?;
        let entry = self.held.remove(at)?;
        let bytes = entry.1.clone();
        self.held.push_back(entry);
        Some(bytes)
    }

    /// Hold an attachment, dropping the least recently used until it fits.
    ///
    /// An attachment larger than the whole cap is held anyway, and on its own:
    /// refusing it would mean the one file somebody is actually watching is
    /// the one that is refetched on every seek.
    pub fn insert(&mut self, handle: String, bytes: Arc<Held>) {
        self.held.retain(|(held, _)| held != &handle);
        self.held.push_back((handle, bytes));

        while self.held.len() > MAX_CACHED
            || (self.held.len() > 1 && self.weight() > MAX_CACHED_BYTES)
        {
            self.held.pop_front();
        }
    }

    fn weight(&self) -> usize {
        self.held.iter().map(|(_, held)| held.bytes.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod addressing {
        use super::*;

        /// One handle and the path the frontend builds for it.
        ///
        /// The literal is the contract. `mediaUrl` in `app/src/lib/api.ts` has
        /// a test asserting it produces this exact string, so a change to
        /// either encoding fails on both sides rather than becoming a silent
        /// 400 for every attachment.
        const HANDLE: &str = r#"{"url":"mxc://example.org/abc","key":{"k":"a+b/c"}}"#;
        const PATH: &str = "/eyJ1cmwiOiJteGM6Ly9leGFtcGxlLm9yZy9hYmMiLCJrZXkiOnsiayI6ImErYi9jIn19";

        #[test]
        fn the_path_the_frontend_builds_reads_back_as_the_handle() {
            // JSON has quotes, braces and slashes in it, and a key has `+` and
            // `/`. None of them can sit in a path as themselves.
            assert_eq!(handle(PATH).as_deref(), Some(HANDLE));
        }

        #[test]
        fn a_path_that_is_not_a_handle_is_refused_rather_than_guessed_at() {
            assert_eq!(handle("/not base64"), None);
        }

        #[test]
        fn a_path_that_decodes_to_something_that_is_not_text_is_refused() {
            assert_eq!(
                handle(&format!("/{}", URL_SAFE_NO_PAD.encode([0xFF, 0xFE]))),
                None
            );
        }
    }

    mod ranges {
        use super::*;

        #[test]
        fn no_header_at_all_is_the_whole_file() {
            assert_eq!(wanted(None, 1_000), Wanted::Whole);
        }

        #[test]
        fn an_open_ended_range_runs_to_the_last_byte() {
            // What every media element opens with.
            assert_eq!(
                wanted(Some("bytes=0-"), 1_000),
                Wanted::Part { start: 0, end: 999 }
            );
        }

        #[test]
        fn a_closed_range_is_taken_as_written() {
            assert_eq!(
                wanted(Some("bytes=100-199"), 1_000),
                Wanted::Part {
                    start: 100,
                    end: 199
                }
            );
        }

        #[test]
        fn a_range_past_the_end_is_clamped_rather_than_refused() {
            // A client asking for more than there is has asked for the rest.
            assert_eq!(
                wanted(Some("bytes=900-5000"), 1_000),
                Wanted::Part {
                    start: 900,
                    end: 999
                }
            );
        }

        #[test]
        fn a_suffix_range_is_the_end_of_the_file() {
            // `bytes=-500` is the last 500 bytes. Reading it as a range
            // starting below zero serves the front of an mp4 to something
            // looking for the index at its back, which is a clip that will
            // not start.
            assert_eq!(
                wanted(Some("bytes=-500"), 1_000),
                Wanted::Part {
                    start: 500,
                    end: 999
                }
            );
        }

        #[test]
        fn a_suffix_longer_than_the_file_is_the_whole_file() {
            assert_eq!(
                wanted(Some("bytes=-5000"), 1_000),
                Wanted::Part { start: 0, end: 999 }
            );
        }

        #[test]
        fn a_range_starting_past_the_end_cannot_be_satisfied() {
            assert_eq!(wanted(Some("bytes=1000-"), 1_000), Wanted::Unsatisfiable);
        }

        #[test]
        fn a_backwards_range_cannot_be_satisfied_either() {
            assert_eq!(wanted(Some("bytes=200-100"), 1_000), Wanted::Unsatisfiable);
        }

        #[test]
        fn a_zero_length_suffix_cannot_be_satisfied() {
            assert_eq!(wanted(Some("bytes=-0"), 1_000), Wanted::Unsatisfiable);
        }

        #[test]
        fn several_ranges_at_once_get_the_whole_file() {
            // Answering the first part of a multipart request would be a lie
            // about what was sent, and the specification allows ignoring a
            // `Range` outright.
            assert_eq!(wanted(Some("bytes=0-99,200-299"), 1_000), Wanted::Whole);
        }

        #[test]
        fn a_unit_this_does_not_speak_gets_the_whole_file() {
            assert_eq!(wanted(Some("frames=0-10"), 1_000), Wanted::Whole);
        }

        #[test]
        fn nonsense_gets_the_whole_file_rather_than_an_error() {
            assert_eq!(wanted(Some("bytes=abc-def"), 1_000), Wanted::Whole);
            assert_eq!(wanted(Some("bytes"), 1_000), Wanted::Whole);
        }

        #[test]
        fn nothing_is_a_range_of_an_empty_file() {
            // A homeserver that has lost the file answers with none, and a 416
            // for that would report it as a bad request from the page.
            assert_eq!(wanted(Some("bytes=0-"), 0), Wanted::Whole);
        }
    }

    mod responses {
        use super::*;

        #[test]
        fn the_whole_file_comes_back_whole() {
            let answer = respond(b"0123456789", "video/mp4", Wanted::Whole);

            assert_eq!(answer.status(), StatusCode::OK);
            assert_eq!(answer.body(), b"0123456789");
            assert_eq!(answer.headers()[header::CONTENT_LENGTH], "10");
        }

        #[test]
        fn every_answer_says_ranges_are_allowed() {
            // What tells a media element it may seek. Without it WebKit
            // downloads the whole file before it will start and then refuses
            // to move the scrub bar.
            let answer = respond(b"0123456789", "video/mp4", Wanted::Whole);

            assert_eq!(answer.headers()[header::ACCEPT_RANGES], "bytes");
        }

        #[test]
        fn a_part_comes_back_as_that_part_and_says_where_it_sits() {
            let answer = respond(
                b"0123456789",
                "video/mp4",
                Wanted::Part { start: 2, end: 5 },
            );

            assert_eq!(answer.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(answer.body(), b"2345");
            assert_eq!(answer.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
            assert_eq!(answer.headers()[header::CONTENT_LENGTH], "4");
        }

        #[test]
        fn the_last_byte_is_included() {
            // Ranges are inclusive at both ends, and an exclusive end here
            // truncates every file by one byte, which for a clip is a decoder
            // error rather than anything obvious.
            let answer = respond(
                b"0123456789",
                "video/mp4",
                Wanted::Part { start: 8, end: 9 },
            );

            assert_eq!(answer.body(), b"89");
        }

        #[test]
        fn a_range_that_cannot_be_satisfied_says_how_long_the_file_is() {
            let answer = respond(b"0123456789", "video/mp4", Wanted::Unsatisfiable);

            assert_eq!(answer.status(), StatusCode::RANGE_NOT_SATISFIABLE);
            assert_eq!(answer.headers()[header::CONTENT_RANGE], "bytes */10");
            assert!(answer.body().is_empty());
        }

        #[test]
        fn a_refusal_says_why_in_words() {
            let answer = refuse(StatusCode::NOT_FOUND, "no attachment there");

            assert_eq!(answer.status(), StatusCode::NOT_FOUND);
            assert_eq!(answer.body(), b"no attachment there");
        }
    }

    mod caching {
        use super::*;

        fn bytes(len: usize) -> Arc<Held> {
            Arc::new(Held {
                mime: "image/png",
                bytes: vec![0u8; len],
            })
        }

        #[test]
        fn what_was_put_in_comes_back_out() {
            let mut cache = Cache::new();
            cache.insert("one".to_owned(), bytes(4));

            assert_eq!(cache.get("one").map(|held| held.bytes.len()), Some(4));
        }

        #[test]
        fn something_never_put_in_is_a_miss() {
            assert!(Cache::new().get("one").is_none());
        }

        #[test]
        fn the_same_handle_twice_is_held_once() {
            let mut cache = Cache::new();
            cache.insert("one".to_owned(), bytes(4));
            cache.insert("one".to_owned(), bytes(8));

            assert_eq!(cache.held.len(), 1);
            assert_eq!(cache.get("one").map(|held| held.bytes.len()), Some(8));
        }

        #[test]
        fn the_least_recently_used_goes_first_when_the_count_is_reached() {
            let mut cache = Cache::new();
            for index in 0..=MAX_CACHED {
                cache.insert(index.to_string(), bytes(1));
            }

            assert!(cache.get("0").is_none());
            assert!(cache.get(&MAX_CACHED.to_string()).is_some());
        }

        #[test]
        fn reading_one_saves_it_from_the_next_eviction() {
            // A room of pictures scrolling past must not evict the clip
            // somebody is watching, which is read from on every seek.
            let mut cache = Cache::new();
            cache.insert("watched".to_owned(), bytes(1));
            for index in 0..MAX_CACHED - 1 {
                cache.insert(index.to_string(), bytes(1));
            }
            cache.get("watched");

            cache.insert("late".to_owned(), bytes(1));

            assert!(cache.get("watched").is_some());
        }

        #[test]
        fn a_large_attachment_pushes_the_rest_out_by_weight() {
            let mut cache = Cache::new();
            cache.insert("small".to_owned(), bytes(16));

            cache.insert("huge".to_owned(), bytes(MAX_CACHED_BYTES));

            assert!(cache.get("small").is_none());
            assert!(cache.get("huge").is_some());
        }

        #[test]
        fn an_attachment_larger_than_the_whole_cap_is_still_held() {
            // Refusing it would mean the one file somebody is actually
            // watching is the one refetched on every seek.
            let mut cache = Cache::new();

            cache.insert("enormous".to_owned(), bytes(MAX_CACHED_BYTES + 1));

            assert!(cache.get("enormous").is_some());
        }
    }
}
