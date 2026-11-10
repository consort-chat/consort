// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Which generation of MatrixRTC a call is held in.
//!
//! A join in the wrong one succeeds and is invisible to the people already
//! there, so it is a named choice rather than a default: `docs/CALLS.md`.

use matrix_rtc_core::compat::MembershipFormat;

/// Which MatrixRTC generation to speak.
///
/// Ours rather than `MembershipFormat` directly, and the move off the pinned
/// forks is what that bought: upstream went from three wire modes to two and
/// the cost was one mapping function rather than every call site. The names
/// still differ in kind, too. Upstream names a wire format; a caller here is
/// choosing a peer generation, and there are three of those.
///
/// Serialisable because it is a setting: an operator whose deployment this
/// cannot detect writes the answer in `settings.json`, and the names on the
/// wire are the names below in camel case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dialect {
    /// MSC4143 plus MSC4354, the current specification. Sticky membership
    /// events, MSC4195 token exchange, per-join SFU identity.
    ///
    /// Where this is going, and not somewhere this build can go yet. See
    /// [`Dialect::readable`].
    Current,
    /// The 2025 Element Call generation. Additive: a join stays valid to a
    /// current peer, but leaves and media keys do not, so a call in this mode
    /// exchanges keys with that generation and not with the current one.
    Sticky,
    /// Before MSC4354. Membership is `org.matrix.msc3401.call.member` room
    /// state, the SFU identity is the plain `{user}:{device}` string, and the
    /// token comes from the pre-MSC4195 `/sfu/get` endpoint. Nothing about a
    /// call in this mode is visible to a current peer.
    ///
    /// This is what a deployment running an Element Call from before the 2026
    /// rewrite needs, and it was the one proven end to end in phase 0.
    ///
    /// The default, because it is the only one of the three this build can
    /// currently hold a call in. See [`Dialect::readable`].
    #[default]
    State,
}

/// Which dialect to speak in a room, given what is visible in it.
///
/// One signal, `live_state_memberships`, which is
/// `Room::active_room_call_participants().len()`. Detection can only push
/// towards [`Dialect::State`], never away from it, and `fallback` is what an
/// empty room gets. `docs/CALLS.md` has why the other two cannot be told apart.
pub fn detect(live_state_memberships: usize, fallback: Dialect) -> Dialect {
    if live_state_memberships > 0 {
        return Dialect::State;
    }

    fallback
}

impl Dialect {
    /// The name this dialect is written under in `settings.json`.
    ///
    /// The same string serde produces, and asserted to stay that way, because
    /// the only reason to name a dialect in a sentence is to tell somebody
    /// what to put in that file.
    pub fn name(self) -> &'static str {
        match self {
            Dialect::Current => "current",
            Dialect::Sticky => "sticky",
            Dialect::State => "state",
        }
    }

    /// Whether this build can read back the membership a call in this dialect
    /// publishes.
    ///
    /// Only [`Dialect::State`] can: Consort syncs with
    /// `sync_with_result_callback`, and the sticky events a call's peers are
    /// read from are filled only on the sliding-sync path. A call in the other
    /// two connects, publishes membership, carries RTP and shows an empty room.
    /// This and every refusal that consults it come out when Consort moves to
    /// sliding sync; `docs/CALLS.md` has the chain.
    pub fn readable(self) -> bool {
        matches!(self, Dialect::State)
    }
}

