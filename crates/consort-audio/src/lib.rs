// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Audio devices, capture, and the voice gate that decides when you are
//! talking.
//!
//! Separate from `consort-matrix` for the reason the spike this is ported from
//! kept it separate: nothing here depends on matrix-sdk, so it builds in
//! seconds. Picking a gate threshold is an iterate-by-ear loop, and a loop that
//! runs through a matrix-sdk rebuild is a loop nobody runs.

pub mod cpal_host;
pub mod devices;
pub mod frames;
pub mod gate;
pub mod settings;

pub use cpal_host::CpalHost;
pub use devices::{AudioDeviceReport, AudioDevices, Device, DeviceList, Direction, Selection};
pub use frames::Frames;
pub use gate::{
    FRAME_MS, FRAME_SAMPLES, GateConfig, GateDecision, Hysteresis, SAMPLE_RATE, VoiceGate,
};
pub use settings::AudioSettings;
