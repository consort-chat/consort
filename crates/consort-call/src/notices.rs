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
//! Away is not either, and it is further from anything the stack knows about:
//! it is not a fact about audio at all, it is a fact about whether there is a
//! person in the chair. Being seen is the whole of its value, so the same
//! channel carries it.
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

use std::collections::{HashMap, HashSet};

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
    /// Whether they have said they are away from the computer.
    ///
    /// Added without touching [`VERSION`], which is what the version field was
    /// for: `Notice` does not deny unknown fields, so a build that predates
    /// this reads a notice carrying it and still gets `deafened` right, and
    /// `#[serde(default)]` means this build reads an older notice as not away.
    /// Bumping to `v: 2` would instead have made the two builds invisible to
    /// each other, which is the opposite of what a version field is for here.
    #[serde(default)]
    pub away: bool,
}

/// The version this build writes and the only one it reads.
pub const VERSION: u8 = 1;

impl Notice {
    /// What this session should be telling everybody right now.
    pub fn new(member_id: impl Into<String>, deafened: bool, away: bool) -> Self {
        Self {
            v: VERSION,
            member_id: member_id.into(),
            deafened,
            away,
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

/// The memberships each notice named, split by what it said.
///
/// Two lists rather than one map because this is what the roster needs: a
/// pass per flag, marking the people every one of whose memberships said it.
/// See [`crate::roster`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// Memberships that have stopped listening.
    pub deafened: Vec<String>,
    /// Memberships whose owner is not at the computer.
    pub away: Vec<String>,
}

/// What everybody in the call has last said about their own audio.
///
/// Named for the act rather than for one of the things announced, because it
/// tracks two and will track more: a type called `Deafened` that also knows who
/// is away is a type whose name has to be read past.
///
/// Keyed by LiveKit participant identity rather than by `member_id`, because
/// leaving is reported in terms of the identity and nothing else, and a person
/// who disconnects without a parting word must not stay deafened forever.
#[derive(Default)]
pub struct Announced(HashMap<String, Notice>);

impl Announced {
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

    /// The memberships currently deafened, and the ones currently away.
    ///
    /// Both in one pass, and separately from each other. A person can be both:
    /// deafening while away is what happens when somebody turns their
    /// headphones off on the way out, and collapsing the two would lose which
    /// icon to draw.
    ///
    /// A membership more than one participant has claimed is dropped from
    /// both. See [`Self::contested`].
    pub fn flags(&self) -> Flags {
        let contested = self.contested();
        let members = |wanted: fn(&Notice) -> bool| {
            self.0
                .values()
                .filter(|notice| wanted(notice))
                .filter(|notice| !contested.contains(notice.member_id.as_str()))
                .map(|notice| notice.member_id.clone())
                .collect()
        };

        Flags {
            deafened: members(|notice| notice.deafened),
            away: members(|notice| notice.away),
        }
    }

    /// Memberships that more than one participant says are theirs.
    ///
    /// A notice names its own sender, and nothing here can check that claim: a
    /// LiveKit `Participant` carries an identity and the membership is derived
    /// from it differently in each MatrixRTC generation, which is the reason
    /// the field exists at all. So anybody in the call can send a notice
    /// carrying somebody else's membership id and have the roster draw a
    /// headphone icon beside a person who is listening.
    ///
    /// What this uses instead is that everybody re-announces on every roster
    /// change. A forged claim about somebody who is in the call and running
    /// Consort therefore sits next to that person's own claim about
    /// themselves, and the two are visible here as one membership arriving
    /// from two identities. Neither is trusted over the other: the flag is
    /// dropped and the person is drawn as ordinary, which is the answer that
    /// is wrong in the least damaging direction.
    ///
    /// Two gaps this does not close, both wanting the sender's identity to be
    /// checked against the roster rather than inferred from a conflict.
    /// Somebody running Element Call sends no notice at all, so a claim about
    /// them meets no opposition. And this is only as good as the re-announce:
    /// a forgery is answered the moment the roster next changes, but not
    /// before. Closing them takes an `identity` on `matrix_rtc_media`'s
    /// `Participant`, which is a change to the fork.
    ///
    /// A reconnection can produce a conflict honestly, for as long as the SFU
    /// still reports the old participant. The icon flickers off and comes
    /// back, which is the same failure in the same safe direction.
    fn contested(&self) -> HashSet<&str> {
        let mut claims: HashMap<&str, usize> = HashMap::new();
        for notice in self.0.values() {
            *claims.entry(notice.member_id.as_str()).or_default() += 1;
        }

        let contested: HashSet<&str> = claims
            .into_iter()
            .filter(|(_, claimants)| *claimants > 1)
            .map(|(member_id, _)| member_id)
            .collect();

        if !contested.is_empty() {
            tracing::warn!(
                ?contested,
                "more than one participant claims the same call membership; \
                 believing neither about it"
            );
        }

        contested
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
        let sent = Notice::new("ada-laptop", true, false);

        assert_eq!(Notice::decode(&sent.encode()), Some(sent));
    }

    #[test]
    fn the_wire_names_are_the_ones_other_clients_will_read() {
        // Pinned because nothing would fail to build if serde's renaming
        // changed underneath it, and the only symptom would be two Consort
        // versions silently not understanding each other.
        let json: serde_json::Value =
            serde_json::from_slice(&Notice::new("ada-laptop", true, false).encode()).unwrap();

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
        let mut announced = Announced::new();

        assert!(announced.note("ada-identity", Notice::new("ada-laptop", true, false)));
        assert_eq!(announced.flags().deafened, vec!["ada-laptop".to_owned()]);
    }

    #[test]
    fn somebody_who_is_merely_present_is_not_listed() {
        let mut announced = Announced::new();

        announced.note("ada-identity", Notice::new("ada-laptop", false, false));

        assert!(announced.flags().deafened.is_empty());
    }

    #[test]
    fn undeafening_takes_them_off_the_list() {
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));

        assert!(announced.note("ada-identity", Notice::new("ada-laptop", false, false)));
        assert!(announced.flags().deafened.is_empty());
    }

