// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Playing a sound out of a chosen device, as a trait.
//!
//! The mirror of [`crate::capture`], and a trait for the same reason: it lets
//! [`crate::thread`] be tested with no sound card. Two things come out of
//! here, the test chime in [`crate::tone`] and the call through
//! [`crate::mixing`], which this crate has to mix itself.

use std::fmt;

use crate::mixing::Voices;
use crate::tone::Tone;

/// Everything that can go wrong before the first sample is played.
///
/// Its own type rather than [`crate::CaptureError`], which says "input" in
/// four of its five messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackError {
    /// The host offers no output device at all.
    NoDevice,
    /// The requested device is not among the ones the host offers.
    UnknownDevice {
        requested: String,
        available: Vec<String>,
    },
    /// The device cannot run at 48 kHz, which nothing here resamples around.
    ///
    /// Not reachable from the picker, which lists only devices that said they
    /// can. Reachable between the list being drawn and the button pressed.
    NoFortyEightKilohertz { device: String },
    /// The device offers a sample format this does not handle.
    UnsupportedFormat { device: String, format: String },
    /// The audio backend said no.
    Backend(String),
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "there is no audio output device"),
            Self::UnknownDevice {
                requested,
                available,
            } => {
                write!(f, "no output device called {requested:?}")?;
                if available.is_empty() {
                    return Ok(());
                }
                write!(f, "; this machine offers {}", available.join(", "))
            }
            Self::NoFortyEightKilohertz { device } => write!(
                f,
                "{device:?} cannot run at 48 kHz, which is the rate Consort \
                 produces sound at and does not resample around"
            ),
            Self::UnsupportedFormat { device, format } => write!(
                f,
                "{device:?} wants samples as {format}, and only f32 and i16 are \
                 handled"
            ),
            Self::Backend(message) => write!(f, "the audio backend failed: {message}"),
        }
    }
}

impl std::error::Error for PlaybackError {}

/// Told once, when the last sample of the chime has been handed to the device.
///
/// Called on the backend's realtime thread, so it must do no more than a
/// channel send. `FnMut` rather than `FnOnce` because a cpal callback is
/// `FnMut` and cannot give up ownership. Implementations call it once.
pub type ToneEnded = Box<dyn FnMut() + Send>;

/// A sound in progress. Dropping it silences the device and gives it back.
pub trait PlaybackStream: Send {
    /// The device this actually opened, which is not always the one asked for.
    fn device_name(&self) -> &str;
}

/// Somewhere to play a sound.
pub trait AudioPlayback: Send + 'static {
    /// Play `tone` through `device`, or through the host's default when it is
    /// `None`, and call `on_end` once it has finished.
    ///
    /// Returns as soon as the sound has started, not when it has finished.
    /// The returned stream is what keeps the device open.
    fn play(
        &self,
        device: Option<&str>,
        tone: Tone,
        on_end: ToneEnded,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError>;

    /// Play everything in `voices` through `device` until the returned stream
    /// is dropped.
    ///
    /// A second method rather than a `Tone` that never ends and an `on_end`
    /// never called. Both may be open at once, because the test button is most
    /// useful during the call it is testing the output of.
    fn play_call(
        &self,
        device: Option<&str>,
        voices: Voices,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError>;
}

/// A [`Tone`] being handed to a device, one buffer at a time.
///
/// The mirror of [`crate::frames::Frames`]: the chime spread across however
/// many channels the device negotiated, in whichever format it asked for.
/// Here rather than in [`crate::cpal_host`] so it can be checked without a
/// sound card, and it owns the only answer to "is it over yet".
pub struct Playing {
    tone: Tone,
    channels: usize,
    /// Whether the last sample of the chime has been written.
    over: bool,
    /// Whether [`on_end`](Self::on_end) has been called for it. Separate from
    /// `over`, or every buffer the device asks for afterwards reports the end
    /// again.
    announced: bool,
    on_end: ToneEnded,
}

impl Playing {
    /// `channels` is what the device negotiated.
    pub fn new(tone: Tone, channels: u16, on_end: ToneEnded) -> Self {
        Self {
            tone,
            // Nothing should claim zero channels, but dividing by it panics
            // inside a realtime callback. `Frames` guards the same thing.
            channels: usize::from(channels).max(1),
            over: false,
            announced: false,
            on_end,
        }
    }

    /// Fill one buffer of `i16` samples, interleaved.
    pub fn fill_i16(&mut self, data: &mut [i16]) {
        for group in data.chunks_mut(self.channels) {
            let sample = self.next();
            // The same sample in every channel, or a chime out of the left
            // speaker tells somebody their right speaker is broken.
            group.fill(sample);
        }
        self.announce();
    }

    /// Fill one buffer of `f32` samples, which cpal wants in `[-1.0, 1.0]`.
    pub fn fill_f32(&mut self, data: &mut [f32]) {
        for group in data.chunks_mut(self.channels) {
            // 32768, not `i16::MAX`: the range is asymmetric and `i16::MIN`
            // over `i16::MAX` is past -1.0.
            let sample = f32::from(self.next()) / 32_768.0;
            group.fill(sample);
        }
        self.announce();
    }

    /// The next mono sample, and silence once the chime is over.
    fn next(&mut self) -> i16 {
        let mut one = [0i16; 1];
        if !self.tone.fill(&mut one) {
            self.over = true;
        }
        one[0]
    }

    /// Say the chime has finished, once.
    fn announce(&mut self) {
        if self.over && !self.announced {
            self.announced = true;
            (self.on_end)();
        }
    }
}
