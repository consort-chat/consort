// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Whether this session is verified.
//!
//! A freshly logged-in device is a stranger to the account that owns it. It
//! cannot read encrypted history, other people's clients warn about it, and
//! MSC4153 means it cannot join an encrypted call at all. None of that is
//! visible from anything the login returned, so it has to be watched for and
//! said out loud.
//!
//! Two halves. [`watch`] reports whether the session is verified, which is a
//! property of the account and is answered from the crypto store without
//! asking anybody. [`flow`] is the doing: one emoji comparison from the moment
//! a request appears to the moment both sides agree, or do not, whether this
//! session asked or was asked.

pub mod dto;
pub mod flow;

pub use dto::{CancelReason, EmojiPair, Flow, FlowState};
pub use flow::{
    Initiator, accept, cancel, confirm, has_devices_to_verify_against, mismatch, start_sas,
    supervise,
};

use matrix_sdk::Client;
use matrix_sdk::encryption::VerificationState;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

/// Whether the homeserver's copy of this device carries a signature from the
/// account that owns it.
///
/// Three states rather than a boolean, and the third one is the point. The
/// SDK genuinely does not know until it has looked, and a client that renders
/// "not yet known" as either answer is a client that tells somebody their
/// messages are safe before it has checked.
///
/// This crosses the IPC boundary, so the wire format is part of the contract
/// with `app/src/lib/api.ts` and the tests below pin it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SessionVerification {
    /// Not worked out yet. The state at startup, and after anything that
    /// leaves the SDK unable to find its own device.
    Unknown,
    /// Signed by its owner. History decrypts and calls will accept it.
    Verified,
    /// Known, and signed by nobody. The state every new login starts in.
    Unverified,
}

/// The SDK's own-device state, in the vocabulary the frontend speaks.
///
/// Note which `VerificationState` this is. `matrix_sdk::encryption` has one
/// describing our own device, with three variants and no payload, and
/// `matrix_sdk_common::deserialized_responses` has a different type of the
/// same name describing who sent a message, whose `Unverified` carries a
/// detailed level. This is the first one: for our own device there is no level
/// to carry, because there is only ever one signature in question.
impl From<VerificationState> for SessionVerification {
    fn from(state: VerificationState) -> Self {
        match state {
            VerificationState::Unknown => Self::Unknown,
            VerificationState::Verified => Self::Verified,
            VerificationState::Unverified => Self::Unverified,
        }
    }
}

/// Filters a stream of states down to the ones that are news.
///
/// The SDK republishes the verification state after every `/keys/query` that
/// mentions one of our own devices, which happens whenever the user opens
/// another client, and it publishes the same answer each time. Forwarding all
/// of them wakes the webview and re-renders for no new information.
///
/// A plain field rather than the `Mutex` its counterpart in [`crate::sync`]
/// needs: that one is shared with a callback the SDK may call from anywhere,
/// while this one is owned by a single sequential loop. Both loops in this
/// module are like that, which is why it is generic rather than copied.
pub(crate) struct Changes<T> {
    last: Option<T>,
}

impl<T: PartialEq> Changes<T> {
    pub(crate) fn new() -> Self {
        Self { last: None }
    }

    /// The state, if it is different from the one before it.
    pub(crate) fn accept(&mut self, state: T) -> Option<T>
    where
        T: Clone,
    {
        if self.last.as_ref() == Some(&state) {
            return None;
        }
        self.last = Some(state.clone());
        Some(state)
    }
}

/// Watch this session's verification state, reporting each change.
///
/// The first report arrives without waiting for anything: the SDK's subscriber
/// hands over the current value before any update, so a caller that subscribes
/// late still learns where things stand.
///
/// # Lifetime
///
/// Same as [`crate::sync::start`]. The task holds the `Client`, and the
/// observable it watches belongs to that client, so the stream never ends and
/// the task never returns. The caller owns the handle and aborts it when the
/// session does.
pub fn watch<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(SessionVerification) + Send + 'static,
{
    tokio::spawn(async move {
        let mut states = client.encryption().verification_state();
        let mut changes = Changes::new();

        while let Some(state) = states.next().await {
            if let Some(state) = changes.accept(state.into()) {
                tracing::info!(?state, "the session verification state changed");
                on_change(state);
            }
        }

        // Only reachable once the client is gone, which cannot happen while
        // this task holds one. Logged rather than ignored so that a future
        // change to that is not silent.
        tracing::warn!("the verification state watcher ended");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod mapping {
        use super::*;
        use matrix_sdk::encryption::VerificationState;

        #[test]
        fn a_device_its_owner_signed_is_verified() {
            assert_eq!(
                SessionVerification::from(VerificationState::Verified),
                SessionVerification::Verified
            );
        }

        #[test]
        fn a_device_nobody_signed_is_unverified() {
            assert_eq!(
                SessionVerification::from(VerificationState::Unverified),
                SessionVerification::Unverified
            );
        }

        #[test]
        fn a_state_the_sdk_has_not_worked_out_yet_is_unknown_and_not_verified() {
            // The one mapping that matters. "We do not know yet" collapsed
            // into either answer is a lie, and collapsed into `Verified` it is
            // the lie that tells somebody their messages are safe.
            let mapped = SessionVerification::from(VerificationState::Unknown);

            assert_eq!(mapped, SessionVerification::Unknown);
            assert_ne!(mapped, SessionVerification::Verified);
        }
    }

    mod wire_format {
        use super::*;

        #[test]
        fn each_state_is_tagged_the_way_the_frontend_reads_it() {
            let tag = |state: SessionVerification| {
                serde_json::to_value(state).unwrap()["state"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            };

            assert_eq!(tag(SessionVerification::Unknown), "unknown");
            assert_eq!(tag(SessionVerification::Verified), "verified");
            assert_eq!(tag(SessionVerification::Unverified), "unverified");
        }

        #[test]
        fn every_state_survives_a_round_trip() {
            for state in [
                SessionVerification::Unknown,
                SessionVerification::Verified,
                SessionVerification::Unverified,
            ] {
                let json = serde_json::to_string(&state).unwrap();
                let back: SessionVerification = serde_json::from_str(&json).unwrap();
                assert_eq!(back, state, "{json} did not come back the same");
            }
        }
    }

    mod changes {
        use super::*;

        #[test]
        fn the_first_state_is_always_news() {
            let mut changes = Changes::new();

            assert_eq!(
                changes.accept(SessionVerification::Unknown),
                Some(SessionVerification::Unknown)
            );
        }

        #[test]
        fn repeating_a_state_is_not_news() {
            // The SDK re-publishes this on every keys query that mentions one
            // of our own devices, which is every time the user opens another
            // client. Forwarding each one is a webview wake-up carrying
            // nothing.
            let mut changes = Changes::new();
            changes.accept(SessionVerification::Unverified);

            assert_eq!(changes.accept(SessionVerification::Unverified), None);
        }

        #[test]
        fn a_state_that_changes_back_is_news_again() {
            let mut changes = Changes::new();
            changes.accept(SessionVerification::Unverified);
            changes.accept(SessionVerification::Verified);

            assert_eq!(
                changes.accept(SessionVerification::Unverified),
                Some(SessionVerification::Unverified)
            );
        }
    }
}
