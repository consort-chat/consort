// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What gets written down, and what happens when it is read back by a version
//! that did not write it.
//!
//! A settings file outlives the build that made it. It gets read by an older
//! Consort after a downgrade, by a newer one after an upgrade, and by whatever
//! a person turned it into with a text editor. None of those should stop the
//! application from starting.

use consort_audio::{AudioSettings, GateConfig};

#[test]
fn settings_round_trip_through_json_unchanged() {
    let settings = AudioSettings {
        input: Some("Yeti Stereo Microphone".to_owned()),
        output: Some("HD-Audio Generic".to_owned()),
        gate: GateConfig {
            open_at: 0.71,
            close_at: 0.22,
            attack_frames: 4,
            hold_ms: 450,
            denoise: false,
        },
    };

    let json = serde_json::to_string(&settings).expect("serialise");
    let read_back: AudioSettings = serde_json::from_str(&json).expect("deserialise");

    assert_eq!(read_back, settings);
}

#[test]
fn an_empty_object_is_every_default() {
    let settings: AudioSettings = serde_json::from_str("{}").expect("deserialise");

    assert_eq!(settings, AudioSettings::default());
    assert_eq!(
        settings.gate,
        GateConfig::default(),
        "a file written before the gate was configurable must still start the \
         gate, rather than starting it with every threshold at zero"
    );
}

#[test]
fn a_field_this_version_does_not_know_about_is_ignored() {
    // A newer Consort wrote this, then somebody downgraded. Refusing to start
    // would be a worse answer than ignoring the part we cannot use.
    let json = r#"{"input":"Yeti","noiseSuppression":"rnnoise-v2","videoDevice":"C920"}"#;

    let settings: AudioSettings = serde_json::from_str(json).expect("deserialise");

    assert_eq!(settings.input.as_deref(), Some("Yeti"));
}

#[test]
fn a_partly_written_gate_keeps_the_defaults_for_the_rest() {
    let json = r#"{"gate":{"openAt":0.8}}"#;

    let settings: AudioSettings = serde_json::from_str(json).expect("deserialise");

    assert_eq!(settings.gate.open_at, 0.8);
    assert_eq!(
        settings.gate.hold_ms,
        GateConfig::default().hold_ms,
        "one tuned threshold should not silently reset the others"
    );
}

#[test]
fn the_json_is_camel_case_because_the_frontend_reads_it() {
    let json = serde_json::to_string(&AudioSettings::default()).expect("serialise");

    assert!(json.contains("\"openAt\""), "got {json}");
    assert!(json.contains("\"attackFrames\""), "got {json}");
    assert!(json.contains("\"holdMs\""), "got {json}");
    assert!(
        !json.contains("_"),
        "no snake_case should reach the wire: {json}"
    );
}

#[test]
fn no_device_chosen_is_the_default_rather_than_an_empty_name() {
    let settings = AudioSettings::default();

    assert_eq!(settings.input, None);
    assert_eq!(
        settings.output, None,
        "None means \"ask the host\", and an empty string would be a device \
         name that can never match"
    );
}

#[test]
fn a_null_device_reads_back_as_no_choice() {
    let settings: AudioSettings =
        serde_json::from_str(r#"{"input":null,"output":null}"#).expect("deserialise");

    assert_eq!(settings, AudioSettings::default());
}
