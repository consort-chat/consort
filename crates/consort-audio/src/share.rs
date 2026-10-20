// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Capturing what this machine is playing, to send with a shared screen.
//!
//! A screen share's sound is not the microphone and must never be mixed into
//! it: a listener who turns the sharer down would lose the share with them,
//! and a microphone carrying what the speakers are playing feeds back. So this
//! is a second capture feeding a second publication, and it is a trait for the
//! reason [`crate::AudioCapture`] is: only one platform has it.
//!
//! Which platforms, and why Linux is not one of them:
//! `docs/adr/0020-send-a-shares-sound-where-a-build-can-capture-it.md`.

use crate::capture::{CaptureError, CaptureStream, FrameSink};

/// Somewhere to open the sound a machine is playing.
pub trait ShareSound: Send + Sync + 'static {
    /// Whether this build can capture it at all.
    ///
    /// Read before anything is drawn. A switch for a capture that cannot
    /// happen is a switch that lies about what it does.
    fn available(&self) -> bool;

    /// Open the sound coming out of `device`, or out of the host's default
    /// output, and deliver mono 48 kHz `i16` frames of
    /// [`crate::FRAME_SAMPLES`] samples to `on_frame`.
    ///
    /// `on_frame` runs on the backend's realtime thread, on the same terms as
    /// [`crate::AudioCapture::open`]: keep it to a channel send.
    fn open(
        &self,
        device: Option<&str>,
        on_frame: FrameSink,
    ) -> Result<Box<dyn CaptureStream>, CaptureError>;
}

/// A build that cannot capture what the machine is playing.
///
/// Everywhere but Windows. It refuses rather than returning silence, so the
/// one caller that matters can tell the difference.
#[derive(Default)]
pub struct NoShareSound;

impl ShareSound for NoShareSound {
    fn available(&self) -> bool {
        false
    }

    fn open(
        &self,
        _device: Option<&str>,
        _on_frame: FrameSink,
    ) -> Result<Box<dyn CaptureStream>, CaptureError> {
        Err(CaptureError::NoShareSound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_that_cannot_capture_a_share_says_so_rather_than_offering_it() {
        // What the switch is drawn from. Answering yes here and failing at the
        // open is a control somebody turns on and watches do nothing.
        assert!(!NoShareSound.available());
    }

    #[test]
    fn opening_a_share_sound_this_build_has_no_backend_for_names_the_build() {
        let opened = NoShareSound.open(None, Box::new(|_| {}));

        assert_eq!(opened.err(), Some(CaptureError::NoShareSound));
    }

    #[test]
    fn the_refusal_blames_the_build_rather_than_the_machine() {
        // Somebody reading this in a log should not go looking for a missing
        // device on a platform where there was never anything to find.
        let said = CaptureError::NoShareSound.to_string();

        assert!(said.contains("build"), "{said}");
    }
}
