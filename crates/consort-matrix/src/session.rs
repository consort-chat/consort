// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Persisting a login across restarts.
//!
//! Three things have to survive a restart and they live in three places.
//!
//! The SDK's own state and crypto stores are a SQLite database the SDK
//! manages; we only choose the directory. The access and refresh tokens go to
//! the platform keyring, or to an owner-only file when no keyring is reachable,
//! which is [`crate::secrets`]. Everything else, meaning the homeserver URL,
//! the store directory, the user ID and the device ID, is not secret and goes
//! in a small JSON file.
//!
//! ## Why the split
//!
//! Keeping the whole `MatrixSession` in one JSON blob would mean putting the
//! access token wherever that blob goes. Splitting the tokens out is what lets
//! the non-secret half stay a readable file you can inspect while debugging,
//! and the secret half go somewhere encrypted.
//!
//! The split is also what makes a token refresh cheap. The SDK rotates tokens
//! on its own once `handle_refresh_tokens` is on, and only the keyring entry
//! has to be rewritten when it does.

use std::path::PathBuf;

use matrix_sdk::authentication::SessionTokens;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::ruma::{OwnedDeviceId, OwnedUserId};
use matrix_sdk::{SessionMeta, ruma};
use serde::{Deserialize, Serialize};

use crate::atomic;
use crate::error::{Error, Result};
use crate::secrets::{Backend, BackendKind, short_digest};

/// Name Consort gives itself to the platform credential store.
///
/// Matches the Tauri bundle identifier so the entry is recognisable in
/// Seahorse, KWalletManager or Keychain Access rather than showing up as an
/// anonymous blob.
pub const KEYRING_SERVICE: &str = "chat.consort.desktop";

/// Everything needed to bring a logged-in client back without a password.
#[derive(Clone, Debug)]
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

/// The half of a session that is not a secret.
///
/// Serialised to `session.json`. Deliberately contains no token: if this file
/// leaks, it tells an attacker which account is signed in and nothing that lets
/// them act as it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SessionMetadata {
    homeserver: String,
    store_path: PathBuf,
    user_id: String,
    device_id: String,
    /// Which secret store held the tokens when this was written. Advisory: the
    /// tokens are looked up the same way regardless. It exists so a machine
    /// that had a keyring yesterday and does not today produces a log line
    /// that explains the sign-out instead of a mystery.
    #[serde(default)]
    token_store: Option<BackendKind>,
}

/// Reads and writes the single active [`StoredSession`].
///
/// One session at a time. Multi-account is a real feature with real UI
/// consequences, and pretending to support it here with a map would be
/// speculative structure for a screen that does not exist. The pieces are
/// shaped so it stays possible: [`Self::store_path_for`] already gives every
/// account its own SQLite directory, and secrets are keyed per user ID.
#[derive(Clone)]
pub struct SessionStore {
    data_dir: PathBuf,
    secrets: std::sync::Arc<dyn Backend>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("data_dir", &self.data_dir)
            .field("secrets", &self.secrets.kind())
            .finish()
    }
}

