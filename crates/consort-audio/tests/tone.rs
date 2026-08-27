// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The sound the output test makes.
//!
//! An input device can be checked by talking at it. An output device cannot be
//! checked by anything at all unless something plays, so this is the only
//! evidence the output picker will ever have, and it has to be evidence
//! somebody can interpret: audible, obviously deliberate, and over quickly.
//!
//! Pure arithmetic, so all of it is testable without a sound card. The cpal
//! call that hands these samples to a device is four lines in `cpal_host.rs`
//! and is excluded from coverage; everything that decides what the sound is
//! lives here.

use consort_audio::{SAMPLE_RATE, Tone};

/// Play the whole thing into one buffer, in pieces of `chunk` samples.
///
/// Returns the samples and how many fills reported that there was more to
/// come.
fn play(chunk: usize) -> (Vec<i16>, usize) {
    let mut tone = Tone::check();
    let mut played = Vec::new();
    let mut buffer = vec![0i16; chunk];
    let mut going = 0;

    // The `+ 8` is slack: a caller that keeps filling after the end must keep
    // getting silence rather than a panic or a repeat.
    for _ in 0..(Tone::SAMPLES / chunk) + 8 {
        if tone.fill(&mut buffer) {
            going += 1;
        }
        played.extend_from_slice(&buffer);
    }

    (played, going)
}

#[test]
fn the_chime_is_over_before_anybody_has_to_wonder_about_it() {
    // Long enough to hear which speaker it came out of, short enough that
    // pressing the button twice by accident is not an event.
    let seconds = Tone::SAMPLES as f32 / SAMPLE_RATE as f32;

    assert!(
        (0.2..=1.0).contains(&seconds),
        "a test tone lasting {seconds:.2}s is either inaudible or a nuisance"
    );
}

#[test]
fn it_starts_and_ends_in_silence() {
    // A tone that begins at full amplitude begins with a click, and a click is
    // the one sound that plays through every output ever made. Somebody
    // checking their speakers would hear the click and learn nothing about the
    // tone.
    let (played, _) = play(64);

    assert_eq!(played[0], 0, "the first sample is a click");
    assert_eq!(
        played[Tone::SAMPLES - 1],
        0,
        "the last sample is a click"
    );
}

#[test]
fn it_is_loud_enough_to_hear_and_quiet_enough_not_to_hurt() {
    // Somebody presses this with headphones on, having just plugged them in,
    // with no idea what the system volume is set to.
    let (played, _) = play(480);
    let peak = played.iter().map(|s| s.unsigned_abs()).max().unwrap();
    let fraction = f32::from(peak) / 32_768.0;

    assert!(
        (0.1..=0.4).contains(&fraction),
        "a peak at {fraction:.2} of full scale is the wrong side of comfortable"
    );
}

#[test]
fn it_ends_and_stays_ended() {
    // The stream outlives the sound: the thread only drops it once the
    // callback has said it is finished, and until then the device keeps asking
    // for samples.
    let (played, going) = play(480);

    assert!(going > 0, "it never started");
    assert!(
        going < (Tone::SAMPLES / 480) + 8,
        "it never reported an end, so the stream would play forever"
    );
    assert!(
        played[Tone::SAMPLES..].iter().all(|&s| s == 0),
        "something is still playing after the chime finished"
    );
}

#[test]
fn the_second_note_is_higher_than_the_first() {
    // Two notes rather than one. A single tone is indistinguishable from the
    // hum an unhappy audio interface makes on its own, and the whole job of
    // this sound is to be obviously deliberate.
    let (played, _) = play(480);
    let half = Tone::SAMPLES / 2;
    let crossings = |samples: &[i16]| {
        samples
            .windows(2)
            .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
            .count()
    };

    let first = crossings(&played[..half]);
    let second = crossings(&played[half..Tone::SAMPLES]);

    assert!(
        second > first,
        "the notes are {first} and {second} crossings, which is not a rise"
    );
}

#[test]
fn how_the_device_asks_for_it_does_not_change_what_it_sounds_like() {
    // cpal picks the buffer size, it differs per backend, and it is not
    // necessarily a divisor of anything. A tone whose phase or envelope
    // restarted per buffer would be a different sound on every machine.
    let (in_small_pieces, _) = play(32);
    let (in_large_pieces, _) = play(1024);

    assert_eq!(
        in_small_pieces[..Tone::SAMPLES],
        in_large_pieces[..Tone::SAMPLES],
        "the chime depends on how the device asked for it"
    );
}

#[test]
fn a_buffer_bigger_than_what_is_left_is_padded_rather_than_wrapped() {
    // The last callback of the chime is the one that straddles the end. Half
    // a buffer of tone and half a buffer of the beginning again is an audible
    // stutter, and it is the natural bug here.
    let mut tone = Tone::check();
    let mut buffer = vec![-1i16; Tone::SAMPLES + 480];

    let going = tone.fill(&mut buffer);

    assert!(!going, "a buffer past the end still reported more to come");
    assert!(
        buffer[Tone::SAMPLES..].iter().all(|&s| s == 0),
        "the tail of the last buffer is not silence"
    );
}

#[test]
fn each_chime_is_the_same_chime() {
    // A fresh `Tone` per press. Two presses that sound different would mean
    // state leaking out of one into the next.
    let (first, _) = play(256);
    let (second, _) = play(256);

    assert_eq!(first, second);
}
