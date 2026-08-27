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
        voice_activity: false,
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

#[test]
fn with_voice_activity_off_a_silent_frame_is_still_published() {
    // The model runs either way, so this is the difference the toggle makes
    // where it can be seen: a frame RNNoise would score as nothing at all
    // still reaches the output buffer.
    let mut gate = VoiceGate::new(GateConfig {
        voice_activity: false,
        ..GateConfig::default()
    });
    let quiet = vec![40i16; FRAME_SAMPLES];
    let mut output = vec![0i16; FRAME_SAMPLES];
    // Past the warm-up frame, which is dropped in both modes.
    gate.process(&quiet, &mut output);

    let decision = gate.process(&quiet, &mut output);

    assert!(decision.open, "the gate is off and still swallowed a frame");
    assert!(
        output.iter().any(|sample| *sample != 0),
        "nothing was published: {decision:?}"
    );
}

#[test]
fn turning_voice_activity_off_does_not_turn_denoising_off_with_it() {
    // Two separate choices that both live in `GateConfig`. Somebody who wants
    // everything transmitted still wants their fan suppressed.
    let voiced = vowel::voiced_frames(40);
    let mut gate = VoiceGate::new(GateConfig {
        voice_activity: false,
        denoise: true,
        ..GateConfig::default()
    });
    let mut output = vec![0i16; FRAME_SAMPLES];

    let mut differed = false;
    for frame in voiced.as_chunks::<FRAME_SAMPLES>().0 {
        gate.process(frame, &mut output);
        differed |= output.as_slice() != frame.as_slice();
    }

    assert!(
        differed,
        "the output was the input untouched, so the denoiser did not run"
    );
}

#[test]
fn retuning_keeps_the_denoiser_warm() {
    // The denoiser carries the spectral history that makes it work. Rebuilding
    // it to change a threshold would put a fresh warm-up artifact into the
    // middle of a sentence, which is the opposite of what a retune is for.
    let voiced = vowel::voiced_frames(40);
    let frames = voiced.as_chunks::<FRAME_SAMPLES>().0;
    let mut gate = VoiceGate::new(GateConfig::default());
    let mut output = vec![0i16; FRAME_SAMPLES];
    let mut ever_open = false;
    for frame in frames {
        ever_open |= gate.process(frame, &mut output).open;
    }
    assert!(ever_open, "the gate never opened on a vowel");

    gate.retune(GateConfig {
        hold_ms: 500,
        ..GateConfig::default()
    });

    let decision = gate.process(&frames[1], &mut output);
    assert!(
        decision.open,
        "the retune reset something it should not have: {decision:?}"
    );
    assert_eq!(gate.config().hold_ms, 500);
}
