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
//! ## Detection is not here yet
//!
//! This is the vocabulary and the mapping. Working out which generation a
//! given room is using belongs with the roster work that reads the same room
//! state, and is planned for phase 3 of `docs/PLAN-voice-call.md`. Until then
//! the caller says which one, and says it out loud.

use matrix_rtc_livekit::compat::ElementCallCompat;

/// Which MatrixRTC generation to speak.
///
/// Ours rather than `ElementCallCompat` directly, for two reasons. The
/// upstream type is documented as scaffolding with a delete-by date, and
/// keeping it out of Consort's own vocabulary means its removal is one
/// mapping function to fix rather than every call site. And its names describe
/// the wire format, where what a caller here is choosing is a peer generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dialect {
    /// MSC4143 plus MSC4354, the current specification. Sticky membership
    /// events, MSC4195 token exchange, per-join SFU identity.
    #[default]
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
    State,
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
    fn the_default_is_the_specification_rather_than_the_workaround() {
        // A default that quietly picks a compatibility mode is a default that
        // outlives the deployment needing it. When Element Call catches up,
        // nothing here has to change for a new deployment to be right.
        assert_eq!(Dialect::default(), Dialect::Current);
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
