// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a failed microphone says.
//!
//! Not decoration. Opening a device on a real Linux desktop fails often: ALSA
//! reports devices that PipeWire is holding, `dsnoop` refuses when the hardware
//! is busy, and a saved device can simply be gone. These strings are the whole
//! of what somebody gets told when that happens, so they are tested like any
//! other output.

use consort_audio::capture::CaptureError;

#[test]
fn a_machine_with_no_microphone_says_so_plainly() {
    let error = CaptureError::NoDevice;

    assert_eq!(error.to_string(), "there is no audio input device");
}

#[test]
fn a_missing_device_names_it_and_offers_what_there_is() {
    let error = CaptureError::UnknownDevice {
        requested: "Yeti".to_owned(),
        available: vec!["Built-in".to_owned(), "Webcam".to_owned()],
    };

    let message = error.to_string();

    assert!(message.contains("\"Yeti\""), "got {message}");
    assert!(
        message.contains("Built-in, Webcam"),
        "somebody whose device is gone needs to see what is there instead: {message}"
    );
}

#[test]
fn a_missing_device_on_an_empty_machine_does_not_trail_off() {
    let error = CaptureError::UnknownDevice {
        requested: "Yeti".to_owned(),
        available: Vec::new(),
    };

    let message = error.to_string();

    assert_eq!(message, "no input device called \"Yeti\"");
    assert!(
        !message.ends_with(' '),
        "an empty list should end the sentence, not dangle: {message:?}"
    );
}

#[test]
fn a_device_that_cannot_do_forty_eight_kilohertz_says_which_and_why() {
    // The one refusal a person is most likely to think is a bug. Nothing here
    // resamples, so this is a decision rather than a failure, and the message
    // has to carry that.
    let error = CaptureError::NoFortyEightKilohertz {
        device: "Ancient USB Headset".to_owned(),
    };

    let message = error.to_string();

    assert!(message.contains("\"Ancient USB Headset\""), "got {message}");
    assert!(message.contains("48 kHz"), "got {message}");
    assert!(
        message.contains("resample"),
        "the reason matters as much as the fact: {message}"
    );
}

#[test]
fn an_unhandled_sample_format_names_the_format() {
    let error = CaptureError::UnsupportedFormat {
        device: "Odd Interface".to_owned(),
        format: "U8".to_owned(),
    };

    let message = error.to_string();

    assert!(message.contains("\"Odd Interface\""), "got {message}");
    assert!(message.contains("U8"), "got {message}");
}

#[test]
fn a_backend_failure_carries_what_the_backend_said() {
    // The most common one in practice, and the least predictable, so passing
    // the backend's own words through beats paraphrasing them.
    let error = CaptureError::Backend(
        "The requested device is temporarily busy. Another application or \
         stream may be using it."
            .to_owned(),
    );

    let message = error.to_string();

    assert!(message.starts_with("the audio backend failed: "), "got {message}");
    assert!(message.contains("temporarily busy"), "got {message}");
}

#[test]
fn every_failure_is_a_real_error_type() {
    // So a caller can put one in a `Box<dyn Error>` or ask it for a source
    // rather than having to match on it to log it.
    fn assert_error<E: std::error::Error>(_: &E) {}

    assert_error(&CaptureError::NoDevice);
}
