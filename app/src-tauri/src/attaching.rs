// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Getting an attachment's bytes to Rust, which happens three ways.
//!
//! Two of them are a path and one of them is not, and that split is the whole
//! shape of this module.
//!
//! The file picker and a drag onto the window both end in a path, and the
//! webview has `core:default` and no filesystem capability, so the picker is
//! opened from Rust and the read is Rust's. What crosses to the page is
//! [`Chosen`]: a name and a length, enough to draw the thing waiting to be
//! sent, and no bytes at all. They are read once, at the moment somebody
//! presses send.
//!
//! A paste ends in bytes rather than a path, because a screenshot on the
//! clipboard is not a file anywhere. Those bytes are read here too, off the
//! desktop's own clipboard, rather than in the page: WebKitGTK hands a
//! `paste` event no image at all, so a composer that waited for one waited
//! forever. What crosses is [`Pasted`], a name and a length on the same terms
//! as [`Chosen`], and the picture stays on this side until it is sent.
//!
//! [`MAX_BYTES`] bounds all three. On the path side it is checked against the
//! file's length before the read rather than after it, which is the difference
//! between refusing a four-gigabyte file and running out of memory reading one.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use consort_matrix::timeline::MAX_BYTES;
use serde::Serialize;

use crate::commands::CommandError;

/// What the room will call a screenshot, which arrives with no name of its own.
pub const PASTED_NAME: &str = "pasted-image.png";

/// A file somebody chose, before anything has been read of it.
///
/// What the composer draws while an attachment is waiting to be sent. The path
/// goes back to Rust unread when they press send, on the same terms as an
/// attachment handle: it is this side's own string and the page does nothing
/// with it but hand it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chosen {
    pub path: String,
    /// The file's own name, which is what the room will call it.
    pub name: String,
    /// How many bytes it is, for saying what is about to be sent.
    pub size: u64,
}

/// One path as something to draw, or `None` when there is nothing there to
/// send.
///
/// A directory answers `None`, which is what dragging a folder onto the window
/// produces, and so does a file that cannot be read or whose name is not text.
/// All three are the same answer to the only question being asked.
pub fn chosen(path: &Path) -> Option<Chosen> {
    let facts = std::fs::metadata(path).ok()?;
    if !facts.is_file() {
        return None;
    }

    Some(Chosen {
        path: path.to_str()?.to_owned(),
        name: name_of(path)?,
        size: facts.len(),
    })
}

/// What the room will call this file.
///
/// The last component of the path, and never what the page said it was. The
/// page was handed this name by [`chosen`] a moment ago, so taking its word
/// would be believing a copy of something already known here, and the copy is
/// the one somebody could have changed.
pub fn name_of(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_str()?.to_owned())
}

/// One file's bytes, refused by its length before it is opened.
///
/// The order matters. Reading first and checking afterwards is the same
/// program with the bound removed: whatever it was protecting has already been
/// spent by the time the check runs.
pub fn read(path: &str) -> Result<Vec<u8>, CommandError> {
    let path = PathBuf::from(path);
    let facts = std::fs::metadata(&path).map_err(|error| unreadable(&path, &error))?;
    within_the_ceiling(facts.len())?;

    std::fs::read(&path).map_err(|error| unreadable(&path, &error))
}

/// What to call an attachment whose own name is not usable.
pub const FALLBACK_NAME: &str = "attachment";

/// The name to open a Save As window on, from the name a message carried.
///
/// The sender writes that name and nothing has checked it. It arrives here as
/// the dialog's starting filename, and a filename entry resolves a relative
/// path, so a message called `../../.bashrc` would offer to save somewhere
/// nobody chose. Only the last component survives, separators of either
/// platform, because the sender's is unknown.
pub fn suggested_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return FALLBACK_NAME.to_owned();
    }
    cleaned.to_owned()
}

/// A screenshot waiting in the composer, before it is sent.
///
/// The counterpart of [`Chosen`] for something with no path, and carrying no
/// more than it does: the bytes are held in `AppState` and addressed rather
/// than handed over, which is the same arrangement a verification flow uses
/// and for the same reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pasted {
    pub name: String,
    pub size: u64,
}

/// A picture off the clipboard, in the shape every platform hands it over in.
pub struct Screenshot {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The desktop's clipboard, as the two questions asked of it.
///
/// A trait so the rule about what a paste means is testable without one, the
/// way [`crate::secrets::Backend`] makes the keyring testable. `image` is
/// separate from `text` and not a field beside it because reading a
/// full-screen picture costs tens of megabytes, and an ordinary paste of words
/// must never pay that.
/// `Send + Sync` because a Tauri command's future crosses threads and this is
/// borrowed across the await that stores what was read.
pub trait Clipboard: Send + Sync {
    fn text(&self) -> Option<String>;
    fn image(&self) -> Option<Screenshot>;
}

/// A clipboard picture as the PNG that will be sent.
///
/// Every platform hands over raw pixels rather than a file format, so
/// something has to encode them, and PNG is what a screenshot is expected to
/// arrive as.
pub fn png_of(shot: &Screenshot) -> Result<Vec<u8>, CommandError> {
    let picture = image::RgbaImage::from_raw(shot.width, shot.height, shot.rgba.clone())
        .ok_or_else(|| {
            // Not unreachable: the dimensions and the buffer both come from
            // whatever owns the clipboard, which is another application.
            CommandError::new(
                "Consort could not read what was on the clipboard.",
                format!(
                    "{} bytes is not {}x{} pixels",
                    shot.rgba.len(),
                    shot.width,
                    shot.height
                ),
            )
        })?;

    let mut png = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| {
            CommandError::new(
                "Consort could not read what was on the clipboard.",
                format!("encoding a pasted screenshot: {error}"),
            )
        })?;

