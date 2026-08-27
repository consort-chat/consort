// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What is plugged in, and which of it to use.
//!
//! The host is a trait for a practical reason: CI has no sound card, so a
//! `cpal::default_host()` call in the middle of a function is a function no
//! test can run. `EventSink` in the Tauri crate already establishes this shape
//! here. Everything that decides anything takes the device list as data, and
//! the only code that talks to cpal is [`crate::cpal_host`], which is thin
//! enough to leave out of the coverage numbers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One device the host offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// What the host calls it, which is also all the identity there is.
    ///
    /// cpal 0.18 removed `Device::name()` and offers only `Display`, so two
    /// identical capture cards are indistinguishable to us. Known, accepted,
    /// and the reason a saved choice can resolve to the wrong twin.
    pub name: String,
    /// Whether the host reports this as the one it would pick.
    pub is_default: bool,
}

/// Which way the audio flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Input,
    Output,
}

/// Somewhere to ask what is plugged in.
pub trait AudioDevices: Send + Sync + 'static {
    /// Every device in this direction, in host order.
    ///
    /// May repeat itself and may be empty. [`catalogue`] is what tidies it.
    fn enumerate(&self, direction: Direction) -> Vec<Device>;
}

/// What a saved choice resolved to.
///
/// Four cases rather than an `Option`, because the settings screen draws each
/// one differently and collapsing them would mean drawing a device picker that
/// cannot explain itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The saved device, and it is plugged in.
    Saved(Device),
    /// Nothing was saved, so this is the host's answer.
    Default(Device),
    /// Something was saved, it is not here any more, and this is being used
    /// instead. The screen has to say so: a person who picked a headset and is
    /// being recorded by a laptop lid microphone deserves to be told.
    Substituted { wanted: String, using: Device },
    /// There is nothing to choose from.
    Nothing,
}

impl Selection {
    /// The device that will actually be used, if there is one.
    pub fn device(&self) -> Option<&Device> {
        match self {
            Self::Saved(device) | Self::Default(device) => Some(device),
            Self::Substituted { using, .. } => Some(using),
            Self::Nothing => None,
        }
    }

    /// The name to hand the audio backend, where `None` means "your default".
    ///
    /// Not the same question as [`device`](Self::device), which answers what
    /// to draw as selected. This answers what to open, and the two differ in
    /// exactly one case: when the resolved device is the one the host already
    /// calls its default, the backend is asked for the default rather than for
    /// that name.
    ///
    /// The difference is not cosmetic. A name is a photograph of the machine
    /// at the moment the list was read; the host's default is a live answer.
    /// Plug in a headset on Windows or macOS and the system default moves,
    /// which is what somebody who never opened this screen expects to happen.
    /// A saved name cannot move, and cpal 0.18 offers no identity beyond the
    /// display name, so re-resolving one can also land on the wrong twin of an
    /// identical pair.
    ///
    /// The exception is a host that lists devices and flags none as default.
    /// There is nothing to defer to, so the fallback is named: asking for the
    /// default there fails with "there is no audio input device" on a machine
    /// that visibly has one.
    pub fn name_to_open(&self) -> Option<&str> {
        match self.device() {
            Some(device) if device.is_default => None,
            Some(device) => Some(&device.name),
            None => None,
        }
    }
}

/// The device list worth showing: deduplicated, unnamed entries dropped, host
/// order kept.
pub fn catalogue(devices: &dyn AudioDevices, direction: Direction) -> Vec<Device> {
    let mut listed: Vec<Device> = Vec::new();
    let mut position: HashMap<String, usize> = HashMap::new();

    for device in devices.enumerate(direction) {
        if device.name.trim().is_empty() || is_plumbing(&device.name) {
            continue;
        }
        match position.get(&device.name) {
            // A repeat. The wrappers are not all equal: if any of them is the
            // one the host would hand back by default, the entry keeps that.
            Some(&index) => {
                let kept: &mut Device = &mut listed[index];
                kept.is_default |= device.is_default;
            }
            None => {
                position.insert(device.name.clone(), listed.len());
                listed.push(device);
            }
        }
    }

    listed
}

