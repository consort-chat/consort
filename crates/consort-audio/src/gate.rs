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
// `default` at the container level, so a file that carries one tuned threshold
// does not silently reset the others to zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
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
    /// Send only while somebody is talking.
    ///
    /// Turning this off publishes every frame and makes the thresholds above
    /// inert. The model still runs, so the probability is still reported and
    /// the denoiser still denoises: this is a choice about what to send, not
    /// about what to compute.
    ///
    /// A choice rather than a policy because the gate is not always wanted. A
    /// quiet room with a good microphone gains nothing from it, and anybody
    /// whose speech the model happens to score badly is better served by
    /// transmitting everything than by being cut off mid-sentence.
    pub voice_activity: bool,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            open_at: 0.60,
            close_at: 0.30,
            attack_frames: 2,
            hold_ms: 300,
            denoise: true,
            // On, because the default has to be the gate rather than the
            // absence of one. Somebody who never opens the settings screen
            // should not be transmitting their keyboard to everybody in the
            // room.
            voice_activity: true,
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

    /// Swap the tuning, keeping the state machine where it is.
    ///
    /// Not a reset. Somebody moving a threshold is doing it mid-sentence with
    /// the meter in front of them, and starting the attack count and the hold
    /// timer again would cut the word they are in the middle of saying. The
    /// new thresholds apply from the next frame, which is 10 ms away.
    pub fn retune(&mut self, config: GateConfig) {
        self.config = config;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the next frame is the one whose output has to be discarded.
    ///
    /// True exactly once per gate, before the first [`step`](Self::step).
    /// [`VoiceGate`] asks so that it can publish silence for that frame: the
    /// probability is meaningless, which this type already handles, and the
    /// denoiser's output for it is a fade-in ramp rather than anything that was
    /// said, which it cannot.
    pub fn is_warming_up(&self) -> bool {
        self.warming_up
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

        if !self.config.voice_activity {
            // Everything goes out. Checked before the thresholds rather than
            // by setting them to zero, so that turning the gate back on finds
            // the tuning exactly as it was left.
            self.open = true;
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

/// How many frames the gate's output is held back, so that an opening edge
/// can still reach for what came before it.
///
/// One more than the default [`GateConfig::attack_frames`], which is the whole
/// point: the attack spends 20 ms proving somebody has started talking, and
/// those 20 ms are the start of the word they said. Three frames is 30 ms, so
/// the proof is covered with a frame to spare.
///
/// Not configurable, because the number that matters is the attack and nothing
/// exposes that either. If the attack ever becomes tunable this has to follow
/// it up.
pub const PRE_ROLL_FRAMES: usize = 3;

/// One frame waiting its turn, and what the gate thought of it at the time.
struct Held {
    samples: Vec<i16>,
    open: bool,
}

impl Held {
    fn silent() -> Self {
        Self {
            samples: vec![0; FRAME_SAMPLES],
            open: false,
        }
    }
}

/// A delay line that lets the gate change its mind about frames it has already
/// seen.
///
/// The gate cannot open on the first frame of a word: [`attack_frames`] exists
/// so that a key press or a desk bump does not open it, and the cost is that by
/// the time the gate is sure, the consonant that made it sure has been and
/// gone. "Pop" arrives as "op".
///
/// So hold every frame back by [`PRE_ROLL_FRAMES`] and publish the oldest. When
/// the gate opens, the frames that convinced it are still here, and marking
/// them open sends the whole word. The attack is paid for in latency instead of
/// in consonants, which is the trade worth making: 30 ms is inaudible next to
/// what a jitter buffer already costs, and a clipped word is not.
///
/// The delay runs even with [`voice_activity`] off, when nothing is ever
/// withheld and it achieves nothing. Bypassing it would mean the audio jumping
/// 30 ms whenever somebody flips that switch, and a click on a settings screen
/// is a worse thing to hear than 30 ms nobody can perceive.
///
/// [`attack_frames`]: GateConfig::attack_frames
/// [`voice_activity`]: GateConfig::voice_activity
pub struct PreRoll {
    depth: usize,
    /// Oldest first. Never longer than `depth + 1`, briefly, inside [`step`].
    ///
    /// [`step`]: Self::step
    line: std::collections::VecDeque<Held>,
    /// The frame handed out last call.
    ///
    /// The caller is done with it by the time it calls again, so it comes back
    /// here to be refilled. That makes this whole type allocation-free after
    /// the first `depth + 1` frames, which matters because it runs on the audio
    /// thread once every 10 ms.
    returned: Option<Held>,
}

impl Default for PreRoll {
    fn default() -> Self {
        Self::new(PRE_ROLL_FRAMES)
    }
}

impl PreRoll {
    /// A line `depth` frames long. Zero is allowed and means no delay at all.
    pub fn new(depth: usize) -> Self {
        Self {
            depth,
            line: std::collections::VecDeque::with_capacity(depth + 1),
            returned: None,
        }
    }

    /// How far behind the microphone this puts the published audio.
    pub fn latency_ms(&self) -> u32 {
        self.depth as u32 * FRAME_MS
    }

    /// Put one frame in, take the frame `depth` frames older out.
    ///
    /// `None` while the line is still filling, which is the first `depth`
    /// frames of a capture and nothing else. The frame that comes back is
    /// already silenced if it is not being sent, so the pair matches
    /// [`crate::GatedSink`] exactly.
    ///
    /// # Panics
    ///
    /// If `frame` is not [`FRAME_SAMPLES`] long, for the reason
    /// [`VoiceGate::process`] panics.
    pub fn step(&mut self, frame: &[i16], decision: GateDecision) -> Option<(&[i16], bool)> {
        assert_eq!(
            frame.len(),
            FRAME_SAMPLES,
            "the frame must be one RNNoise frame"
        );

        if decision.opened {
            // The reason this type exists. Everything still in the line was
            // captured while the gate was making its mind up, which is to say
            // during the start of the word that changed it.
            for held in self.line.iter_mut() {
                held.open = true;
            }
        }

        let mut slot = self.returned.take().unwrap_or_else(Held::silent);
        slot.samples.copy_from_slice(frame);
        slot.open = decision.open;
        self.line.push_back(slot);

        if self.line.len() <= self.depth {
            return None;
        }

        let mut out = self
            .line
            .pop_front()
            .expect("the line is longer than depth, so it is not empty");
        if !out.open {
            // Silenced here rather than by the gate, so that a frame the gate
            // shut on can still be reopened while it waits. The caller keeps
            // publishing it either way: a sender that stops sending looks like
            // a wedged client to a peer, while Opus collapses silence on the
            // wire and costs nothing.
            out.samples.fill(0);
        }
        let out = self.returned.insert(out);
        Some((&out.samples, out.open))
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

    /// Swap the tuning without disturbing the denoiser or the gate's state.
    ///
    /// The denoiser in particular must survive this: it carries the spectral
    /// history that makes it work, and rebuilding it to change a threshold
    /// would put a fresh warm-up artifact into the middle of a sentence.
    pub fn retune(&mut self, config: GateConfig) {
        self.hysteresis.retune(config);
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
        let decision = self.process_ungated(input, output);
        if !decision.open {
            output.fill(0);
        }
        decision
    }

    /// [`process`](Self::process) without the verdict applied to the audio.
    ///
    /// `output` always carries the processed frame, whatever the gate decided.
    /// The decision comes back untouched for the caller to act on, or not act
    /// on yet: [`PreRoll`] exists because the frames worth keeping are the ones
    /// captured *before* the gate opened, and it cannot keep what `process`
    /// has already zeroed.
    ///
    /// # Panics
    ///
    /// As [`process`](Self::process).
    pub fn process_ungated(&mut self, input: &[i16], output: &mut [i16]) -> GateDecision {
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

        let warming_up = self.hysteresis.is_warming_up();
        let probability = self
            .denoiser
            .process_frame(&mut self.output_f32, &self.input_f32);
        let decision = self.hysteresis.step(probability);

        if warming_up {
            // The one frame with no audio worth publishing. RNNoise fades in
            // over its first output, so this frame is a ramp rather than
            // anything anybody said. Silenced here rather than left to the
            // gate, because [`PreRoll`] can reopen a frame the gate shut on and
            // would otherwise send the ramp as the first thing a listener
            // hears.
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
