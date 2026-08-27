// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The gate with the model attached: what comes out of `process`, as opposed to
//! when it decides to open, which `hysteresis.rs` covers on its own.

use consort_audio::{FRAME_SAMPLES, GateConfig, VoiceGate};

mod vowel;

#[test]
fn a_shut_gate_emits_silence_rather_than_stale_audio() {
    let mut gate = VoiceGate::new(GateConfig::default());
    let input = vec![12_000i16; FRAME_SAMPLES];
    let mut output = vec![1i16; FRAME_SAMPLES];

    let decision = gate.process(&input, &mut output);

    assert!(!decision.open, "the first frame is the warm-up frame");
    assert!(
        output.iter().all(|sample| *sample == 0),
        "a shut gate has to overwrite the buffer. The caller keeps publishing \
         it, and leaving the previous frame in place would loop the last thing \
         said before the gate closed"
    );
}

#[test]
fn turning_denoising_off_changes_the_audio_and_not_the_decision() {
    let voiced = vowel::voiced_frames(40);
    let mut denoised = VoiceGate::new(GateConfig {
        denoise: true,
        ..GateConfig::default()
    });
    let mut raw = VoiceGate::new(GateConfig {
        denoise: false,
        ..GateConfig::default()
    });

    let mut differed = false;
    for frame in voiced.as_chunks::<FRAME_SAMPLES>().0 {
        let mut from_denoised = vec![0i16; FRAME_SAMPLES];
        let mut from_raw = vec![0i16; FRAME_SAMPLES];
        let a = denoised.process(frame, &mut from_denoised);
        let b = raw.process(frame, &mut from_raw);

        assert_eq!(
            a.probability, b.probability,
            "the model runs either way, so the reading must not depend on \
             whether its output is the audio that gets published"
        );
        assert_eq!(a.open, b.open, "the gate must behave identically");
        differed |= a.open && from_denoised != from_raw;
    }

    assert!(
        differed,
        "if the published samples are identical then `denoise` is doing nothing \
         and this option is a lie"
    );
}

#[test]
fn an_open_gate_publishes_audio() {
    let voiced = vowel::voiced_frames(40);
    let mut gate = VoiceGate::new(GateConfig::default());

    let mut heard = false;
    for frame in voiced.as_chunks::<FRAME_SAMPLES>().0 {
        let mut output = vec![0i16; FRAME_SAMPLES];
        if gate.process(frame, &mut output).open {
            heard |= output.iter().any(|sample| *sample != 0);
        }
    }

    assert!(heard, "an open gate that publishes silence is a shut gate");
}

#[test]
fn a_gate_reports_the_configuration_it_was_built_with() {
    let config = GateConfig {
        open_at: 0.71,
        close_at: 0.22,
        attack_frames: 4,
        hold_ms: 450,
        denoise: false,
    };

    assert_eq!(VoiceGate::new(config).config(), config);
}

#[test]
#[should_panic(expected = "one RNNoise frame")]
fn an_input_frame_of_the_wrong_length_is_a_bug_not_a_recoverable_error() {
    let mut gate = VoiceGate::new(GateConfig::default());
    let mut output = vec![0i16; FRAME_SAMPLES];

    gate.process(&[0i16; 160], &mut output);
}

#[test]
#[should_panic(expected = "one RNNoise frame")]
fn an_output_frame_of_the_wrong_length_is_a_bug_too() {
    let mut gate = VoiceGate::new(GateConfig::default());
    let mut output = vec![0i16; 160];

    gate.process(&[0i16; FRAME_SAMPLES], &mut output);
}
