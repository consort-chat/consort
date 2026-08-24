// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Logging in, restoring a login, and logging out.

use std::fs;

use matrix_sdk::encryption::EncryptionSettings;
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::session::{SessionStore, StoredSession};

/// Shown to the homeserver in the user's device list.
const DEVICE_DISPLAY_NAME: &str = "Consort";

/// What the login form collects.
#[derive(Clone, Debug)]
pub struct Credentials {
    /// A server name (`lamp.stream`) or a full URL. Server names go through
    /// `.well-known` discovery, which is why a user should be able to type the
    /// short form.
    pub server: String,
    /// A localpart (`bob`) or a full user ID (`@bob:lamp.stream`).
    pub username: String,
    pub password: String,
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
    fs::create_dir_all(&store_path).map_err(|source| Error::SessionStore {
        path: store_path.clone(),
        source,
    })?;

    let client = base_builder()
        .server_name_or_homeserver_url(&server)
        .sqlite_store(&store_path, None)
        .build()
        .await?;

    client
        .matrix_auth()
        .login_username(&credentials.username, &credentials.password)
        .initial_device_display_name(DEVICE_DISPLAY_NAME)
        .send()
        .await
        .map_err(Error::Login)?;

    // Cross-signing bootstrap and device-key upload run as background tasks the
    // login kicks off. Returning before they settle hands the UI a client that
    // looks logged in but has no uploaded keys yet, and the first thing the
    // voice layer does is require this device to be cross-signed (MSC4153).
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
    tracing::info!(user_id = %profile.user_id, device_id = %profile.device_id, "logged in");
    Ok((client, profile))
}

/// Rebuild a logged-in client from a stored session, without a password.
///
/// Uses the stored homeserver URL rather than repeating `.well-known`
/// discovery, so a restore works when the well-known host is unreachable but
/// the homeserver itself is fine.
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

/// Builder settings shared by login and restore, so the two cannot drift.
///
/// A divergence here is the kind of bug that only shows up much later: a client
/// restored with different encryption settings than it was created with keeps
/// working for messaging and then fails when the voice layer checks for a
/// cross-signed device.
fn base_builder() -> ClientBuilder {
    Client::builder()
        .with_encryption_settings(EncryptionSettings {
            auto_enable_cross_signing: true,
            ..EncryptionSettings::default()
        })
        // Refresh tokens are handled by the SDK rather than surfacing an expiry
        // to the UI as a spurious "you have been logged out".
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

/// Reduce `@bob:lamp.stream`, `@bob`, and `bob` to `bob`.
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

    #[test]
    fn normalises_the_three_ways_of_writing_a_username() {
        assert_eq!(normalise_localpart("bob"), "bob");
        assert_eq!(normalise_localpart("@bob"), "bob");
        assert_eq!(normalise_localpart("@bob:lamp.stream"), "bob");
        assert_eq!(normalise_localpart("  @Bob:lamp.stream  "), "bob");
    }

    #[test]
    fn strips_trailing_slashes_and_surrounding_space_from_the_server() {
        assert_eq!(normalise_server("  lamp.stream  ").unwrap(), "lamp.stream");
        assert_eq!(
            normalise_server("https://matrix.lamp.stream/").unwrap(),
            "https://matrix.lamp.stream"
        );
    }

    #[test]
    fn rejects_an_empty_or_spaced_server() {
        assert!(normalise_server("").is_err());
        assert!(normalise_server("   ").is_err());
        assert!(normalise_server("lamp stream").is_err());
    }

    #[test]
    fn the_same_account_typed_two_ways_shares_one_store_directory() {
        let store = SessionStore::new("/tmp/consort-test");
        let typed_short = format!("{}|{}", "lamp.stream", normalise_localpart("bob"));
        let typed_full = format!(
            "{}|{}",
            "lamp.stream",
            normalise_localpart("@bob:lamp.stream")
        );
        assert_eq!(
            store.store_path_for(&typed_short),
            store.store_path_for(&typed_full)
        );
    }

    #[test]
    fn different_accounts_get_different_store_directories() {
        let store = SessionStore::new("/tmp/consort-test");
        let bob = store.store_path_for("lamp.stream|bob");
        let alice = store.store_path_for("lamp.stream|alice");
        assert_ne!(bob, alice);
    }
}
