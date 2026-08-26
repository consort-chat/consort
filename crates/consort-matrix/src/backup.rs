// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What is happening to this session's room keys.
//!
//! Verification signs the device. It does not, by itself, hand over a single
//! message key: those live in the server-side key backup, encrypted to a key
//! the account holds, and getting at them is a second thing that has to work.
//! Without it a session verifies, says so, and still cannot read a word that
//! was sent before it existed, which reads as a broken client rather than as a
//! missing feature.
//!
//! Most of the machinery is the SDK's, and it is switched on in
//! [`crate::auth`] rather than driven from here. `auto_enable_backups` creates
//! a backup at login when the account has none, and
//! `BackupDownloadStrategy::AfterDecryptionFailure` fetches the one room key
//! an undecryptable message needs, when it needs it. What is left, and what
//! this module is, is saying out loud which of those is actually true right
//! now, because all of it happens in the background and none of it is visible
//! from anything a command returns.

use futures_util::StreamExt;
use matrix_sdk::Client;
use matrix_sdk::encryption::backups::BackupState;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::verification::Changes;

/// Whether room keys made on this device survive it.
///
/// Five states because the SDK's `Unknown` is two different answers to the
/// question a person is asking. It means "no backup is active in this
/// session", which is the ordinary state of a session that has not been
/// verified yet, and it is also what a failure to create one looks like.
/// "There is a backup and you cannot read it yet" and "there is no backup at
/// all" want different words, so the one is resolved into the other two by
/// asking the homeserver.
///
/// This crosses the IPC boundary, so the wire format is part of the contract
/// with `app/src/lib/api.ts` and the tests below pin it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum KeyBackup {
    /// Room keys are going up as they are made. The good case.
    Enabled,
    /// A backup is being created, enabled, resumed, or read from. On the way
    /// to `Enabled`, and reported separately so that a session stuck here is
    /// distinguishable from one that never started.
    Preparing,
    /// A backup exists on the server and this session is not using it.
    ///
    /// The ordinary state of an unverified session: the key that opens the
    /// backup arrives with verification, by either route. It is not a fault
    /// and it is not the same thing as having no backup.
    Unusable,
    /// There is no backup for this account at all.
    ///
    /// The one worth interrupting somebody about. Every room key this session
    /// holds exists only on this machine, and signing out or losing it takes
    /// the lot.
    Missing,
    /// The homeserver would not say. Reported rather than guessed, because
    /// both guesses are a claim about whether somebody's messages survive.
    Unknown,
}

/// Watch what is happening to this session's room keys, reporting each change.
///
/// The first report arrives without waiting for anything: the SDK's stream
/// yields the current state before any update.
///
/// # Lifetime
///
/// Same as [`crate::sync::start`] and [`crate::verification::watch`]. The task
/// holds the `Client` and watches a stream belonging to it, so it never ends
/// on its own. The caller owns the handle and aborts it when the session does.
pub fn watch<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(KeyBackup) + Send + 'static,
{
    tokio::spawn(async move {
        let mut states = client.encryption().backups().state_stream();
        let mut changes = Changes::new();

        while let Some(state) = states.next().await {
            let state = match state {
                Ok(state) => state,
                // A broadcast receiver that fell behind. The states in between
                // are gone, and the one after this is still correct, so
                // dropping them is right and saying nothing about it is not:
                // this is the only way a report can be missed.
                Err(error) => {
                    tracing::warn!(%error, "missed some key backup state changes");
                    continue;
                }
            };

            let state = describe(&client, state).await;
            if let Some(state) = changes.accept(state) {
                tracing::info!(?state, "the key backup state changed");
                on_change(state);
            }
        }

        // Only reachable once the client is gone, which cannot happen while
        // this task holds one. Logged rather than ignored so a future change
        // to that is not silent.
        tracing::warn!("the key backup watcher ended");
    })
}

