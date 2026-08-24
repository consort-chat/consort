// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Where the access token actually lives.
//!
//! The access token is a bearer credential for the whole account: anyone
//! holding it can read messages, send them, and add devices. Every desktop
//! platform ships an encrypted store designed for exactly this, so that is the
//! default:
//!
//! - Linux and the BSDs: the Secret Service API, which is what GNOME Keyring
//!   and KWallet both implement.
//! - Windows: the Windows Credential Manager.
//! - macOS: Keychain Services.
//!
//! ## Why there is still a file fallback
//!
//! Secret Service is a DBus service, not a kernel feature, and it is genuinely
//! absent on some Linux systems: a bare window manager with no session daemon,
//! a container, an SSH session with no DBus, a distribution that never
//! installed a keyring. On those machines a keyring-only client cannot log in
//! at all.
//!
//! Refusing to start is the wrong answer, and so is silently degrading. What
//! happens instead: [`preferred`] asks the platform store whether it is
//! actually available, uses it when it is, and otherwise falls back to an
//! owner-only file. Which one was chosen is recorded in [`Backend::kind`], is
//! logged at startup, and is reported to the UI, so the weaker choice is
//! visible rather than assumed.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::atomic;
use crate::error::{Error, Result};

mod keyring_store;

pub use keyring_store::KeyringBackend;

/// Which store a [`Backend`] is talking to.
///
/// Reported to the user, so the wording is the wording that ends up on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    /// The platform's encrypted credential store.
    Keyring,
    /// An owner-only file, because no keyring was reachable.
    File,
    /// Memory only. Tests, and nothing else.
    Memory,
}

impl BackendKind {
    /// One sentence, for a person, about where their token is being kept.
    pub fn description(self) -> &'static str {
        match self {
            Self::Keyring => "Your sign-in is stored in your system keyring.",
            Self::File => {
                "No system keyring was available, so your sign-in is stored in a \
                 file that only your user account can read."
            }
            Self::Memory => "Your sign-in is not being stored.",
        }
    }

    /// Whether this is the store we would have picked given the choice.
    pub fn is_preferred(self) -> bool {
        matches!(self, Self::Keyring)
    }
}

/// Somewhere a small named secret can be kept.
///
/// A trait rather than an enum because the file and memory implementations are
/// exercised directly by tests while the keyring one needs a running desktop
/// session. Keeping the seam here is what lets the session logic above be
/// tested without one.
pub trait Backend: fmt::Debug + Send + Sync {
    /// Store `value` under `key`, replacing any previous value.
    fn set(&self, key: &str, value: &str) -> Result<()>;

    /// Read `key`, or `Ok(None)` if it was never set.
    fn get(&self, key: &str) -> Result<Option<String>>;

    /// Remove `key`. Removing something absent is success.
    fn delete(&self, key: &str) -> Result<()>;

    /// Which store this is.
    fn kind(&self) -> BackendKind;
}

/// The best available backend for this machine.
///
/// `service` names the application to the platform store. `fallback_dir` is
/// only touched if no keyring answers.
pub fn preferred(service: &str, fallback_dir: impl Into<PathBuf>) -> Arc<dyn Backend> {
    match KeyringBackend::available(service) {
        Ok(backend) => {
            tracing::info!("storing the session in the system keyring");
            Arc::new(backend)
        }
        Err(error) => {
            // Not a warning at the point of failure only. The choice is also
            // reported to the UI, because a user on a machine with no keyring
            // should be told, not left to assume.
            tracing::warn!(
                %error,
                "no system keyring is available; falling back to an owner-only file"
            );
            Arc::new(FileBackend::new(fallback_dir))
        }
    }
}

/// Secrets in owner-only files, one per key.
///
/// Used when no platform keyring answered. Each key gets its own file so that
/// removing one cannot disturb another, and every write goes through
/// [`crate::atomic::write_private`].
#[derive(Clone, Debug)]
pub struct FileBackend {
    dir: PathBuf,
}

impl FileBackend {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The file backing `key`.
    ///
    /// Keys are ours, not user input, but they are still encoded rather than
    /// used raw: a key containing `/` or `..` would otherwise escape the
    /// directory, and that is a bug worth making impossible instead of
    /// remembering not to write.
    fn path_for(&self, key: &str) -> PathBuf {
        let encoded: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let digest = short_digest(key);
        self.dir.join(format!("{encoded}.{digest}.secret"))
    }

