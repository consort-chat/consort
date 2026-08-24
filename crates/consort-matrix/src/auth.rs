// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Logging in, restoring a login, and logging out.

use std::fmt;
use std::fs;
use std::path::Path;

use matrix_sdk::config::RequestConfig;
use matrix_sdk::encryption::EncryptionSettings;
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::{Client, ClientBuilder, SessionChange};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::session::{SessionStore, StoredSession};

/// Shown to the homeserver in the user's device list.
const DEVICE_DISPLAY_NAME: &str = "Consort";

/// What the login form collects.
#[derive(Clone)]
pub struct Credentials {
    /// A server name (`example.org`) or a full URL. Server names go through
    /// `.well-known` discovery, which is why a user should be able to type the
    /// short form.
    pub server: String,
    /// A localpart (`bob`) or a full user ID (`@bob:example.org`).
    pub username: String,
    pub password: String,
}

/// Written by hand, and it must stay that way.
///
/// A derived `Debug` prints the password. Nothing logs `Credentials` today, but
/// `tracing::debug!(?credentials)` added during a debugging session would put a
/// user's password into the journal, where it stays. Redacting here closes that
/// off once instead of relying on every future call site to remember.
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("server", &self.server)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// The logged-in user, in the shape the UI wants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub user_id: String,
    pub device_id: String,
    pub homeserver: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

impl Profile {
    /// Read the profile off a logged-in client.
    ///
    /// The display name and avatar are fetched over the network and are allowed
    /// to fail: a profile request that times out should not turn a successful
    /// login into a failed one. The UI falls back to the user ID.
    pub async fn from_client(client: &Client) -> Result<Self> {
        let user_id = client.user_id().ok_or(Error::NotLoggedIn)?;
        let device_id = client.device_id().ok_or(Error::NotLoggedIn)?;

        // Two requests rather than one `fetch_user_profile`, because the SDK's
        // typed accessors survive the extensible-profiles reshuffle that moved
        // these fields around inside the raw response.
        //
        // Each is tolerated separately: an account with a display name and no
        // avatar is ordinary, and a failed avatar lookup should not cost us the
        // name we already have.
        let account = client.account();

        let display_name = account.get_display_name().await.unwrap_or_else(|error| {
            tracing::warn!(%error, "could not fetch the display name; falling back to the user ID");
            None
        });

        let avatar_url = account
            .get_avatar_url()
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "could not fetch the avatar URL");
                None
            })
            .map(|url| url.to_string());

        Ok(Self {
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            homeserver: client.homeserver().to_string(),
            display_name,
            avatar_url,
        })
    }
}

/// Log in with a password and persist the session.
///
/// On success the returned [`Client`] is fully initialised: encryption tasks
/// have settled and cross-signing has been bootstrapped if the account had none.
pub async fn login(store: &SessionStore, credentials: &Credentials) -> Result<(Client, Profile)> {
    let server = normalise_server(&credentials.server)?;
    let localpart = normalise_localpart(&credentials.username);

    // Derived before the login, because the crypto store has to be attached to
    // the client that logs in. See `SessionStore::store_path_for`.
    let store_path = store.store_path_for(&format!("{server}|{localpart}"));
    discard_previous_device_store(&store_path)?;
    crate::atomic::create_dir_private(&store_path)?;

    let client = base_builder()
        .server_name_or_homeserver_url(&server)
        .sqlite_store(&store_path, None)
        .build()
        .await?;

    // The failure is logged here rather than left to the caller, because this
    // is the only place that still holds the SDK's own error text. Above this
    // point it becomes a `user_message`, which is deliberately vague, and the
    // `detail` string only reaches the webview console. A user reporting "it
    // says my password is wrong and it isn't" needs the server's actual reply
    // to be somewhere they can copy it from.
    client
        .matrix_auth()
        .login_username(&credentials.username, &credentials.password)
        .initial_device_display_name(DEVICE_DISPLAY_NAME)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(
                %server,
                matrix_error = ?error.client_api_error_kind(),
                "login rejected: {error}"
            );
            Error::Login(error)
        })?;

    // Cross-signing bootstrap and device-key upload run as background tasks the
    // login kicks off. Returning before they settle hands the UI a client that
    // looks logged in but has no uploaded keys yet, and the first thing the
    // voice layer does is require this device to be cross-signed (MSC4153).
    //
    // This does not reset cross-signing on an account that already has it. The
    // SDK routes `auto_enable_cross_signing` through
    // `bootstrap_cross_signing_if_needed`, which checks for an existing
    // identity first. Resetting would un-verify every other device the user
    // owns, so it is worth knowing that it does not happen.
    client
        .encryption()
        .wait_for_e2ee_initialization_tasks()
        .await;

    let session = client
        .matrix_auth()
        .session()
        .expect("a client that just completed login has a session");

    let stored = StoredSession {
        homeserver: client.homeserver().to_string(),
        store_path,
        session,
    };
    store.save(&stored)?;

    let profile = Profile::from_client(&client).await?;
    tracing::info!(
        user_id = %profile.user_id,
        device_id = %profile.device_id,
        token_store = ?store.backend_kind(),
        "logged in"
    );
    Ok((client, profile))
}