    #[test]
    fn hearing_the_same_thing_twice_is_not_a_change() {
        // Everybody re-announces on every roster change, so most notices
        // repeat. Redrawing the roster for each would mean one redraw per
        // participant every time anybody walked in.
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));

        assert!(!announced.note("ada-identity", Notice::new("ada-laptop", true, false)));
    }

    #[test]
    fn leaving_without_a_parting_word_still_takes_them_off() {
        // A client that crashes or drops its connection says nothing on the
        // way out, and staying deafened forever would be a headphone icon
        // beside somebody who is not in the call.
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));

        assert!(announced.gone("ada-identity"));
        assert!(announced.flags().deafened.is_empty());
    }

    #[test]
    fn somebody_leaving_who_was_never_heard_from_is_not_a_change() {
        let mut announced = Announced::new();

        assert!(!announced.gone("a-stranger"));
    }

    #[test]
    fn two_people_deafened_are_both_listed() {
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));
        announced.note("bob-identity", Notice::new("bob-phone", true, false));

        assert_eq!(
            sorted(announced.flags().deafened),
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
        let mut announced = Announced::new();

        assert!(announced.note("our-identity", Notice::new("our-laptop", true, false)));
        assert!(announced.note("theirs", Notice::new("their-laptop", true, false)));

        assert_eq!(
            sorted(announced.flags().deafened),
            vec!["our-laptop".to_owned(), "their-laptop".to_owned()]
        );
    }

    #[test]
    fn somebody_else_leaving_does_not_take_this_session_with_them() {
        // Our own entry is keyed by our own identity, which the SFU never
        // reports as disconnected. Sharing a key with anybody would undeafen us
        // the moment they hung up.
        let mut announced = Announced::new();
        announced.note("our-identity", Notice::new("our-laptop", true, false));
        announced.note("theirs", Notice::new("their-laptop", true, false));

        assert!(announced.gone("theirs"));

        assert_eq!(announced.flags().deafened, vec!["our-laptop".to_owned()]);
    }

    #[test]
    fn one_person_on_two_devices_is_tracked_per_device() {
        // Deafening a laptop says nothing about a phone that is also in the
        // call, and the roster folds the two together afterwards.
        let mut announced = Announced::new();
        announced.note(
            "ada-laptop-identity",
            Notice::new("ada-laptop", true, false),
        );
        announced.note("ada-phone-identity", Notice::new("ada-phone", false, false));

        assert_eq!(announced.flags().deafened, vec!["ada-laptop".to_owned()]);
    }

    #[test]
    fn a_membership_two_participants_claim_is_believed_from_neither() {
        // Anybody in a call can send a notice naming somebody else's
        // membership, and nothing in the payload proves otherwise. What gives
        // it away is that the person it is about re-announces too, so the
        // forgery and the truth arrive together under one membership id from
        // two identities. Dropping both is wrong in the safe direction: an
        // icon that should be there goes missing, rather than one appearing
        // beside somebody who is listening.
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", false, false));

        announced.note("liar-identity", Notice::new("ada-laptop", true, true));

        let flags = announced.flags();
        assert!(flags.deafened.is_empty());
        assert!(flags.away.is_empty());
    }

    #[test]
    fn a_forged_claim_does_not_take_anybody_else_down_with_it() {
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", false, false));
        announced.note("bob-identity", Notice::new("bob-phone", true, false));

        announced.note("liar-identity", Notice::new("ada-laptop", true, false));

        assert_eq!(announced.flags().deafened, vec!["bob-phone".to_owned()]);
    }

    #[test]
    fn a_forgery_withdrawn_leaves_the_truth_standing() {
        // What a liar disconnecting looks like, and what a reconnection race
        // looks like once the SFU stops reporting the old participant.
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));
        announced.note("liar-identity", Notice::new("ada-laptop", false, false));
        assert!(announced.flags().deafened.is_empty());

        announced.gone("liar-identity");

        assert_eq!(announced.flags().deafened, vec!["ada-laptop".to_owned()]);
    }

    #[test]
    fn two_participants_agreeing_is_still_two_participants() {
        // Not a special case worth making one. A claim nobody can verify is
        // unverified whether or not it happens to match, and a forger who
        // guesses the current state right gains nothing by it.
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", true, false));

        announced.note("liar-identity", Notice::new("ada-laptop", true, false));

        assert!(announced.flags().deafened.is_empty());
    }

    #[test]
    fn somebody_who_is_away_is_listed_separately_from_the_deafened() {
        // Two questions, two icons. A person away with their headphones still
        // on is not deafened, and drawing them as such would say the call
        // cannot reach them when it can.
        let mut announced = Announced::new();

        announced.note("ada-identity", Notice::new("ada-laptop", false, true));

        let flags = announced.flags();
        assert_eq!(flags.away, vec!["ada-laptop".to_owned()]);
        assert!(flags.deafened.is_empty());
    }

    #[test]
    fn somebody_can_be_away_and_deafened_at_once() {
        // What turning your headphones off on the way out looks like.
        let mut announced = Announced::new();

        announced.note("ada-identity", Notice::new("ada-laptop", true, true));

        let flags = announced.flags();
        assert_eq!(flags.deafened, vec!["ada-laptop".to_owned()]);
        assert_eq!(flags.away, vec!["ada-laptop".to_owned()]);
    }

    #[test]
    fn coming_back_takes_them_off_the_away_list() {
        let mut announced = Announced::new();
        announced.note("ada-identity", Notice::new("ada-laptop", false, true));

        assert!(announced.note("ada-identity", Notice::new("ada-laptop", false, false)));
        assert!(announced.flags().away.is_empty());
    }

    #[test]
    fn a_notice_from_a_build_that_predates_away_reads_as_not_away() {
        // The compatibility that the version field was left alone for. An
        // older Consort in the same call sends two fields, and refusing the
        // message or guessing `true` would both be worse than this.
        let older = br#"{"v":1,"memberId":"ada-laptop","deafened":true}"#;

        let notice = Notice::decode(older).expect("an older notice was refused");
        assert!(notice.deafened);
        assert!(!notice.away);
    }

    #[test]
    fn a_build_that_predates_away_can_still_read_ours() {
        // The other direction, which is the one that cannot be tested by
        // deserialising: an older build's `Notice` has no `away` field and
        // serde ignores unknown ones, so what matters is that the two fields
        // it does read are still there and still named the same.
        let json: serde_json::Value =
            serde_json::from_slice(&Notice::new("ada-laptop", true, true).encode()).unwrap();

        assert_eq!(json["v"], 1);
        assert_eq!(json["memberId"], "ada-laptop");
        assert_eq!(json["deafened"], true);
        assert_eq!(json["away"], true);
    }
}