/// Match a saved device name against what is available.
pub fn choose(available: &[Device], saved: Option<&str>) -> Selection {
    let Some(fallback) = default_of(available) else {
        return Selection::Nothing;
    };

    // An empty or blank name is a settings file somebody edited by hand, not a
    // device that has been unplugged.
    let wanted = saved.map(str::trim).filter(|name| !name.is_empty());
    let Some(wanted) = wanted else {
        return Selection::Default(fallback.clone());
    };

    // Exact, not by substring. The saved name came from this same list, so a
    // near miss means the device changed rather than that it needs guessing at.
    match available.iter().find(|device| device.name == wanted) {
        Some(device) => Selection::Saved(device.clone()),
        None => Selection::Substituted {
            wanted: wanted.to_owned(),
            using: fallback.clone(),
        },
    }
}

/// Whether a name is one of ALSA's plugin wrappers rather than something a
/// person could speak into or listen to.
///
/// A PipeWire desktop offers 21 input devices, of which 12 are these. Nobody
/// has ever wanted to be recorded by a rate converter, and a list that long
/// where most of it is plumbing is a list people scroll past instead of read.
///
/// Matched against how ALSA names its wrappers, not against any use of the
/// word, because dropping somebody's actual microphone would be far worse than
/// leaving a resampler in the list. Sound servers (PipeWire, PulseAudio, JACK)
/// are deliberately kept: on a modern Linux desktop they are the entries most
/// worth selecting.
fn is_plumbing(name: &str) -> bool {
    const WRAPPERS: [&str; 3] = ["Rate Converter Plugin ", "Plugin using ", "Plugin for "];
    const NULL_DEVICE: &str = "Discard all samples (playback) or generate zero samples (capture)";

    let name = name.trim();
    name == NULL_DEVICE || WRAPPERS.iter().any(|prefix| name.starts_with(prefix))
}

/// Whichever device the host flagged, or the first one if it flagged none.
fn default_of(available: &[Device]) -> Option<&Device> {
    available
        .iter()
        .find(|device| device.is_default)
        .or_else(|| available.first())
}

/// What the settings screen is told about one direction.
///
/// Three facts, because a picker needs three and no more: what there is, which
/// one is in use, and whether that is the one that was asked for. The third is
/// the easiest to leave out and the most expensive to have left out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    /// Everything worth offering, in host order.
    pub devices: Vec<Device>,
    /// The device audio will actually go through, by name.
    ///
    /// `None` only when there are no devices at all. A picker showing nothing
    /// selected while audio is flowing would be lying.
    pub selected: Option<String>,
    /// The saved device, when it is not here any more.
    ///
    /// `None` in every other case, including when there is nothing to fall back
    /// to: with no devices at all there is no substitution to report, only an
    /// empty machine.
    pub missing: Option<String>,
}

impl DeviceList {
    /// Resolve `saved` against `devices` and describe the outcome.
    pub fn of(devices: Vec<Device>, saved: Option<&str>) -> Self {
        let selection = choose(&devices, saved);
        Self {
            selected: selection.device().map(|device| device.name.clone()),
            missing: match &selection {
                Selection::Substituted { wanted, .. } => Some(wanted.clone()),
                Selection::Saved(_) | Selection::Default(_) | Selection::Nothing => None,
            },
            devices,
        }
    }
}

/// Both directions at once, which is what the settings screen asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceReport {
    pub input: DeviceList,
    pub output: DeviceList,
}

impl AudioDeviceReport {
    /// Ask `host` what it has and resolve each direction against its own saved
    /// choice.
    ///
    /// Two arguments rather than one settings struct, so that crossing them is
    /// a type error at the call site rather than something that silently
    /// records from the speakers.
    pub fn of(host: &dyn AudioDevices, input: Option<&str>, output: Option<&str>) -> Self {
        Self {
            input: DeviceList::of(catalogue(host, Direction::Input), input),
            output: DeviceList::of(catalogue(host, Direction::Output), output),
        }
    }
}
