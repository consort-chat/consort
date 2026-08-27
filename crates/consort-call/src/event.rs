// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the call thread has to say.

use consort_matrix::Participant;
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
    /// In the call, and here is who else is.
    ///
    /// Emitted again, unchanged apart from the roster, every time somebody
    /// joins or leaves. Being in a call is a state and the people in it are
    /// part of that state, so this is one message rather than two: a reader
    /// that keeps the latest thing said on this channel is a reader that has
    /// both, and one that missed a roster change has not also lost track of
    /// whether it is in a call.
    ///
    /// The roster comes from MatrixRTC signalling rather than from room state,
    /// which is what makes it right in every dialect. It is per person: a
    /// membership is per device, and somebody on a laptop and a phone appears
    /// once.
    Connected {
        room_id: String,
        participants: Vec<Participant>,
        /// Why this call cannot be heard, if it cannot.
        ///
        /// One sentence written for a person, already, or `None` when there is
        /// nothing wrong. Here rather than on a channel of its own for the
        /// same reason the roster is: it is part of what being in this call
        /// currently means, and a reader that has the call has this too.
        ///
        /// The failure it exists for is the one phase 0 reproduced: a call
        /// where every membership publishes, both rosters are right and RTP
        /// flows, and neither side can decrypt a word. Everything an interface
        /// normally draws says that call is working.
        trouble: Option<String>,
    },
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
            tag(&CallEvent::Connected {
                room_id: room_id(),
                participants: Vec::new(),
                trouble: None,
            }),
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
                participants: vec![Participant {
                    id: "@bob:example.org".to_owned(),
                    name: "Bob".to_owned(),
                }],
                trouble: Some("nobody can hear you".to_owned()),
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
