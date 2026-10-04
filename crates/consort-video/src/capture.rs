// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Opening a camera, as a trait, and deciding what to ask it for.
//!
//! The trait is what lets everything above it be tested on a machine with no
//! camera. The real implementation is in [`crate::v4l_host`].

use std::fmt;

use crate::pixels::{Picture, PixelFormat};

/// What to ask a camera for.
///
/// 720p30 because it is what every other client publishes and what people
/// judge a call by immediately. The publication is simulcast, so a receiver
/// drawing a small tile takes a smaller layer rather than this one.
pub const WANTED: Resolution = Resolution {
    width: 1280,
    height: 720,
    fps: 30,
};

/// A frame size and rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Resolution {
    fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// One thing a camera said it can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    /// The best frame rate the device offers at this format and size.
    pub fps: u32,
}

impl Offer {
    fn size(self) -> Resolution {
        Resolution {
            width: self.width,
            height: self.height,
            fps: self.fps,
        }
    }
}

/// Pick what to open, out of everything a camera offers.
///
/// Rate, then nearest size, then YUYV to break a tie. Rate first because a
/// C920 offers YUYV at 720p and ten frames a second against MJPEG at thirty,
/// and ten is not a video call. See `docs/PLAN-webcam.md`.
pub fn choose_offer(offers: &[Offer], want: Resolution) -> Option<Offer> {
    offers
        .iter()
        .min_by_key(|offer| {
            (
                offer.fps < want.fps,
                offer.size().area().abs_diff(want.area()),
                offer.format != PixelFormat::Yuyv,
            )
        })
        .copied()
}

/// Everything that can go wrong before the first frame arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// This machine offers no camera at all.
    NoCamera,
    /// The requested camera is not among the ones this machine offers.
    UnknownCamera {
        requested: String,
        available: Vec<String>,
    },
    /// The camera is there and offers nothing this can read. Its own variant
    /// because the screen has to name the device: nothing here decodes H.264,
    /// and a camera offering only that is not a camera failure.
    NoUsableFormat { camera: String },
    /// Somebody else has it open. Common enough to be worth its own sentence,
    /// because the fix is to close the other application rather than to
    /// investigate anything.
    Busy { camera: String },
    /// Windows' camera privacy switch is keeping desktop applications out.
    ///
    /// Its own variant because the camera works and the fix is a setting,
    /// which nothing about "access denied" says.
    Blocked,
    /// The driver said no.
    Backend(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCamera => write!(f, "there is no camera on this machine"),
            Self::UnknownCamera {
                requested,
                available,
            } => {
                write!(f, "no camera at {requested:?}")?;
                if available.is_empty() {
                    return Ok(());
                }
                write!(f, "; this machine offers {}", available.join(", "))
            }
            Self::NoUsableFormat { camera } => write!(
                f,
                "{camera:?} offers no format Consort can read, which needs YUYV or MJPEG"
            ),
            Self::Busy { camera } => {
                write!(f, "{camera:?} is already in use by another application")
            }
            Self::Blocked => write!(
                f,
                "Windows is keeping desktop apps away from the camera; turn on \
                 \"Let desktop apps access your camera\" under Settings, \
                 Privacy & security, Camera"
            ),
            Self::Backend(message) => write!(f, "the camera failed: {message}"),
        }
    }
}

impl std::error::Error for CaptureError {}

impl CaptureError {
    /// One sentence for the interface to draw.
    ///
    /// Separate from [`Display`](fmt::Display), which is for the log, because
    /// the one variant that carries a driver's own words should not put them in
    /// front of somebody: "No such file or directory (os error 2)" is a fact
    /// about an `ioctl` and not something anybody can act on.
    pub fn user_message(&self) -> String {
        match self {
            Self::Backend(_) => "the camera could not be started".to_owned(),
            other => other.to_string(),
        }
    }
}

/// Where captured frames go.
///
/// Called on the capture thread, once per frame, already converted to I420.
pub type FrameSink = Box<dyn FnMut(Picture) + Send>;

/// A running camera. Dropping it releases the device.
pub trait CameraStream: Send {
    /// The camera this actually opened, by the id a saved choice holds.
    fn camera_id(&self) -> &str;

    /// What was negotiated, which is not always what was asked for.
    fn resolution(&self) -> Resolution;
}

/// Somewhere to open a camera.
pub trait VideoCapture: Send + Sync + 'static {
    /// Open the camera with id `camera`, or the first one when it is `None`,
    /// and deliver I420 frames to `on_frame`.
    ///
    /// `on_frame` runs on the capture thread, so an implementation must keep
    /// it to a queue offer. Blocking there drops frames at the device.
    fn open(
        &self,
        camera: Option<&str>,
        want: Resolution,
        on_frame: FrameSink,
    ) -> Result<Box<dyn CameraStream>, CaptureError>;
}
