// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What this machine remembers about its camera.

use serde::{Deserialize, Serialize};

/// The camera half of the settings file.
///
/// A section of its own rather than a field on the audio one, because a camera
/// is not a sound card and the settings screen writes each section whole. See
/// `set_video_settings_for` in the Tauri crate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoSettings {
    /// The chosen camera by [`crate::Camera::id`], or `None` for the first one
    /// found.
    ///
    /// `None` rather than an empty string, which is an id that can never match.
    pub camera: Option<String>,
    /// Whether to send what this machine is playing along with a shared
    /// screen. Off by default: on the one platform that can capture it the
    /// capture is the whole output, not one application.
    pub share_sound: bool,
}
