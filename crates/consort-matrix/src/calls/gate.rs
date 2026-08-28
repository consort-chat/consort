// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Whether to let a join through.
//!
//! [`readiness`] answers whether this session can distribute media keys. That
//! is only half of the question, because it only matters in a room that
//! encrypts. MSC4143 forbids RTC encryption in an unencrypted room, and
//! `matrix_rtc_livekit::connect_with_optional_e2ee` says so in as many words:
//! no keys are distributed there at all, and a transport that encrypted anyway
//! would leave every conforming peer decoding ciphertext as audio.
//!
//! So an unverified session in an unencrypted voice channel works perfectly.
//! Refusing it would break a working call in the name of a failure that cannot
//! happen in it, which is why the two questions are asked together here rather
//! than the readiness one being asked alone at the call site.

use matrix_sdk::Client;

use super::readiness::{CallReadiness, readiness};
use crate::Result;

/// Whether a call in a particular room should be joined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinVerdict {
    /// Go ahead. Either the room does not encrypt, or this session can be
    /// heard in it.
    Allowed,
    /// Do not, and put this in front of somebody.
    ///
    /// Carries the readiness rather than a sentence, because the two failures
    /// ask for two different actions and the interface phrases both. See
    /// [`CallReadiness`].
    Refused(CallReadiness),
}

impl JoinVerdict {
    /// Whether the join may proceed.
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// What is known about whether a room encrypts.
///
/// Three answers rather than a boolean, because "nobody has been able to look"
/// is a real state and it decides the opposite way from "looked, and no". The
/// SDK's own `EncryptionState::is_encrypted` collapses the two by returning
/// false for `Unknown`, which is the wrong direction for a gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encrypts {
    /// The room has an `m.room.encryption` event.
    Yes,
    /// The room does not, and the store is sure of it.
    No,
    /// Not established: the state fetch failed, the room is not one this
    /// account is in, or the room id was not a room id.
    Unknown,
}

/// Whether to join the call in `room_id`.
///
/// One request at worst, and usually none: `latest_encryption_state` asks the
/// homeserver only when the store cannot answer, and [`readiness`] reads the
/// crypto store unless the account's own identity has not arrived yet.
///
/// Asked at the click rather than read off the watcher, because a click is
/// exactly the moment when being one sync stale matters. The watcher exists so
/// the channel list can be drawn before anybody clicks; this is the decision.
pub async fn can_join(client: &Client, room_id: &str) -> Result<JoinVerdict> {
    if !gates_on_readiness(encrypts(client, room_id).await) {
        return Ok(JoinVerdict::Allowed);
    }

    Ok(match readiness(client).await? {
        CallReadiness::Ready => JoinVerdict::Allowed,
        refusal => JoinVerdict::Refused(refusal),
    })
}

/// Whether this room's calls carry media keys, and so whether being able to
/// send them decides anything.
///
/// `Unknown` gates. Being wrong that way costs a refusal somebody clears by
/// verifying, and they can then join. Being wrong the other way is the failure
/// this whole module exists to prevent: a call that connects, shows a full
/// roster, and carries no audio, with nothing on screen saying why.
///
/// Separated from the lookup because the lookup needs a homeserver and the rule
/// does not, and the rule is the part that has to be right.
fn gates_on_readiness(encrypts: Encrypts) -> bool {
    !matches!(encrypts, Encrypts::No)
}

/// Ask the room, resolving every way of not getting an answer to `Unknown`.
///
/// `latest_encryption_state` rather than `encryption_state`, because the
/// cheap one reports `Unknown` for any room whose state sync has not covered,
/// and under the rule above that would gate every voice channel this account
/// has not opened yet.
async fn encrypts(client: &Client, room_id: &str) -> Encrypts {
    use matrix_sdk::ruma::RoomId;

    let Ok(room_id) = RoomId::parse(room_id) else {
        return Encrypts::Unknown;
    };
    let Some(room) = client.get_room(&room_id) else {
        return Encrypts::Unknown;
    };

    match room.latest_encryption_state().await {
        // Matched positively rather than through `is_encrypted`, which is
        // false for `Unknown` and would make an unanswerable room joinable.
        Ok(state) if state.is_encrypted() => Encrypts::Yes,
        Ok(matrix_sdk::EncryptionState::NotEncrypted) => Encrypts::No,
        Ok(_) => Encrypts::Unknown,
        Err(error) => {
            tracing::warn!(%error, %room_id, "could not find out whether the room encrypts");
            Encrypts::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod deciding {
        use super::*;

        #[test]
        fn an_encrypted_room_needs_this_session_to_be_able_to_send_keys() {
            assert!(gates_on_readiness(Encrypts::Yes));
        }

        #[test]
        fn an_unencrypted_room_needs_nothing_of_the_kind() {
            // The row that catches an over-eager gate. There are no media keys
            // in an unencrypted call, so an unverified session is heard
            // perfectly and refusing it would break something that works.
            assert!(!gates_on_readiness(Encrypts::No));
        }

        #[test]
        fn a_room_nobody_could_ask_about_is_gated() {
            assert!(gates_on_readiness(Encrypts::Unknown));
        }

        #[test]
        fn only_a_definite_no_opens_the_gate() {
            for encrypts in [Encrypts::Yes, Encrypts::No, Encrypts::Unknown] {
                assert_eq!(
                    gates_on_readiness(encrypts),
                    encrypts != Encrypts::No,
                    "{encrypts:?} decided the wrong way"
                );
            }
        }
    }

    mod verdicts {
        use super::*;

        #[test]
        fn only_being_allowed_lets_a_join_through() {
            assert!(JoinVerdict::Allowed.is_allowed());
            assert!(!JoinVerdict::Refused(CallReadiness::NoIdentity).is_allowed());
            assert!(!JoinVerdict::Refused(CallReadiness::SessionUnverified).is_allowed());
        }

        #[test]
        fn a_refusal_carries_which_failure_it_was() {
            // The two ask for different things from different places, so a
            // verdict that only said "no" would leave the interface guessing
            // which sentence to draw.
            assert_ne!(
                JoinVerdict::Refused(CallReadiness::NoIdentity),
                JoinVerdict::Refused(CallReadiness::SessionUnverified)
            );
        }
    }
}
