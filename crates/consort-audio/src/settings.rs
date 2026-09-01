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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::gate::GateConfig;
use crate::mixing::FULL_VOLUME;

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
    /// **Off** by default, which is the one default here that is not the
    /// obvious one, so it is worth saying why. The chime and the sentence
    /// below announce the same event, and the chime's whole job was to get
    /// somebody's attention for the sentence that followed. Where there is a
    /// sentence, a chime in front of it is a doorbell before somebody who is
    /// already talking. Two notifications for one arrival is how a person ends
    /// up switching both off.
    ///
    /// So the pair ships as one sound rather than two, and this is the half
    /// that is off. Somebody who wants the chime as well turns it on and gets
    /// both, in that order; somebody who wants the chime *instead* turns the
    /// sentence off. Both are one click and neither is the state anybody lands
    /// in without choosing it.
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
    /// On by default, which is why this struct writes its own `Default`
    /// instead of deriving one: a derived `bool` is `false`, and a
    /// notification nobody has heard of is a notification nobody switches on.
    pub call_voices: bool,
    /// How loud a call should be, as a percentage.
    ///
    /// The master, covering everybody in the call and the notifications above
    /// them. A percentage rather than decibels because it is what a slider
    /// draws, and an integer rather than a float because a settings file gets
    /// hand-edited and `70` is easier to mean than `0.7071`.
    ///
    /// Full by default. Somebody who has not asked for anything should hear
    /// what the sound card was handed.
    pub output_volume: u8,
    /// How loud the chimes and spoken notifications should be, as a percentage
    /// of [`output_volume`](Self::output_volume).
    ///
    /// Sixty by default rather than full, which is the number the recordings
    /// asked for. A notification is mastered to be heard on its own; a call is
    /// somebody talking three feet from a microphone. Played at the same
    /// level, the notification is the loud thing in the room, and the first
    /// arrival after somebody puts headphones on is the wrong moment to find
    /// that out.
    ///
    /// Underneath the master rather than beside it, so turning a call down
    /// turns these down with it.
    pub notification_volume: u8,
    /// How loud one particular person should be, as a percentage, by Matrix
    /// user ID.
    ///
    /// Absent means full, so the map holds only the people somebody has
    /// actually adjusted rather than an entry per person they have ever been
    /// in a call with.
    ///
    /// Above a hundred is a boost rather than an attenuation, up to
    /// [`MAX_PERSON_VOLUME`](crate::MAX_PERSON_VOLUME). Unlike the two levels
    /// above it, which are a master and cannot usefully go past full, this one
    /// is a single voice among several and somebody who arrives quiet has to
    /// be brought up rather than everybody else brought down.
    ///
    /// A `BTreeMap` rather than a `HashMap` because this is written to a file
    /// a person may open: ordered keys mean the same settings produce the same
    /// bytes, and a diff shows the line that changed rather than a reshuffle.
    ///
    /// Per person and per machine, which is the only place it can be. There is
    /// no account data for "this one is too loud in my headphones", and there
    /// should not be: it is a fact about the room somebody is sitting in.
    pub person_volumes: BTreeMap<String, u8>,
}

impl Default for AudioSettings {
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
