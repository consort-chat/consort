// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The shape [`AudioEvent`] arrives in on the other side of the IPC.
//!
//! Asserted here rather than left to whatever serde does by default, because
//! nothing on the TypeScript side would fail to build if it changed. A rename
//! is a `switch` that stops matching and a level bar that never moves, which
//! is the sort of thing that gets debugged for an hour.

use consort_audio::{AudioEvent, Reading};

fn json(event: &AudioEvent) -> serde_json::Value {
    serde_json::to_value(event).expect("an audio event has to survive the trip")
}

#[test]
fn every_variant_is_tagged_by_a_state_field() {
    // Internally tagged, matching every other union that crosses this
    // boundary: `Connection`, `Verification`, `KeyBackup`. One convention, so
    // a reader of `api.ts` learns it once.
    let events = [
        AudioEvent::Started {
            device: "Yeti".to_owned(),
        },
        AudioEvent::Stopped,
        AudioEvent::Failed {
            error: "no".to_owned(),
        },
        AudioEvent::Level(Reading {
            level: 0.5,
            probability: 0.9,
            open: true,
        }),
    ];

    for event in events {
        let payload = json(&event);
        assert!(
            payload
                .get("state")
                .and_then(serde_json::Value::as_str)
                .is_some(),
            "{payload} carries no state tag"
        );
    }
}

#[test]
fn the_states_are_named_the_way_the_frontend_reads_them() {
    assert_eq!(
        json(&AudioEvent::Started {
            device: "Yeti".to_owned()
        })["state"],
        "started"
    );
    assert_eq!(json(&AudioEvent::Stopped)["state"], "stopped");
    assert_eq!(
        json(&AudioEvent::Failed {
            error: "no".to_owned()
        })["state"],
        "failed"
    );
    assert_eq!(
        json(&AudioEvent::Level(Reading {
            level: 0.0,
            probability: 0.0,
            open: false,
        }))["state"],
        "level"
    );
}

#[test]
fn a_started_event_names_the_device_that_actually_opened() {
    // Which is not always the one asked for, and is the only place the
    // interface can find that out.
    let payload = json(&AudioEvent::Started {
        device: "Yeti Stereo Microphone".to_owned(),
    });

    assert_eq!(payload["device"], "Yeti Stereo Microphone");
}

#[test]
fn a_failure_carries_the_backends_own_words() {
    // The backend's sentence, not a category. "The requested audio device is
    // not available" and "cannot run at 48 kHz" send somebody to two different
    // places, and flattening them to "could not start" sends them to neither.
    let payload = json(&AudioEvent::Failed {
        error: "the audio backend failed: device busy".to_owned(),
    });

    assert_eq!(payload["error"], "the audio backend failed: device busy");
}

#[test]
fn a_reading_arrives_flat_rather_than_wrapped() {
    // The variant is a newtype, so there is a real risk of
    // `{"state":"level","0":{...}}` or a nested `level` object. Either one is
    // a bar that never moves.
    let payload = json(&AudioEvent::Level(Reading {
        level: 0.25,
        probability: 0.75,
        open: true,
    }));

    assert_eq!(payload["level"], 0.25);
    assert_eq!(payload["probability"], 0.75);
    assert_eq!(payload["open"], true);
}
