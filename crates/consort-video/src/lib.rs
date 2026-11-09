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

#[cfg(windows)]
mod com;

#[cfg(windows)]
pub mod mf_host;

#[cfg(windows)]
pub mod wgc_host;

pub use capture::{CameraStream, CaptureError, FrameSink, Resolution, VideoCapture};
pub use devices::{Camera, CameraDevices, CameraList, NoCameras, Selection, catalogue, choose};
pub use pixels::{FrameError, Picture, PixelFormat, decode, from_bgra, to_rgb};
pub use screens::{
    NoScreens, ScreenCapture, SeenWindow, ShareError, ShareKind, ShareSource, ShareStream, screen,
    shareable,
};
pub use settings::{Sending, VideoSettings};

/// The camera backend for this build.
///
/// V4L2 on Linux and Media Foundation on Windows. Anywhere else a session
/// should still start, so the absence is a host that lists nothing and says
/// the build is why when asked to open one. The `cfg`s are here and nowhere
/// else, which keeps them out of every call site.
#[cfg(target_os = "linux")]
pub type Host = v4l_host::V4lHost;

#[cfg(windows)]
pub type Host = mf_host::MfHost;

#[cfg(not(any(target_os = "linux", windows)))]
pub type Host = devices::NoCameras;

/// The screen capture backend for this build.
///
/// X11 on Linux and Windows.Graphics.Capture on Windows. Wayland would be a
/// second Linux host behind the same trait and is not built: see
/// `docs/adr/0006-share-a-screen-over-x11.md`. Anywhere else the host refuses
/// and says the build is why.
#[cfg(target_os = "linux")]
pub type Screens = x11_host::X11Host;

#[cfg(windows)]
pub type Screens = wgc_host::WgcHost;

#[cfg(not(any(target_os = "linux", windows)))]
pub type Screens = screens::NoScreens;
