// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The sound the output test makes.
//!
//! A microphone can be checked by talking at it, and the level meter reports
//! the result. Speakers have no equivalent: nothing on the settings screen
//! makes a sound, so the output picker is a control with no feedback, and a
//! control with no feedback is one nobody can trust. This is the feedback.
//!
//! Arithmetic only, and no allocation after construction, because this runs
//! inside the backend's realtime callback. The cpal call that hands the
//! samples to a device is a few lines in [`crate::cpal_host`] and is excluded
//! from coverage; everything that decides what the sound actually is lives
//! here, where a test can listen to it.

use std::f32::consts::TAU;

use crate::gate::SAMPLE_RATE;

/// The two notes, in hertz: A4 then E5, a rising fifth.
///
/// Two rather than one. A single steady tone is what a badly grounded audio
/// interface produces on its own, and the entire job of this sound is to be
/// unmistakably something Consort did on purpose.
const NOTES: [f32; 2] = [440.0, 659.25];

/// How long each note lasts.
const NOTE_MS: usize = 160;

/// Samples in one note.
const NOTE_SAMPLES: usize = SAMPLE_RATE as usize * NOTE_MS / 1000;

/// How long each note takes to arrive and to leave.
///
/// Not zero. A note that begins at full amplitude begins with a click, and a
/// click is the one sound that comes out of every speaker ever made: somebody
/// checking their output would hear it, learn that something played, and learn
/// nothing about whether the tone did.
const FADE_MS: usize = 12;

/// Samples in one fade.
const FADE_SAMPLES: usize = SAMPLE_RATE as usize * FADE_MS / 1000;

/// Peak amplitude as a fraction of full scale.
///
/// Quiet on purpose. This gets pressed by somebody who has just plugged in
/// headphones and has no idea where the system volume is.
const AMPLITUDE: f32 = 0.22;

/// The test chime, played once.
///
/// One per press. It carries its own position, so a fresh one always sounds
/// the same and two of them cannot interfere.
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
    /// `false` means the chime has finished and the stream can be closed.
    /// Filling again after that is allowed and writes silence: the device goes
    /// on asking for samples until somebody drops the stream, and the gap
    /// between the last note and that happening has to be quiet rather than a
    /// repeat.
    ///
    /// Position is carried across calls rather than restarted, so a backend
    /// that asks for 64 samples at a time and one that asks for 1024 get the
    /// same sound.
    pub fn fill(&mut self, out: &mut [i16]) -> bool {
        for (offset, sample) in out.iter_mut().enumerate() {
            *sample = amplitude_at(self.at + offset);
        }
        // Saturating, because a caller is entitled to keep filling forever and
        // wrapping round to the start of the chime would be a stutter.
        self.at = self.at.saturating_add(out.len()).min(Self::SAMPLES);
        self.at < Self::SAMPLES
    }
}

/// The chime's amplitude at one sample position, as an `i16`.
///
/// A pure function of the position, which is what makes the sound independent
/// of the buffer sizes a backend happens to choose.
fn amplitude_at(at: usize) -> i16 {
    let Some(frequency) = NOTES.get(at / NOTE_SAMPLES) else {
        return 0;
    };
    let within = at % NOTE_SAMPLES;

    // Phase measured from the start of each note rather than from the start of
    // the chime. Both notes therefore begin at zero, and since the envelope is
    // also zero there, the join between them is silent.
    let phase = TAU * frequency * within as f32 / SAMPLE_RATE as f32;
    let value = AMPLITUDE * envelope(within) * phase.sin();

    (value * f32::from(i16::MAX)) as i16
}

/// How much of the note is audible at `within` samples into it.
///
/// A raised cosine at each end rather than a straight line. The ear hears the
/// corner of a linear fade as a faint tick, which is the thing the fade exists
/// to avoid.
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
