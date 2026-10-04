// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What somebody can share, and the seam that captures it.
//!
//! The list is decided here, as data, and read off a window manager in
//! `x11_host`. Why a window list exists at all on one platform and not the
//! other: `docs/adr/0006-share-a-screen-over-x11.md`.

use serde::{Deserialize, Serialize};

use crate::capture::{FrameSink, Resolution};

/// Whether a share points at a whole screen or at one window.
///
/// The two tabs the picker draws, and the two things a capture resolves
/// differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShareKind {
    Screen,
    Window,
}

/// One thing somebody could share, named for them to choose it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareSource {
    /// What to ask a capture for. Opaque to everything above this crate.
    pub id: String,
    /// What to put in the picker, and what the indicator says is going out.
    pub title: String,
    pub kind: ShareKind,
    /// Whether this window is filling a screen.
    ///
    /// Read by the picker to say which entry is the game, and by
    /// [`shareable`] to offer it first.
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
}

/// A window as the window manager describes it, before anything is decided.
///
/// Every field is one property read. Nothing here is interpreted: that is
/// [`shareable`]'s job, which is why this crosses into a test as a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeenWindow {
    pub id: u32,
    /// `_NET_WM_NAME`, which is frequently empty.
    pub title: String,
    /// `WM_CLASS`, which is the application rather than the document.
    pub app: String,
    /// `_NET_WM_PID`, absent on a window that does not set it.
    pub pid: Option<u32>,
    /// Whether `_NET_WM_WINDOW_TYPE` is normal, or absent and so assumed to be.
    pub normal: bool,
    pub skip_taskbar: bool,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
}

/// What a window with no name of its own is called.
const UNNAMED: &str = "Untitled window";

/// Which of `seen` somebody may share, in the order to offer them.
///
/// `seen` arrives in stacking order, bottom first, because that is how
/// `_NET_CLIENT_LIST_STACKING` reports it. What comes back is topmost first,
/// with any fullscreen window ahead of that: somebody who alt-tabbed out of a
/// game to share it has left the game below the window they tabbed to.
///
/// `ours` is this process's pid, and the windows carrying it come out. Why the
/// ordering is this rather than by resource use: issue #70.
pub fn shareable(seen: Vec<SeenWindow>, ours: u32) -> Vec<ShareSource> {
    let mut offered: Vec<ShareSource> = seen
        .into_iter()
        .rev()
        .filter(|window| {
            window.normal
                && !window.skip_taskbar
                && window.width > 0
                && window.height > 0
                && window.pid != Some(ours)
        })
        .map(|window| ShareSource {
            id: format!("window:{}", window.id),
            title: name(&window),
            kind: ShareKind::Window,
            fullscreen: window.fullscreen,
            width: window.width,
            height: window.height,
        })
        .collect();

    // Stable, so the stacking order above decides between two fullscreen
    // windows rather than this.
    offered.sort_by_key(|source| !source.fullscreen);
    offered
}

/// What to call `window` in a picker.
///
/// The title, then the application, then a placeholder. A row with nothing in
/// it cannot be chosen deliberately, and for a screen share that is the one
/// thing that must not happen.
fn name(window: &SeenWindow) -> String {
    [window.title.trim(), window.app.trim()]
        .into_iter()
        .find(|candidate| !candidate.is_empty())
        .unwrap_or(UNNAMED)
        .to_owned()
}

/// One monitor, as a thing to share.
///
/// Separate from [`SeenWindow`] because a monitor has no window manager
/// properties to interpret: the connector name and the geometry are the whole
/// of it.
pub fn screen(connector: &str, width: u32, height: u32) -> ShareSource {
    ShareSource {
        id: format!("screen:{connector}"),
        title: format!("{connector} ({width}x{height})"),
        kind: ShareKind::Screen,
        fullscreen: false,
        width,
        height,
    }
}

/// Everything that can stop a share before its first frame.
///
/// Its own type rather than [`crate::CaptureError`], which names a camera in
/// every sentence it has. The failures are also genuinely different: a screen
/// is never busy, and a camera is never absent because of which display
/// server is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareError {
    /// This build has no screen capture at all.
    Unsupported,
    /// There is no X11 display to read.
    ///
    /// Which on a desktop session means Wayland, and the sentence says so:
    /// the portal path this would need is not built. See ADR-0006.
    NoX11,
    /// The chosen screen or window is not there any more.
    Gone { source: String },
    /// The display server said no.
    Backend(String),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "this build cannot share a screen"),
            Self::NoX11 => write!(
                f,
                "sharing a screen needs an X11 session; this one is not, so \
                 Consort cannot read it. Wayland needs the desktop portal, \
                 which is not built yet"
            ),
            Self::Gone { source } => {
                write!(f, "{source} is no longer on screen; choose again")
            }
            Self::Backend(message) => write!(f, "the display server refused: {message}"),
        }
    }
}

impl std::error::Error for ShareError {}

impl ShareError {
    /// One sentence for the interface to draw.
    ///
    /// Separate from [`Display`](std::fmt::Display) for the reason
    /// [`crate::CaptureError::user_message`] gives: the one variant carrying
    /// the display server's own words must not put them in front of anybody.
    pub fn user_message(&self) -> String {
        match self {
            Self::Backend(_) => "the screen could not be captured".to_owned(),
            other => other.to_string(),
        }
    }
}

/// A share that is running, held for as long as its frames are wanted.
///
/// Dropping it stops the capture. The same contract
/// [`crate::CameraStream`] has, and for the same reason: the one thing a
/// caller must be able to do without ceremony is make it stop.
pub trait ShareStream: Send {
    /// What is being captured, for the indicator to name.
    fn source(&self) -> &ShareSource;

    /// The size the frames are actually arriving at, which is what a
    /// publication has to be set up for.
    fn resolution(&self) -> Resolution;
}

/// Something that can list and capture what is on this machine's screen.
///
/// The seam ADR-0006 describes. One implementation reads X11; the fallback
/// finds nothing, so a build on a platform with no backend offers an empty
/// picker rather than failing to start.
pub trait ScreenCapture: Send + Sync {
    /// Everything shareable right now, screens and windows together.
    ///
    /// Screens first, because they are the answer that is always right. Read
    /// fresh every time: windows open and close while a picker is on screen.
    fn sources(&self) -> Result<Vec<ShareSource>, ShareError>;

    /// Start capturing `id`, pushing frames at `on_frame`.
    ///
    /// `id` is a [`ShareSource::id`] from this same backend. One that no
    /// longer names anything is [`ShareError::Gone`]: a window can close
    /// between the picker being drawn and somebody clicking it.
    ///
    /// `on_frame` runs on the capture thread, so an implementation must keep
    /// it to a queue offer, exactly as [`crate::VideoCapture::open`] requires.
    fn open(&self, id: &str, on_frame: FrameSink) -> Result<Box<dyn ShareStream>, ShareError>;
}

/// The backend for a build with no screen capture.
///
/// Refuses to list as well as to open. An empty list draws "there is nothing
/// here to share" over a desktop full of windows, which is #163; a refusal
/// draws the reason.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoScreens;

impl ScreenCapture for NoScreens {
    fn sources(&self) -> Result<Vec<ShareSource>, ShareError> {
        Err(ShareError::Unsupported)
    }

    fn open(&self, _id: &str, _on_frame: FrameSink) -> Result<Box<dyn ShareStream>, ShareError> {
        Err(ShareError::Unsupported)
    }
}
