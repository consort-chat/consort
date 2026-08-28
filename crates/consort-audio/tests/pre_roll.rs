// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The delay line that lets an opening gate reach backwards.
//!
//! No model and no audio worth the name, in the spirit of `hysteresis.rs`:
//! what is being tested is bookkeeping over frames, so the frames are numbered
//! constants and the decisions are written by hand.

use consort_audio::{FRAME_MS, FRAME_SAMPLES, GateDecision, PRE_ROLL_FRAMES, PreRoll};

/// A frame of one repeated value, so that what comes out can be named.
fn frame(marker: i16) -> Vec<i16> {
    vec![marker; FRAME_SAMPLES]
}

fn shut() -> GateDecision {
    GateDecision {
        open: false,
        opened: false,
        closed: false,
        probability: 0.1,
    }
}

fn opening() -> GateDecision {
    GateDecision {
        open: true,
        opened: true,
        closed: false,
        probability: 0.9,
    }
}

fn open() -> GateDecision {
    GateDecision {
        open: true,
        opened: false,
        closed: false,
        probability: 0.9,
    }
}

#[test]
fn nothing_comes_out_until_the_line_has_filled() {
    let mut line = PreRoll::new(3);

    for marker in 1..=3 {
        assert!(
            line.step(&frame(marker), open()).is_none(),
            "frame {marker} is still filling the line"
        );
    }

    let (samples, _) = line.step(&frame(4), open()).expect("the line is full");
    assert_eq!(
        samples[0], 1,
        "the fourth frame in yields the first frame in"
    );
}

#[test]
fn an_opening_gate_reaches_back_for_the_start_of_the_word() {
    // The reason this type exists. The attack spends its frames proving
    // somebody has started talking, and those frames are the consonant that
    // proved it. Without the reach back, "pop" arrives as "op".
    let mut line = PreRoll::new(3);

    for marker in 1..=3 {
        line.step(&frame(marker), shut());
    }
    let (samples, sending) = line.step(&frame(4), opening()).expect("the line is full");

    assert!(
        sending,
        "the frame the gate had not opened for yet still goes"
    );
    assert_eq!(
        samples[0], 1,
        "and it goes out carrying what was captured, not the silence a shut \
         gate would have put there"
    );
}

#[test]
fn every_frame_the_attack_cost_is_recovered_and_not_just_the_oldest() {
    let mut line = PreRoll::new(3);
    for marker in 1..=3 {
        line.step(&frame(marker), shut());
    }

    let mut recovered = Vec::new();
    line.step(&frame(4), opening());
    for marker in 5..=7 {
        let (samples, sending) = line.step(&frame(marker), open()).expect("full");
        recovered.push((samples[0], sending));
    }

    assert_eq!(
        recovered,
        vec![(2, true), (3, true), (4, true)],
        "all three frames held while the gate made its mind up have to come \
         out, in order and unsilenced"
    );
}

#[test]
fn a_frame_that_stays_shut_goes_out_silent() {
    let mut line = PreRoll::new(2);
    line.step(&frame(1000), shut());
    line.step(&frame(1000), shut());

    let (samples, sending) = line.step(&frame(1000), shut()).expect("the line is full");

    assert!(!sending);
    assert!(
        samples.iter().all(|sample| *sample == 0),
        "a frame nobody is sending must be silence rather than the audio it \
         held, or a listener hears whatever was in the room"
    );
}

#[test]
fn closing_leaves_what_is_already_in_the_line_alone() {
    // The hold already covers the tail of a word. A closing edge that reached
    // back the way an opening one does would cut it off early.
    let mut line = PreRoll::new(2);
    line.step(&frame(1), open());
    line.step(&frame(2), open());

    let (samples, sending) = line
        .step(
            &frame(3),
            GateDecision {
                open: false,
                opened: false,
                closed: true,
                probability: 0.1,
            },
        )
        .expect("the line is full");

    assert!(
        sending,
        "the frames behind a closing edge were open when captured"
    );
    assert_eq!(samples[0], 1);
}

#[test]
fn a_line_of_no_depth_is_a_passthrough() {
    let mut line = PreRoll::new(0);

    let (samples, sending) = line.step(&frame(7), open()).expect("nothing to wait for");

    assert_eq!(samples[0], 7);
    assert!(sending);
    assert_eq!(line.latency_ms(), 0);
}

#[test]
fn the_delay_is_what_it_says_it_is() {
    let line = PreRoll::default();

    assert_eq!(line.latency_ms(), PRE_ROLL_FRAMES as u32 * FRAME_MS);
    assert_eq!(
        line.latency_ms(),
        30,
        "30 ms, and it should stay that cheap"
    );
}

#[test]
fn the_line_is_deep_enough_for_the_attack_it_exists_to_cover() {
    // The default attack is two frames. A line shorter than that plus the
    // frame the gate opens on would still lose part of the word, which is the
    // failure this whole type is here to prevent.
    let attack = consort_audio::GateConfig::default().attack_frames as usize;

    assert!(
        PRE_ROLL_FRAMES > attack,
        "the line has to outlast the attack: {PRE_ROLL_FRAMES} frames against \
         an attack of {attack}"
    );
}

#[test]
#[should_panic(expected = "one RNNoise frame")]
fn a_frame_of_the_wrong_length_is_a_bug_rather_than_something_to_absorb() {
    PreRoll::new(1).step(&[0i16; 3], open());
}
