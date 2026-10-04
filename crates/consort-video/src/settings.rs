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
    /// The chosen camera by device node, or `None` for the first one found.
    ///
    /// `None` rather than an empty string, which is an id that can never match.
    pub camera: Option<String>,
}
