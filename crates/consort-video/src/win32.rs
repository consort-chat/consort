// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What Windows says about cameras and windows, read into the terms the rest
//! of this crate decides in.
//!
//! Plain data and no Windows API, so it compiles everywhere and the Linux CI
//! that never builds `mf_host` or `wgc_host` still tests every decision they
//! make. The hosts read a property and hand it here; nothing is interpreted
//! where it cannot be tested.

use crate::capture::CaptureError;
use crate::pixels::PixelFormat;
use crate::screens::SeenWindow;

/// The format a Media Foundation video subtype names, where this can read it.
///
/// Every video subtype GUID Windows defines for a camera format is a FourCC
/// in its first field, so that is all that is passed.
pub fn media_subtype(fourcc: [u8; 4]) -> Option<PixelFormat> {
    match &fourcc {
        b"YUY2" => Some(PixelFormat::Yuyv),
        b"MJPG" => Some(PixelFormat::Mjpeg),
        b"NV12" => Some(PixelFormat::Nv12),
        _ => None,
    }
}

/// `MF_MT_FRAME_SIZE`, which packs width over height into one `u64`.
pub fn frame_size(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// `MF_MT_FRAME_RATE`, which packs a numerator over a denominator, as whole
/// frames a second.
///
/// Rounded rather than truncated, so the 30000/1001 a camera reports for 30
/// is not ranked below a target of 30 by [`crate::capture::choose_offer`].
pub fn frame_rate(packed: u64) -> u32 {
    let (numerator, denominator) = (packed >> 32, packed & u64::from(u32::MAX));
    if denominator == 0 {
        return 30;
    }
    ((numerator + denominator / 2) / denominator) as u32
}

const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
const MF_E_HW_MFT_FAILED_START_STREAMING: i32 = 0xC00D_3704_u32 as i32;
const MF_E_VIDEO_DEVICE_LOCKED: i32 = 0xC00D_4E24_u32 as i32;
const MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED: i32 = 0xC00D_3EA3_u32 as i32;

/// Name a Media Foundation failure the way somebody can act on.
///
/// `code` is the `HRESULT`. Written out here rather than taken from the
/// `windows` crate so this compiles on every platform; `mf_host` pins each
/// one against the crate's own constant.
pub fn camera_failure(code: i32, camera: &str) -> CaptureError {
    match code {
        E_ACCESSDENIED => CaptureError::Blocked,
        MF_E_HW_MFT_FAILED_START_STREAMING
        | MF_E_VIDEO_DEVICE_LOCKED
        | MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED => CaptureError::Busy {
            camera: camera.to_owned(),
        },
        other => CaptureError::Backend(format!("{camera}: HRESULT {:#x}", other as u32)),
    }
}

/// A rectangle in screen pixels, edges rather than a size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    fn width(self) -> u32 {
        self.right.saturating_sub(self.left).max(0) as u32
    }

    fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top).max(0) as u32
    }

    fn covers(self, other: Rect) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
}

/// A top-level window as Win32 describes it, before anything is decided.
///
/// Every field is one call's answer, which is why this crosses into a test as
/// a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopLevel {
    /// The `HWND`. A window handle fits in 32 bits on 64-bit Windows, which
    /// is what lets a 32-bit process address one.
    pub handle: u32,
    /// `GetWindowTextW`, which is frequently empty.
    pub title: String,
    /// The executable's file name without its extension.
    pub app: String,
    pub pid: u32,
    /// `IsWindowVisible`.
    pub visible: bool,
    /// `DWMWA_CLOAKED`: hidden by the compositor while reporting itself
    /// visible, which is a suspended store app or another virtual desktop.
    pub cloaked: bool,
    /// `IsIconic`.
    pub minimised: bool,
    /// `WS_EX_TOOLWINDOW`.
    pub tool_window: bool,
    /// `WS_EX_APPWINDOW`.
    pub app_window: bool,
    /// Whether `GW_OWNER` names another window, as a dialog's does.
    pub owned: bool,
    /// `DWMWA_EXTENDED_FRAME_BOUNDS`, which is what a capture produces and
    /// leaves out the invisible resize border `GetWindowRect` includes.
    pub bounds: Rect,
    /// The monitor the window is mostly on.
    pub monitor: Rect,
}

/// Every window `EnumWindows` reported, as [`crate::screens::shareable`]
/// expects them.
///
/// `EnumWindows` reports topmost first and `shareable` takes the bottom-first
/// order X11 reports, so the list is reversed here and nowhere else.
///
/// What counts as a window somebody could mean is the taskbar's own rule: a
/// window gets a button if it asks for one with `WS_EX_APPWINDOW`, or if it is
/// neither owned nor a tool window. A minimised window is left out because
/// Windows.Graphics.Capture produces no frames from one.
pub fn seen(top_first: Vec<TopLevel>) -> Vec<SeenWindow> {
    top_first
        .into_iter()
        .rev()
        .map(|window| SeenWindow {
            id: window.handle,
            normal: window.visible && !window.cloaked && !window.minimised,
            skip_taskbar: !(window.app_window || (!window.owned && !window.tool_window)),
            fullscreen: window.bounds.covers(window.monitor),
            width: window.bounds.width(),
            height: window.bounds.height(),
            pid: Some(window.pid),
            title: window.title,
            app: window.app,
        })
        .collect()
}

/// What to call a monitor, out of the device name `GetMonitorInfoW` gives it.
///
/// `\\.\DISPLAY1` is a path in the Win32 device namespace; the part after it
/// is the name, and the part a share's id carries.
pub fn monitor_name(device: &str) -> &str {
    device.strip_prefix(r"\\.\").unwrap_or(device)
}
