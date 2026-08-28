// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the call thread has to say.

use consort_matrix::Participant;
use serde::{Deserialize, Serialize};

/// What a session is doing with its own audio.
///
/// Two switches over one state, because each is only meaningful next to the
/// other: deafening mutes, and undeafening must not unmute somebody who had
/// already muted themselves. Kept together so there is one answer rather than
/// two that can disagree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfAudio {
    /// Whether this session's microphone is off because somebody said so.
    pub muted: bool,
    /// Whether this session has stopped receiving everybody else's audio.
    pub deafened: bool,
    /// Whether this session has said it is away from the computer.
    ///
    /// The third state, and it is not a shade of either of the others. It
    /// mutes, because a live microphone in an empty room is the thing it
    /// exists to prevent, and it deliberately does not deafen: the whole
    /// point of walking away rather than leaving is that you can hear your
    /// name from the next room and come back.
    ///
    /// It is also the only one of the three whose entire value is that other
    /// people can see it. A muted person may be listening intently; an away
    /// person is not there, and everybody else can stop waiting for them to
    /// answer.
    ///
    /// `#[serde(default)]` so a client that predates this reads a payload
    /// carrying it and still gets the other two right.
    #[serde(default)]
    pub away: bool,
}

impl SelfAudio {
    /// Whether the microphone is off, for any of the three reasons.
    ///
    /// Deafening mutes. It is what every client with both buttons does, and it
    /// is the only honest option: carrying on talking into a room you have
    /// stopped listening to is not a state anybody means to be in.
    ///
    /// Being away mutes for a plainer reason. Nobody is at the keyboard, so
    /// nothing said near it was said to the call.
    ///
    /// All three stay independent underneath. Pressing unmute while away or
    /// deafened sets `muted` and changes nothing audible, which is the same
    /// bargain the deafen button has always made: the state you asked for is
    /// recorded, and the stronger one still decides.
    pub fn microphone_off(self) -> bool {
        self.muted || self.deafened || self.away
    }
}

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
    /// What this session is doing with its own audio.
    ///
    /// Its own event rather than two more fields on [`Connected`], for two
    /// reasons. It is not part of any one call: mute and deafen survive a
    /// channel switch, because a person who muted themselves and then moved
    /// rooms has not asked to be heard again. And `Connected` carries a roster,
    /// which costs a member-store read per person to name, which is not a price
    /// worth paying every time somebody taps a button.
    ///
    /// Emitted whenever either changes, and only then. Both start false, here
    /// and in the interface, so a reader that has heard nothing is not a
    /// reader that knows nothing.
    ///
    /// [`Connected`]: Self::Connected
    SelfAudio(SelfAudio),
    /// Who is talking right now, by Matrix user ID.
    ///
    /// Its own event, and deliberately not part of [`Connected`]. The SFU
    /// revises this several times a second, and `Connected` carries a roster
    /// that costs a member-store read per person to name. Folding the two
    /// together would put a database read behind every syllable anybody says.
    ///
    /// Per person rather than per membership, to match the roster it is drawn
    /// against: somebody talking on one of their two devices is one person
    /// talking.
    ///
    /// The SFU decides who counts as speaking, from the RTP it is already
    /// receiving. That is deliberate: it is one answer for everybody in the
    /// call, arrived at the same way, rather than each client guessing from
    /// whatever it happens to be able to measure.
    ///
    /// [`Connected`]: Self::Connected
    Speaking { user_ids: Vec<String> },
    /// The join did not happen. The thread is still alive and can be asked
    /// again.
    Failed { room_id: String, error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_audio_puts_its_flags_beside_the_tag() {
        // A newtype variant under an internal tag flattens, which is what the
        // frontend reads: `{state, muted, deafened, away}` and no nesting.
        // Worth pinning, because it is the one shape here that comes from how
        // serde treats the variant rather than from how the enum is written.
        let json = serde_json::to_value(CallEvent::SelfAudio(SelfAudio {
            muted: true,
            deafened: false,
            away: true,
        }))
        .unwrap();

        assert_eq!(json["state"], "selfAudio");
        assert_eq!(json["muted"], true);
        assert_eq!(json["deafened"], false);
        assert_eq!(json["away"], true);
    }

    #[test]
    fn deafening_is_enough_to_have_the_microphone_off() {
        assert!(
            SelfAudio {
                deafened: true,
                ..SelfAudio::default()
            }
            .microphone_off()
        );
        assert!(!SelfAudio::default().microphone_off());
    }

    #[test]
    fn being_away_is_enough_to_have_the_microphone_off() {
        // The half of away that is not just an icon. Somebody who is not at
        // the keyboard is not talking to the call, whatever the room they left
        // it in sounds like.
        assert!(
            SelfAudio {
                away: true,
                ..SelfAudio::default()
            }
            .microphone_off()
        );
    }

    #[test]
    fn away_does_not_imply_deafened() {
        // The entire difference from deafen. Walking away and still hearing
        // your name is the reason to press this rather than leave.
        let away = SelfAudio {
            away: true,
            ..SelfAudio::default()
        };

        assert!(away.microphone_off());
        assert!(!away.deafened);
    }

    #[test]
    fn the_three_states_stay_independent() {
        // `microphone_off` collapses them for the one question it answers, and
        // nothing else may. An interface drawing an away icon has to be able
        // to tell an away person from a muted one.
        let all = SelfAudio {
            muted: true,
            deafened: true,
            away: true,
        };
        let json = serde_json::to_value(all).unwrap();

        assert_eq!(json["muted"], true);
        assert_eq!(json["deafened"], true);
        assert_eq!(json["away"], true);
    }

    #[test]
    fn a_payload_from_a_client_that_predates_away_still_reads() {
        // Two Consort builds in one call. The older one sends two fields and
        // this must not refuse the whole message over the third.
        let old: SelfAudio =
            serde_json::from_str(r#"{"muted":true,"deafened":false}"#).unwrap();

        assert!(old.muted);
        assert!(!old.away);
    }

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
            tag(&CallEvent::SelfAudio(SelfAudio::default())),
            "selfAudio"
        );
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
                participants: vec![Participant::named("@bob:example.org", "Bob")],
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
