// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Whether this session can be heard in an encrypted call.
//!
//! A call joins fine without cross-signing. Membership publishes, the roster
//! fills in, both peers see each other, RTP flows. The only thing that does
//! not happen is that anybody can decrypt anybody, because the media key never
//! leaves the device that made it.
//!
//! That failure was measured rather than assumed. Two sessions, neither
//! cross-signed, in a real two-peer call: both logged `Encryption failed
//! because cross-signing is not set up on your account` when key distribution
//! ran, and each saw the other as a participant with no key installed.
//! Everything looked connected and nothing could be heard.
//!
//! So the question is asked before the join rather than after it. What is
//! asked is not a proxy for the real condition, it is the real condition: the
//! SDK's identity-based sharing strategy refuses in exactly two shapes, and
//! this module asks the same two questions in the same order so that the
//! answer cannot drift from the one that matters.

use matrix_sdk::Client;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::verification::Changes;
use crate::{Error, Result};

/// Whether an encrypted call joined right now would be audible.
///
/// The two failures are kept apart because they send a person to two
/// different places. "This account has never set up cross-signing" is fixed
/// once, on any client, by setting up recovery. "This session is not
/// verified" is fixed on this device, again after every reinstall, by
/// verifying it. Collapsing them into one "not verified" tells somebody who
/// has already done the first thing to go and do it again.
///
/// There is no "not known yet" here, unlike [`crate::SessionVerification`].
/// That one is a state this crate watches and republishes, so it has to be
/// able to say it has not looked. This is a question with an answer, asked
/// when somebody clicks a channel, and [`readiness`] does not return until it
/// has one.
///
/// This crosses the IPC boundary, so the wire format is part of the contract
/// with `app/src/lib/api.ts` and the tests below pin it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CallReadiness {
    /// Media keys will be distributed. Join.
    Ready,
    /// The account has no cross-signing identity at all.
    ///
    /// Rarer than it sounds, and not the state a fresh Consort sign-in lands
    /// in. `base_builder` sets `auto_enable_cross_signing`, so both
    /// [`crate::auth::login`] and [`crate::auth::restore`] create an identity
    /// when the account has none. What is left is a bootstrap that has not
    /// finished yet, or one the homeserver refused, which is what Synapse
    /// does to `/keys/device_signing/upload` on an account that already has
    /// keys and wants interactive auth before replacing them.
    ///
    /// Nothing done on this device fixes the second case, because the missing
    /// piece belongs to the account. Set up recovery, here or on any other
    /// client signed in to it.
    NoIdentity,
    /// The account has an identity, and this session is not trusted against
    /// it.
    ///
    /// The common failure, and the one verification exists to clear. Every
    /// new device on an account that already has cross-signing starts here,
    /// because the auto-bootstrap correctly declines to replace an identity
    /// other devices are already signed by.
    SessionUnverified,
}

