// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the level bar draws.
//!
//! Batches the gate's hundred frames a second down to something worth a JSON
//! round trip across the IPC boundary. Counted rather than clocked: the frame
//! rate is fixed by construction, so there is no `Instant` here and the tests
//! are exact without sleeping.

use serde::{Deserialize, Serialize};

use crate::gate::{FRAME_MS, GateDecision};

/// Frames folded into one reading.
///
/// Five frames is 50 ms: fast enough that the bar tracks a voice, slow enough
/// that the IPC is not the expensive part.
pub const FRAMES_PER_READING: usize = 5;

/// Readings per second, which follows from the constant above.
pub const READINGS_PER_SECOND: u32 = 1000 / (FRAME_MS * FRAMES_PER_READING as u32);

/// One update for the level bar.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    /// Peak amplitude in the batch as a fraction of full scale, 0 to 1.
    ///
    /// The peak, not the mean: a transient in the middle of a batch is what
    /// somebody tapping their microphone is watching for.
    pub level: f32,
    /// The highest voice probability the model reported in the batch.
    ///
    /// Also a maximum, so that the number somebody reads while choosing a
    /// threshold is the number the gate would have seen.
    pub probability: f32,
    /// Whether the gate was open at any point in the batch.
    ///
    /// Sticky, because the gate genuinely closes between syllables and a bar
    /// that flickers off mid-word looks broken.
    pub open: bool,
}

/// Folds gate decisions into readings.
#[derive(Debug, Default)]
pub struct Meter {
    counted: usize,
    level: f32,
    probability: f32,
    open: bool,
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in one frame, and hand back a reading when the batch is full.
    pub fn fold(&mut self, decision: GateDecision, frame: &[i16]) -> Option<Reading> {
        self.level = self.level.max(peak_of(frame));
        self.probability = self.probability.max(decision.probability);
        self.open |= decision.open;
        self.counted += 1;

        if self.counted < FRAMES_PER_READING {
            return None;
        }

        let reading = Reading {
            level: self.level,
            probability: self.probability,
            open: self.open,
        };
        // Reset, not decay: otherwise the peak only climbs and the bar sticks
        // at the loudest thing since launch.
        *self = Self::default();
        Some(reading)
    }
}

/// The largest magnitude an `i16` sample can have.
///
/// 32768, not `i16::MAX`: the range is asymmetric, so dividing by `i16::MAX`
/// lets `i16::MIN` report 1.00003 and draws the bar past its own track.
const FULL_SCALE: f32 = 32_768.0;

/// The loudest sample in a frame, as a fraction of full scale.
pub(crate) fn peak_of(frame: &[i16]) -> f32 {
    let peak = frame
        .iter()
        // `unsigned_abs`, because `i16::MIN.abs()` overflows: it has no
        // positive counterpart.
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0);
    f32::from(peak) / FULL_SCALE
}
