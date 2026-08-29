// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The audio choices worth remembering between runs.
//!
//! Data only. Where this is written, and in what file, is the application's
//! business, and keeping it out of here is what lets this crate stay free of
//! everything the application depends on.
//!
//! Every field defaults. A settings file outlives the build that wrote it: it
//! gets read by an older Consort after a downgrade, by a newer one after an
//! upgrade, and by whatever somebody turned it into with a text editor. None of
//! those should stop the application from starting, so an unreadable field
//! falls back rather than failing.

use serde::{Deserialize, Serialize};

use crate::gate::GateConfig;

/// Which devices to use, how eager the voice gate should be, and what noise a
/// call makes about the people coming and going in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// The chosen input device by name, or `None` to ask the host.
    ///
    /// `None` rather than an empty string, because an empty string is a device
    /// name that can never match anything.
    pub input: Option<String>,
    /// The chosen output device by name, or `None` to ask the host.
    pub output: Option<String>,
    /// How the voice gate is tuned. Per person and per room, so there is no
    /// useful default beyond a starting point.
    pub gate: GateConfig,
    /// Whether to make a sound when somebody joins or leaves the voice
    /// channel this session is in.
    ///
    /// On by default, which is why this struct writes its own `Default`
    /// instead of deriving one: a derived `bool` is `false`, and the whole
    /// point of these is that somebody who has not gone looking for a setting
    /// still hears that company arrived.
    ///
    /// Worth turning off, which is why it is a setting at all. A channel with
    /// a lot of coming and going is one where these stop being information
    /// and become a noise.
    pub call_sounds: bool,
    /// Whether to say out loud what the chimes above only announce.
    ///
    /// A second setting rather than a mode of the first, because the two are
    /// wanted separately in both directions and neither is the greater
    /// helping of the other. A chime says something happened; a voice says
    /// what. Somebody who wants to know that company arrived without being
    /// read a sentence about it, and somebody who wants the sentence and no
    /// chime before it, are both ordinary.
    ///
    /// On by default, and hand-written for the same reason `call_sounds` is:
    /// a derived `bool` is `false`, and a notification nobody has heard of is
    /// a notification nobody switches on.
    pub call_voices: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            gate: GateConfig::default(),
            call_sounds: true,
            call_voices: true,
        }
    }
}