impl SessionStore {
    /// Point the store at an application data directory, choosing the best
    /// secret backend this machine offers.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let secrets = crate::secrets::preferred(KEYRING_SERVICE, data_dir.join("secrets"));
        Self { data_dir, secrets }
    }

    /// Point the store at a directory with an explicit secret backend.
    ///
    /// This is the constructor tests use, with
    /// [`crate::secrets::MemoryBackend`], so that running the suite never
    /// touches the developer's real keyring.
    pub fn with_backend(
        data_dir: impl Into<PathBuf>,
        secrets: std::sync::Arc<dyn Backend>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            secrets,
        }
    }

    /// Where the tokens are being kept, for logging and for the UI.
    pub fn backend_kind(&self) -> BackendKind {
        self.secrets.kind()
    }

    /// Path of the session metadata file.
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
        self.data_dir
            .join("accounts")
            .join(short_digest(account_key))
    }

    /// Load the stored session, if there is one.
    ///
    /// A missing file is `Ok(None)`, not an error: a first launch is the normal
    /// case, not a failure. Metadata present with no matching tokens is also
    /// `Ok(None)`, which is what a keyring cleared behind our back looks like.
    pub fn load(&self) -> Result<Option<StoredSession>> {
        let path = self.session_file();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::secret_file(&path, source)),
        };

        let metadata: SessionMetadata =
            serde_json::from_slice(&bytes).map_err(Error::CorruptSession)?;

        let user_id = parse_user_id(&metadata.user_id)?;
        let device_id: OwnedDeviceId = metadata.device_id.as_str().into();

        let Some(tokens) = self.load_tokens(&metadata.user_id)? else {
            tracing::warn!(
                user_id = %metadata.user_id,
                stored_in = ?metadata.token_store,
                looked_in = ?self.secrets.kind(),
                "session metadata exists but no tokens were found for it"
            );
            return Ok(None);
        };

        Ok(Some(StoredSession {
            homeserver: metadata.homeserver,
            store_path: metadata.store_path,
            session: MatrixSession {
                meta: SessionMeta { user_id, device_id },
                tokens,
            },
        }))
    }

    /// Write the session, replacing any existing one.
    ///
    /// Tokens go first. If the metadata write then fails there is an orphaned
    /// keyring entry, which is harmless and gets overwritten by the next login.
    /// The other order would leave metadata pointing at tokens that do not
    /// exist, which reads as a corrupt session.
    pub fn save(&self, session: &StoredSession) -> Result<()> {
        let user_id = session.session.meta.user_id.to_string();

        self.save_tokens(&user_id, &session.session.tokens)?;

        let metadata = SessionMetadata {
            homeserver: session.homeserver.clone(),
            store_path: session.store_path.clone(),
            user_id,
            device_id: session.session.meta.device_id.to_string(),
            token_store: Some(self.secrets.kind()),
        };

        let json = serde_json::to_vec_pretty(&metadata)
            .expect("SessionMetadata is plain data and always serialises");

        // The metadata holds no secret, but it is written the same way as one:
        // atomically, so a crash cannot leave a truncated file that fails to
        // parse and signs the user out.
        atomic::write_private(&self.session_file(), &json, "session")
    }

    /// Replace only the tokens, leaving the metadata alone.
    ///
    /// Called when the SDK rotates a token. Refresh tokens are commonly
    /// single-use, so a rotation that is not persisted leaves a stored session
    /// that cannot be refreshed and cannot be used, which surfaces to the user
    /// as being logged out for no reason after an idle period.
    pub fn save_tokens(&self, user_id: &str, tokens: &SessionTokens) -> Result<()> {
        let json = serde_json::to_string(tokens)
            .expect("SessionTokens is plain data and always serialises");
        self.secrets.set(&token_key(user_id), &json)
    }

    /// Read the tokens for an account, if they are there.
    pub fn load_tokens(&self, user_id: &str) -> Result<Option<SessionTokens>> {
        let Some(json) = self.secrets.get(&token_key(user_id))? else {
            return Ok(None);
        };
        serde_json::from_str(&json)
            .map(Some)
            .map_err(Error::CorruptSession)
    }

    /// Remove the stored session, metadata and tokens both.
    ///
    /// Missing is success, since the caller wanted it gone and it is gone.
    ///
    /// Leaves the SQLite stores alone on purpose. They hold the device's
    /// Megolm keys, and deleting them makes previously readable history
    /// permanently undecryptable for that device. Signing out is not a request
    /// to destroy message history.
    pub fn clear(&self) -> Result<()> {
        // Read the metadata before removing it, because it names the account
        // whose tokens have to go. A metadata file we cannot parse still gets
        // deleted: leaving it would fail the same way on every launch.
        let user_id = match self.read_metadata() {
            Ok(Some(metadata)) => Some(metadata.user_id),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error, "clearing an unreadable session file");
                None
            }
        };

        atomic::remove_if_present(&self.session_file())?;

        if let Some(user_id) = user_id {
            self.secrets.delete(&token_key(&user_id))?;
        }

        Ok(())
    }

    fn read_metadata(&self) -> Result<Option<SessionMetadata>> {
        let path = self.session_file();
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(Error::CorruptSession),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::secret_file(&path, source)),
        }
    }
}

