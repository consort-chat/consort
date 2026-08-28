// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What Consort clients tell each other that nothing else in the stack says.
//!
//! ## Why this exists
//!
//! Mute needs nothing here. It is a real thing to a LiveKit track, the SFU
//! broadcasts it, and every client including Element Call already draws it.
//!
//! Deafen is not. Nothing in MatrixRTC or LiveKit has a name for "I have
//! stopped listening", because it is built out of this session's own
//! subscription state and subscriptions are nobody else's business. So a
//! deafened person appears to everybody else as merely muted, which is true
//! but is not the interesting half.
//!
//! ## Why a data message and not an attribute
//!
//! LiveKit participant attributes are exactly the right shape for this: a
//! small key-value bag per participant, synced to everyone, cleaned up on
//! leave. They are also gated behind a token grant, `canUpdateOwnMetadata`,
//! and every released `lk-jwt-service` up to and including 0.4.4 leaves that
//! field unset. LiveKit defaults it to **false** when unset, unlike
//! `canPublishData`, which defaults to whatever `canPublish` is and is
//! therefore already granted. So attributes would be refused by the SFU on
//! every deployment that exists today, and data messages are accepted on all
//! of them.
//!
//! ## Why not a Matrix event
//!
//! A custom room event would work and needs nothing from the SFU, but this is
//! a state somebody toggles while they are talking, and every toggle would
//! become a permanent entry in the room's history, replicated to every server
//! in it, for a fact that stops being true when the call ends. A data message
//! is transient in exactly the way the state is.
//!
//! ## How a late arrival finds out
//!
//! It does not, and does not need to. A data message reaches whoever is
//! connected when it is sent, so somebody who joins afterwards has missed it.
//! What saves it is that joining *is* a roster change, and the call thread
//! re-pushes this session's own audio state on every roster change (for the
//! separate reason that deafening is per participant and a new arrival would
//! otherwise be audible). So everybody re-announces whenever anybody arrives,
//! and the newcomer is told by all of them without having to ask.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The LiveKit data topic this travels on.
///
/// Namespaced because it shares a room with Element Call, which sends its own
/// data messages, and neither client should have to parse the other's.
pub const TOPIC: &str = "consort.self_audio";

/// One client saying what it is doing with its own audio.
///
/// Carries the sender's `member_id` rather than relying on the LiveKit
/// participant identity it arrives with. The two are derived from each other,
/// but the derivation differs per MatrixRTC generation, and re-deriving it here
/// would mean this quietly stopping working in whichever dialect nobody
/// happened to test. Saying who you are is one field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// Format version, so a future field can be added without older clients
    /// mis-reading it. Anything else is ignored.
    pub v: u8,
    /// The `m.rtc.member` membership id of the sender.
    pub member_id: String,
    /// Whether they have stopped listening to the call.
    pub deafened: bool,
}

/// The version this build writes and the only one it reads.
pub const VERSION: u8 = 1;

impl Notice {
    /// What this session should be telling everybody right now.
    pub fn new(member_id: impl Into<String>, deafened: bool) -> Self {
        Self {
            v: VERSION,
            member_id: member_id.into(),
            deafened,
        }
    }

    /// The bytes to put on the wire.
    pub fn encode(&self) -> Vec<u8> {
        // Cannot fail: three owned fields of plain types. Encoded as an empty
        // payload if it somehow did, which the reader below discards.
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Read one off the wire, or `None` if it is not one of ours.
    ///
    /// Everything unrecognised is dropped silently rather than logged. This is
    /// a shared channel: another client's traffic arriving here is ordinary,
    /// not a fault, and a warning per packet would drown the log.
    pub fn decode(payload: &[u8]) -> Option<Self> {
        let notice: Self = serde_json::from_slice(payload).ok()?;
        (notice.v == VERSION).then_some(notice)
    }
}

/// Who in the call has deafened themselves.
///
/// Keyed by LiveKit participant identity rather than by `member_id`, because
/// leaving is reported in terms of the identity and nothing else, and a person
/// who disconnects without a parting word must not stay deafened forever.
#[derive(Default)]
pub struct Deafened(HashMap<String, Notice>);

impl Deafened {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record what `identity` just said. Says whether anything changed.
    ///
    /// The answer is what decides whether to redraw. Everybody re-announces on
    /// every roster change, so the overwhelming majority of these repeat what
    /// the last one said, and acting on all of them would redraw the roster
    /// once per participant per arrival.
    pub fn note(&mut self, identity: &str, notice: Notice) -> bool {
        if self.0.get(identity) == Some(&notice) {
            return false;
        }
        self.0.insert(identity.to_owned(), notice);
        true
    }

    /// Forget `identity`, who has left. Says whether anything changed.
    pub fn gone(&mut self, identity: &str) -> bool {
        self.0.remove(identity).is_some()
    }

