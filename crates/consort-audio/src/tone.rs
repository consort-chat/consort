// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The sound the output test makes.
//!
//! Arithmetic only, and no allocation after construction, because this runs
//! inside the backend's realtime callback. Kept out of [`crate::cpal_host`],
//! which needs hardware, so a test can listen to it.

use std::f32::consts::TAU;

use crate::gate::SAMPLE_RATE;

/// The two notes, in hertz: A4 then E5, a rising fifth.
///
/// Two rather than one, because a single steady tone is what a badly grounded
/// audio interface produces on its own.
const NOTES: [f32; 2] = [440.0, 659.25];

/// How long each note lasts.
const NOTE_MS: usize = 160;

/// Samples in one note.
const NOTE_SAMPLES: usize = SAMPLE_RATE as usize * NOTE_MS / 1000;

/// How long each note takes to arrive and to leave.
///
/// Not zero: a note beginning at full amplitude begins with a click, and a
/// click comes out of every speaker ever made, working or not.
const FADE_MS: usize = 12;

/// Samples in one fade.
const FADE_SAMPLES: usize = SAMPLE_RATE as usize * FADE_MS / 1000;

/// Peak amplitude as a fraction of full scale.
///
/// Quiet on purpose: this gets pressed by somebody who has just plugged in
/// headphones and has no idea where the system volume is.
const AMPLITUDE: f32 = 0.22;

/// The test chime, played once.
///
/// Carries its own position, so a fresh one always sounds the same and two of
/// them cannot interfere.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tone {
    /// How far through the chime the last [`fill`](Self::fill) got to.
    at: usize,
}

impl Tone {
    /// Samples in the whole chime.
    pub const SAMPLES: usize = NOTES.len() * NOTE_SAMPLES;

    /// A chime, at the beginning.
    pub fn check() -> Self {
        Self::default()
    }

    /// Write the next `out.len()` samples, and say whether there are more.
    ///
    /// `false` means the chime has finished. Filling again after that writes
    /// silence, because a device goes on asking until the stream is dropped.
    pub fn fill(&mut self, out: &mut [i16]) -> bool {
        for (offset, sample) in out.iter_mut().enumerate() {
            *sample = amplitude_at(self.at + offset);
        }
        // Saturating: a caller may keep filling forever, and wrapping round to
        // the start of the chime would be a stutter.
        self.at = self.at.saturating_add(out.len()).min(Self::SAMPLES);
        self.at < Self::SAMPLES
    }
}

/// The chime's amplitude at one sample position, as an `i16`.
///
/// Pure in the position, which is what makes the sound independent of the
/// buffer sizes a backend happens to choose.
fn amplitude_at(at: usize) -> i16 {
    let Some(frequency) = NOTES.get(at / NOTE_SAMPLES) else {
        return 0;
    };
    let within = at % NOTE_SAMPLES;

    // Phase from the start of each note, not of the chime, so both begin at
    // zero and the join between them is silent.
    let phase = TAU * frequency * within as f32 / SAMPLE_RATE as f32;
    let value = AMPLITUDE * envelope(within) * phase.sin();

    (value * f32::from(i16::MAX)) as i16
}

/// How much of the note is audible at `within` samples into it.
///
/// A raised cosine at each end, not a straight line: the ear hears the corner
/// of a linear fade as the tick the fade exists to avoid.
fn envelope(within: usize) -> f32 {
    let rise = |progress: usize| {
        0.5 - 0.5 * (std::f32::consts::PI * progress as f32 / FADE_SAMPLES as f32).cos()
    };

    if within < FADE_SAMPLES {
        return rise(within);
    }
    let from_end = NOTE_SAMPLES - 1 - within;
    if from_end < FADE_SAMPLES {
        return rise(from_end);
    }
    1.0
}