/// The key an account's tokens are filed under.
fn token_key(user_id: &str) -> String {
    format!("session-tokens:{user_id}")
}

fn parse_user_id(value: &str) -> Result<OwnedUserId> {
    ruma::UserId::parse(value).map_err(|_| Error::InvalidStoredIdentifier {
        field: "user_id",
        value: value.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;
    use std::sync::Arc;

    fn tokens(access: &str) -> SessionTokens {
        SessionTokens {
            access_token: access.to_owned(),
            refresh_token: Some(format!("{access}-refresh")),
        }
    }

    fn session(user: &str) -> StoredSession {
        StoredSession {
            homeserver: "https://example.org/".to_owned(),
            store_path: PathBuf::from("/tmp/consort/accounts/abcd"),
            session: MatrixSession {
                meta: SessionMeta {
                    user_id: ruma::UserId::parse(user).unwrap(),
                    device_id: "HZTIUXZKUU".into(),
                },
                tokens: tokens("syt_access"),
            },
        }
    }

    fn store() -> (tempfile::TempDir, SessionStore, Arc<MemoryBackend>) {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(MemoryBackend::new());
        let store = SessionStore::with_backend(dir.path(), backend.clone());
        (dir, store, backend)
    }

    #[test]
    fn a_session_round_trips() {
        let (_dir, store, _) = store();
        let original = session("@bob:example.org");

        store.save(&original).unwrap();
        let loaded = store.load().unwrap().expect("a session was saved");

        assert_eq!(loaded.homeserver, original.homeserver);
        assert_eq!(loaded.store_path, original.store_path);
        assert_eq!(loaded.session.meta.user_id, original.session.meta.user_id);
        assert_eq!(
            loaded.session.meta.device_id,
            original.session.meta.device_id
        );
        assert_eq!(
            loaded.session.tokens.access_token,
            original.session.tokens.access_token
        );
        assert_eq!(
            loaded.session.tokens.refresh_token,
            original.session.tokens.refresh_token
        );
    }

    #[test]
    fn no_session_file_means_no_session() {
        let (_dir, store, _) = store();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn the_access_token_is_never_written_to_the_metadata_file() {
        // The entire reason the split exists. If a refactor ever puts the
        // whole MatrixSession back in one blob, this fails.
        let (_dir, store, _) = store();
        store.save(&session("@bob:example.org")).unwrap();

        let on_disk = std::fs::read_to_string(store.session_file()).unwrap();

        assert!(!on_disk.contains("syt_access"));
        assert!(!on_disk.contains("syt_access-refresh"));
        assert!(!on_disk.contains("access_token"));
    }

    #[test]
    fn the_metadata_file_does_hold_the_non_secret_fields() {
        let (_dir, store, _) = store();
        store.save(&session("@bob:example.org")).unwrap();

        let on_disk = std::fs::read_to_string(store.session_file()).unwrap();

        assert!(on_disk.contains("@bob:example.org"));
        assert!(on_disk.contains("HZTIUXZKUU"));
        assert!(on_disk.contains("https://example.org/"));
    }

    #[test]
    fn the_tokens_go_to_the_secret_backend() {
        let (_dir, store, backend) = store();

        store.save(&session("@bob:example.org")).unwrap();

        assert_eq!(backend.len(), 1);
        let stored = backend
            .get("session-tokens:@bob:example.org")
            .unwrap()
            .expect("tokens were stored under the user id");
        assert!(stored.contains("syt_access"));
    }

    #[test]
    #[cfg(unix)]
    fn the_metadata_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, store, _) = store();
        store.save(&session("@bob:example.org")).unwrap();

        let mode = std::fs::metadata(store.session_file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn saving_twice_replaces_rather_than_appends() {
        let (_dir, store, backend) = store();
        store.save(&session("@bob:example.org")).unwrap();

        let mut second = session("@bob:example.org");
        second.session.tokens = tokens("syt_second");
        store.save(&second).unwrap();

        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.session.tokens.access_token, "syt_second");
        assert_eq!(backend.len(), 1);
    }

    #[test]
    fn metadata_without_tokens_is_treated_as_signed_out() {
        // What a cleared keyring looks like. Reporting "no session" sends the
        // user to the login form, which is the only useful screen.
        let (_dir, store, backend) = store();
        store.save(&session("@bob:example.org")).unwrap();

        backend.delete("session-tokens:@bob:example.org").unwrap();

        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn a_corrupt_metadata_file_is_an_error_that_invalidates_the_session() {
        let (_dir, store, _) = store();
        atomic::write_private(&store.session_file(), b"{ not json", "session").unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(error, Error::CorruptSession(_)));
        assert!(error.invalidates_session());
    }

    #[test]
    fn a_metadata_file_with_an_unparseable_user_id_is_reported_as_such() {
        let (_dir, store, _) = store();
        let json = br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"not-a-user-id","device_id":"DEV"}"#;
        atomic::write_private(&store.session_file(), json, "session").unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(
            error,
            Error::InvalidStoredIdentifier {
                field: "user_id",
                ..
            }
        ));
        assert!(error.invalidates_session());
    }

    #[test]
    fn a_metadata_file_written_before_token_store_was_recorded_still_loads() {
        // Forward compatibility with the first release, which had no
        // `token_store` field. serde(default) covers it; this proves it.
        let (_dir, store, backend) = store();
        backend
            .set(
                "session-tokens:@bob:example.org",
                r#"{"access_token":"syt_old"}"#,
            )
            .unwrap();
        let json = br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#;
        atomic::write_private(&store.session_file(), json, "session").unwrap();

        let loaded = store.load().unwrap().expect("should still load");

        assert_eq!(loaded.session.tokens.access_token, "syt_old");
        assert_eq!(loaded.session.tokens.refresh_token, None);
    }

    #[test]
    fn corrupt_tokens_are_reported_rather_than_silently_dropped() {
        let (_dir, store, backend) = store();
        store.save(&session("@bob:example.org")).unwrap();
        backend
            .set("session-tokens:@bob:example.org", "{ not json")
            .unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(error, Error::CorruptSession(_)));
    }

    #[test]
    fn clearing_removes_both_halves() {
        let (_dir, store, backend) = store();
        store.save(&session("@bob:example.org")).unwrap();

        store.clear().unwrap();

        assert!(!store.session_file().exists());
        assert!(backend.is_empty());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn clearing_when_nothing_is_stored_succeeds() {
        let (_dir, store, _) = store();
        store.clear().unwrap();
    }

    #[test]
    fn clearing_an_unparseable_session_file_still_deletes_it() {
        // Otherwise the user is stuck: every launch fails to parse, and the
        // sign-out button cannot clean it up either.
        let (_dir, store, _) = store();
        atomic::write_private(&store.session_file(), b"{ not json", "session").unwrap();

        store.clear().unwrap();

        assert!(!store.session_file().exists());
    }

    #[test]
    fn clearing_leaves_the_sqlite_store_directory_alone() {
        // Deleting it would make old history permanently undecryptable.
        let (dir, store, _) = store();
        let account_dir = dir.path().join("accounts").join("deadbeef");
        std::fs::create_dir_all(&account_dir).unwrap();
        std::fs::write(account_dir.join("matrix-sdk-crypto.sqlite3"), b"keys").unwrap();
        store.save(&session("@bob:example.org")).unwrap();

        store.clear().unwrap();

        assert!(account_dir.join("matrix-sdk-crypto.sqlite3").exists());
    }

    #[test]
    fn rotated_tokens_can_be_saved_without_rewriting_the_metadata() {
        let (_dir, store, _) = store();
        store.save(&session("@bob:example.org")).unwrap();
        let before = std::fs::read_to_string(store.session_file()).unwrap();

        store
            .save_tokens("@bob:example.org", &tokens("syt_rotated"))
            .unwrap();

        let after = std::fs::read_to_string(store.session_file()).unwrap();
        assert_eq!(before, after, "metadata should not have changed");
        assert_eq!(
            store.load().unwrap().unwrap().session.tokens.access_token,
            "syt_rotated"
        );
    }

    #[test]
    fn loading_tokens_for_an_unknown_account_is_none() {
        let (_dir, store, _) = store();
        assert!(store.load_tokens("@nobody:example.org").unwrap().is_none());
    }

    #[test]
    fn a_failing_secret_backend_surfaces_as_an_error_that_keeps_the_session() {
        // The regression guard for finding 5. A keyring that is temporarily
        // unreachable must not be read as "the user is signed out".
        let (_dir, store, backend) = store();
        backend.start_failing("the session bus went away");

        let error = store
            .save(&session("@bob:example.org"))
            .expect_err("save should surface the backend failure");

        assert!(matches!(error, Error::SecretStore { .. }));
        assert!(!error.invalidates_session());
    }

    #[test]
    fn the_same_account_always_gets_the_same_store_directory() {
        let (_dir, store, _) = store();
        assert_eq!(
            store.store_path_for("example.org|bob"),
            store.store_path_for("example.org|bob")
        );
    }

    #[test]
    fn different_accounts_get_different_store_directories() {
        let (_dir, store, _) = store();
        assert_ne!(
            store.store_path_for("example.org|bob"),
            store.store_path_for("example.org|alice")
        );
    }

    #[test]
    fn a_store_directory_is_a_safe_path_component_even_for_a_hostile_key() {
        let (dir, store, _) = store();

        let path = store.store_path_for("../../etc|bob");

        assert!(path.starts_with(dir.path()));
        assert!(!path.to_string_lossy().contains(".."));
    }

    #[test]
    fn the_store_reports_which_backend_it_is_using() {
        let (_dir, store, _) = store();
        assert_eq!(store.backend_kind(), BackendKind::Memory);
    }

    #[test]
    fn debug_shows_the_backend_but_never_a_token() {
        let (_dir, store, _) = store();
        store.save(&session("@bob:example.org")).unwrap();

        let rendered = format!("{store:?}");

        assert!(rendered.contains("Memory"));
        assert!(!rendered.contains("syt_access"));
    }

    #[test]
    fn the_token_key_is_scoped_per_account() {
        assert_ne!(
            token_key("@bob:example.org"),
            token_key("@alice:example.org")
        );
        assert!(token_key("@bob:example.org").contains("@bob:example.org"));
    }

    #[test]
    fn two_accounts_can_hold_tokens_side_by_side() {
        // Not a feature yet, but the storage layer must not be what blocks it.
        let (_dir, store, backend) = store();

        store
            .save_tokens("@bob:example.org", &tokens("bob-token"))
            .unwrap();
        store
            .save_tokens("@alice:matrix.org", &tokens("alice-token"))
            .unwrap();

        assert_eq!(backend.len(), 2);
        assert_eq!(
            store
                .load_tokens("@bob:example.org")
                .unwrap()
                .unwrap()
                .access_token,
            "bob-token"
        );
        assert_eq!(
            store
                .load_tokens("@alice:matrix.org")
                .unwrap()
                .unwrap()
                .access_token,
            "alice-token"
        );
    }

    #[test]
    fn the_session_file_sits_directly_in_the_data_directory() {
        let (dir, store, _) = store();
        assert_eq!(store.session_file(), dir.path().join("session.json"));
    }

    #[test]
    fn parse_user_id_accepts_a_real_one_and_rejects_nonsense() {
        assert!(parse_user_id("@bob:example.org").is_ok());
        assert!(parse_user_id("bob").is_err());
        assert!(parse_user_id("").is_err());
    }

    #[test]
    fn session_metadata_survives_a_json_round_trip() {
        let metadata = SessionMetadata {
            homeserver: "https://example.org/".to_owned(),
            store_path: PathBuf::from("/tmp/x"),
            user_id: "@bob:example.org".to_owned(),
            device_id: "DEV".to_owned(),
            token_store: Some(BackendKind::Keyring),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let back: SessionMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(back, metadata);
    }
}