impl CallReadiness {
    /// Whether a join should be allowed to proceed.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Ask whether this session can send media keys.
///
/// # The order of the questions is the SDK's
///
/// `CollectStrategy::IdentityBasedStrategy` in matrix-sdk-crypto checks the
/// account's own identity, then whether that identity is verified, and
/// returns a different error for each. MatrixRTC distributes media keys to
/// device, which puts every call through that strategy. Asking anything else
/// here would produce an answer that agrees with the SDK by luck.
///
/// # One request, and only when the answer would otherwise be no
///
/// The local crypto store is asked first, and on a session that has been up
/// for any length of time that is the end of it. An identity missing from the
/// store is not the same fact as an identity missing from the account,
/// though: the account's own identity arrives by `/keys/query`, and a session
/// seconds old may not have run one. Telling somebody to go and set up
/// cross-signing they already have is worse than one request on a path that
/// is not the common one.
pub async fn readiness(client: &Client) -> Result<CallReadiness> {
    let user_id = client.user_id().ok_or(Error::NotLoggedIn)?;

    let known = client
        .encryption()
        .get_user_identity(user_id)
        .await
        .map_err(matrix_sdk::Error::from)?;

    let identity = match known {
        Some(identity) => Some(identity),
        None => client.encryption().request_user_identity(user_id).await?,
    };

    // Our own identity, so `is_verified` is not "somebody vouched for them".
    // It is whether this device trusts the account's master key, which is
    // what verifying a session establishes and what the strategy demands
    // before it will encrypt to anyone.
    Ok(classify(identity.map(|identity| identity.is_verified())))
}

/// Watch whether this session can send media keys, reporting each change.
///
/// # Why this is watched rather than asked once
///
/// The answer moves, and it moves in the direction that matters. Every session
/// starts unverified, somebody verifies it during that session, and from then
/// on calls work. Asked once at startup and cached, that person stays locked
/// out of every call until they restart the application, which is a worse bug
/// than the one the gate exists to fix.
///
/// # Why the verification stream is the trigger
///
/// It is the same fact seen from a lower resolution.
/// `encryption().verification_state()` publishes whenever this device's trust
/// against its own account changes, which is exactly when [`readiness`] would
/// answer differently. What it cannot do is stand in for the answer:
/// `VerificationState` has no way to say "this account has no cross-signing
/// identity at all", and that is one of the two failures, needing a different
/// sentence and a different action. So the stream says *when* to ask and
/// [`readiness`] says *what*.
///
/// Each wake costs one crypto-store read, and only on the uncommon path a
/// request. The SDK republishes the same state after every `/keys/query`
/// mentioning one of our devices, so most wakes report nothing: [`Changes`]
/// drops the repeats before anybody is told.
///
/// # Lifetime
///
/// Same as [`crate::verification::watch`], and owned the same way. The task
/// holds the `Client` and watches an observable belonging to it, so the stream
/// never ends on its own. The caller aborts the handle when the session goes.
pub fn watch<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(CallReadiness) + Send + 'static,
{
    tokio::spawn(async move {
        let mut states = client.encryption().verification_state();
        let mut changes = Changes::new();

        // The value, not just the changes: the SDK's subscriber hands over
        // where things currently stand before any update, so the first pass
        // through this loop is the answer at startup and no separate one-shot
        // call is needed beside it.
        while states.next().await.is_some() {
            match readiness(&client).await {
                Ok(state) => {
                    if let Some(state) = changes.accept(state) {
                        tracing::info!(?state, "whether this session can be heard in a call");
                        on_change(state);
                    }
                }
                // Not reported as a state. Every variant of `CallReadiness` is
                // a finding about the account, and a homeserver that would not
                // answer is a finding about the network: publishing one as the
                // other would put "set up cross-signing" in front of somebody
                // whose wifi dropped. The last good answer stands.
                Err(error) => {
                    tracing::warn!(%error, "could not work out call readiness");
                }
            }
        }

        // Only reachable once the client is gone, which cannot happen while
        // this task holds one. Logged rather than ignored so that a future
        // change to that is not silent.
        tracing::warn!("the call readiness watcher ended");
    })
}

/// The decision, over the two facts the strategy actually consults.
///
/// `None` is an account with no cross-signing identity. `Some` is an account
/// with one, carrying whether this device trusts it. Separated from the
/// lookup because the lookup needs a homeserver and the rule does not, and
/// the rule is the part that has to be right.
fn classify(identity_verified: Option<bool>) -> CallReadiness {
    match identity_verified {
        None => CallReadiness::NoIdentity,
        Some(false) => CallReadiness::SessionUnverified,
        Some(true) => CallReadiness::Ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod deciding {
        use super::*;

        #[test]
        fn an_account_with_no_identity_has_no_cross_signing() {
            assert_eq!(classify(None), CallReadiness::NoIdentity);
        }

        #[test]
        fn an_identity_this_device_does_not_trust_is_an_unverified_session() {
            assert_eq!(classify(Some(false)), CallReadiness::SessionUnverified);
        }

        #[test]
        fn an_identity_this_device_trusts_is_ready() {
            assert_eq!(classify(Some(true)), CallReadiness::Ready);
        }

        #[test]
        fn only_the_trusted_identity_is_ready_to_join() {
            // The rule stated as the gate reads it, so that a later variant
            // added to `CallReadiness` cannot quietly become joinable.
            for identity in [None, Some(false), Some(true)] {
                assert_eq!(
                    classify(identity).is_ready(),
                    identity == Some(true),
                    "{identity:?} decided the wrong way"
                );
            }
        }
    }

    mod gating {
        use super::*;

        #[test]
        fn only_ready_lets_a_join_through() {
            assert!(CallReadiness::Ready.is_ready());
        }

        #[test]
        fn neither_failure_lets_a_join_through() {
            // Joining anyway is the outcome this module exists to prevent: a
            // call that connects, shows a full roster, and carries no audio
            // in either direction with nothing on screen saying why.
            assert!(!CallReadiness::NoIdentity.is_ready());
            assert!(!CallReadiness::SessionUnverified.is_ready());
        }
    }

    mod wire_format {
        use super::*;

        #[test]
        fn each_state_is_tagged_the_way_the_frontend_reads_it() {
            let tag = |state: CallReadiness| {
                serde_json::to_value(state).unwrap()["state"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };

            assert_eq!(tag(CallReadiness::Ready), "ready");
            assert_eq!(tag(CallReadiness::NoIdentity), "noIdentity");
            assert_eq!(tag(CallReadiness::SessionUnverified), "sessionUnverified");
        }

        #[test]
        fn every_state_survives_a_round_trip() {
            for state in [
                CallReadiness::Ready,
                CallReadiness::NoIdentity,
                CallReadiness::SessionUnverified,
            ] {
                let json = serde_json::to_string(&state).unwrap();
                let back: CallReadiness = serde_json::from_str(&json).unwrap();
                assert_eq!(back, state, "{json} did not come back the same");
            }
        }

        #[test]
        fn the_two_failures_are_distinguishable_on_the_wire() {
            // They are one word apart in the interface and one action apart
            // in what they ask of somebody. If they ever serialise to the
            // same thing the frontend cannot tell them apart, and will show
            // one of the two messages at random.
            assert_ne!(
                serde_json::to_string(&CallReadiness::NoIdentity).unwrap(),
                serde_json::to_string(&CallReadiness::SessionUnverified).unwrap()
            );
        }
    }
}
