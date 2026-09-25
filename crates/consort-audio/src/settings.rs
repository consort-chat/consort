// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The audio choices worth remembering between runs.
//!
//! Data only: where this is written is the application's business, and keeping
//! it out of here is what lets this crate stay free of matrix-sdk. Every field
//! defaults, because a settings file outlives the build that wrote it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::gate::GateConfig;
use crate::mixing::FULL_VOLUME;

/// Which devices to use, how eager the voice gate should be, and what noise a
/// call makes about the people coming and going in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// The chosen input device by name, or `None` to ask the host. `None`
    /// rather than an empty string, which is a name nothing can match.
    pub input: Option<String>,
    /// The chosen output device by name, or `None` to ask the host.
    pub output: Option<String>,
    /// How the voice gate is tuned.
    pub gate: GateConfig,
    /// Whether to chime when somebody joins or leaves the voice channel. Off
    /// by default: the sentence below announces the same arrival, and two
    /// notifications for one event is how somebody switches both off.
    pub call_sounds: bool,
    /// Whether to say out loud what the chime above only announces.
    pub call_voices: bool,
    /// How loud a call should be, as a percentage. The master, covering
    /// everybody in it and the notifications underneath them.
    pub output_volume: u8,
    /// How loud chimes and spoken notifications are, as a percentage of
    /// [`output_volume`](Self::output_volume). Sixty, not full: the recordings
    /// are mastered louder than somebody talking three feet from a microphone.
    pub notification_volume: u8,
    /// How loud one person should be, by Matrix user ID; absent means full,
    /// and the ceiling is [`MAX_PERSON_VOLUME`](crate::MAX_PERSON_VOLUME).
    /// A `BTreeMap` for stable bytes in a file somebody may hand-edit.
    pub person_volumes: BTreeMap<String, u8>,
}

impl Default for AudioSettings {
    /// Hand-written: a derived one would mute both volumes and switch the
    /// spoken notifications off.
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            gate: GateConfig::default(),
            call_sounds: false,
            call_voices: true,
            output_volume: FULL_VOLUME,
            notification_volume: 60,
            person_volumes: BTreeMap::new(),
        }
    }
}
