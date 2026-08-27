// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The gate's state machine, driven by hand.
//!
//! Every one of these feeds probabilities straight in rather than audio, so
//! none of them depend on what RNNoise thinks of a synthesised vowel. That is
//! the point of `Hysteresis` being its own type: the decision logic is where
//! the bugs live, and it can be tested exhaustively in microseconds without a
//! model, a microphone, or a sound card.

use consort_audio::{GateConfig, Hysteresis};

/// A gate past its warm-up frame, which is where all but one of these start.
fn warmed(config: GateConfig) -> Hysteresis {
    let mut gate = Hysteresis::new(config);
    gate.step(0.0);
    gate
}

fn config() -> GateConfig {
    GateConfig {
        open_at: 0.6,
        close_at: 0.3,
        attack_frames: 2,
        hold_ms: 300,
        denoise: true,
        voice_activity: true,
    }
}

/// The same, with the gate turned off entirely.
fn always_on() -> GateConfig {
    GateConfig {
        voice_activity: false,
        ..config()
    }
}

#[test]
fn the_first_frame_is_dropped_because_the_model_is_warming_up() {
    let mut gate = Hysteresis::new(GateConfig {
        attack_frames: 1,
        ..config()
    });

    let decision = gate.step(0.99);

    assert!(
        !decision.open,
        "the first frame must not open the gate: RNNoise's first output carries \
         fade-in artifacts and its probability for that frame is not meaningful"
    );
    assert!(!decision.opened);
}

#[test]
fn a_single_loud_transient_does_not_open_the_gate() {
    let mut gate = warmed(config());

    let decision = gate.step(0.99);

    assert!(
        !decision.open,
        "one frame above the threshold is a key press or a desk bump, not speech"
    );
}

#[test]
fn the_gate_opens_once_the_attack_window_is_satisfied() {
    let mut gate = warmed(config());

    gate.step(0.99);
    let decision = gate.step(0.99);

    assert!(decision.open);
    assert!(decision.opened, "the second frame is the rising edge");
}

#[test]
fn the_attack_window_has_to_be_consecutive() {
    let mut gate = warmed(config());

    gate.step(0.99);
    gate.step(0.10);
    let decision = gate.step(0.99);

    assert!(
        !decision.open,
        "a frame below the threshold resets the streak, so this is the first of \
         a new attack rather than the second of the old one"
    );
}

#[test]
fn an_attack_of_zero_frames_still_needs_one() {
    let mut gate = warmed(GateConfig {
        attack_frames: 0,
        ..config()
    });

    let decision = gate.step(0.99);

    assert!(
        decision.open,
        "zero is meaningless and is treated as one rather than as always-open"
    );
}

#[test]
fn the_gate_holds_through_the_pause_between_two_words() {
    let mut gate = warmed(GateConfig {
        attack_frames: 1,
        hold_ms: 300,
        ..config()
    });
    gate.step(0.99);

    // 300 ms of hold is 30 frames at 10 ms each. The first 29 are inside it.
    for frame in 0..29 {
        let decision = gate.step(0.0);
        assert!(
            decision.open,
            "the gate shut {frame} frames into a 300 ms hold, which would clip \
             the tail of every word"
        );
    }
}

#[test]
fn the_gate_closes_once_the_hold_has_run_out() {
    let mut gate = warmed(GateConfig {
        attack_frames: 1,
        hold_ms: 300,
        ..config()
    });
    gate.step(0.99);
    for _ in 0..29 {
        gate.step(0.0);
    }

    let decision = gate.step(0.0);

    assert!(!decision.open);
    assert!(
        decision.closed,
        "the thirtieth silent frame is the falling edge"
    );
}

#[test]
fn a_frame_between_the_thresholds_recharges_the_hold() {
    let mut gate = warmed(GateConfig {
        attack_frames: 1,
        hold_ms: 100,
        ..config()
    });
    gate.step(0.99);
    for _ in 0..5 {
        gate.step(0.0);
    }

    // 0.45 is under `open_at` so it could not open the gate, and over
    // `close_at` so it keeps one that is already open.
    gate.step(0.45);

    for frame in 0..9 {
        assert!(
            gate.step(0.0).open,
            "the hold was not recharged; shut {frame} frames after a voiced frame"
        );
    }
    assert!(!gate.step(0.0).open);
}

#[test]
fn the_rising_edge_is_reported_once() {
    let mut gate = warmed(GateConfig {
        attack_frames: 1,
        ..config()
    });

    assert!(gate.step(0.99).opened);
    for _ in 0..10 {
        assert!(
            !gate.step(0.99).opened,
            "`opened` is an edge, not a synonym for `open`"
        );
    }
}

