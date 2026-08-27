// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Handing the chime to a device.
//!
//! The mirror of `frames.rs`, which turns what a microphone delivers into what
//! the model wants. This turns what [`Tone`] produces into what a device wants:
//! interleaved across however many channels it has, in whichever of two sample
//! formats it asked for, in buffers it chose the size of.
//!
//! It also owns the only answer to "is it over yet". A stream goes on asking
//! for samples until somebody drops it, and the code filling the buffer is the
//! only code that can see the end coming.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use consort_audio::playback::Playing;
use consort_audio::{SAMPLE_RATE, Tone};

/// A `Playing` and a count of how many times it said it had finished.
fn playing(channels: u16) -> (Playing, Arc<AtomicUsize>) {
    let ends = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ends);
    let playing = Playing::new(
        Tone::check(),
        channels,
        Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (playing, ends)
}

#[test]
fn the_same_sound_comes_out_of_every_channel() {
    // A chime that plays out of the left speaker only is a chime that tells
    // somebody their right speaker is broken.
    let (mut playing, _) = playing(2);
    let mut buffer = vec![0i16; 960];

    playing.fill_i16(&mut buffer);

    for pair in buffer.as_chunks::<2>().0 {
        assert_eq!(pair[0], pair[1], "the channels are carrying different audio");
    }
    assert!(
        buffer.iter().any(|&sample| sample != 0),
        "nothing was played at all"
    );
}

#[test]
fn a_mono_device_gets_the_same_chime() {
    let (mut stereo, _) = playing(2);
    let (mut mono, _) = playing(1);
    let mut in_stereo = vec![0i16; 960];
    let mut in_mono = vec![0i16; 480];

    stereo.fill_i16(&mut in_stereo);
    mono.fill_i16(&mut in_mono);

    let left: Vec<i16> = in_stereo.as_chunks::<2>().0.iter().map(|pair| pair[0]).collect();
    assert_eq!(left, in_mono);
}

#[test]
fn a_surround_device_gets_it_out_of_all_six() {
    // 5.1 is what an HDMI sink commonly negotiates, and it is the case where
    // "just write the buffer" quietly plays a chime at a sixth of the rate.
    let (mut playing, _) = playing(6);
    let mut buffer = vec![0i16; 6 * 480];

    playing.fill_i16(&mut buffer);

    for group in buffer.as_chunks::<6>().0 {
        assert!(
            group.iter().all(|&sample| sample == group[0]),
            "the channels are carrying different audio: {group:?}"
        );
    }
}

#[test]
fn it_says_it_has_finished_exactly_once() {
    // Once, because the thread closes the device on hearing it and a second
    // report would close whatever had started in the meantime. At all, because
    // nothing else can tell: the device keeps asking for samples forever.
    let (mut playing, ends) = playing(2);
    let mut buffer = vec![0i16; 2 * 480];

    for _ in 0..(Tone::SAMPLES / 480) + 10 {
        playing.fill_i16(&mut buffer);
    }

    assert_eq!(ends.load(Ordering::SeqCst), 1);
}

#[test]
fn it_says_so_on_the_buffer_that_carries_the_last_sample() {
    // Not a buffer later. At 10 ms a buffer that is an audible gap of silence
    // holding the device open after the sound is over.
    let (mut playing, ends) = playing(1);
    let mut buffer = vec![0i16; 480];
    let buffers = Tone::SAMPLES / 480;

    for _ in 0..buffers - 1 {
        playing.fill_i16(&mut buffer);
    }
    assert_eq!(ends.load(Ordering::SeqCst), 0, "it finished early");
    playing.fill_i16(&mut buffer);

    assert_eq!(ends.load(Ordering::SeqCst), 1, "it did not finish on time");
}

#[test]
fn what_it_plays_after_the_end_is_silence() {
    // The gap between the chime finishing and the thread dropping the stream.
    // Repeating the chime there is the natural bug and would be a stutter.
    let (mut playing, _) = playing(2);
    let mut buffer = vec![0i16; 2 * 480];
    for _ in 0..(Tone::SAMPLES / 480) + 1 {
        playing.fill_i16(&mut buffer);
    }

    buffer.fill(-1);
    playing.fill_i16(&mut buffer);

    assert!(
        buffer.iter().all(|&sample| sample == 0),
        "something is still playing after the chime ended"
    );
}

#[test]
fn a_float_device_gets_the_same_chime_between_minus_one_and_one() {
    // cpal hands out f32 in [-1.0, 1.0]. Writing i16-range floats there is
    // roughly 90 dB of clipping, which is not a chime, it is a bang.
    let (mut in_floats, _) = playing(2);
    let (mut in_integers, _) = playing(2);
    let mut floats = vec![0.0f32; 2 * 480];
    let mut integers = vec![0i16; 2 * 480];

    in_floats.fill_f32(&mut floats);
    in_integers.fill_i16(&mut integers);

    assert!(
        floats.iter().all(|sample| (-1.0..=1.0).contains(sample)),
        "a float sample escaped the range cpal expects"
    );
    for (float, integer) in floats.iter().zip(&integers) {
        assert!(
            (float * 32_768.0 - f32::from(*integer)).abs() <= 1.0,
            "{float} and {integer} are not the same sample"
        );
    }
}

#[test]
fn a_float_device_is_told_it_has_finished_too() {
    let (mut playing, ends) = playing(2);
    let mut buffer = vec![0.0f32; 2 * 480];

    for _ in 0..(Tone::SAMPLES / 480) + 2 {
        playing.fill_f32(&mut buffer);
    }

    assert_eq!(ends.load(Ordering::SeqCst), 1);
}

#[test]
fn a_buffer_that_is_not_a_whole_number_of_frames_does_not_panic() {
    // cpal is not obliged to hand over a multiple of the channel count, and a
    // panic in a realtime callback takes the audio thread with it.
    let (mut playing, _) = playing(2);
    let mut buffer = vec![0i16; 481];

    playing.fill_i16(&mut buffer);
}

#[test]
fn a_device_claiming_no_channels_does_not_divide_by_zero() {
    // Nothing should, and `Frames` guards the same thing on the way in for the
    // same reason: the worst place to find out is inside a realtime callback.
    let (mut playing, _) = playing(0);
    let mut buffer = vec![0i16; 480];

    playing.fill_i16(&mut buffer);
}

#[test]
fn the_chime_plays_at_the_rate_the_device_was_opened_at() {
    // The tone is generated at 48 kHz and nothing resamples, which is why a
    // device that cannot do 48 kHz is refused rather than played to slowly.
    let (mut playing, ends) = playing(1);
    let mut buffer = vec![0i16; SAMPLE_RATE as usize];

    playing.fill_i16(&mut buffer);

    assert_eq!(ends.load(Ordering::SeqCst), 1, "a second was not enough");
    assert!(
        buffer[Tone::SAMPLES..].iter().all(|&sample| sample == 0),
        "the chime ran past its own length"
    );
}
