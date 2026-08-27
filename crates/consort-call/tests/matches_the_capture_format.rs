// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The two ends of the microphone agree on what a frame is.
//!
//! `consort-audio` produces mono 48 kHz `i16` because that is the only rate
//! RNNoise was trained for. `matrix-rtc-media` wants mono 48 kHz `i16` because
//! that is what `AudioSourceConfig::default()` says. Neither knows about the
//! other, and nothing in the type system connects them: the frames cross as a
//! `Vec<i16>` and the format is restated on arrival.
//!
//! So it is asserted. A mismatch here would not fail to compile and would not
//! fail to run. It would produce a call in which everybody sounds like they
//! are underwater, or one octave out, which is a long way to travel to find a
//! constant.

use consort_audio::{FRAME_MS, FRAME_SAMPLES, SAMPLE_RATE};
use matrix_rtc_media::AudioSourceConfig;

#[test]
fn the_gate_and_the_publication_want_the_same_sample_rate() {
    assert_eq!(SAMPLE_RATE, AudioSourceConfig::default().sample_rate);
}

#[test]
fn the_gate_and_the_publication_want_the_same_channel_count() {
    // The gate is mono and cannot be anything else: it sums whatever the
    // device negotiated down to one channel before the denoiser sees it.
    assert_eq!(AudioSourceConfig::default().num_channels, 1);
}

#[test]
fn a_frame_is_the_ten_milliseconds_both_sides_were_built_around() {
    // 480 samples at 48 kHz. The publication derives `samples_per_channel`
    // from the length it is handed, so this is not a number it has to be told,
    // but it is the number that makes the two cadences line up.
    assert_eq!(FRAME_SAMPLES as u32 * (1_000 / FRAME_MS), SAMPLE_RATE);
}