    /// The directory this backend writes into.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Backend for FileBackend {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        atomic::write_private(&self.path_for(key), value.as_bytes(), "secret")
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let path = self.path_for(key);
        match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::SecretStore {
                    backend: "file",
                    message: format!("the secret at {} is not valid UTF-8", path.display()),
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::secret_file(&path, source)),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        atomic::remove_if_present(&self.path_for(key))
    }

    fn kind(&self) -> BackendKind {
        BackendKind::File
    }
}

/// Secrets held in memory for the life of the process.
///
/// For tests, and for nothing else. Deliberately public so that integration
/// tests outside this crate can use it too.
#[derive(Debug, Default)]
pub struct MemoryBackend {
    entries: Mutex<BTreeMap<String, String>>,
    /// When set, every call fails with this message. Lets a test drive the
    /// error paths that a real store only reaches when the desktop session
    /// dies mid-write.
    failure: Mutex<Option<String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make every subsequent call fail.
    pub fn start_failing(&self, message: impl Into<String>) {
        *self.failure.lock().expect("not poisoned") = Some(message.into());
    }

    /// Number of secrets currently held.
    pub fn len(&self) -> usize {
        self.entries.lock().expect("not poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn check(&self) -> Result<()> {
        match self.failure.lock().expect("not poisoned").clone() {
            Some(message) => Err(Error::SecretStore {
                backend: "memory",
                message,
            }),
            None => Ok(()),
        }
    }
}

impl Backend for MemoryBackend {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.check()?;
        self.entries
            .lock()
            .expect("not poisoned")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        self.check()?;
        Ok(self.entries.lock().expect("not poisoned").get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.check()?;
        self.entries.lock().expect("not poisoned").remove(key);
        Ok(())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Memory
    }
}

/// First eight bytes of a SHA-256, hex encoded.
///
/// Shared by the file backend and the session store, which both need a short
/// filesystem-safe stand-in for a string that is not filesystem-safe.
pub(crate) fn short_digest(input: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let digest = Sha256::digest(input.as_bytes());
    digest[..8].iter().fold(String::new(), |mut acc, byte| {
        let _ = write!(acc, "{byte:02x}");
        acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_backend() -> (tempfile::TempDir, FileBackend) {
        let dir = tempfile::tempdir().unwrap();
        let backend = FileBackend::new(dir.path());
        (dir, backend)
    }

    #[test]
    fn a_file_secret_round_trips() {
        let (_dir, backend) = file_backend();

        backend.set("token", "s3cret").unwrap();

        assert_eq!(backend.get("token").unwrap().as_deref(), Some("s3cret"));
    }

    #[test]
    fn a_missing_file_secret_is_none_not_an_error() {
        let (_dir, backend) = file_backend();
        assert_eq!(backend.get("absent").unwrap(), None);
    }

    #[test]
    fn setting_a_file_secret_twice_replaces_it() {
        let (_dir, backend) = file_backend();

        backend.set("token", "first").unwrap();
        backend.set("token", "second").unwrap();

        assert_eq!(backend.get("token").unwrap().as_deref(), Some("second"));
    }

    #[test]
    fn deleting_a_file_secret_removes_it() {
        let (_dir, backend) = file_backend();
        backend.set("token", "s3cret").unwrap();

        backend.delete("token").unwrap();

        assert_eq!(backend.get("token").unwrap(), None);
    }

    #[test]
    fn deleting_an_absent_file_secret_succeeds() {
        let (_dir, backend) = file_backend();
        backend.delete("absent").unwrap();
    }

    #[test]
    fn separate_keys_do_not_collide() {
        let (_dir, backend) = file_backend();

        backend.set("alice", "one").unwrap();
        backend.set("bob", "two").unwrap();
        backend.delete("alice").unwrap();

        assert_eq!(backend.get("bob").unwrap().as_deref(), Some("two"));
    }

    #[test]
    #[cfg(unix)]
    fn a_file_secret_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, backend) = file_backend();
        backend.set("token", "s3cret").unwrap();

        let mode = std::fs::metadata(backend.path_for("token"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_key_containing_path_separators_cannot_escape_the_directory() {
        let (dir, backend) = file_backend();

        backend.set("../../escaped", "s3cret").unwrap();

        let path = backend.path_for("../../escaped");
        assert_eq!(path.parent().unwrap(), dir.path());
        assert!(!path.to_string_lossy().contains(".."));
        assert_eq!(
            backend.get("../../escaped").unwrap().as_deref(),
            Some("s3cret")
        );
    }

    #[test]
    fn keys_that_encode_to_the_same_name_still_get_different_files() {
        // `a/b` and `a_b` both sanitise to `a_b`. The digest suffix is what
        // stops one overwriting the other.
        let (_dir, backend) = file_backend();

        backend.set("a/b", "one").unwrap();
        backend.set("a_b", "two").unwrap();

        assert_eq!(backend.get("a/b").unwrap().as_deref(), Some("one"));
        assert_eq!(backend.get("a_b").unwrap().as_deref(), Some("two"));
    }

    #[test]
    fn a_file_secret_that_is_not_utf8_is_an_error_not_a_panic() {
        let (_dir, backend) = file_backend();
        backend.set("token", "placeholder").unwrap();
        std::fs::write(backend.path_for("token"), [0xff, 0xfe]).unwrap();

        let error = backend.get("token").unwrap_err();

        assert!(error.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn the_file_backend_reports_its_kind() {
        let (_dir, backend) = file_backend();
        assert_eq!(backend.kind(), BackendKind::File);
        assert!(!backend.kind().is_preferred());
    }

    #[test]
    fn the_file_backend_exposes_its_directory() {
        let (dir, backend) = file_backend();
        assert_eq!(backend.dir(), dir.path());
    }

    #[test]
    fn a_memory_secret_round_trips() {
        let backend = MemoryBackend::new();

        backend.set("token", "s3cret").unwrap();

        assert_eq!(backend.get("token").unwrap().as_deref(), Some("s3cret"));
        assert_eq!(backend.len(), 1);
        assert!(!backend.is_empty());
    }

    #[test]
    fn a_memory_backend_starts_empty() {
        let backend = MemoryBackend::default();
        assert!(backend.is_empty());
        assert_eq!(backend.get("token").unwrap(), None);
    }

    #[test]
    fn deleting_a_memory_secret_removes_it() {
        let backend = MemoryBackend::new();
        backend.set("token", "s3cret").unwrap();

        backend.delete("token").unwrap();

        assert!(backend.is_empty());
    }

    #[test]
    fn a_failing_memory_backend_reports_every_call() {
        let backend = MemoryBackend::new();
        backend.start_failing("the session bus went away");

        assert!(backend.set("token", "s3cret").is_err());
        assert!(backend.get("token").is_err());
        assert!(backend.delete("token").is_err());
    }

    #[test]
    fn the_memory_backend_reports_its_kind() {
        assert_eq!(MemoryBackend::new().kind(), BackendKind::Memory);
    }

    #[test]
    fn every_backend_kind_has_a_description_for_a_person() {
        for kind in [BackendKind::Keyring, BackendKind::File, BackendKind::Memory] {
            let description = kind.description();
            assert!(!description.is_empty());
            assert!(description.ends_with('.'), "{kind:?}: {description}");
        }
    }

    #[test]
    fn only_the_keyring_counts_as_the_preferred_store() {
        assert!(BackendKind::Keyring.is_preferred());
        assert!(!BackendKind::File.is_preferred());
        assert!(!BackendKind::Memory.is_preferred());
    }

    #[test]
    fn backend_kind_serialises_for_the_ui() {
        let json = serde_json::to_string(&BackendKind::Keyring).unwrap();
        assert_eq!(json, "\"keyring\"");
        let back: BackendKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, BackendKind::Keyring);
    }

    #[test]
    fn the_digest_is_stable_and_distinguishes_inputs() {
        assert_eq!(short_digest("alice"), short_digest("alice"));
        assert_ne!(short_digest("alice"), short_digest("bob"));
        assert_eq!(short_digest("alice").len(), 16);
    }

    #[test]
    fn preferred_always_returns_a_usable_backend() {
        // On a CI runner there is no Secret Service, so this exercises the
        // fallback. On a developer desktop it exercises the keyring. Either
        // way the contract is the same: something usable comes back.
        let dir = tempfile::tempdir().unwrap();
        let backend = preferred("consort-test-preferred", dir.path());

        backend.set("probe", "value").unwrap();
        assert_eq!(backend.get("probe").unwrap().as_deref(), Some("value"));
        backend.delete("probe").unwrap();
        assert_eq!(backend.get("probe").unwrap(), None);
    }
}
