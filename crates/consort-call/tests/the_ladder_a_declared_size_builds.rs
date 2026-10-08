// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What livekit publishes for a given declared capture size.
//!
//! The one sender-side lever this client has is the size it opens a capture
//! at (#196), and livekit derives both the rungs and their frame-rate caps
//! from it. These pin the derivation that
//! `docs/adr/0017-a-sender-may-send-less-camera.md` reasons from, so a version
//! bump that moves the preset tables fails here rather than in a call.

use livekit::options::{TrackPublishOptions, compute_video_encodings};
use livekit::track::TrackSource;

/// Each rung as the pair that decides what peers get: how far the frame is
/// scaled down, and the frames a second it is capped at.
///
/// The `simulcast` condition is matrix-rtc-livekit's, which refuses the
/// degenerate single-encoding shape below 480 on the long edge.
fn rungs(width: u32, height: u32, source: TrackSource) -> Vec<(f64, f64)> {
    let options = TrackPublishOptions {
        source,
        simulcast: u32::max(width, height) >= 480,
        ..Default::default()
    };

    compute_video_encodings(width, height, &options)
        .iter()
        .map(|rung| {
            (
                rung.scale_resolution_down_by.unwrap_or(1.0),
                rung.max_framerate.unwrap_or(0.0),
            )
        })
        .collect()
}

fn camera(width: u32, height: u32) -> Vec<(f64, f64)> {
    rungs(width, height, TrackSource::Camera)
}

fn screen(width: u32, height: u32) -> Vec<(f64, f64)> {
    rungs(width, height, TrackSource::Screenshare)
}

#[test]
fn a_camera_keeps_its_frame_rate_when_it_is_opened_smaller() {
    // Which is what makes a send-side size the whole feature: 960 by 540 is
    // half the pixels of 720p and still 30 frames a second at the top.
    assert_eq!(camera(1280, 720).first(), Some(&(1.0, 30.0)));
    assert_eq!(camera(960, 540).first(), Some(&(1.0, 30.0)));
}

#[test]
fn a_camera_opened_at_livekits_middle_preset_publishes_one_rung_twice() {
    // 640 by 360 is `video::DEFAULT_SIMULCAST_PRESETS`' upper preset, so both
    // encodings land on it and the 320 by 180 rung is discarded. This is why
    // `Sending` has no 640.
    assert_eq!(camera(640, 360), vec![(1.0, 25.0), (1.0, 20.0)]);
}

#[test]
fn a_camera_small_enough_publishes_a_single_rung() {
    assert_eq!(camera(320, 180), vec![(1.0, 15.0)]);
}

#[test]
fn a_share_opened_smaller_loses_its_frame_rate() {
    // The reason `Sending` leaves the screen alone. livekit's screenshare
    // presets tie the cap to the size, so capping the capture of a share is
    // the choppiness #195 reported, arriving by a different route.
    assert_eq!(screen(1920, 1080).first(), Some(&(1.0, 30.0)));
    assert_eq!(screen(1280, 720).first(), Some(&(1.0, 15.0)));
    assert_eq!(screen(960, 540).first(), Some(&(1.0, 5.0)));
    assert_eq!(screen(320, 180), vec![(1.0, 3.0)]);
}

#[test]
fn a_share_publishes_its_second_rung_at_three_frames_a_second() {
    // ADR-0016's finding, pinned where it can be read rather than reasoned
    // about: this is the rung a receive-side cap picks.
    assert_eq!(screen(1920, 1080), vec![(1.0, 30.0), (2.0, 3.0)]);
}
