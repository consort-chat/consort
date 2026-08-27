// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! A synthesised voiced vowel, for tests that need the model to react.
//!
//! Not speech. It is a glottal harmonic stack shaped by three formants, which
//! carries the structure RNNoise keys on, and that is enough to tell "the model
//! is receiving a signal" from "the model is receiving nothing". Anything
//! finer than that belongs in a test with a real recording, and there is not
//! one of those in this repository.

use consort_audio::{FRAME_SAMPLES, SAMPLE_RATE};

/// Fundamental of a low adult voice.
const F0: f32 = 125.0;

/// Centre frequency and bandwidth of each formant.
const FORMANTS: [(f32, f32); 3] = [(700.0, 130.0), (1220.0, 100.0), (2600.0, 160.0)];

/// A syllable-rate amplitude envelope, so this is not a steady tone.
const SYLLABLES_PER_SECOND: f32 = 4.0;

/// `frames` frames of vowel, scaled to about -6 dBFS in i16 terms, which is a
/// realistic speaking level.
pub fn voiced(frames: usize) -> Vec<f32> {
    let total = frames * FRAME_SAMPLES;
    let mut out = Vec::with_capacity(total);
    for n in 0..total {
        let t = n as f32 / SAMPLE_RATE as f32;
        let mut sample = 0.0;
        for harmonic in 1..=40u32 {
            let freq = F0 * harmonic as f32;
            if freq >= SAMPLE_RATE as f32 / 2.0 {
                break;
            }
            // The glottal source rolls off with harmonic number, then each
            // formant boosts the band around it.
            let source = 1.0 / harmonic as f32;
            let gain: f32 = FORMANTS
                .iter()
                .map(|(centre, width)| 1.0 / (1.0 + ((freq - centre) / width).powi(2)))
                .sum();
            sample += source * gain * (std::f32::consts::TAU * freq * t).sin();
        }
        let envelope = 0.6 + 0.4 * (std::f32::consts::TAU * SYLLABLES_PER_SECOND * t).sin();
        out.push(sample * envelope);
    }

    let peak = out.iter().fold(0.0f32, |acc, s| acc.max(s.abs())).max(1e-9);
    let target = 0.5 * f32::from(i16::MAX);
    out.iter().map(|s| s / peak * target).collect()
}

/// The same signal as the i16 samples a sound card would deliver.
pub fn voiced_frames(frames: usize) -> Vec<i16> {
    voiced(frames).iter().map(|s| *s as i16).collect()
}