    within_the_ceiling(png.len() as u64)?;
    Ok(png)
}

/// Whether this many bytes is more than this build will hold at once.
///
/// Takes the length rather than the thing, which is what lets it be driven
/// from a test: the ceiling is half a gigabyte, and a test that had to produce
/// one would be half a gigabyte of temporary file per run.
fn within_the_ceiling(bytes: u64) -> Result<(), CommandError> {
    if bytes <= MAX_BYTES as u64 {
        return Ok(());
    }

    Err(CommandError::new(
        "That file is too large for Consort to send.",
        format!("{bytes} bytes is past the {MAX_BYTES} this build will hold"),
    ))
}

/// What to say about a file that will not open.
fn unreadable(path: &Path, error: &std::io::Error) -> CommandError {
    CommandError::new(
        "Consort could not read that file. It may have been moved or renamed.",
        format!("reading {}: {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    mod saving {
        use super::*;

        #[test]
        fn an_ordinary_name_is_left_alone() {
            assert_eq!(suggested_name("holiday.jpg"), "holiday.jpg");
            assert_eq!(suggested_name(".bashrc"), ".bashrc");
        }

        #[test]
        fn a_name_that_is_a_path_keeps_only_its_last_component() {
            // What a sender writes in `filename` reaches the Save As window as
            // the name it opens on, and a name entry resolves a relative path.
            assert_eq!(suggested_name("../../.bashrc"), ".bashrc");
            assert_eq!(suggested_name("/etc/passwd"), "passwd");
            assert_eq!(suggested_name(r"..\..\autorun.inf"), "autorun.inf");
        }

        #[test]
        fn control_characters_are_dropped() {
            assert_eq!(suggested_name("cat\n.png"), "cat.png");
            assert_eq!(suggested_name("cat\u{7f}.png"), "cat.png");
        }

        #[test]
        fn a_name_with_nothing_left_in_it_falls_back() {
            assert_eq!(suggested_name(""), FALLBACK_NAME);
            assert_eq!(suggested_name("   "), FALLBACK_NAME);
            assert_eq!(suggested_name("/"), FALLBACK_NAME);
            assert_eq!(suggested_name(".."), FALLBACK_NAME);
            assert_eq!(suggested_name("."), FALLBACK_NAME);
        }
    }

    #[test]
    fn a_file_is_named_and_measured() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.png");
        std::fs::write(&path, b"0123456789").unwrap();

        let chosen = chosen(&path).expect("a file that exists");

        assert_eq!(chosen.name, "cat.png");
        assert_eq!(chosen.size, 10);
        assert_eq!(chosen.path, path.to_str().unwrap());
    }

    #[test]
    fn a_path_with_no_last_component_names_nothing() {
        // Not reachable from the picker or from a drop, both of which answer
        // with real files. It is the parse having to answer something.
        assert_eq!(name_of(Path::new("/")), None);
        assert_eq!(name_of(Path::new("cat.png")).as_deref(), Some("cat.png"));
    }

    #[test]
    fn a_folder_is_not_something_to_send() {
        // What dragging a directory onto the window produces.
        let dir = tempfile::tempdir().unwrap();

        assert!(chosen(dir.path()).is_none());
    }

    #[test]
    fn something_that_is_not_there_is_not_something_to_send() {
        let dir = tempfile::tempdir().unwrap();

        assert!(chosen(&dir.path().join("gone.png")).is_none());
    }

    #[test]
    fn a_files_bytes_come_back_as_they_are_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cat.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();

        let bytes = read(path.to_str().unwrap()).expect("a file that exists");

        assert_eq!(bytes, b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn a_file_that_has_gone_since_it_was_chosen_is_reported_rather_than_a_panic() {
        // Ordinary rather than a bug: a picker and a send are two moments, and
        // a screenshot tool that cleans up after itself sits between them.
        let dir = tempfile::tempdir().unwrap();

        let error = read(dir.path().join("gone.png").to_str().unwrap()).unwrap_err();

        assert!(error.message().contains("moved or renamed"), "{error:?}");
    }

    #[test]
    fn a_file_past_the_ceiling_is_refused_by_its_length() {
        // Before it is read, which is the whole point: a bound applied after
        // the read has already spent what it exists to protect.
        let refused = within_the_ceiling(MAX_BYTES as u64 + 1).unwrap_err();

        assert!(refused.message().contains("too large"), "{refused:?}");
    }

    #[test]
    fn a_file_at_exactly_the_ceiling_is_still_sendable() {
        assert!(within_the_ceiling(MAX_BYTES as u64).is_ok());
    }

    #[test]
    fn a_clipboard_picture_encodes_to_a_png_of_the_same_pixels() {
        let shot = Screenshot {
            rgba: vec![
                255, 0, 0, 255, // red
                0, 255, 0, 255, // green
                0, 0, 255, 255, // blue
                255, 255, 255, 255, // white
            ],
            width: 2,
            height: 2,
        };

        let png = png_of(&shot).expect("four pixels");

        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let read_back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(read_back.dimensions(), (2, 2));
        assert_eq!(read_back.into_raw(), shot.rgba);
    }

    #[test]
    fn a_clipboard_picture_that_is_not_its_own_dimensions_is_reported_rather_than_a_panic() {
        // What is on the clipboard belongs to another application, so the two
        // disagreeing is that application's bug arriving here rather than ours.
        let shot = Screenshot {
            rgba: vec![255, 0, 0, 255],
            width: 64,
            height: 64,
        };

        let error = png_of(&shot).unwrap_err();

        assert!(error.message().contains("clipboard"), "{error:?}");
    }
}
