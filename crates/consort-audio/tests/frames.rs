// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning whatever the sound card hands over into the fixed frame the model
//! insists on.
//!
//! A backend delivers interleaved samples, in its own format, in buffers of
//! whatever size suits it, none of which the gate accepts. This is the part in
//! between, and it is pure arithmetic, so it is tested here rather than
//! inferred from whether a microphone sounded right.

use consort_audio::{FRAME_SAMPLES, Frames};

/// Collect whole frames out of a sequence of pushes.
fn collect_i16(channels: u16, pushes: &[&[i16]]) -> Vec<Vec<i16>> {
    let mut frames = Frames::new(channels);
    let mut out = Vec::new();
    for push in pushes {
        frames.push_i16(push, |frame| out.push(frame.to_vec()));
    }
    out
}

fn collect_f32(channels: u16, data: &[f32]) -> Vec<Vec<i16>> {
    let mut frames = Frames::new(channels);
    let mut out = Vec::new();
    frames.push_f32(data, |frame| out.push(frame.to_vec()));
    out
}

#[test]
fn a_partial_frame_is_not_emitted() {
    let out = collect_i16(1, &[&vec![100i16; FRAME_SAMPLES * 2 - 1]]);

    assert_eq!(out.len(), 1, "one sample short of two frames is one frame");
    assert_eq!(out[0].len(), FRAME_SAMPLES);
}

#[test]
fn a_frame_split_across_two_buffers_is_still_one_frame() {
    // The backend picks its own buffer size and it has no reason to be a
    // multiple of 480. If the leftover were dropped between callbacks, audio
    // would be quietly decimated and would still sound almost right.
    let half = vec![100i16; FRAME_SAMPLES / 2];

    let out = collect_i16(1, &[&half, &half]);

    assert_eq!(out.len(), 1);
    assert!(out[0].iter().all(|sample| *sample == 100));
}

#[test]
fn mono_passes_through_untouched() {
    let data: Vec<i16> = (0..FRAME_SAMPLES).map(|n| n as i16).collect();

    let out = collect_i16(1, &[&data]);

    assert_eq!(out[0], data);
}

#[test]
fn stereo_is_downmixed_to_mono() {
    let mut interleaved = Vec::new();
    for _ in 0..FRAME_SAMPLES {
        interleaved.push(1000i16);
        interleaved.push(2000i16);
    }

    let out = collect_i16(2, &[&interleaved]);

    assert_eq!(out.len(), 1, "two channels of one frame is one mono frame");
    assert!(
        out[0].iter().all(|sample| *sample == 1500),
        "left and right should average, not concatenate"
    );
}

#[test]
fn downmixing_cannot_overflow_on_loud_stereo() {
    // Two channels at full scale sum to more than an i16 holds. Summing in i16
    // would wrap and turn the loudest possible signal into noise.
    let interleaved = vec![i16::MAX; FRAME_SAMPLES * 2];

    let out = collect_i16(2, &[&interleaved]);

    assert!(out[0].iter().all(|sample| *sample == i16::MAX));
}

#[test]
fn f32_is_scaled_into_i16_range() {
    // cpal hands out f32 in [-1.0, 1.0]. Everything downstream speaks i16
    // range, including the model. See tests/scaling.rs for what happens when
    // this is got backwards.
    let out = collect_f32(1, &vec![1.0f32; FRAME_SAMPLES]);

    assert_eq!(out[0][0], i16::MAX);
}

#[test]
fn f32_overshoot_is_clamped_at_both_ends() {
    let loud = collect_f32(1, &vec![4.0f32; FRAME_SAMPLES]);
    let quiet = collect_f32(1, &vec![-4.0f32; FRAME_SAMPLES]);

    assert_eq!(loud[0][0], i16::MAX, "a cast would have wrapped");
    assert_eq!(quiet[0][0], i16::MIN);
}

#[test]
fn a_device_claiming_no_channels_is_treated_as_mono() {
    // Nothing should report zero channels, and dividing by it would panic in
    // the realtime callback, which is the worst place for one.
    let out = collect_i16(0, &[&vec![100i16; FRAME_SAMPLES]]);

    assert_eq!(out.len(), 1);
    assert!(out[0].iter().all(|sample| *sample == 100));
}

#[test]
fn an_empty_buffer_emits_nothing() {
    assert!(collect_i16(1, &[&[]]).is_empty());
    assert!(collect_f32(1, &[]).is_empty());
}

#[test]
fn a_trailing_sample_that_does_not_fill_a_channel_group_is_ignored() {
    // An odd number of samples on a stereo device means the backend gave us
    // half a stereo pair. There is no sensible mono value for it.
    let mut interleaved = vec![1000i16; FRAME_SAMPLES * 2];
    interleaved.push(9999);

    let out = collect_i16(2, &[&interleaved]);

    assert_eq!(out.len(), 1);
    assert!(out[0].iter().all(|sample| *sample == 1000));
}
