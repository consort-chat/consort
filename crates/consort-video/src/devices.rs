// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What cameras are plugged in, and which of them to use.
//!
//! The shape of `consort_audio::devices` with a simpler rule, because V4L2
//! gives a camera a stable id and no system default: the twin problem and the
//! live-default preference in `docs/adr/0004-trust-no-device-list.md` both
//! fall away, so a saved id is opened by id and the fallback is the first.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::capture::{CameraStream, CaptureError, FrameSink, Resolution, VideoCapture};

/// One camera this machine offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    /// The device node, and the identity a saved choice holds.
    pub id: String,
    /// What the driver calls it, for somebody to read.
    pub name: String,
}

/// Somewhere to ask what cameras are plugged in.
pub trait CameraDevices: Send + Sync + 'static {
    /// Every camera that can actually be opened and read, in device order.
    ///
    /// Readable, not merely present: a laptop's `/dev/video1` is routinely a
    /// metadata node with no capture formats on it at all. May repeat itself
    /// and may be empty, which [`catalogue`] tidies.
    fn enumerate(&self) -> Vec<Camera>;
}

/// What a saved choice resolved to.
///
/// Four cases rather than an `Option`, because the settings screen draws each
/// one differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The saved camera, and it is plugged in.
    Saved(Camera),
    /// Nothing was saved, so this is the first one listed.
    First(Camera),
    /// Something was saved, it is gone, and this is being used instead. The
    /// screen has to say so: somebody who chose an external camera and is
    /// being filmed by a laptop lid deserves to be told before a call, not
    /// during one.
    Substituted { wanted: String, using: Camera },
    /// There is nothing to choose from.
    Nothing,
}

impl Selection {
    /// The camera that will actually be opened, if there is one.
    pub fn camera(&self) -> Option<&Camera> {
        match self {
            Self::Saved(camera) | Self::First(camera) => Some(camera),
            Self::Substituted { using, .. } => Some(using),
            Self::Nothing => None,
        }
    }
}

/// The camera list worth showing: deduplicated by id, unnamed entries dropped,
/// device order kept.
pub fn catalogue(devices: &dyn CameraDevices) -> Vec<Camera> {
    let mut listed: Vec<Camera> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for camera in devices.enumerate() {
        if camera.id.trim().is_empty() || camera.name.trim().is_empty() {
            continue;
        }
        if seen.insert(camera.id.clone()) {
            listed.push(camera);
        }
    }

    listed
}

/// Match a saved camera id against what is plugged in.
pub fn choose(available: &[Camera], saved: Option<&str>) -> Selection {
    let Some(fallback) = available.first() else {
        return Selection::Nothing;
    };

    // A blank id is a settings file somebody edited by hand, not a camera that
    // has been unplugged.
    let wanted = saved.map(str::trim).filter(|id| !id.is_empty());
    let Some(wanted) = wanted else {
        return Selection::First(fallback.clone());
    };

    match available.iter().find(|camera| camera.id == wanted) {
        Some(camera) => Selection::Saved(camera.clone()),
        None => Selection::Substituted {
            wanted: wanted.to_owned(),
            using: fallback.clone(),
        },
    }
}

/// What the settings screen is told about cameras.
///
/// Three facts, because a picker needs three: what there is, which one is in
/// use, and whether that is the one that was asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraList {
    /// Everything worth offering, in device order.
    pub cameras: Vec<Camera>,
    /// The camera that will be opened, by id. `None` only when there are none.
    pub selected: Option<String>,
    /// The saved camera, when it is not here any more.
    pub missing: Option<String>,
}

impl CameraList {
    /// Resolve `saved` against `cameras` and describe the outcome.
    pub fn of(cameras: Vec<Camera>, saved: Option<&str>) -> Self {
        let selection = choose(&cameras, saved);
        Self {
            selected: selection.camera().map(|camera| camera.id.clone()),
            missing: match &selection {
                Selection::Substituted { wanted, .. } => Some(wanted.clone()),
                Selection::Saved(_) | Selection::First(_) | Selection::Nothing => None,
            },
            cameras,
        }
    }
}

/// The backend for a platform with no camera support compiled in.
///
/// Exists because the Windows release build has to compile and a settings
/// screen there should draw an empty picker rather than refuse to open. See
/// [`crate::Host`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCameras;

impl CameraDevices for NoCameras {
    fn enumerate(&self) -> Vec<Camera> {
        Vec::new()
    }
}

impl VideoCapture for NoCameras {
    fn open(
        &self,
        _camera: Option<&str>,
        _want: Resolution,
        _on_frame: FrameSink,
    ) -> Result<Box<dyn CameraStream>, CaptureError> {
        Err(CaptureError::NoCamera)
    }
}