/// The states the SDK has already answered, which is all but one of them.
///
/// Split out so that the one state costing a request is visible as the one
/// state costing a request, rather than a branch buried in a match that mostly
/// does not.
fn settled(state: BackupState) -> Option<KeyBackup> {
    match state {
        BackupState::Enabled => Some(KeyBackup::Enabled),
        BackupState::Creating
        | BackupState::Enabling
        | BackupState::Resuming
        | BackupState::Downloading => Some(KeyBackup::Preparing),
        // Being torn down on purpose. Whatever it is about to be it is not
        // taking keys now, and what follows it is `Unknown`, which arrives a
        // moment later and gets resolved properly.
        BackupState::Disabling => Some(KeyBackup::Preparing),
        BackupState::Unknown => None,
    }
}

/// Turn the SDK's state into the answer somebody is actually asking for.
///
/// `Unknown` is the one state the SDK cannot resolve by itself, by design:
/// homeservers do not announce a backup being created or deleted, so the only
/// way to know is to go and look. Looking here rather than leaving it to the
/// caller is what stops the two answers hiding inside it being confused for
/// each other by accident.
async fn describe(client: &Client, state: BackupState) -> KeyBackup {
    if let Some(state) = settled(state) {
        return state;
    }

    match client.encryption().backups().fetch_exists_on_server().await {
        Ok(true) => KeyBackup::Unusable,
        Ok(false) => KeyBackup::Missing,
        Err(error) => {
            tracing::warn!(%error, "could not find out whether a key backup exists");
            KeyBackup::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod settling {
        use super::*;

        #[test]
        fn a_backup_taking_keys_is_the_good_answer() {
            assert_eq!(settled(BackupState::Enabled), Some(KeyBackup::Enabled));
        }

        #[test]
        fn every_state_on_the_way_to_enabled_reads_as_on_the_way() {
            // Five names for "not yet, hold on". Collapsing them is what makes
            // a session stuck part-way distinguishable from one that never
            // started, which is the only reason to report the middle at all.
            for state in [
                BackupState::Creating,
                BackupState::Enabling,
                BackupState::Resuming,
                BackupState::Downloading,
                BackupState::Disabling,
            ] {
                assert_eq!(
                    settled(state),
                    Some(KeyBackup::Preparing),
                    "{state:?} was not reported as in progress"
                );
            }
        }

        #[test]
        fn the_one_state_that_costs_a_request_is_the_one_the_sdk_cannot_answer() {
            // The SDK's `Unknown` is "no backup is active here", which is both
            // "there is one and this session cannot read it" and "there is
            // none at all". Opposite pieces of news, and the only way to tell
            // them apart is to ask the homeserver.
            assert_eq!(settled(BackupState::Unknown), None);
        }
    }

    mod wire_format {
        use super::*;

        fn tag(state: KeyBackup) -> String {
            serde_json::to_value(state).unwrap()["state"]
                .as_str()
                .unwrap()
                .to_owned()
        }

        #[test]
        fn each_state_is_tagged_the_way_the_frontend_reads_it() {
            assert_eq!(tag(KeyBackup::Enabled), "enabled");
            assert_eq!(tag(KeyBackup::Preparing), "preparing");
            assert_eq!(tag(KeyBackup::Unusable), "unusable");
            assert_eq!(tag(KeyBackup::Missing), "missing");
            assert_eq!(tag(KeyBackup::Unknown), "unknown");
        }

        #[test]
        fn every_state_survives_a_round_trip() {
            for state in [
                KeyBackup::Enabled,
                KeyBackup::Preparing,
                KeyBackup::Unusable,
                KeyBackup::Missing,
                KeyBackup::Unknown,
            ] {
                let json = serde_json::to_string(&state).unwrap();
                let back: KeyBackup = serde_json::from_str(&json).unwrap();
                assert_eq!(back, state, "{json} did not come back the same");
            }
        }
    }
}
