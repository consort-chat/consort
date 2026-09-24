// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What is plugged in, and which of it to use.
//!
//! The host is a trait because CI has no sound card: everything that decides
//! anything takes the device list as data, and only [`crate::cpal_host`] talks
//! to cpal. Why none of that list is trusted:
//! `docs/adr/0004-trust-no-device-list.md`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One device the host offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// What the host calls it, and all the identity there is: cpal 0.18
    /// removed `Device::name()`. Two identical cards are indistinguishable, so
    /// a saved choice can resolve to the wrong twin.
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

/// What a host said when asked whether a device can do what we need.
///
/// A reply can be a fact about the device, a fact about this moment, or
/// unhelpful. Collapsing those into a boolean gets one of them wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// It can.
    Yes,
    /// It answered, and it cannot. A microphone asked to play sound.
    No,
    /// Somebody has it open. That it is in use is proof it works.
    Busy,
    /// Not there, or not there in this direction.
    Absent,
    /// There, and this process is not allowed to open it.
    Forbidden,
    /// The host said something not recognised here.
    Unclear,
}

impl Answer {
    /// Whether a device that answered this way belongs in the picker.
    ///
    /// Dropped only on a definite no. Hiding a device that works costs
    /// somebody more than listing one that does not.
    pub fn worth_listing(self) -> bool {
        match self {
            Self::Yes | Self::Busy | Self::Unclear => true,
            Self::No | Self::Absent | Self::Forbidden => false,
        }
    }
}

/// Somewhere to ask what is plugged in.
pub trait AudioDevices: Send + Sync + 'static {
    /// Every device that can actually be opened in this direction, in host
    /// order.
    ///
    /// Usable, not merely present: an implementation that passes a host's
    /// direction flags straight through offers a webcam to play sound out of.
    /// May repeat itself and may be empty. [`catalogue`] tidies it.
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
    /// Something was saved, it is gone, and this is being used instead. The
    /// screen has to say so, or somebody who picked a headset is recorded by
    /// a laptop lid without being told.
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
    /// Not [`device`](Self::device), which answers what to draw as selected.
    /// The host's default is a live answer and a saved name is a photograph,
    /// so the two differ when the resolved device is the host's own default.
    /// Why that matters: `docs/adr/0004-trust-no-device-list.md`.
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
            // A repeat. If any of them is the one the host would hand back by
            // default, the entry keeps that.
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
/// Matched against how ALSA names its wrappers, not against any use of the
/// words, because dropping a real microphone is worse than leaving a resampler
/// listed. PipeWire, PulseAudio and JACK are deliberately kept.
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
/// Three facts, because a picker needs three: what there is, which one is in
/// use, and whether that is the one that was asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    /// Everything worth offering, in host order.
    pub devices: Vec<Device>,
    /// The device audio will actually go through, by name. `None` only when
    /// there are no devices at all.
    pub selected: Option<String>,
    /// The saved device, when it is not here any more. `None` when there is
    /// nothing to fall back to either: that is an empty machine, not a
    /// substitution.
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
    pub fn of(host: &dyn AudioDevices, input: Option<&str>, output: Option<&str>) -> Self {
        Self {
            input: DeviceList::of(catalogue(host, Direction::Input), input),
            output: DeviceList::of(catalogue(host, Direction::Output), output),
        }
    }
}
