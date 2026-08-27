// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Deciding whether the person at the microphone is talking.
//!
//! RNNoise is a denoiser, and it reports a voice probability for each frame as
//! a byproduct of denoising. One pass therefore answers both halves of the
//! question: what to publish, and whether to publish it at all.
//!
//! Two types, because they fail differently. [`Hysteresis`] is the decision:
//! pure arithmetic over a probability, with no model behind it, so it can be
//! driven frame by frame in a test and every branch reached in microseconds.
//! [`VoiceGate`] is that plus the model, and its tests need synthesised audio
//! and a real inference pass.
//!
//! Ported from the `matrix-rtc-vad-spike` prototype, which tuned these defaults
//! by ear against a live call.

use nnnoiseless::DenoiseState;
use serde::{Deserialize, Serialize};

/// Samples in one frame. RNNoise accepts this length and no other.
pub const FRAME_SAMPLES: usize = DenoiseState::FRAME_SIZE;

/// The only sample rate RNNoise was trained for.
///
/// Nothing in this crate resamples. 480 samples at 48 kHz is exactly 10 ms,
/// which is also what the media layer wants, so the pieces line up and a device
/// that cannot do 48 kHz is a hard error rather than a silent quality loss.
pub const SAMPLE_RATE: u32 = 48_000;

/// Milliseconds in one frame, which follows from the two constants above.
pub const FRAME_MS: u32 = 10;

/// How eagerly the gate opens, and how reluctantly it shuts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateConfig {
    /// Voice probability at or above which the gate may open.
    pub open_at: f32,
    /// Voice probability below which the hold timer starts running down.
    ///
    /// Lower than [`open_at`](Self::open_at) on purpose. One threshold
    /// chatters: a voice hovering near it toggles the gate every few frames and
    /// the listener hears the front of each word clipped.
    pub close_at: f32,
    /// Consecutive frames above `open_at` required to open.
    ///
    /// Rejects the single loud transient, a key press or a desk bump, that
    /// momentarily reads as speech.
    pub attack_frames: u32,
    /// How long the gate stays open after the probability drops below
    /// `close_at`.
    ///
    /// This is what keeps the tail of a word, and the pause between two words,
    /// from being cut.
    pub hold_ms: u32,
    /// Publish the denoised signal rather than the raw capture.
    ///
    /// Turning this off still runs the model for its probability, so the gate
    /// behaves identically and only the audio differs. It exists to A/B the
    /// denoiser by ear.
    pub denoise: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            open_at: 0.60,
            close_at: 0.30,
            attack_frames: 2,
            hold_ms: 300,
            denoise: true,
        }
    }
}

impl GateConfig {
    fn hold_frames(&self) -> u32 {
        self.hold_ms.div_ceil(FRAME_MS)
    }
}

/// What the gate decided about one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateDecision {
    /// Whether this frame carries voice out to the network.
    pub open: bool,
    /// The gate opened on this frame. An edge, so it is true once per opening.
    pub opened: bool,
    /// The gate closed on this frame. An edge, so it is true once per closing.
    pub closed: bool,
    /// The model's raw voice probability for this frame, before hysteresis.
    ///
    /// Passed through untouched, because this is what the meter draws and a
    /// meter showing the gate's opinion rather than the model's would be
    /// useless for choosing a threshold.
    pub probability: f32,
}

/// The gate's state machine: probabilities in, decisions out.
///
/// No model and no audio, which is what makes it exhaustively testable.
#[derive(Clone, Debug)]
pub struct Hysteresis {
    config: GateConfig,
    above_streak: u32,
    hold_left: u32,
    open: bool,
    /// RNNoise has fade-in artifacts on its very first output frame, and its
    /// probability for that frame is not meaningful either. Drop one.
    warming_up: bool,
}

impl Hysteresis {
    pub fn new(config: GateConfig) -> Self {
        Self {
            config,
            above_streak: 0,
            hold_left: 0,
            open: false,
            warming_up: true,
        }
    }