#[test]
fn the_falling_edge_is_reported_once() {
    // 20 ms of hold is two frames, so the gate survives one silent frame and
    // shuts on the second.
    let mut gate = warmed(GateConfig {
        attack_frames: 1,
        hold_ms: 20,
        ..config()
    });
    gate.step(0.99);

    assert!(!gate.step(0.0).closed, "one frame of hold is still left");
    assert!(gate.step(0.0).closed, "this is the falling edge");
    for _ in 0..10 {
        assert!(
            !gate.step(0.0).closed,
            "`closed` is an edge, not a synonym for shut"
        );
    }
}

#[test]
fn the_probability_is_passed_through_untouched() {
    let mut gate = warmed(config());

    for probability in [0.0, 0.123_456, 0.5, 0.987_654, 1.0] {
        assert_eq!(
            gate.step(probability).probability,
            probability,
            "hysteresis decides the gate, it does not get to edit the reading \
             the meter draws"
        );
    }
}

#[test]
fn a_gate_reports_the_configuration_it_was_built_with() {
    let config = config();

    let gate = Hysteresis::new(config);

    assert_eq!(gate.config(), config);
}

#[test]
fn a_gate_starts_shut() {
    assert!(!Hysteresis::new(config()).is_open());
}

// Voice activity as a choice. The gate is not always what somebody wants: a
// quiet room with a good microphone gains nothing from it, and anybody whose
// speech the model happens to score badly is better served by transmitting
// everything than by being cut off mid-sentence.

#[test]
fn with_voice_activity_off_silence_still_goes_out() {
    // The whole of the setting. A frame the gate would certainly reject has
    // to be published anyway, or the toggle does nothing.
    let mut gate = warmed(always_on());

    let decision = gate.step(0.0);

    assert!(decision.open, "the gate is off and still refused a frame");
}

#[test]
fn with_voice_activity_off_the_model_is_still_reported() {
    // The meter draws the probability, and a bar that reads zero because
    // nobody is consulting the model would be a worse screen than one that
    // shows what the model thinks of audio it is not being asked to judge.
    let mut gate = warmed(always_on());

    let decision = gate.step(0.42);

    assert_eq!(decision.probability, 0.42);
}

#[test]
fn with_voice_activity_off_it_opens_once_rather_than_every_frame() {
    // `opened` is an edge. A caller counting them, or logging them, should
    // see one opening and not a hundred a second.
    let mut gate = warmed(always_on());

    let first = gate.step(0.0);
    let second = gate.step(0.0);

    assert!(first.opened);
    assert!(!second.opened, "it reported opening while already open");
}

#[test]
fn voice_activity_off_still_drops_the_warm_up_frame() {
    // The warm-up is about the denoiser, not about the gate. RNNoise's first
    // output frame carries fade-in artifacts whether or not anything is going
    // to judge it, and publishing that is publishing a click.
    let mut gate = Hysteresis::new(always_on());

    let decision = gate.step(0.99);

    assert!(!decision.open, "the first frame is a denoiser artifact");
}

#[test]
fn turning_voice_activity_off_opens_a_shut_gate_at_once() {
    // Somebody flips the switch mid-sentence, having been cut off. Waiting
    // for an attack that will never be satisfied would look like the switch
    // did nothing.
    let mut gate = warmed(config());
    assert!(!gate.step(0.0).open);

    gate.retune(always_on());

    assert!(gate.step(0.0).open);
}

#[test]
fn turning_voice_activity_back_on_shuts_a_gate_that_nothing_is_holding_open() {
    let mut gate = warmed(always_on());
    assert!(gate.step(0.0).open);

    gate.retune(config());

    let decision = gate.step(0.0);
    assert!(!decision.open, "silence held the gate open after the gate came back");
    assert!(decision.closed, "the closing edge went unreported");
}

#[test]
fn retuning_reports_the_configuration_it_was_given() {
    let mut gate = warmed(config());

    gate.retune(always_on());

    assert_eq!(gate.config(), always_on());
}

#[test]
fn retuning_does_not_shut_a_gate_that_was_open() {
    // Somebody moving a threshold is doing it mid-sentence with the meter in
    // front of them. Resetting the state machine would clip the word they are
    // in the middle of saying, which is exactly the artifact the hold exists
    // to prevent.
    let mut gate = warmed(config());
    gate.step(0.9);
    gate.step(0.9);
    assert!(gate.is_open());

    gate.retune(GateConfig {
        hold_ms: 500,
        ..config()
    });

    assert!(gate.is_open(), "changing a threshold cut somebody off");
}

#[test]
fn a_raised_threshold_applies_from_the_next_frame() {
    let mut gate = warmed(config());

    gate.retune(GateConfig {
        open_at: 0.95,
        ..config()
    });
    gate.step(0.9);
    gate.step(0.9);

    assert!(
        !gate.is_open(),
        "speech below the new threshold opened the gate anyway"
    );
}
