// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Persisting a login across restarts.
//!
//! Two things have to survive a restart, and they live in different places.
//! The SDK's own state and crypto stores are a SQLite database the SDK manages;
//! we only choose the directory. The access token and device ID are ours to
//! keep, and they go in a small JSON file next to it.
//!
//! ## On storing the access token in a file
//!
//! That token is a bearer credential for the account. It is written with mode
//! `0600` on Unix, inside the OS per-user application data directory, which is
//! the same protection the SDK's own SQLite store gets and the same choice most
//! desktop Matrix clients make today.
//!
//! It is still weaker than the OS keyring, and moving it there is tracked as a
//! known limitation in the README rather than quietly ignored. The reason it is
//! not the default yet is that the keyring backends on Linux (Secret Service,
//! kwallet) are not always running, and a first launch that fails on a missing
//! keyring daemon is worse than this for the audience the project has today.

use std::fs;
use std::path::{Path, PathBuf};

use matrix_sdk::authentication::matrix::MatrixSession;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Everything needed to bring a logged-in client back without a password.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredSession {
    /// The resolved homeserver URL, kept so a restore skips well-known
    /// discovery. Discovery needs the network, and a restore should work on a
    /// flaky connection when the token is still valid.
    pub homeserver: String,
    /// Directory holding the SDK's SQLite state and crypto stores.
    pub store_path: PathBuf,
    /// User ID, device ID, and the access/refresh tokens.
    pub session: MatrixSession,
}

/// Reads and writes the single active [`StoredSession`].
///
/// One session at a time. Multi-account is a real feature with real UI
/// consequences, and pretending to support it here with a map would be
/// speculative structure for a screen that does not exist.
#[derive(Clone, Debug)]
pub struct SessionStore {
    data_dir: PathBuf,
}

impl SessionStore {
    /// Point the store at an application data directory. The directory is
    /// created on first write, not here, so constructing this cannot fail.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Path of the session JSON file.
    pub fn session_file(&self) -> PathBuf {
        self.data_dir.join("session.json")
    }

    /// Where the SDK's SQLite stores for an account belong.
    ///
    /// `account_key` is any stable string identifying the account. It is hashed
    /// rather than used directly for two reasons: a Matrix user ID contains `:`
    /// and can contain characters Windows rejects in a path component, and the
    /// key is derived partly from user input that has not been validated as a
    /// safe path fragment.
    ///
    /// This must be computable *before* login, because the SDK needs its crypto
    /// store on the client that performs the login. Logging in on a storeless
    /// client and copying the session to a second one loses the device keys the
    /// first client already uploaded, leaving a device on the server whose
    /// private keys no longer exist anywhere.
    pub fn store_path_for(&self, account_key: &str) -> PathBuf {
        let digest = Sha256::digest(account_key.as_bytes());
        let short = digest[..8].iter().fold(String::new(), |mut acc, byte| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{byte:02x}");
            acc
        });
        self.data_dir.join("accounts").join(short)
    }

    /// Load the stored session, if there is one.
    ///
    /// A missing file is `Ok(None)`, not an error: a first launch is the normal
    /// case, not a failure.
    pub fn load(&self) -> Result<Option<StoredSession>> {
        let path = self.session_file();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::SessionStore { path, source }),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(Error::CorruptSession)
    }

    /// Write the session, replacing any existing one.
    ///
    /// Writes to a temporary file and renames, so a crash midway leaves the
    /// previous session intact instead of a truncated file that fails to parse
    /// and logs the user out.
    pub fn save(&self, session: &StoredSession) -> Result<()> {
        let path = self.session_file();
        self.ensure_dir(&self.data_dir)?;

        let json = serde_json::to_vec_pretty(session)
            .expect("StoredSession is plain data and always serialises");

        let temp = path.with_extension("json.tmp");
        fs::write(&temp, &json).map_err(|source| Error::SessionStore {
            path: temp.clone(),
            source,
        })?;
        restrict_permissions(&temp)?;
        fs::rename(&temp, &path).map_err(|source| Error::SessionStore { path, source })
    }

    /// Remove the stored session. Missing is success, since the caller wanted
    /// it gone and it is gone.
    ///
    /// Leaves the SQLite stores alone on purpose. They hold the device's
    /// Megolm keys, and deleting them makes previously readable history
    /// permanently undecryptable for that device. Signing out is not a request
    /// to destroy message history.
    pub fn clear(&self) -> Result<()> {
        let path = self.session_file();
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::SessionStore { path, source }),
        }
    }

    fn ensure_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).map_err(|source| Error::SessionStore {
            path: dir.to_path_buf(),
            source,
        })
    }
}

/// Owner-only permissions on the file holding the access token.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        Error::SessionStore {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// No-op on platforms without Unix mode bits. Windows inherits the ACL of the
/// per-user AppData directory, which is already owner-scoped.
#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
