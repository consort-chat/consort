// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning 100 gate decisions a second into something worth sending to a
//! webview.
//!
//! The gate runs at one frame every 10 ms. Sending 100 IPC messages a second,
//! each a JSON round trip, to move a bar that redraws at 60 Hz at best is
//! waste. This batches them, and the batching has to preserve the two things a
//! person is looking for: the loudest moment and the highest confidence. An
//! average would hide both.
//!
//! Batched by frame count rather than by clock. The frame rate is fixed at 100
//! Hz by construction, so counting is exact and needs no `Instant`, which also
//! means none of these tests sleep.

use consort_audio::{FRAME_SAMPLES, FRAMES_PER_READING, GateDecision, Meter, Reading};

fn decision(probability: f32, open: bool) -> GateDecision {
    GateDecision {
        open,
        opened: false,
        closed: false,
        probability,
    }
}

fn quiet() -> Vec<i16> {
    vec![0i16; FRAME_SAMPLES]
}

fn at(amplitude: i16) -> Vec<i16> {
    vec![amplitude; FRAME_SAMPLES]
}

/// Fold `frames` frames of identical input and collect whatever comes out.
fn readings(frames: usize, probability: f32, open: bool) -> Vec<Reading> {
    let mut meter = Meter::new();
    let mut out = Vec::new();
    for _ in 0..frames {
        if let Some(reading) = meter.fold(decision(probability, open), &quiet()) {
            out.push(reading);
        }
    }
    out
}

#[test]
fn a_hundred_frames_a_second_becomes_twenty_readings() {
    let out = readings(100, 0.5, true);

    assert_eq!(
        out.len(),
        100 / FRAMES_PER_READING,
        "one second of audio should produce one second of meter updates, not \
         one hundred IPC messages"
    );
}

#[test]
fn a_partial_batch_produces_nothing() {
    let out = readings(FRAMES_PER_READING - 1, 0.5, true);

    assert!(out.is_empty(), "half a batch is not a reading");
}

#[test]
fn no_frames_at_all_produce_nothing() {
    assert!(readings(0, 0.0, false).is_empty());
}

#[test]
fn the_reading_carries_the_loudest_moment_in_the_batch() {
    // A transient that lands in the middle of a batch is exactly what somebody
    // is watching for when they tap the microphone. An average would swallow it.
    let mut meter = Meter::new();
    let mut out = None;
    for index in 0..FRAMES_PER_READING {
        let frame = if index == 2 { at(i16::MAX) } else { quiet() };
        if let Some(reading) = meter.fold(decision(0.1, false), &frame) {
            out = Some(reading);
        }
    }

    let reading = out.expect("a whole batch should have produced a reading");
    assert!(
        reading.level > 0.99,
        "the peak was lost: got {}",
        reading.level
    );
}

#[test]
fn the_reading_carries_the_highest_probability_in_the_batch() {
    let mut meter = Meter::new();
    let mut out = None;
    for index in 0..FRAMES_PER_READING {
        let probability = if index == 3 { 0.92 } else { 0.05 };
        if let Some(reading) = meter.fold(decision(probability, false), &quiet()) {
            out = Some(reading);
        }
    }

    assert_eq!(out.expect("reading").probability, 0.92);
}

#[test]
fn a_gate_that_opened_at_any_point_reads_as_open() {
    // A bar that flickers off in the middle of a word looks broken, and the
    // gate genuinely does close and reopen between syllables.
    let mut meter = Meter::new();
    let mut out = None;
    for index in 0..FRAMES_PER_READING {
        if let Some(reading) = meter.fold(decision(0.5, index == 1), &quiet()) {
            out = Some(reading);
        }
    }

    assert!(out.expect("reading").open);
}

#[test]
fn a_gate_that_never_opened_reads_as_shut() {
    let out = readings(FRAMES_PER_READING, 0.1, false);

    assert!(!out[0].open);
}

#[test]
fn each_batch_starts_from_nothing() {
    // Without a reset the peak would only ever climb, and the meter would stick
    // at whatever the loudest thing since launch was.
    let mut meter = Meter::new();
    for _ in 0..FRAMES_PER_READING {
        meter.fold(decision(0.9, true), &at(i16::MAX));
    }

    let mut second = None;
    for _ in 0..FRAMES_PER_READING {
        if let Some(reading) = meter.fold(decision(0.02, false), &quiet()) {
            second = Some(reading);
        }
    }

    let reading = second.expect("reading");
    assert_eq!(reading.level, 0.0, "the peak carried over");
    assert_eq!(reading.probability, 0.02);
    assert!(!reading.open);
}

#[test]
fn silence_reads_as_no_level_rather_than_as_an_error() {
    let out = readings(FRAMES_PER_READING, 0.0, false);

    assert_eq!(out[0].level, 0.0);
}

#[test]
fn the_level_is_a_fraction_of_full_scale() {
    let mut meter = Meter::new();
    let mut out = None;
    for _ in 0..FRAMES_PER_READING {
        if let Some(reading) = meter.fold(decision(0.0, false), &at(i16::MAX / 2)) {
            out = Some(reading);
        }
    }

    let level = out.expect("reading").level;
    assert!(
        (level - 0.5).abs() < 0.01,
        "half of full scale should read as about 0.5, got {level}"
    );
}

#[test]
fn the_deepest_negative_sample_cannot_overflow_the_level() {
    // i16::MIN has no positive counterpart, so negating it wraps.
    let mut meter = Meter::new();
    let mut out = None;
    for _ in 0..FRAMES_PER_READING {
        if let Some(reading) = meter.fold(decision(0.0, false), &at(i16::MIN)) {
            out = Some(reading);
        }
    }

    let level = out.expect("reading").level;
    assert!(
        (0.0..=1.0).contains(&level),
        "a level outside [0, 1] is not one: {level}"
    );
}

#[test]
fn a_reading_is_camel_case_because_the_frontend_reads_it() {
    let reading = readings(FRAMES_PER_READING, 0.5, true).remove(0);

    let json = serde_json::to_string(&reading).expect("serialise");

    assert!(json.contains("\"level\""), "got {json}");
    assert!(json.contains("\"probability\""), "got {json}");
    assert!(json.contains("\"open\""), "got {json}");
}
