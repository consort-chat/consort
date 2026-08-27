// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Guards the one mistake that silently kills this whole pipeline.
//!
//! nnnoiseless wants `f32` samples that are still in **i16 range**, not the
//! `[-1.0, 1.0]` that float PCM normally means. Dividing by 32768 on the way in
//! is the reflex: it compiles, it runs, and it hands the model a signal about
//! 90 dB below what it was trained on. The model then reports silence forever,
//! the gate never opens, and nothing anywhere logs a problem.
//!
//! These assert that the model reacts to a voiced vowel at i16 scale and does
//! not at normalised scale. That is a statement about the convention, not about
//! how good the model is.

use consort_audio::{FRAME_SAMPLES, GateConfig, VoiceGate};
use nnnoiseless::DenoiseState;

mod vowel;

/// Mean voice probability over every frame but the first, which RNNoise warns
/// carries fade-in artifacts.
fn mean_probability(signal: &[f32]) -> f32 {
    let mut denoiser = DenoiseState::new();
    let mut output = vec![0.0f32; FRAME_SAMPLES];
    let mut total = 0.0;
    let mut counted = 0;
    for (index, frame) in signal.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
        let probability = denoiser.process_frame(&mut output, frame);
        if index > 0 {
            total += probability;
            counted += 1;
        }
    }
    total / counted.max(1) as f32
}

#[test]
fn the_model_responds_at_i16_scale_and_not_at_normalised_scale() {
    let voiced = vowel::voiced(100);
    let silence = vec![0.0f32; 100 * FRAME_SAMPLES];
    let normalised: Vec<f32> = voiced.iter().map(|s| s / f32::from(i16::MAX)).collect();

    let p_voiced = mean_probability(&voiced);
    let p_silence = mean_probability(&silence);
    let p_normalised = mean_probability(&normalised);

    println!("i16 scale   {p_voiced:.3}");
    println!("silence     {p_silence:.3}");
    println!("normalised  {p_normalised:.3}  <- the mistake");

    assert!(
        p_silence < 0.05,
        "silence should not read as voice, got {p_silence:.3}"
    );
    assert!(
        p_voiced > 0.5,
        "a voiced vowel at i16 scale should read as voice, got {p_voiced:.3}. \
         If this drops, check the i16-range convention on the way into the model"
    );
    assert!(
        p_normalised < p_voiced / 2.0,
        "dividing by 32768 should visibly starve the model ({p_normalised:.3} \
         against {p_voiced:.3}). If these are close, this test no longer guards \
         anything"
    );
}

#[test]
fn the_gate_opens_on_a_voiced_vowel_and_shuts_after_it() {
    let config = GateConfig::default();
    let mut gate = VoiceGate::new(config);
    let mut output = vec![0i16; FRAME_SAMPLES];
    let voiced = vowel::voiced_frames(50);

    let mut opened_at = None;
    for (index, frame) in voiced.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
        if gate.process(frame, &mut output).opened {
            opened_at = Some(index);
            break;
        }
    }

    let opened_at = opened_at.expect("the gate never opened on voiced audio");
    // The warm-up frame plus the attack window, with room to spare. Much later
    // than this and the default thresholds have drifted somewhere unusable.
    assert!(
        opened_at <= 8,
        "the gate took {opened_at} frames ({} ms) to open",
        opened_at * 10
    );

    let silence = vec![0i16; FRAME_SAMPLES];
    let mut shut_after = None;
    for index in 0..100 {
        if gate.process(&silence, &mut output).closed {
            shut_after = Some(index + 1);
            break;
        }
    }

    let shut_after = shut_after.expect("the gate never shut on silence");
    let hold_frames = (config.hold_ms / 10) as usize;
    assert!(
        shut_after >= hold_frames,
        "the gate shut after {shut_after} frames, before its {hold_frames}-frame \
         hold had elapsed"
    );
    println!("opened after {opened_at} frames, shut {shut_after} frames into silence");
}
