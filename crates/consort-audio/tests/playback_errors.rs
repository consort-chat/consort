// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a failed test tone says.
//!
//! Its own type rather than [`consort_audio::CaptureError`], which says
//! "input" in four of its five messages. Somebody who pressed a button to hear
//! their speakers and was told there is no audio input device would reasonably
//! conclude Consort is confused, and would be right.

use consort_audio::playback::PlaybackError;

#[test]
fn a_machine_with_nothing_to_play_through_says_so_plainly() {
    let error = PlaybackError::NoDevice;

    assert_eq!(error.to_string(), "there is no audio output device");
}

#[test]
fn a_missing_device_names_it_and_offers_what_there_is() {
    let error = PlaybackError::UnknownDevice {
        requested: "Headphones".to_owned(),
        available: vec!["Built-in".to_owned(), "HDMI".to_owned()],
    };

    let message = error.to_string();

    assert!(message.contains("\"Headphones\""), "got {message}");
    assert!(
        message.contains("Built-in, HDMI"),
        "somebody whose speakers are gone needs to see what is there: {message}"
    );
}

#[test]
fn a_missing_device_on_an_empty_machine_does_not_trail_off() {
    let error = PlaybackError::UnknownDevice {
        requested: "Headphones".to_owned(),
        available: Vec::new(),
    };

    let message = error.to_string();

    assert_eq!(message, "no output device called \"Headphones\"");
    assert!(!message.ends_with(' '), "got {message:?}");
}

#[test]
fn a_device_that_cannot_do_forty_eight_kilohertz_says_which_and_why() {
    // Not reachable from the picker, which only lists devices that already
    // said they can. Reachable between the list being drawn and the button
    // being pressed, which on a desktop with hot-plugged audio is a real gap.
    let error = PlaybackError::NoFortyEightKilohertz {
        device: "Ancient USB Headset".to_owned(),
    };

    let message = error.to_string();

    assert!(message.contains("\"Ancient USB Headset\""), "got {message}");
    assert!(message.contains("48 kHz"), "got {message}");
}

#[test]
fn an_unhandled_sample_format_names_the_format() {
    let error = PlaybackError::UnsupportedFormat {
        device: "Odd Interface".to_owned(),
        format: "U8".to_owned(),
    };

    let message = error.to_string();

    assert!(message.contains("\"Odd Interface\""), "got {message}");
    assert!(message.contains("U8"), "got {message}");
}

#[test]
fn a_backend_failure_carries_what_the_backend_said() {
    let error = PlaybackError::Backend("device or resource busy".to_owned());

    let message = error.to_string();

    assert!(
        message.starts_with("the audio backend failed: "),
        "got {message}"
    );
    assert!(message.contains("device or resource busy"), "got {message}");
}

#[test]
fn every_failure_is_a_real_error_type() {
    fn assert_error<E: std::error::Error>(_: &E) {}

    assert_error(&PlaybackError::NoDevice);
}