/// Remove the crypto store left behind by a previous device on this account.
///
/// matrix-sdk-crypto binds a store to the device that created it and refuses
/// to open one belonging to another, with "the account in the store doesn't
/// match the account in the constructor". A password login always mints a new
/// device, so a store already sitting at this path is guaranteed to be the
/// wrong one.
///
/// Anything that ends a session without ending the store gets here: a sign
/// out, a session file from an older format being discarded, a crash between
/// writing the store and writing the session. Before this existed, any one of
/// those made the account permanently unsignable-into, and the only way out
/// was to delete a hashed directory by hand.
///
/// Nothing usable is lost. The keys in that store belong to the old device,
/// and without that device's access token they cannot be used again. The
/// caller is responsible for not calling `login` while a session for the same
/// account is live, which `login_for` enforces by reusing the existing client
/// instead.
fn discard_previous_device_store(store_path: &Path) -> Result<()> {
    if !store_path.exists() {
        return Ok(());
    }

    tracing::info!(
        path = %store_path.display(),
        "discarding the previous device's encryption store before signing in again"
    );

    fs::remove_dir_all(store_path).map_err(|source| Error::SessionStore {
        path: store_path.to_path_buf(),
        source,
    })
}

/// Rebuild a logged-in client from a stored session, without a password.
///
/// Uses the stored homeserver URL rather than repeating `.well-known`
/// discovery, so a restore works when the well-known host is unreachable but
/// the homeserver itself is fine. With the URL already known the SDK's
/// `build()` makes no network request at all, which means a restore also works
/// entirely offline.
pub async fn restore(stored: &StoredSession) -> Result<(Client, Profile)> {
    let client = base_builder()
        .homeserver_url(&stored.homeserver)
        .sqlite_store(&stored.store_path, None)
        .build()
        .await?;

    client
        .matrix_auth()
        .restore_session(stored.session.clone(), RoomLoadSettings::default())
        .await?;

    let profile = Profile::from_client(&client).await?;
    tracing::info!(user_id = %profile.user_id, "restored session");
    Ok((client, profile))
}

/// Log out on the server and forget the local session.
///
/// The local session is cleared even if the server call fails. A user who
/// clicked sign out should end up signed out locally regardless of whether the
/// homeserver was reachable, and a token we have discarded is one we cannot
/// invalidate later anyway.
pub async fn logout(client: &Client, store: &SessionStore) -> Result<()> {
    let server_result = client.matrix_auth().logout().await;
    store.clear()?;

    match server_result {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::warn!(%error, "server-side logout failed; the local session was still cleared");
            Ok(())
        }
    }
}