    /// The memberships currently deafened.
    pub fn members(&self) -> Vec<String> {
        self.0
            .values()
            .filter(|notice| notice.deafened)
            .map(|notice| notice.member_id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut members: Vec<String>) -> Vec<String> {
        members.sort();
        members
    }

    #[test]
    fn a_notice_survives_the_round_trip() {
        let sent = Notice::new("ada-laptop", true);

        assert_eq!(Notice::decode(&sent.encode()), Some(sent));
    }

    #[test]
    fn the_wire_names_are_the_ones_other_clients_will_read() {
        // Pinned because nothing would fail to build if serde's renaming
        // changed underneath it, and the only symptom would be two Consort
        // versions silently not understanding each other.
        let json: serde_json::Value =
            serde_json::from_slice(&Notice::new("ada-laptop", true).encode()).unwrap();

        assert_eq!(json["v"], 1);
        assert_eq!(json["memberId"], "ada-laptop");
        assert_eq!(json["deafened"], true);
    }

    #[test]
    fn somebody_else_s_traffic_on_the_same_room_is_not_ours() {
        // Element Call sends its own data messages. Parsing one as a notice
        // would be worse than ignoring it.
        assert_eq!(Notice::decode(b"not json at all"), None);
        assert_eq!(Notice::decode(br#"{"something":"else"}"#), None);
        assert_eq!(Notice::decode(b""), None);
    }

    #[test]
    fn a_notice_from_a_version_this_build_does_not_know_is_ignored() {
        // The point of the field. A future Consort adding a meaning to an
        // existing key must not have this build act on the old meaning.
        let future = br#"{"v":99,"memberId":"ada-laptop","deafened":true}"#;

        assert_eq!(Notice::decode(future), None);
    }

    #[test]
    fn somebody_who_deafened_is_listed() {
        let mut deafened = Deafened::new();

        assert!(deafened.note("ada-identity", Notice::new("ada-laptop", true)));
        assert_eq!(deafened.members(), vec!["ada-laptop".to_owned()]);
    }

    #[test]
    fn somebody_who_is_merely_present_is_not_listed() {
        let mut deafened = Deafened::new();

        deafened.note("ada-identity", Notice::new("ada-laptop", false));

        assert!(deafened.members().is_empty());
    }

    #[test]
    fn undeafening_takes_them_off_the_list() {
        let mut deafened = Deafened::new();
        deafened.note("ada-identity", Notice::new("ada-laptop", true));

        assert!(deafened.note("ada-identity", Notice::new("ada-laptop", false)));
        assert!(deafened.members().is_empty());
    }

    #[test]
    fn hearing_the_same_thing_twice_is_not_a_change() {
        // Everybody re-announces on every roster change, so most notices
        // repeat. Redrawing the roster for each would mean one redraw per
        // participant every time anybody walked in.
        let mut deafened = Deafened::new();
        deafened.note("ada-identity", Notice::new("ada-laptop", true));

        assert!(!deafened.note("ada-identity", Notice::new("ada-laptop", true)));
    }

    #[test]
    fn leaving_without_a_parting_word_still_takes_them_off() {
        // A client that crashes or drops its connection says nothing on the
        // way out, and staying deafened forever would be a headphone icon
        // beside somebody who is not in the call.
        let mut deafened = Deafened::new();
        deafened.note("ada-identity", Notice::new("ada-laptop", true));

        assert!(deafened.gone("ada-identity"));
        assert!(deafened.members().is_empty());
    }

    #[test]
    fn somebody_leaving_who_was_never_heard_from_is_not_a_change() {
        let mut deafened = Deafened::new();

        assert!(!deafened.gone("a-stranger"));
    }

    #[test]
    fn two_people_deafened_are_both_listed() {
        let mut deafened = Deafened::new();
        deafened.note("ada-identity", Notice::new("ada-laptop", true));
        deafened.note("bob-identity", Notice::new("bob-phone", true));

        assert_eq!(
            sorted(deafened.members()),
            vec!["ada-laptop".to_owned(), "bob-phone".to_owned()]
        );
    }

    #[test]
    fn this_session_files_its_own_state_alongside_everybody_else_s() {
        // The local half of the same record. LiveKit does not deliver a data
        // message back to its publisher, so this session's own state is put in
        // by hand, and it has to land in the same map or the roster would draw
        // the headphones beside everybody except the person who pressed the
        // button.
        let mut deafened = Deafened::new();

        assert!(deafened.note("our-identity", Notice::new("our-laptop", true)));
        assert!(deafened.note("theirs", Notice::new("their-laptop", true)));

        assert_eq!(
            sorted(deafened.members()),
            vec!["our-laptop".to_owned(), "their-laptop".to_owned()]
        );
    }

    #[test]
    fn somebody_else_leaving_does_not_take_this_session_with_them() {
        // Our own entry is keyed by our own identity, which the SFU never
        // reports as disconnected. Sharing a key with anybody would undeafen us
        // the moment they hung up.
        let mut deafened = Deafened::new();
        deafened.note("our-identity", Notice::new("our-laptop", true));
        deafened.note("theirs", Notice::new("their-laptop", true));

        assert!(deafened.gone("theirs"));

        assert_eq!(deafened.members(), vec!["our-laptop".to_owned()]);
    }

    #[test]
    fn one_person_on_two_devices_is_tracked_per_device() {
        // Deafening a laptop says nothing about a phone that is also in the
        // call, and the roster folds the two together afterwards.
        let mut deafened = Deafened::new();
        deafened.note("ada-laptop-identity", Notice::new("ada-laptop", true));
        deafened.note("ada-phone-identity", Notice::new("ada-phone", false));

        assert_eq!(deafened.members(), vec!["ada-laptop".to_owned()]);
    }
}
