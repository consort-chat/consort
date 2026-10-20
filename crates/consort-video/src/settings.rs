// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What this machine remembers about its camera.

use serde::{Deserialize, Serialize};

use crate::capture::{Resolution, WANTED};

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
    /// How much camera this machine sends.
    pub sending: Sending,
}

/// How much camera this machine sends.
///
/// A size to open the device at, not a layer to publish from: a receiver can
/// only choose among rungs the publisher built, so the only sender-side
/// control there is moves the whole ladder. Why this is the camera alone, and
/// why a share cannot have it:
/// `docs/adr/0017-a-sender-may-send-less-camera.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sending {
    /// As much as the camera offers, which is the absence of a choice.
    #[default]
    Auto,
    Medium,
    Low,
}

impl Sending {
    /// What to ask the camera for.
    ///
    /// The frame rate is asked for untouched at every setting, and 960 and 320
    /// are the two sizes below [`WANTED`] that livekit builds a usable ladder
    /// from: 640 is its fixed middle camera preset, so a camera opened there
    /// publishes that one rung twice.
    pub fn camera(self) -> Resolution {
        match self {
            Self::Auto => WANTED,
            Self::Medium => Resolution {
                width: 960,
                height: 540,
                fps: WANTED.fps,
            },
            Self::Low => Resolution {
                width: 320,
                height: 180,
                fps: WANTED.fps,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_capped_until_somebody_chooses_less() {
        // The absence of a choice has to leave the capture exactly as it was,
        // or every session that never opens settings sends less than it did.
        assert_eq!(Sending::default(), Sending::Auto);
        assert_eq!(Sending::Auto.camera(), WANTED);
    }

    #[test]
    fn a_lower_setting_asks_the_camera_for_fewer_pixels() {
        assert_eq!(
            Sending::Medium.camera(),
            Resolution {
                width: 960,
                height: 540,
                fps: WANTED.fps,
            }
        );
        assert_eq!(
            Sending::Low.camera(),
            Resolution {
                width: 320,
                height: 180,
                fps: WANTED.fps,
            }
        );
    }

    #[test]
    fn every_setting_asks_for_the_same_frame_rate() {
        // The whole difference from a rung swap, which is what #195 reported
        // as choppiness: a rung below the best drops frames.
        for sending in [Sending::Auto, Sending::Medium, Sending::Low] {
            assert_eq!(sending.camera().fps, WANTED.fps, "{sending:?}");
        }
    }

    #[test]
    fn no_setting_opens_the_camera_at_livekits_middle_preset() {
        // 640 by 360 is `video::DEFAULT_SIMULCAST_PRESETS`' upper preset, so a
        // camera opened there publishes two rungs of the same size and throws
        // the useful small one away. Pinned in
        // `consort-call/tests/the_ladder_a_declared_size_builds.rs`.
        for sending in [Sending::Auto, Sending::Medium, Sending::Low] {
            let size = sending.camera();
            assert_ne!((size.width, size.height), (640, 360), "{sending:?}");
        }
    }

    #[test]
    fn a_saved_section_without_a_choice_reads_back_as_no_choice() {
        // Every settings file written before this field existed.
        let settings: VideoSettings =
            serde_json::from_str(r#"{"camera":"/dev/video2"}"#).expect("parse");

        assert_eq!(settings.sending, Sending::Auto);
    }
}
