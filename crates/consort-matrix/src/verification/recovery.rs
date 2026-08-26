// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Verifying this session with the account's recovery key.
//!
//! The route that works when the other device is in the next room, or when
//! there is no other device at all. Emoji needs two sessions online at the
//! same moment and a person looking at both of them. A recovery key needs
//! neither, which makes it the only way a fresh install can become usable on
//! an account whose only session is the one being set up.
//!
//! It arrives at the same place by a different road. Both routes end with this
//! device holding the cross-signing private keys and signing itself, and the
//! SDK reports the result on the same channel either way, so nothing
//! downstream has to know which was used.
//!
//! What a person types here may be a 48-character base58 key or a passphrase,
//! and which of the two an account accepts is a property of the account rather
//! than a choice made here. The SDK tries the passphrase first when the
//! account has one and falls back to base58, so both arrive through one input
//! and one function.

use matrix_sdk::Client;
use matrix_sdk::encryption::recovery::{RecoveryError, RecoveryState};
use matrix_sdk::encryption::secret_storage::SecretStorageError;
use matrix_sdk_base::crypto::secret_storage::DecodeError;

use crate::{Error, Result};

/// Whether this account has recovery set up, so that a key is worth asking for.
///
/// Asked before the box is drawn. An account with no secret storage has no key
/// for anybody to have kept, and offering an input that cannot succeed invites
/// somebody to hunt through a password manager for something that was never
/// created.
pub async fn has_recovery_set_up(client: &Client) -> Result<bool> {
    match client.encryption().recovery().state() {
        // Set up, and this session either has every secret already or is
        // missing some. Both mean there is a key that opens it.
        RecoveryState::Enabled | RecoveryState::Incomplete => Ok(true),
        RecoveryState::Disabled => Ok(false),
        // Not "the answer is unknowable": the SDK fills this in from the
        // background task login starts, and a restored session does not wait
        // for it. Somebody looking at the screen a second after launch would
        // get "we have not looked yet" as an answer to "is there a key". Ask
        // the homeserver the question the SDK is about to ask.
        RecoveryState::Unknown => Ok(client.encryption().secret_storage().is_enabled().await?),
    }
}

/// Verify this session with the account's recovery key.
///
/// On success the cross-signing private keys are in this device's store, the
/// device has signed itself, and the verification watcher publishes `Verified`
/// without being asked. There is nothing further for the caller to do.
pub async fn recover(client: &Client, recovery_key: &str) -> Result<()> {
    // Answered here rather than by the homeserver. An empty box is a slip, and
    // on an account whose secret storage takes a passphrase the server's
    // verdict on "" is "that is not the right passphrase", which is true and
    // useless.
    if recovery_key.trim().is_empty() {
        return Err(Error::MalformedRecoveryKey);
    }

    client
        .encryption()
        .recovery()
        .recover(recovery_key)
        .await
        .map_err(recovery_failure)?;

    // Succeeding means the secrets came back, not that any of them were the
    // ones that matter. Secret storage is a bag rather than a fixed set, and
    // one holding only a megolm backup key imports cleanly and leaves the
    // session exactly as unverified as it started. Silence there looks, to
    // somebody who just typed 48 correct characters, like nothing happened.
    let status = client
        .encryption()
        .cross_signing_status()
        .await
        .ok_or(Error::NotLoggedIn)?;

    if !status.has_self_signing {
        return Err(Error::RecoveryWithoutIdentity);
    }

    tracing::info!("this session verified itself with the account's recovery key");
    Ok(())
}

/// Turn a failed recovery into something worth putting in front of a person.
///
/// The distinction that earns its keep is malformed against wrong. "That is
/// not a recovery key" and "that is a recovery key, but not this account's"
/// send somebody to two different places: the first back to whatever they
/// pasted from, the second to the account they meant to be signing in to. One
/// "verification failed" covering both sends them nowhere.
fn recovery_failure(error: RecoveryError) -> Error {
    match error {
        RecoveryError::SecretStorage(SecretStorageError::SecretStorageKey(decode)) => {
            decode_failure(decode)
        }
        // Recovery was set up when the interface asked and is not now. Rare,
        // and the honest answer to it is not "wrong key".
        RecoveryError::SecretStorage(SecretStorageError::MissingKeyInfo { .. }) => {
            Error::NoRecoverySetUp
        }
        RecoveryError::Sdk(error)
        | RecoveryError::SecretStorage(SecretStorageError::Sdk(error)) => Error::Sdk(error),
        // Everything left is the store failing, a secret that will not
        // deserialise, or a backup that already exists, none of which
        // `recover` can produce from bad input. Kept as an SDK error so the
        // text survives into the log.
        other => Error::Sdk(matrix_sdk::Error::UnknownError(Box::new(other))),
    }
}