impl From<Dialect> for MembershipFormat {
    /// `Sticky` and `State` answer the same, because upstream reads the sticky
    /// dialect inside the room-state format. `Sticky` is kept because the
    /// variant names are a settings file's contract: `docs/CALLS.md`.
    fn from(dialect: Dialect) -> Self {
        match dialect {
            Dialect::Current => MembershipFormat::Msc,
            Dialect::Sticky | Dialect::State => MembershipFormat::RoomState,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_dialect_is_written_down_under_a_name_a_person_could_type() {
        // These names are a settings file's contract. A rename here silently
        // turns somebody's hand-written choice back into the default.
        let name = |dialect: Dialect| serde_json::to_string(&dialect).unwrap();

        assert_eq!(name(Dialect::Current), "\"current\"");
        assert_eq!(name(Dialect::Sticky), "\"sticky\"");
        assert_eq!(name(Dialect::State), "\"state\"");
    }

    #[test]
    fn a_room_somebody_is_sitting_in_through_room_state_is_answered_in_that_dialect() {
        // The case that matters. Somebody is in there right now, and their
        // membership is room state, so anything else joins a call they cannot
        // see and cannot be seen in.
        assert_eq!(detect(1, Dialect::Current), Dialect::State);
        assert_eq!(detect(9, Dialect::Sticky), Dialect::State);
    }

    #[test]
    fn an_empty_room_gets_the_fallback_rather_than_a_guess() {
        // There is nothing in an empty channel to read a generation off. The
        // caller's configured answer is better than anything invented here.
        assert_eq!(detect(0, Dialect::Current), Dialect::Current);
        assert_eq!(detect(0, Dialect::Sticky), Dialect::Sticky);
        assert_eq!(detect(0, Dialect::State), Dialect::State);
    }

    #[test]
    fn detection_never_talks_a_caller_out_of_the_state_dialect() {
        // The asymmetry, asserted rather than left to the prose. A deployment
        // that has been configured for the pre-MSC4354 generation is one whose
        // Element Call speaks it, and no count of zero is evidence against
        // that: this build cannot see sticky membership at all.
        for present in [0, 1, 2, 50] {
            assert_eq!(detect(present, Dialect::State), Dialect::State);
        }
    }

    #[test]
    fn each_dialect_maps_to_the_wire_mode_that_speaks_it() {
        assert_eq!(
            MembershipFormat::from(Dialect::Current),
            MembershipFormat::Msc
        );
        assert_eq!(
            MembershipFormat::from(Dialect::State),
            MembershipFormat::RoomState
        );
    }

    #[test]
    fn the_default_is_a_dialect_a_call_can_actually_be_held_in() {
        // The default used to be `Current`, on the grounds that a default
        // which quietly picks a compatibility mode outlives the deployment
        // needing it. True, and beaten by the fact that this build cannot read
        // a call in that dialect at all: the principled default produced a
        // call that connected and showed an empty room.
        //
        // Written against `readable` rather than against `State` so that the
        // day sliding sync lands, this test starts passing for `Current` on
        // its own and the default can move back without anybody having to
        // remember why it moved.
        assert!(
            Dialect::default().readable(),
            "the default has to be a dialect this build can read"
        );
    }

    #[test]
    fn only_the_pre_msc4354_dialect_can_be_read_by_this_build() {
        // The constraint itself. Sticky membership is ingested on the sliding
        // sync path only, and Consort syncs with `sync_with_result_callback`.
        assert!(Dialect::State.readable());
        assert!(!Dialect::Current.readable());
        assert!(!Dialect::Sticky.readable());
    }

    #[test]
    fn detection_can_only_ever_land_on_a_readable_dialect_or_the_fallback() {
        // Belt and braces on the asymmetry: an occupied room is answered
        // `State`, which is readable, so detection can never turn a working
        // configuration into a broken one.
        for present in [1, 2, 50] {
            for fallback in [Dialect::Current, Dialect::Sticky, Dialect::State] {
                assert!(detect(present, fallback).readable());
            }
        }
    }

    #[test]
    fn the_name_a_dialect_is_told_by_is_the_name_it_is_written_under() {
        // `name` exists to tell somebody what to put in `settings.json`. If it
        // drifts from serde, the advice sends them to a value that parses as
        // nothing and silently falls back to the default.
        for dialect in [Dialect::Current, Dialect::Sticky, Dialect::State] {
            assert_eq!(
                serde_json::to_string(&dialect).unwrap(),
                format!("\"{}\"", dialect.name())
            );
        }
    }

    #[test]
    fn the_one_pair_that_shares_a_wire_mode_is_a_pair_nothing_can_reach() {
        // Two dialects collapsing into one would make the choice unobservable
        // at exactly the point it stopped working, which is what the three-mode
        // version of this test guarded. Upstream has two modes now, so the
        // guarantee has to be made the other way round: the pair that shares a
        // mode is a pair a join never gets to, because `readable` refuses
        // `Sticky` first.
        assert_eq!(
            MembershipFormat::from(Dialect::Sticky),
            MembershipFormat::from(Dialect::State)
        );
        assert!(!Dialect::Sticky.readable());

        // And the pair that is still distinguishable stays so.
        assert_ne!(
            MembershipFormat::from(Dialect::Current),
            MembershipFormat::from(Dialect::State)
        );
    }
}
