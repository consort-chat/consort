// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The key the SDK's SQLite databases are encrypted with.
//!
//! matrix-sdk encrypts its state and crypto stores, but only when it is handed
//! something to encrypt them with. Handed nothing, it writes the Olm account,
//! every Megolm inbound session, and the cached cross-signing private keys into
//! a SQLite file in the clear. Those are not equivalent to the access token
//! that already goes to the keyring: a stolen token can be revoked by signing
//! the device out, and stolen room keys decrypt every message they cover for
//! as long as the messages exist.
//!
//! ## Why a key rather than a passphrase
//!
//! The SDK takes either. A passphrase is run through 200,000 rounds of PBKDF2
//! first, which is the right thing to do to a secret a person chose and an
//! attacker could guess. This one is 32 bytes straight from the operating
//! system's CSPRNG, so there is nothing to stretch: the KDF would cost a
//! noticeable pause on every launch and buy exactly no entropy.
//!
//! ## What this is not
//!
//! Protection from the user's own account. The key lives in the same keyring
//! as the access token, so anything running as the user that can ask the
//! keyring can have both. What it stops is everything that reads files without
//! going through the keyring: a backup that swept up the data directory, a
//! recovered disk, a stray `chmod`, another account on a shared machine, and
//! on Windows and macOS every other process the user runs, none of which face
//! a mode bit at all.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngCore;

/// How many bytes the SDK's store cipher takes. Not ours to choose.
const KEY_BYTES: usize = 32;

/// The key one account's SQLite stores are encrypted with.
#[derive(Clone)]
pub struct StoreKey([u8; KEY_BYTES]);

/// Written by hand for the same reason `Credentials` is: a derived one prints
/// the key, and this type is a field of [`crate::StoredSession`], which is the
/// kind of thing that ends up inside a `tracing::debug!` during an afternoon of
/// debugging and stays in the journal afterwards.
impl std::fmt::Debug for StoreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreKey").finish_non_exhaustive()
    }
}

impl StoreKey {
    /// A new key from the operating system's randomness.
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_BYTES];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// The bytes, in the shape `SqliteStoreConfig::key` wants.
    pub fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// How the key is written into the secret store.
    pub fn encode(&self) -> String {
        STANDARD.encode(self.0)
    }

    /// Read a key back, or `None` if what came out of the store is not one.
    ///
    /// `None` rather than an error because there is only one thing to do about
    /// it, and it is the same thing an absent key calls for: the store this
    /// key belonged to cannot be opened, so the session it belonged to is over.
    /// Signing in again discards that store and builds a fresh one.
    pub fn decode(encoded: &str) -> Option<Self> {
        let bytes = STANDARD.decode(encoded).ok()?;
        Some(Self(bytes.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_survives_a_round_trip_through_the_secret_store_encoding() {
        let key = StoreKey::generate();

        let back = StoreKey::decode(&key.encode()).expect("its own encoding decodes");

        assert_eq!(back.as_bytes(), key.as_bytes());
    }

    #[test]
    fn two_generated_keys_differ() {
        assert_ne!(
            StoreKey::generate().as_bytes(),
            StoreKey::generate().as_bytes()
        );
    }

    #[test]
    fn a_generated_key_is_not_all_zeroes() {
        // The failure mode of a randomness source that is not wired up. It
        // would still round-trip, still be the right length, and encrypt
        // everything under a key an attacker already has.
        assert_ne!(StoreKey::generate().as_bytes(), &[0u8; KEY_BYTES]);
    }

    #[test]
    fn what_is_not_base64_is_not_a_key() {
        assert!(StoreKey::decode("not base64!").is_none());
    }

    #[test]
    fn base64_of_the_wrong_length_is_not_a_key() {
        assert!(StoreKey::decode(&STANDARD.encode([0u8; 16])).is_none());
        assert!(StoreKey::decode(&STANDARD.encode([0u8; 64])).is_none());
        assert!(StoreKey::decode("").is_none());
    }

    #[test]
    fn debug_never_prints_the_key() {
        // The regression guard. A derived Debug fails this immediately.
        let key = StoreKey([7u8; KEY_BYTES]);

        let rendered = format!("{key:?}");

        assert!(!rendered.contains('7'));
        assert_eq!(rendered, "StoreKey { .. }");
    }
}
