// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Opening a microphone, as a trait.
//!
//! The trait is what lets the thread in [`crate::thread`] be tested on a
//! machine with no sound card. The real implementation is in
//! [`crate::cpal_host`].

use std::fmt;

/// Everything that can go wrong before the first frame arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The host offers no input device at all.
    NoDevice,
    /// The requested device is not among the ones the host offers.
    UnknownDevice {
        requested: String,
        available: Vec<String>,
    },
    /// The device cannot run at 48 kHz.
    ///
    /// Its own variant because nothing here resamples, so this is a refusal
    /// rather than a degradation, and the screen has to be able to say which
    /// device and why rather than just failing.
    NoFortyEightKilohertz { device: String },
    /// The device offers a sample format this does not handle.
    UnsupportedFormat { device: String, format: String },
    /// The audio backend said no.
    Backend(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "there is no audio input device"),
            Self::UnknownDevice {
                requested,
                available,
            } => {
                write!(f, "no input device called {requested:?}")?;
                if available.is_empty() {
                    return Ok(());
                }
                write!(f, "; this machine offers {}", available.join(", "))
            }
            Self::NoFortyEightKilohertz { device } => write!(
                f,
                "{device:?} cannot run at 48 kHz, which the voice gate requires \
                 and Consort does not resample around"
            ),
            Self::UnsupportedFormat { device, format } => write!(
                f,
                "{device:?} offers samples as {format}, and only f32 and i16 are \
                 handled"
            ),
            Self::Backend(message) => write!(f, "the audio backend failed: {message}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Where captured frames go.
///
/// Called on the backend's realtime thread, once per whole frame.
pub type FrameSink = Box<dyn FnMut(&[i16]) + Send>;

/// A running capture stream. Dropping it stops the microphone.
pub trait CaptureStream: Send {
    /// The device this actually opened, which is not always the one asked for.
    fn device_name(&self) -> &str;
}

/// Somewhere to open a microphone.
pub trait AudioCapture: Send + 'static {
    /// Open `device`, or the host's default when it is `None`, and deliver mono
    /// 48 kHz `i16` frames of [`crate::FRAME_SAMPLES`] samples to `on_frame`.
    ///
    /// `on_frame` runs on the backend's realtime thread. Implementations must
    /// keep it to a channel send: anything that blocks, allocates heavily or
    /// takes a lock shows up as crackle.
    fn open(
        &self,
        device: Option<&str>,
        on_frame: FrameSink,
    ) -> Result<Box<dyn CaptureStream>, CaptureError>;
}
