// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the call thread has to say.

use serde::{Deserialize, Serialize};

/// One thing that happened to this session's call.
///
/// Serialised internally tagged, matching every other union that crosses the
/// IPC boundary. The wire shape is pinned by the tests below, because nothing
/// in TypeScript would fail to build if it drifted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CallEvent {
    /// Working on it. Emitted before anything that can take a while, so the
    /// interface has something to show between the click and the call.
    Connecting { room_id: String },
    /// In the call.
    Connected { room_id: String },
    /// Not in a call, and nothing went wrong. Both a completed leave and the
    /// idle state at startup.
    Disconnected,
    /// The join did not happen. The thread is still alive and can be asked
    /// again.
    Failed { room_id: String, error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(event: &CallEvent) -> String {
        serde_json::to_value(event).unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn each_event_is_tagged_the_way_the_frontend_reads_it() {
        let room_id = || "!general:example.org".to_owned();

        assert_eq!(
            tag(&CallEvent::Connecting { room_id: room_id() }),
            "connecting"
        );
        assert_eq!(
            tag(&CallEvent::Connected { room_id: room_id() }),
            "connected"
        );
        assert_eq!(tag(&CallEvent::Disconnected), "disconnected");
        assert_eq!(
            tag(&CallEvent::Failed {
                room_id: room_id(),
                error: "no".to_owned(),
            }),
            "failed"
        );
    }

    #[test]
    fn every_event_survives_a_round_trip() {
        let events = [
            CallEvent::Connecting {
                room_id: "!a:example.org".to_owned(),
            },
            CallEvent::Connected {
                room_id: "!a:example.org".to_owned(),
            },
            CallEvent::Disconnected,
            CallEvent::Failed {
                room_id: "!a:example.org".to_owned(),
                error: "the homeserver said no".to_owned(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let back: CallEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(back, event, "{json} did not come back the same");
        }
    }

    #[test]
    fn a_failure_carries_which_room_it_was() {
        // Somebody can click a second channel while the first is still
        // connecting. Without the room id on the failure, the interface
        // cannot tell whether the message it just received is about the
        // channel it is currently showing.
        let event = CallEvent::Failed {
            room_id: "!general:example.org".to_owned(),
            error: "no transport".to_owned(),
        };

        let json = serde_json::to_value(&event).unwrap();

        assert_eq!(json["roomId"], "!general:example.org");
        assert_eq!(json["error"], "no transport");
    }
}
