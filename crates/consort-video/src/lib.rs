// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Cameras, and the frames they produce. No Matrix and no SFU.
//!
//! The other half of what `consort-audio` is: a hardware backend behind a
//! trait, so the crate that carries a call is handed frames rather than
//! opening a device. Why the camera is read here at all rather than through
//! the webview's `getUserMedia`: `docs/adr/0005-capture-the-camera-in-rust.md`.

pub mod capture;
pub mod devices;
pub mod pixels;
pub mod screens;
pub mod settings;
pub mod win32;

#[cfg(target_os = "linux")]
pub mod v4l_host;

#[cfg(target_os = "linux")]
pub mod x11_host;

pub use capture::{CameraStream, CaptureError, FrameSink, Resolution, VideoCapture};
pub use devices::{Camera, CameraDevices, CameraList, NoCameras, Selection, catalogue, choose};
pub use pixels::{FrameError, Picture, PixelFormat, decode, from_bgra, to_rgb};
pub use screens::{
    NoScreens, ScreenCapture, SeenWindow, ShareError, ShareKind, ShareSource, ShareStream, screen,
    shareable,
};
pub use settings::VideoSettings;

/// The camera backend for this build.
///
/// V4L2 is kernel ioctls on `/dev/videoN`, so it is Linux and nothing else.
/// The Windows release build has to compile, and a session on a platform with
/// no backend should offer an empty picker rather than fail to start, so the
/// absence is a host that finds nothing. One `cfg` pair, here, keeps it out of
/// every call site.
#[cfg(target_os = "linux")]
pub type Host = v4l_host::V4lHost;

#[cfg(not(target_os = "linux"))]
pub type Host = devices::NoCameras;

/// The screen capture backend for this build.
///
/// X11 and nothing else, for the same reason [`Host`] is V4L2 and nothing
/// else. Wayland would be a second host behind the same trait and is not
/// built: see `docs/adr/0006-share-a-screen-over-x11.md`.
#[cfg(target_os = "linux")]
pub type Screens = x11_host::X11Host;

#[cfg(not(target_os = "linux"))]
pub type Screens = screens::NoScreens;
