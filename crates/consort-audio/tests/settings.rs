// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What gets written down, and what happens when it is read back by a version
//! that did not write it.
//!
//! A settings file outlives the build that made it. It gets read by an older
//! Consort after a downgrade, by a newer one after an upgrade, and by whatever
//! a person turned it into with a text editor. None of those should stop the
//! application from starting.

use std::collections::BTreeMap;

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
            voice_activity: false,
        },
        call_sounds: false,
        call_voices: false,
        output_volume: 80,
        notification_volume: 35,
        person_volumes: BTreeMap::from([("@ada:example.org".to_owned(), 55)]),
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
fn the_defaults_are_the_pair_of_sounds_a_fresh_install_should_make() {
    // Both halves of one decision, and the reason this is a test rather than a
    // comment: they are two independent booleans one refactor away from being
    // set the obvious way round. The sentence is the notification; the chime in
    // front of it is optional and off, because two announcements of one arrival
    // is how somebody ends up turning both of them off.
    let settings = AudioSettings::default();

    assert!(!settings.call_sounds, "the chime is the optional half");
    assert!(settings.call_voices, "the sentence is the notification");
    assert_eq!(
        settings.output_volume, 100,
        "somebody who has asked for nothing should get what the sound card was handed"
    );
    assert!(
        settings.notification_volume < settings.output_volume,
        "a notification is mastered to be heard on its own and a call is not, so \
         playing them at one level puts the notification on top"
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
    assert!(json.contains("\"voiceActivity\""), "got {json}");
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

#[test]
fn voice_activity_is_on_unless_somebody_turned_it_off() {
    // The default has to be the gate, not the absence of one. Somebody who
    // never opens this screen should not be transmitting their keyboard to
    // everybody in the room.
    let settings: AudioSettings = serde_json::from_str("{}").expect("deserialise");

    assert!(settings.gate.voice_activity);
}

#[test]
fn a_file_written_before_the_toggle_existed_keeps_the_gate() {
    // Every settings file written so far. Reading one as "voice activity off"
    // would turn the gate off for everybody who upgrades.
    let json = r#"{"gate":{"openAt":0.8,"closeAt":0.3,"attackFrames":2,"holdMs":300}}"#;

    let settings: AudioSettings = serde_json::from_str(json).expect("deserialise");

    assert!(settings.gate.voice_activity);
    assert_eq!(settings.gate.open_at, 0.8);
}

#[test]
fn the_chime_is_off_for_somebody_who_never_chose() {
    // It was on, and the sentence turning up is what changed the answer: the
    // chime's job was to get somebody's attention for a notification that used
    // to be nothing but the chime. Two announcements of one arrival is not
    // twice the information, it is a doorbell in front of somebody already
    // talking.
    let settings: AudioSettings = serde_json::from_str("{}").expect("deserialise");

    assert!(!settings.call_sounds);
}

#[test]
fn a_settings_file_written_before_call_sounds_existed_gets_the_current_default() {
    // The upgrade case. A file from an older build has no such key, and the
    // container-level `default` fills it in from `Default` rather than from
    // the zero value. That the two now agree is a coincidence of this
    // particular default and not the mechanism, which is what the two volume
    // assertions are here to keep honest.
    let older = r#"{"input":"Yeti Stereo Microphone","output":null}"#;

    let settings: AudioSettings = serde_json::from_str(older).expect("deserialise");

    assert_eq!(settings.input.as_deref(), Some("Yeti Stereo Microphone"));
    assert!(!settings.call_sounds);
    assert_eq!(
        settings.output_volume,
        AudioSettings::default().output_volume
    );
    assert_eq!(
        settings.notification_volume,
        AudioSettings::default().notification_volume,
    );
}

#[test]
fn a_volume_of_zero_is_a_choice_and_not_a_missing_field() {
    // The trap that comes with defaulting a numeric field. Somebody who drags
    // a slider to nothing has said something, and a reader that treated the
    // zero as absent would hand them full volume on the next launch and look
    // like the setting does not work.
    let json = r#"{"outputVolume":0,"notificationVolume":0}"#;

    let settings: AudioSettings = serde_json::from_str(json).expect("deserialise");

    assert_eq!(settings.output_volume, 0);
    assert_eq!(settings.notification_volume, 0);
}

#[test]
fn a_persons_own_level_is_kept_by_user_id() {
    // Per person and per machine, because there is nowhere else it could live:
    // no account data says "this one is too loud in my headphones", and it is a
    // fact about the room somebody is sitting in rather than about the account.
    let json = r#"{"personVolumes":{"@ada:example.org":55}}"#;

    let settings: AudioSettings = serde_json::from_str(json).expect("deserialise");

    assert_eq!(settings.person_volumes.get("@ada:example.org"), Some(&55));
    assert_eq!(
        settings.person_volumes.get("@grace:example.org"),
        None,
        "somebody nobody has adjusted should have no entry rather than a written-down 100"
    );
}

#[test]
fn spoken_notifications_are_on_for_somebody_who_never_chose() {
    // The second field whose default is not the zero value, and it has to be
    // written out for the same reason as the first: nobody switches on a
    // notification they have never heard.
    let settings: AudioSettings = serde_json::from_str("{}").expect("deserialise");

    assert!(settings.call_voices);
}

#[test]
fn a_settings_file_written_before_the_voices_existed_still_has_them_on() {
    // The upgrade case, and the one that matters most here: everybody running
    // Consort today has a settings file that predates this field.
    let older = r#"{"input":null,"output":null,"callSounds":false}"#;

    let settings: AudioSettings = serde_json::from_str(older).expect("deserialise");

    assert!(!settings.call_sounds, "the older choice was not honoured");
    assert!(settings.call_voices);
}

#[test]
fn the_chimes_and_the_voices_switch_independently() {
    // The whole reason there are two. Either one off and the other on has to
    // be a state the file can hold, or the second setting is decoration.
    let chimes_only = r#"{"callSounds":true,"callVoices":false}"#;
    let voices_only = r#"{"callSounds":false,"callVoices":true}"#;

    let chimes: AudioSettings = serde_json::from_str(chimes_only).expect("deserialise");
    let voices: AudioSettings = serde_json::from_str(voices_only).expect("deserialise");

    assert!(chimes.call_sounds && !chimes.call_voices);
    assert!(!voices.call_sounds && voices.call_voices);
}

#[test]
fn turning_the_voices_off_survives_the_round_trip() {
    let off = AudioSettings {
        call_voices: false,
        ..AudioSettings::default()
    };

    let json = serde_json::to_string(&off).expect("serialise");
    let read_back: AudioSettings = serde_json::from_str(&json).expect("deserialise");

    assert!(!read_back.call_voices);
    assert_eq!(
        read_back.call_sounds,
        AudioSettings::default().call_sounds,
        "the chimes moved when only the voices were touched"
    );
}

#[test]
fn turning_the_chime_on_survives_the_round_trip() {
    // The setting exists to be moved, and a preference that does not persist is
    // worse than no preference at all: it looks like it worked and reverts on
    // the next launch. Written as the non-default direction on purpose, so a
    // serialiser that dropped the field entirely would fail here.
    let on = AudioSettings {
        call_sounds: true,
        ..AudioSettings::default()
    };

    let json = serde_json::to_string(&on).expect("serialise");
    let read_back: AudioSettings = serde_json::from_str(&json).expect("deserialise");

    assert!(read_back.call_sounds);
}
