// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Between the sound card and the model.
//!
//! A backend delivers interleaved samples, in its own sample format, in buffers
//! sized to suit itself. The gate accepts exactly [`FRAME_SAMPLES`] mono `i16`
//! samples and nothing else. This closes that gap, and it is the only part of
//! the capture path that is arithmetic rather than glue, so it lives away from
//! the cpal call that cannot be tested.
//!
//! Deliberately no resampling. 48 kHz is the only rate RNNoise accepts and it
//! is also what the media layer wants, so a device that cannot do 48 kHz is an
//! error at startup rather than a silent quality loss.

use crate::gate::FRAME_SAMPLES;

/// Accumulates a stream of interleaved samples into whole mono frames.
pub struct Frames {
    channels: usize,
    pending: Vec<i16>,
}

impl Frames {
    /// `channels` is what the device negotiated.
    pub fn new(channels: u16) -> Self {
        Self {
            // Nothing should claim zero channels, but dividing by it would
            // panic inside a realtime audio callback, which is the worst place
            // in the program to find out.
            channels: usize::from(channels).max(1),
            pending: Vec::with_capacity(FRAME_SAMPLES),
        }
    }

    /// Feed one buffer of `i16` samples.
    ///
    /// `on_frame` runs once per whole frame completed. On the capture path it
    /// runs on the backend's realtime thread, so keep it to a channel send:
    /// anything that blocks, allocates heavily, or locks turns into crackle.
    pub fn push_i16(&mut self, data: &[i16], mut on_frame: impl FnMut(&[i16])) {
        for group in data.chunks_exact(self.channels) {
            // Summed as i32 because two channels at full scale do not fit in an
            // i16, and wrapping there would turn the loudest possible signal
            // into noise.
            let sum: i32 = group.iter().copied().map(i32::from).sum();
            let mono = sum / self.channels as i32;
            self.push(clamp_i32(mono), &mut on_frame);
        }
    }

    /// Feed one buffer of `f32` samples, which cpal delivers in `[-1.0, 1.0]`.
    pub fn push_f32(&mut self, data: &[f32], mut on_frame: impl FnMut(&[i16])) {
        for group in data.chunks_exact(self.channels) {
            let mono = group.iter().sum::<f32>() / self.channels as f32;
            self.push(clamp_f32(mono * f32::from(i16::MAX)), &mut on_frame);
        }
    }

    fn push(&mut self, sample: i16, on_frame: &mut impl FnMut(&[i16])) {
        self.pending.push(sample);
        if self.pending.len() == FRAME_SAMPLES {
            on_frame(&self.pending);
            self.pending.clear();
        }
    }
}

fn clamp_i32(sample: i32) -> i16 {
    sample.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn clamp_f32(sample: f32) -> i16 {
    sample.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}
