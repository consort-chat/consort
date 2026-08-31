// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Which generation of MatrixRTC a call is being held in.
//!
//! MatrixRTC has been rewritten twice and the deployments have not caught up
//! together. A join in the wrong generation is the worst kind of failure to
//! diagnose: it succeeds. Membership publishes, the SFU accepts the
//! connection, the log says connected, and the people already in the channel
//! see nobody arrive, because they are reading a different event type.
//!
//! So it is a named choice with three values rather than a default nobody
//! looks at.
//!
//! ## What can and cannot be detected
//!
//! [`detect`] answers half the question, and it is the important half. See its
//! documentation for why the other half is not answerable from here.
//!
//! ## Only one of the three works today
//!
//! [`Dialect::readable`] is the other constraint, and it is a property of this
//! build rather than of any deployment. See it before adding a dialect or
//! changing the default.

use matrix_rtc_livekit::compat::ElementCallCompat;

/// Which MatrixRTC generation to speak.
///
/// Ours rather than `ElementCallCompat` directly, for two reasons. The
/// upstream type is documented as scaffolding with a delete-by date, and
/// keeping it out of Consort's own vocabulary means its removal is one
/// mapping function to fix rather than every call site. And its names describe
/// the wire format, where what a caller here is choosing is a peer generation.
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
/// One signal, and it is the only one available without a matrix-sdk feature
/// this workspace does not enable: `live_state_memberships` is how many
/// unexpired pre-MSC4354 room-state memberships the room holds, which is
/// `Room::active_room_call_participants().len()`. Anybody in there through
/// room state is somebody speaking [`Dialect::State`], and joining any other
/// way makes this session invisible to them.
///
/// ## Why the other two cannot be told apart here
///
/// Nothing here can see MSC4354 sticky membership at all, for the reasons in
/// [`Dialect::readable`], so an empty answer above means either "an empty
/// channel" or "a channel full of people whose membership this build cannot
/// see". Even once it can, [`Dialect::Current`] and
/// [`Dialect::Sticky`] would still not be distinguishable by counting: a
/// sticky-dialect join is deliberately additive and stays valid to a current
/// peer, so both generations write an event that reads the same from outside.
/// The difference is in the field names inside it and in which to-device type
/// carries the media keys.
///
/// So detection can only ever push towards [`Dialect::State`], never away from
/// it, and `fallback` is what an empty room gets. That asymmetry is the right
/// way round: joining an empty channel in the wrong dialect is a call nobody
/// has arrived at yet, while joining an occupied one in the wrong dialect is a
/// call that looks connected and is not.
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
    /// Only [`Dialect::State`] can, and the reason is a chain rather than a
    /// preference. Each link was read rather than assumed, so it is written
    /// down here to be re-checked rather than rediscovered:
    ///
    /// 1. `matrix-rtc-bridge` reads the peers of a call with
    ///    `Room::live_sticky_events`.
    /// 2. That is filled by exactly one call site in matrix-sdk,
    ///    `sticky_manager.dispatch` in `matrix-sdk-base`'s `sliding_sync`.
    /// 3. Consort syncs with `sync_with_result_callback`, which is sync v2 and
    ///    never reaches that call site. Turning on `unstable-msc4354` does not
    ///    change this: it adds the ruma types, not the ingestion.
    /// 4. The sticky bridge takes a second wake source, room updates, only in
    ///    `ElementCallCompat::StateEvents`. In the other two it has no source
    ///    at all, so it seeds an empty roster and then waits forever.
    ///
    /// So a call in the other two dialects connects, publishes membership,
    /// carries RTP and shows an empty room to the person who joined it. That
    /// is the failure this module opens by naming, which is why the LiveKit
    /// transport refuses such a join rather than making it.
    ///
    /// This is a build limitation with an end date. When Consort moves to
    /// sliding sync, this method and every refusal that consults it come out
    /// together, and the tests below are what say so.
    pub fn readable(self) -> bool {
        matches!(self, Dialect::State)
    }
}

impl From<Dialect> for ElementCallCompat {
    fn from(dialect: Dialect) -> Self {
        match dialect {
            Dialect::Current => ElementCallCompat::Off,
            Dialect::Sticky => ElementCallCompat::StickyEvents,
            Dialect::State => ElementCallCompat::StateEvents,
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
            ElementCallCompat::from(Dialect::Current),
            ElementCallCompat::Off
        );
        assert_eq!(
            ElementCallCompat::from(Dialect::Sticky),
            ElementCallCompat::StickyEvents
        );
        assert_eq!(
            ElementCallCompat::from(Dialect::State),
            ElementCallCompat::StateEvents
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
    fn no_two_dialects_map_to_the_same_wire_mode() {
        // The mapping is the whole crate's leverage over a failure that looks
        // like success. Two dialects collapsing into one would make the
        // choice unobservable at exactly the point it stopped working.
        let modes = [Dialect::Current, Dialect::Sticky, Dialect::State]
            .map(ElementCallCompat::from)
            .map(|mode| format!("{mode:?}"));

        let mut unique = modes.to_vec();
        unique.sort();
        unique.dedup();

        assert_eq!(unique.len(), modes.len(), "{modes:?}");
    }
}
