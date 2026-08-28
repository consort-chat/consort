// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Audio devices, capture, and the voice gate that decides when you are
//! talking.
//!
//! Separate from `consort-matrix` for the reason the spike this is ported from
//! kept it separate: nothing here depends on matrix-sdk, so it builds in
//! seconds. Picking a gate threshold is an iterate-by-ear loop, and a loop that
//! runs through a matrix-sdk rebuild is a loop nobody runs.

pub mod capture;
pub mod cpal_host;
pub mod devices;
pub mod frames;
pub mod gate;
pub mod meter;
pub mod mixing;
pub mod playback;
pub mod settings;
pub mod sound;
pub mod thread;
pub mod tone;

pub use capture::{AudioCapture, CaptureError, CaptureStream, FrameSink};
pub use cpal_host::CpalHost;
pub use devices::{
    Answer, AudioDeviceReport, AudioDevices, Device, DeviceList, Direction, Selection, catalogue,
    choose,
};
pub use frames::Frames;
pub use gate::{
    FRAME_MS, FRAME_SAMPLES, GateConfig, GateDecision, Hysteresis, PRE_ROLL_FRAMES, PreRoll,
    SAMPLE_RATE, VoiceGate,
};
pub use meter::{FRAMES_PER_READING, Meter, READINGS_PER_SECOND, Reading};
pub use mixing::{JITTER_FRAMES, JITTER_SAMPLES, Mixing, SOUND_SAMPLES, Voices};
pub use playback::{AudioPlayback, PlaybackError, PlaybackStream, Playing, ToneEnded};
pub use settings::AudioSettings;
pub use sound::Sound;
pub use thread::{AudioEvent, AudioThread, GatedSink};
pub use tone::Tone;