/// Which half of "that key did not work" this is.
fn decode_failure(error: DecodeError) -> Error {
    match error {
        // Decoded, then failed its own checksum. That is what a genuine key
        // for a different account looks like, and what a wrong passphrase
        // looks like too.
        DecodeError::Mac(_) => Error::WrongRecoveryKey,
        // Never got that far: not base58 at all, the wrong length once
        // decoded, or the wrong prefix or parity byte. Whatever was pasted, it
        // was not one of these.
        DecodeError::Base58(_)
        | DecodeError::Base64(_)
        | DecodeError::KeyLength(..)
        | DecodeError::Parity(..)
        | DecodeError::Prefix(..) => Error::MalformedRecoveryKey,
        // The rest describe the account's own key description rather than the
        // input: a stored MAC or IV of the wrong size, an encryption algorithm
        // this client does not implement, a KDF iteration count nobody should
        // have written down. Telling the person their key is wrong would be a
        // lie, and there is nothing they can retype to fix it.
        other => Error::UnsupportedRecovery(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `DecodeError` of each shape, produced rather than described.
    ///
    /// The variants carry types from three crates below matrix-sdk, one of
    /// which (`MacError`) has no public constructor, so the errors are made by
    /// asking the SDK to decode a key it will refuse. That also keeps the test
    /// honest: it asserts against what the SDK actually returns for a given
    /// input rather than against a variant we assumed it would pick.
    mod classifying {
        use super::*;
        use matrix_sdk::ruma::events::secret_storage::key::{
            SecretStorageEncryptionAlgorithm, SecretStorageKeyEventContent,
            SecretStorageV1AesHmacSha2Properties,
        };
        use matrix_sdk::ruma::serde::Base64;
        use matrix_sdk_base::crypto::secret_storage::SecretStorageKey;

        /// The key description from the SDK's own example, which `RIGHT_KEY`
        /// opens and nothing else does.
        fn key_description() -> SecretStorageKeyEventContent {
            SecretStorageKeyEventContent::new(
                "bmur2d9ypPUH1msSwCxQOJkuKRmJI55e".to_owned(),
                SecretStorageEncryptionAlgorithm::V1AesHmacSha2(
                    SecretStorageV1AesHmacSha2Properties::new(
                        Some(Base64::parse("xv5b6/p3ExEw++wTyfSHEg==").unwrap()),
                        Some(
                            Base64::parse("ujBBbXahnTAMkmPUX2/0+VTfUh63pGyVRuBcDMgmJC8=").unwrap(),
                        ),
                    ),
                ),
            )
        }

        /// The key that opens it, written the way one is shown to a person.
        const RIGHT_KEY: &str = "EsTj 3yST y93F SLpB jJsz eAXc 2XzA ygD3 w69H fGaN TKBj jXEd";

        /// Decode `input` against a description, and classify whatever comes
        /// back the way [`recover`] would.
        fn decoding_against(
            description: SecretStorageKeyEventContent,
            input: &str,
        ) -> std::result::Result<(), Error> {
            SecretStorageKey::from_account_data(input, description)
                .map(|_| ())
                .map_err(decode_failure)
        }

        fn decoding(input: &str) -> std::result::Result<(), Error> {
            decoding_against(key_description(), input)
        }

        #[test]
        fn the_right_key_is_not_an_error_at_all() {
            // The fixture guard. Without it every assertion below could be
            // passing because the description is broken rather than because
            // the input is.
            assert!(decoding(RIGHT_KEY).is_ok());
        }

        #[test]
        fn a_key_for_another_account_is_wrong_rather_than_malformed() {
            // Well-formed base58, right length, right parity, and it opens
            // somebody else's storage. This is the case one "that did not
            // work" message serves worst.
            let other_account = SecretStorageKey::new().to_base58();

            let error = decoding(&other_account).unwrap_err();

            assert!(matches!(error, Error::WrongRecoveryKey), "{error}");
        }

        #[test]
        fn a_wrong_passphrase_is_also_wrong_rather_than_malformed() {
            // An account can take a passphrase instead, and then nothing typed
            // is malformed: any string is a candidate. Answering "that is not
            // a recovery key" would send somebody hunting for a formatting
            // mistake that cannot exist.
            let key = SecretStorageKey::new_from_passphrase("the right passphrase");

            let error = decoding_against(key.event_content().to_owned(), "the wrong passphrase")
                .unwrap_err();

            assert!(matches!(error, Error::WrongRecoveryKey), "{error}");
        }

        #[test]
        fn something_that_is_not_a_key_says_so() {
            for input in [
                "hunter2",
                // Base58 has no 0, O, I or l in its alphabet, so this cannot
                // even decode.
                "0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl 0OIl",
                // Valid base58, far too short once decoded.
                "EsTj3ySTy93F",
            ] {
                let error = decoding(input).unwrap_err();
                assert!(
                    matches!(error, Error::MalformedRecoveryKey),
                    "{input:?} gave {error}",
                );
            }
        }

        #[test]
        fn whitespace_in_a_key_is_insignificant() {
            // The spec says so, and it matters: a key is displayed in groups
            // of four and people paste it as displayed.
            assert!(decoding(&RIGHT_KEY.replace(' ', "")).is_ok());
            assert!(decoding(&format!("  {RIGHT_KEY}\n")).is_ok());
        }
    }

    mod failures {
        use super::*;

        #[test]
        fn losing_secret_storage_between_the_question_and_the_answer_says_so() {
            let error = recovery_failure(RecoveryError::SecretStorage(
                SecretStorageError::MissingKeyInfo { key_id: None },
            ));

            assert!(matches!(error, Error::NoRecoverySetUp), "{error}");
        }

        #[test]
        fn a_homeserver_failure_stays_a_homeserver_failure() {
            // The network half. Every route into this function that is not
            // about the key itself has to keep the SDK's own error, because
            // that is the one with the status code in it.
            for error in [
                RecoveryError::Sdk(matrix_sdk::Error::AuthenticationRequired),
                RecoveryError::SecretStorage(SecretStorageError::Sdk(
                    matrix_sdk::Error::AuthenticationRequired,
                )),
            ] {
                let mapped = recovery_failure(error);
                assert!(matches!(mapped, Error::Sdk(_)), "{mapped}");
            }
        }

        #[test]
        fn a_key_description_this_client_cannot_read_is_not_the_typist_s_fault() {
            // The account's own `m.secret_storage.key.*` event, not the input.
            // An IV of the wrong length makes every key wrong, so telling
            // somebody to check their typing would send them round forever.
            use matrix_sdk::ruma::events::secret_storage::key::{
                SecretStorageEncryptionAlgorithm, SecretStorageKeyEventContent,
                SecretStorageV1AesHmacSha2Properties,
            };
            use matrix_sdk::ruma::serde::Base64;
            use matrix_sdk_base::crypto::secret_storage::SecretStorageKey;

            let truncated_iv = SecretStorageKeyEventContent::new(
                "bmur2d9ypPUH1msSwCxQOJkuKRmJI55e".to_owned(),
                SecretStorageEncryptionAlgorithm::V1AesHmacSha2(
                    SecretStorageV1AesHmacSha2Properties::new(
                        Some(Base64::parse("AAAA").unwrap()),
                        Some(
                            Base64::parse("ujBBbXahnTAMkmPUX2/0+VTfUh63pGyVRuBcDMgmJC8=").unwrap(),
                        ),
                    ),
                ),
            );

            let error = SecretStorageKey::from_account_data(
                "EsTj 3yST y93F SLpB jJsz eAXc 2XzA ygD3 w69H fGaN TKBj jXEd",
                truncated_iv,
            )
            .map(|_| ())
            .map_err(decode_failure)
            .unwrap_err();

            assert!(matches!(error, Error::UnsupportedRecovery(_)), "{error}");
            // And the reason survives into the log, where somebody debugging a
            // homeserver's account data will want it.
            assert!(error.to_string().contains("IV"), "{error}");
        }

        #[test]
        fn a_backup_that_already_exists_is_not_blamed_on_the_key() {
            // Unreachable through `recover`, and the point is that if it ever
            // becomes reachable it does not turn into "your key is wrong".
            let error = recovery_failure(RecoveryError::BackupExistsOnServer);

            assert!(
                !matches!(error, Error::WrongRecoveryKey | Error::MalformedRecoveryKey),
                "{error}"
            );
        }

        #[test]
        fn no_recovery_failure_ever_signs_anybody_out() {
            // Every one of these is a thing to try again, not a session to
            // throw away. Getting this wrong means one typo signs somebody out.
            for error in [
                Error::MalformedRecoveryKey,
                Error::WrongRecoveryKey,
                Error::NoRecoverySetUp,
                Error::RecoveryWithoutIdentity,
                Error::UnsupportedRecovery("m.made.up".to_owned()),
            ] {
                assert!(!error.invalidates_session(), "{error}");
            }
        }
    }
}
