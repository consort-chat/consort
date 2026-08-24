// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The platform keyring, behind [`Backend`].
//!
//! Thin on purpose. Everything here is a translation of one `keyring` call
//! into our error type, and there is no logic to get wrong that a test could
//! catch. It is a separate file so that coverage can exclude it honestly
//! rather than by pretending a mock exercised the real Secret Service.
//!
//! The tests at the bottom are `#[ignore]`d because they need a live desktop
//! session. Run them on a developer machine with:
//!
//! ```text
//! cargo test -p consort-matrix -- --ignored keyring
//! ```

use keyring::Entry;

use crate::error::{Error, Result};
use crate::secrets::{Backend, BackendKind};

/// Secrets in the platform's encrypted credential store.
#[derive(Clone, Debug)]
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    /// Construct one, but only if the platform store actually answers.
    ///
    /// `keyring` initialises its store lazily on first use, and
    /// `Entry::store_status` forces and reports that initialisation. Asking up
    /// front is what lets [`super::preferred`] choose the fallback at startup
    /// rather than discovering the problem during a login.
    pub fn available(service: &str) -> Result<Self> {
        match Entry::store_status() {
            Ok(()) => Ok(Self {
                service: service.to_owned(),
            }),
            Err(error) => Err(Error::SecretStore {
                backend: "keyring",
                message: error.to_string(),
            }),
        }
    }

    fn entry(&self, key: &str) -> Result<Entry> {
        Entry::new(&self.service, key).map_err(|error| Error::SecretStore {
            backend: "keyring",
            message: error.to_string(),
        })
    }
}

impl Backend for KeyringBackend {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|error| Error::SecretStore {
                backend: "keyring",
                message: error.to_string(),
            })
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            // Never stored, or already deleted. Not a failure.
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(Error::SecretStore {
                backend: "keyring",
                message: error.to_string(),
            }),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(Error::SecretStore {
                backend: "keyring",
                message: error.to_string(),
            }),
        }
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Keyring
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVICE: &str = "chat.consort.desktop.test";

    #[test]
    #[ignore = "needs a live platform keyring"]
    fn keyring_round_trips_a_secret() {
        let backend = KeyringBackend::available(SERVICE).unwrap();
        backend.set("round-trip", "s3cret").unwrap();

        assert_eq!(
            backend.get("round-trip").unwrap().as_deref(),
            Some("s3cret")
        );

        backend.delete("round-trip").unwrap();
        assert_eq!(backend.get("round-trip").unwrap(), None);
    }

    #[test]
    #[ignore = "needs a live platform keyring"]
    fn keyring_reports_a_missing_entry_as_none() {
        let backend = KeyringBackend::available(SERVICE).unwrap();
        assert_eq!(backend.get("never-set-by-any-test").unwrap(), None);
    }

    #[test]
    #[ignore = "needs a live platform keyring"]
    fn keyring_deleting_an_absent_entry_succeeds() {
        let backend = KeyringBackend::available(SERVICE).unwrap();
        backend.delete("never-set-by-any-test").unwrap();
    }

    #[test]
    #[ignore = "needs a live platform keyring"]
    fn keyring_reports_its_kind() {
        let backend = KeyringBackend::available(SERVICE).unwrap();
        assert_eq!(backend.kind(), BackendKind::Keyring);
        assert!(backend.kind().is_preferred());
    }
}