/// Keep the stored tokens in step with the ones the SDK is using.
///
/// `handle_refresh_tokens` lets the SDK rotate an expired access token on its
/// own, and homeservers commonly make refresh tokens single-use. Without this,
/// the pair on disk is the pair from login: the access token expires, the
/// refresh token has already been spent, and the next launch signs the user out
/// with nothing in the log to explain it.
///
/// # Lifetime
///
/// This holds a `Client`, so it keeps that client alive for as long as it
/// runs, and the broadcast channel it watches lives inside the same client.
/// It therefore cannot end on its own: the caller spawns it and the caller
/// aborts it, which in practice means when the user signs out or a different
/// account signs in. `matrix_sdk::WeakClient` would let it exit by itself, but
/// it is `pub(crate)` in the SDK and not ours to use.
pub async fn persist_token_refreshes(client: Client, store: SessionStore) {
    let mut changes = client.subscribe_to_session_changes();

    loop {
        match changes.recv().await {
            Ok(SessionChange::TokensRefreshed) => {
                let (Some(user_id), Some(tokens)) = (client.user_id(), client.session_tokens())
                else {
                    continue;
                };

                match store.save_tokens(user_id.as_str(), &tokens) {
                    Ok(()) => tracing::info!("persisted refreshed access token"),
                    // Deliberately not fatal. The in-memory client still works
                    // for this run; only the next launch is affected, and
                    // signing the user out now would be a worse trade.
                    Err(error) => {
                        tracing::error!(%error, "could not persist refreshed tokens")
                    }
                }
            }
            Ok(SessionChange::UnknownToken(_)) => {
                tracing::warn!("the homeserver rejected our access token");
            }
            // Lagged means we missed some notifications, not that we should
            // stop watching. The next refresh still arrives.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "missed session change notifications");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Builder settings shared by login and restore, so the two cannot drift.
///
/// A divergence here is the kind of bug that only shows up much later: a client
/// restored with different encryption settings than it was created with keeps
/// working for messaging and then fails when the voice layer checks for a
/// cross-signed device.
fn base_builder() -> ClientBuilder {
    Client::builder()
        // Bounded retries, because the default is unbounded and invisible.
        //
        // Left alone, matrix-sdk retries a 5xx behind the caller's back for
        // fifteen minutes with no way to observe it. For a login that is a
        // form which never comes back. For the sync loop it is worse: the loop
        // sits inside one `sync_once` for a quarter of an hour while the UI
        // goes on saying "Connected", which is the exact failure
        // `consort_matrix::sync` exists to make impossible.
        //
        // Three attempts, roughly a second and a half, then the error reaches
        // us and we decide what it means. Retrying is still the right answer
        // to most of them; it just belongs somewhere it can be reported.
        .request_config(RequestConfig::short_retry())
        .with_encryption_settings(EncryptionSettings {
            auto_enable_cross_signing: true,
            ..EncryptionSettings::default()
        })
        // Refresh tokens are handled by the SDK rather than surfacing an expiry
        // to the UI as a spurious "you have been logged out". See
        // `persist_token_refreshes` for the other half of making that work.
        .handle_refresh_tokens()
        .user_agent(concat!("Consort/", env!("CARGO_PKG_VERSION")))
}

/// Trim and sanity-check what the user typed into the server field.
fn normalise_server(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return Err(Error::InvalidServer(input.to_owned()));
    }
    Ok(trimmed.to_owned())
}

/// Reduce `@bob:example.org`, `@bob`, and `bob` to `bob`.
///
/// Only used to key the local store directory, so that the same account typed
/// two different ways does not produce two devices. The homeserver still
/// receives the untouched input and remains the authority on what it means.
fn normalise_localpart(username: &str) -> String {
    let trimmed = username.trim();
    let without_sigil = trimmed.strip_prefix('@').unwrap_or(trimmed);
    without_sigil
        .split_once(':')
        .map(|(localpart, _)| localpart)
        .unwrap_or(without_sigil)
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;
    use std::sync::Arc;

    fn store(dir: &tempfile::TempDir) -> SessionStore {
        SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()))
    }

    #[test]
    fn normalises_the_three_ways_of_writing_a_username() {
        assert_eq!(normalise_localpart("bob"), "bob");
        assert_eq!(normalise_localpart("@bob"), "bob");
        assert_eq!(normalise_localpart("@bob:example.org"), "bob");
        assert_eq!(normalise_localpart("  @Bob:example.org  "), "bob");
    }

    #[test]
    fn normalising_a_username_is_idempotent() {
        let once = normalise_localpart("@Bob:example.org");
        assert_eq!(normalise_localpart(&once), once);
    }

    #[test]
    fn an_empty_username_normalises_to_empty_rather_than_panicking() {
        assert_eq!(normalise_localpart(""), "");
        assert_eq!(normalise_localpart("@"), "");
        assert_eq!(normalise_localpart("   "), "");
    }

    #[test]
    fn strips_trailing_slashes_and_surrounding_space_from_the_server() {
        assert_eq!(normalise_server("  example.org  ").unwrap(), "example.org");
        assert_eq!(
            normalise_server("https://matrix.example.org/").unwrap(),
            "https://matrix.example.org"
        );
    }

    #[test]
    fn strips_every_trailing_slash_not_just_one() {
        assert_eq!(
            normalise_server("https://example.org///").unwrap(),
            "https://example.org"
        );
    }

    #[test]
    fn rejects_an_empty_or_spaced_server() {
        assert!(normalise_server("").is_err());
        assert!(normalise_server("   ").is_err());
        assert!(normalise_server("example org").is_err());
        assert!(normalise_server("\t\n").is_err());
    }

    #[test]
    fn a_server_that_is_only_slashes_is_rejected() {
        assert!(normalise_server("///").is_err());
    }

    #[test]
    fn the_rejection_quotes_what_was_typed_not_the_trimmed_form() {
        let error = normalise_server("  bad server  ").unwrap_err();
        match error {
            Error::InvalidServer(value) => assert_eq!(value, "  bad server  "),
            other => panic!("expected InvalidServer, got {other:?}"),
        }
    }

    #[test]
    fn the_same_account_typed_two_ways_shares_one_store_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let typed_short = format!("{}|{}", "example.org", normalise_localpart("bob"));
        let typed_full = format!(
            "{}|{}",
            "example.org",
            normalise_localpart("@bob:example.org")
        );
        assert_eq!(
            store.store_path_for(&typed_short),
            store.store_path_for(&typed_full)
        );
    }

    #[test]
    fn different_accounts_get_different_store_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_ne!(
            store.store_path_for("example.org|bob"),
            store.store_path_for("example.org|alice")
        );
    }

    #[test]
    fn the_same_localpart_on_different_servers_does_not_share_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_ne!(
            store.store_path_for("example.org|bob"),
            store.store_path_for("matrix.org|bob")
        );
    }

    #[test]
    fn debug_on_credentials_never_prints_the_password() {
        // The regression guard. A derived Debug would fail this immediately.
        let credentials = Credentials {
            server: "example.org".to_owned(),
            username: "bob".to_owned(),
            password: "hunter2-correct-horse".to_owned(),
        };

        let rendered = format!("{credentials:?}");

        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("correct-horse"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn debug_on_credentials_keeps_the_fields_worth_debugging() {
        let credentials = Credentials {
            server: "example.org".to_owned(),
            username: "bob".to_owned(),
            password: "hunter2".to_owned(),
        };

        let rendered = format!("{credentials:?}");

        assert!(rendered.contains("example.org"));
        assert!(rendered.contains("bob"));
    }

    #[test]
    fn a_cloned_credential_redacts_too() {
        let credentials = Credentials {
            server: "example.org".to_owned(),
            username: "bob".to_owned(),
            password: "hunter2".to_owned(),
        };

        let rendered = format!("{:?}", credentials.clone());

        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn a_profile_survives_a_json_round_trip() {
        let profile = Profile {
            user_id: "@bob:example.org".to_owned(),
            device_id: "DEV".to_owned(),
            homeserver: "https://example.org/".to_owned(),
            display_name: Some("Bob".to_owned()),
            avatar_url: None,
        };

        let json = serde_json::to_string(&profile).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();

        assert_eq!(back.user_id, profile.user_id);
        assert_eq!(back.device_id, profile.device_id);
        assert_eq!(back.display_name, profile.display_name);
        assert_eq!(back.avatar_url, None);
    }

    #[test]
    fn a_profile_serialises_the_field_names_the_frontend_expects() {
        // api.ts mirrors these by hand. A rename here has to break something.
        let profile = Profile {
            user_id: "@bob:example.org".to_owned(),
            device_id: "DEV".to_owned(),
            homeserver: "https://example.org/".to_owned(),
            display_name: None,
            avatar_url: None,
        };

        let json = serde_json::to_value(&profile).unwrap();

        for field in [
            "user_id",
            "device_id",
            "homeserver",
            "display_name",
            "avatar_url",
        ] {
            assert!(json.get(field).is_some(), "missing {field}");
        }
    }
}