    pub fn config(&self) -> GateConfig {
        self.config
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Advance by one frame's worth of probability.
    pub fn step(&mut self, probability: f32) -> GateDecision {
        let was_open = self.open;
        self.advance(probability);
        GateDecision {
            open: self.open,
            opened: self.open && !was_open,
            closed: !self.open && was_open,
            probability,
        }
    }

    fn advance(&mut self, probability: f32) {
        if self.warming_up {
            self.warming_up = false;
            return;
        }

        if probability >= self.config.open_at {
            self.above_streak = self.above_streak.saturating_add(1);
        } else {
            self.above_streak = 0;
        }

        if !self.open {
            // `max(1)` because an attack of zero frames is not a gate that is
            // always open, it is a configuration mistake.
            if self.above_streak >= self.config.attack_frames.max(1) {
                self.open = true;
                self.hold_left = self.config.hold_frames();
            }
            return;
        }

        // Open. Any frame still above the lower threshold recharges the hold,
        // so a normal sentence never has to re-satisfy the attack.
        if probability >= self.config.close_at {
            self.hold_left = self.config.hold_frames();
            return;
        }

        self.hold_left = self.hold_left.saturating_sub(1);
        if self.hold_left == 0 {
            self.open = false;
            self.above_streak = 0;
        }
    }
}

/// The denoiser plus the state machine. One per capture stream.
pub struct VoiceGate {
    denoiser: Box<DenoiseState<'static>>,
    hysteresis: Hysteresis,
    input_f32: Vec<f32>,
    output_f32: Vec<f32>,
}

impl VoiceGate {
    pub fn new(config: GateConfig) -> Self {
        Self {
            denoiser: DenoiseState::new(),
            hysteresis: Hysteresis::new(config),
            input_f32: vec![0.0; FRAME_SAMPLES],
            output_f32: vec![0.0; FRAME_SAMPLES],
        }
    }

    pub fn config(&self) -> GateConfig {
        self.hysteresis.config()
    }

    /// Run one frame through the denoiser and the gate.
    ///
    /// `input` and `output` must both be [`FRAME_SAMPLES`] long, mono, 48 kHz.
    ///
    /// When the gate is shut, `output` is filled with silence rather than left
    /// alone. The caller is expected to keep publishing it: a sender that stops
    /// sending looks like a wedged client to a peer, while Opus collapses
    /// silence on the wire and costs nothing.
    ///
    /// # Panics
    ///
    /// If either slice is not [`FRAME_SAMPLES`] long. A frame of the wrong
    /// length means the capture layer is misconfigured, which is a bug at
    /// startup rather than something to recover from once per 10 ms.
    pub fn process(&mut self, input: &[i16], output: &mut [i16]) -> GateDecision {
        assert_eq!(
            input.len(),
            FRAME_SAMPLES,
            "the input frame must be one RNNoise frame"
        );
        assert_eq!(
            output.len(),
            FRAME_SAMPLES,
            "the output frame must be one RNNoise frame"
        );

        // nnnoiseless wants f32 that is still in i16 RANGE, not the [-1.0, 1.0]
        // that float PCM usually means. So this is a plain cast. Dividing by
        // 32768 here, which is the reflex, hands the model a signal about 90 dB
        // too quiet and it reports silence forever. See `tests/scaling.rs`.
        for (destination, source) in self.input_f32.iter_mut().zip(input) {
            *destination = f32::from(*source);
        }

        let probability = self
            .denoiser
            .process_frame(&mut self.output_f32, &self.input_f32);
        let decision = self.hysteresis.step(probability);

        if !decision.open {
            output.fill(0);
            return decision;
        }

        let voiced = if self.config().denoise {
            &self.output_f32
        } else {
            &self.input_f32
        };
        for (destination, source) in output.iter_mut().zip(voiced) {
            // The denoiser can overshoot the input range, so clamp rather than
            // let the cast saturate into a click.
            *destination = source.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
        decision
    }
}
