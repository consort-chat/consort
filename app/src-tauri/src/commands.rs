// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The commands the frontend can invoke.
//!
//! Each `#[tauri::command]` is a one-line delegate to a plain async function
//! taking `&AppState`. That split is not decoration: `State<'_, AppState>` can
//! only be produced by a running Tauri application, so logic written directly
//! inside a command is logic no test can reach. The delegates below are the
//! only untested lines in this file, and there is nothing in them to break.

use consort_matrix::{BackendKind, Credentials, Profile, auth, verification};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// An error in the shape the frontend consumes.
///
/// Carries two strings on purpose. `message` is written for a person and is
/// what the UI renders. `detail` is the underlying error text, which goes to
/// the console for whoever is debugging and is never shown in the interface.
#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
    detail: String,
}

/// Read accessors for the two halves.
///
/// Test-only. In the application both fields cross the IPC boundary by
/// serialisation and are never read from Rust, so exposing them outside a test
/// build would be API nobody calls.
#[cfg(test)]
impl CommandError {
    /// What the UI will render.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What goes to the console.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<consort_matrix::Error> for CommandError {
    fn from(error: consort_matrix::Error) -> Self {
        Self {
            message: error.user_message(),
            detail: error.to_string(),
        }
    }
}

/// Result of asking whether anyone is signed in.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SessionStatus {
    /// Nobody is signed in. Show the login screen.
    SignedOut,
    /// Somebody is signed in, either already or just now restored from disk.
    SignedIn { profile: Profile },
}

/// Where this machine is keeping the access token.
///
/// Surfaced to the UI so that a fallback to a plain file is something the user
/// is told about rather than something they would have to read the source to
/// discover.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStorage {
    pub kind: BackendKind,
    pub description: String,
    /// False when we had to fall back, which is the UI's cue to say so.
    pub is_preferred: bool,
}

/// Decide which screen to show.
pub async fn session_status_for(state: &AppState) -> Result<SessionStatus, CommandError> {
    if let Some(client) = state.client().await {
        let profile = Profile::from_client(&client).await?;
        return Ok(SessionStatus::SignedIn { profile });
    }

    let stored = match state.store().load() {
        Ok(Some(stored)) => stored,
        Ok(None) => return Ok(SessionStatus::SignedOut),
        Err(error) => {
            // Only discard what is genuinely unusable. A session file we cannot
            // parse would fail identically on every launch, so it goes. A
            // keyring that did not answer, or a store another process has
            // locked, is a reason to try again later and emphatically not a
            // reason to delete the one credential we hold.
            if error.invalidates_session() {
                tracing::warn!(%error, "discarding an unusable stored session");
                let _ = state.store().clear();
                return Ok(SessionStatus::SignedOut);
            }

            tracing::error!(%error, "could not read the stored session; keeping it");
            return Err(error.into());
        }
    };

    match auth::restore(&stored).await {
        Ok((client, profile)) => {
            // `set_client` also starts the task that writes rotated tokens
            // back to the store. Without it the next launch restores a spent
            // refresh token. See `auth::persist_token_refreshes`.
            state.set_client(client).await;
            Ok(SessionStatus::SignedIn { profile })
        }
        Err(error) if error.invalidates_session() => {
            tracing::warn!(%error, "the homeserver rejected the stored session; signing out");
            let _ = state.store().clear();
            Ok(SessionStatus::SignedOut)
        }
        Err(error) => {
            // Offline, or the homeserver is down. The session is still good.
            tracing::warn!(%error, "could not restore the session right now; keeping it");
            Err(error.into())
        }
    }
}

/// Sign in with a password.
pub async fn login_for(
    state: &AppState,
    server: String,
    username: String,
    password: String,
) -> Result<Profile, CommandError> {
    // Held across the whole login. Two concurrent calls would otherwise
    // register two devices on the homeserver and race on the session store.
    let _gate = state.lock_auth().await;

    // Somebody else may have completed a login while this call waited.
    if let Some(client) = state.client().await {
        tracing::info!("a login completed while this one waited; reusing it");
        return Ok(Profile::from_client(&client).await?);
    }

    let credentials = Credentials {
        server,
        username,
        password,
    };
    let (client, profile) = auth::login(state.store(), &credentials).await?;
    state.set_client(client).await;
    Ok(profile)
}

/// Sign out, locally and on the server.
pub async fn logout_for(state: &AppState) -> Result<(), CommandError> {
    let _gate = state.lock_auth().await;

    if let Some(client) = state.client().await {
        auth::logout(&client, state.store()).await?;
    } else {
        // No client but possibly a session file, for instance if a restore
        // failed earlier in this run. Clearing is still the right outcome.
        state.store().clear()?;
    }
    state.clear_client().await;
    Ok(())
}

/// The signed-in client, or an error the interface can render.
///
/// Every verification action needs one, and none of them can do anything
/// useful without it. Reaching this with nobody signed in is not a bug in the
/// application: any script running in the webview can invoke a command, and
/// unwrapping here would turn that into a panic in a command thread.
async fn signed_in_client(state: &AppState) -> Result<consort_matrix::Client, CommandError> {
    state
        .client()
        .await
        .ok_or_else(|| consort_matrix::Error::NotLoggedIn.into())
}

/// Agree to a verification somebody else asked for.
pub async fn verification_accept_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::accept(&client, &user_id, &flow_id).await?)
}

/// Start the emoji comparison from this side.
pub async fn verification_start_sas_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::start_sas(&client, &user_id, &flow_id).await?)
}

/// Say the emoji matched.
pub async fn verification_confirm_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::confirm(&client, &user_id, &flow_id).await?)
}

/// Say the emoji did not match.
pub async fn verification_mismatch_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::mismatch(&client, &user_id, &flow_id).await?)
}

/// Call the verification off.
pub async fn verification_cancel_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::cancel(&client, &user_id, &flow_id).await?)
}

/// Report where the access token is being kept.
pub fn token_storage_for(state: &AppState) -> TokenStorage {
    let kind = state.store().backend_kind();
    TokenStorage {
        kind,
        description: kind.description().to_owned(),
        is_preferred: kind.is_preferred(),
    }
}

/// Called once on startup to decide which screen to show.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> Result<SessionStatus, CommandError> {
    session_status_for(&state).await
}

/// Sign in with a password.
#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    server: String,
    username: String,
    password: String,
) -> Result<Profile, CommandError> {
    login_for(&state, server, username, password).await
}

/// Sign out, locally and on the server.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), CommandError> {
    logout_for(&state).await
}

/// Where the access token lives on this machine.
#[tauri::command]
pub fn token_storage(state: State<'_, AppState>) -> TokenStorage {
    token_storage_for(&state)
}

/// Re-send the current state of every push channel.
///
/// Called by the frontend once its listeners are attached. The background
/// tasks start with the session, which on a restored session is before the
/// webview has run a line of JavaScript, so their first states are published
/// to nobody. These are state channels rather than streams of incidents:
/// missing one is not a missed message, it is an interface stuck on its
/// initial guess until something else happens to change.
#[tauri::command]
pub fn resend_state(state: State<'_, AppState>) {
    state.resend_state();
}

// The five verification actions.
//
// Each takes the pair of identifiers the `verification-flow` event carried,
// rather than the frontend holding a handle to anything. Nothing on either
// side of the boundary owns a flow: the SDK has a registry keyed by exactly
// this pair, and naming the flow every time is what makes two concurrent
// verifications representable, which they are.
//
// The user id looks redundant while only self-verification exists, since it is
// always our own. Taking it anyway costs one string and means verifying
// another person, when it arrives, is not a change to five signatures and the
// TypeScript that calls them.

/// Agree to a verification somebody else asked for.
#[tauri::command]
pub async fn verification_accept(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_accept_for(&state, user_id, flow_id).await
}

/// Start the emoji comparison from this side.
#[tauri::command]
pub async fn verification_start_sas(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_start_sas_for(&state, user_id, flow_id).await
}

/// Say the emoji matched.
#[tauri::command]
pub async fn verification_confirm(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_confirm_for(&state, user_id, flow_id).await
}

/// Say the emoji did not match.
#[tauri::command]
pub async fn verification_mismatch(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_mismatch_for(&state, user_id, flow_id).await
}

/// Call the verification off.
#[tauri::command]
pub async fn verification_cancel(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_cancel_for(&state, user_id, flow_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RecordingSink;
    use consort_matrix::Backend;
    use consort_matrix::SessionStore;
    use consort_matrix::secrets::MemoryBackend;
    use std::sync::Arc;

    fn state() -> (tempfile::TempDir, AppState, Arc<MemoryBackend>) {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(MemoryBackend::new());
        let store = SessionStore::with_backend(dir.path(), backend.clone());
        let state = AppState::new(store, Arc::new(RecordingSink::new()));
        (dir, state, backend)
    }

    pub(super) fn status_name(status: &SessionStatus) -> &'static str {
        match status {
            SessionStatus::SignedOut => "signedOut",
            SessionStatus::SignedIn { .. } => "signedIn",
        }
    }

    /// The five verification actions, as `(name, run it)`.
    ///
    /// A list rather than five near-identical tests, because what is being
    /// checked is that none of them behaves differently, and five copies of
    /// one assertion is how the odd one out gets missed.
    #[allow(clippy::type_complexity)]
    pub(super) fn every_action() -> Vec<(
        &'static str,
        fn(
            &AppState,
            String,
            String,
        )
            -> std::pin::Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + '_>>,
    )> {
        vec![
            ("accept", |state, user, flow| {
                Box::pin(verification_accept_for(state, user, flow))
            }),
            ("start_sas", |state, user, flow| {
                Box::pin(verification_start_sas_for(state, user, flow))
            }),
            ("confirm", |state, user, flow| {
                Box::pin(verification_confirm_for(state, user, flow))
            }),
            ("mismatch", |state, user, flow| {
                Box::pin(verification_mismatch_for(state, user, flow))
            }),
            ("cancel", |state, user, flow| {
                Box::pin(verification_cancel_for(state, user, flow))
            }),
        ]
    }

    #[tokio::test]
    async fn no_verification_action_works_without_a_signed_in_session() {
        // Reachable: the webview can invoke a command directly, and a signed
        // out app still has a page that could. Every one of these has to be an
        // error rather than an unwrap on `Option<Client>`.
        let (_dir, state, _) = state();

        for (name, run) in every_action() {
            let error = run(&state, "@bob:example.org".to_owned(), "flow".to_owned())
                .await
                .expect_err("{name} succeeded with nobody signed in");
            assert!(
                error.message().contains("No user is signed in"),
                "{name}: {}",
                error.message()
            );
        }
    }

    #[tokio::test]
    async fn no_stored_session_means_signed_out() {
        let (_dir, state, _) = state();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
    }

    #[tokio::test]
    async fn an_unparseable_session_file_is_discarded_and_reports_signed_out() {
        let (_dir, state, _) = state();
        std::fs::write(state.store().session_file(), b"{ not json").unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
        assert!(
            !state.store().session_file().exists(),
            "an unusable session file should be cleaned up"
        );
    }

    #[tokio::test]
    async fn a_keyring_failure_is_an_error_and_keeps_the_session_file() {
        // The regression guard for finding 5. Before this change, any error
        // reading the session deleted it, so a keyring that was briefly
        // unreachable logged the user out permanently.
        let (_dir, state, backend) = state();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();
        backend.start_failing("the session bus went away");

        let error = session_status_for(&state)
            .await
            .expect_err("a keyring failure should surface, not sign the user out");

        assert!(error.message().contains("keyring"));
        assert!(
            state.store().session_file().exists(),
            "the session file must survive a transient keyring failure"
        );
    }

    #[tokio::test]
    async fn metadata_with_no_tokens_reports_signed_out_without_erroring() {
        let (_dir, state, _) = state();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
    }

    #[tokio::test]
    async fn logging_out_with_no_client_still_clears_the_stored_session() {
        let (_dir, state, backend) = state();
        backend
            .set(
                "session-tokens:@bob:example.org",
                r#"{"access_token":"syt_x"}"#,
            )
            .unwrap();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();

        logout_for(&state).await.unwrap();

        assert!(!state.store().session_file().exists());
        assert!(backend.is_empty());
    }

    #[tokio::test]
    async fn logging_out_twice_is_not_an_error() {
        let (_dir, state, _) = state();
        logout_for(&state).await.unwrap();
        logout_for(&state).await.unwrap();
    }

    #[tokio::test]
    async fn token_storage_reports_the_backend_in_use() {
        let (_dir, state, _) = state();

        let storage = token_storage_for(&state);

        assert_eq!(storage.kind, BackendKind::Memory);
        assert!(!storage.is_preferred);
        assert!(!storage.description.is_empty());
    }

    #[test]
    fn token_storage_serialises_with_the_field_names_the_frontend_expects() {
        let storage = TokenStorage {
            kind: BackendKind::File,
            description: BackendKind::File.description().to_owned(),
            is_preferred: false,
        };

        let json = serde_json::to_value(&storage).unwrap();

        assert_eq!(json.get("kind").unwrap(), "file");
        assert_eq!(json.get("isPreferred").unwrap(), false);
        assert!(json.get("description").is_some());
    }

    #[test]
    fn a_command_error_splits_the_person_facing_and_developer_facing_text() {
        let error: CommandError = consort_matrix::Error::InvalidServer("bad one".to_owned()).into();

        assert!(error.message().contains("bad one"));
        assert!(error.message().contains("does not look like"));
        assert!(error.detail().contains("bad one"));
    }

    #[test]
    fn a_command_error_never_shows_an_io_path_to_the_user() {
        let error: CommandError = consort_matrix::Error::SessionStore {
            path: std::path::PathBuf::from("/home/someone/.local/share/consort/session.json"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        }
        .into();

        assert!(!error.message().contains("/home/someone"));
        // The detail is for the console, and there the path is what you want.
        assert!(error.detail().contains("/home/someone"));
    }

    #[test]
    fn a_command_error_serialises_both_fields() {
        let error: CommandError = consort_matrix::Error::NotLoggedIn.into();
        let json = serde_json::to_value(&error).unwrap();

        assert!(json.get("message").is_some());
        assert!(json.get("detail").is_some());
    }

    #[test]
    fn session_status_serialises_as_a_tagged_union() {
        let json = serde_json::to_value(SessionStatus::SignedOut).unwrap();
        assert_eq!(json.get("status").unwrap(), "signedOut");

        let json = serde_json::to_value(SessionStatus::SignedIn {
            profile: Profile {
                user_id: "@bob:example.org".to_owned(),
                device_id: "DEV".to_owned(),
                homeserver: "https://example.org/".to_owned(),
                display_name: None,
                avatar_url: None,
            },
        })
        .unwrap();
        assert_eq!(json.get("status").unwrap(), "signedIn");
        assert!(json.get("profile").is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_logout_waits_for_the_first_rather_than_running_beside_it() {
        let (_dir, state, _) = state();
        let state = Arc::new(state);

        let held = state.lock_auth().await;
        let other = {
            let state = state.clone();
            tokio::spawn(async move { logout_for(&state).await })
        };

        // While the gate is held the second call cannot have finished.
        tokio::task::yield_now().await;
        assert!(!other.is_finished());

        drop(held);
        other.await.unwrap().unwrap();
    }
}

/// The command paths that need something answering like a homeserver.
///
/// Split from the unit tests above because they are slower and because they
/// need `MatrixMockServer`, which only exists with matrix-sdk's `testing`
/// feature. Without them `login_for` and the restore half of
/// `session_status_for` are only covered by running the app by hand.
#[cfg(test)]
mod against_a_mock_homeserver {
    use super::tests::status_name;
    use super::*;
    use crate::events::RecordingSink;
    use consort_matrix::secrets::MemoryBackend;
    use consort_matrix::{Connection, SessionStore, SessionVerification, StopReason};
    use matrix_sdk::ruma;
    use matrix_sdk::test_utils::mocks::{LoginResponseTemplate200, MatrixMockServer};
    use std::sync::Arc;

    const DEVICE: &str = "HZTIUXZKUU";

    fn state() -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()));
        let sink = Arc::new(RecordingSink::new());
        (dir, AppState::new(store, sink.clone()), sink)
    }

    async fn mount_login(server: &MatrixMockServer) {
        server.mock_versions().ok().mount().await;
        server.mock_well_known().ok().mount().await;
        server
            .mock_login()
            .ok_with(LoginResponseTemplate200::new(
                "syt_first",
                DEVICE,
                ruma::user_id!("@bob:example.org"),
            ))
            .mount()
            .await;
        server.mock_upload_keys().ok().mount().await;
        server.mock_query_keys().ok().mount().await;
    }

    #[tokio::test]
    async fn a_verification_action_on_a_flow_that_has_gone_is_reported_not_swallowed() {
        // Proves the delegation, which is the only thing these commands do:
        // the identifiers reach the SDK, it finds nothing, and the error comes
        // back as something the interface can render rather than a panic in a
        // command thread.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        for (name, run) in super::tests::every_action() {
            let error = run(&state, "@bob:example.org".to_owned(), "gone".to_owned())
                .await
                .expect_err("{name} succeeded on a flow that does not exist");
            assert!(
                error.message().contains("no longer"),
                "{name}: {}",
                error.message()
            );
        }
    }

    #[tokio::test]
    async fn a_successful_login_returns_the_profile() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();

        let profile = login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert_eq!(profile.user_id, "@bob:example.org");
        assert_eq!(profile.device_id, DEVICE);
    }

    #[tokio::test]
    async fn a_successful_login_leaves_the_client_in_state() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.client().await.is_some());
        assert!(
            state.has_refresh_task().await,
            "signing in should start persisting token rotations"
        );
    }

    #[tokio::test]
    async fn a_failed_login_leaves_no_client_behind() {
        let server = MatrixMockServer::new().await;
        server.mock_versions().ok().mount().await;
        server.mock_well_known().ok().mount().await;
        server
            .mock_login()
            .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "Invalid password" }),
            ))
            .mount()
            .await;
        let (_dir, state, _sink) = state();

        let error = login_for(&state, server.uri(), "bob".to_owned(), "wrong".to_owned())
            .await
            .unwrap_err();

        assert_eq!(error.message(), "Incorrect username or password.");
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn asking_for_the_status_with_a_live_client_does_not_touch_the_store() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        std::fs::remove_file(state.store().session_file()).unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedIn");
    }

    #[tokio::test]
    async fn a_stored_session_is_restored_on_startup() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        // A second run of the app over the same data directory, sharing the
        // secret backend the way a real keyring would be shared.
        let fresh = AppState::new(first.store().clone(), Arc::new(RecordingSink::new()));

        let status = session_status_for(&fresh).await.unwrap();

        assert_eq!(status_name(&status), "signedIn");
        assert!(fresh.client().await.is_some());
        assert!(dir.path().exists());
    }

    #[tokio::test]
    async fn signing_out_clears_the_client_and_the_stored_session() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        logout_for(&state).await.unwrap();

        assert!(state.client().await.is_none());
        assert!(!state.has_refresh_task().await);
        assert!(state.store().load().unwrap().is_none());
        assert_eq!(
            status_name(&session_status_for(&state).await.unwrap()),
            "signedOut"
        );
    }

    /// Poll until the sink has seen a connection state matching `want`.
    ///
    /// The sync loop is a spawned task, so nothing it reports is available on
    /// the line after `login_for` returns.
    async fn wait_for_connection(sink: &RecordingSink, want: impl Fn(&Connection) -> bool) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if sink
                .events()
                .iter()
                .any(|event| matches!(event, crate::events::AppEvent::Connection(c) if want(c)))
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out; saw {:?}",
                sink.events()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Poll until the sink has seen a verification state matching `want`.
    async fn wait_for_verification(
        sink: &RecordingSink,
        want: impl Fn(&SessionVerification) -> bool,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if sink
                .events()
                .iter()
                .any(|event| matches!(event, crate::events::AppEvent::Verification(v) if want(v)))
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out; saw {:?}",
                sink.events()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn signing_in_reports_the_session_unverified() {
        // A brand new device is signed by nobody. Saying so is the whole of
        // this milestone: until it is verified it cannot read encrypted
        // history and no encrypted call will have it.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.has_verification_task().await);
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
    }

    #[tokio::test]
    async fn a_restored_session_reports_its_verification_state_too() {
        // The startup path again. Restoring goes through a different function
        // from logging in, so it is its own way to end up with a session that
        // never says whether it is verified.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        let sink = Arc::new(RecordingSink::new());
        let fresh = AppState::new(first.store().clone(), sink.clone());
        session_status_for(&fresh).await.unwrap();

        assert!(fresh.has_verification_task().await);
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
    }

    #[tokio::test]
    async fn signing_out_stops_the_verification_watcher() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;

        logout_for(&state).await.unwrap();

        assert!(!state.has_verification_task().await);
    }

    #[tokio::test]
    async fn a_late_subscriber_can_ask_for_the_state_it_missed() {
        // The race this exists for: the tasks publish while the webview is
        // still loading, and a listener attached afterwards hears nothing
        // until the next change, which on a healthy session may be never.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_connection(&sink, |c| *c == Connection::Live).await;
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
        let before = sink.events().len();

        state.resend_state();

        let resent = &sink.events()[before..];
        assert_eq!(
            resent.len(),
            2,
            "expected one state per channel, got {resent:?}"
        );
        assert_eq!(sink.last_connection(), Some(Connection::Live));
        assert_eq!(
            sink.last_verification(),
            Some(SessionVerification::Unverified)
        );
    }

    #[tokio::test]
    async fn signing_in_starts_the_sync_loop() {
        // Without this a signed-in Consort can be talked to and cannot hear:
        // to-device events, and therefore every verification request, arrive
        // through sync and nowhere else.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, _sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.has_sync_task().await);
    }

    #[tokio::test]
    async fn signing_in_tells_the_frontend_the_connection_is_live() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        wait_for_connection(&sink, |c| *c == Connection::Live).await;
    }

    #[tokio::test]
    async fn a_restored_session_starts_syncing_too() {
        // The startup path. Restoring is the common case and it goes through
        // a different function from logging in, so it is its own way to end
        // up with a client that never syncs.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        let sink = Arc::new(RecordingSink::new());
        let fresh = AppState::new(first.store().clone(), sink.clone());
        session_status_for(&fresh).await.unwrap();

        assert!(fresh.has_sync_task().await);
        wait_for_connection(&sink, |c| *c == Connection::Live).await;
    }

    #[tokio::test]
    async fn signing_out_stops_the_sync_loop_and_says_so() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_connection(&sink, |c| *c == Connection::Live).await;

        logout_for(&state).await.unwrap();

        assert!(!state.has_sync_task().await);
        assert_eq!(
            sink.last_connection(),
            Some(Connection::Stopped {
                reason: StopReason::SignedOut
            }),
            "the UI was left believing it was still connected"
        );
    }

    #[tokio::test]
    async fn a_second_sign_in_does_not_leave_the_first_loop_running() {
        // Two sync loops on one account is two clients holding the same
        // SQLite crypto store and two sets of to-device events being claimed.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, _sink) = state();
        let (client, _) = consort_matrix::auth::login(
            state.store(),
            &consort_matrix::Credentials {
                server: server.uri(),
                username: "bob".to_owned(),
                password: "hunter2".to_owned(),
            },
        )
        .await
        .unwrap();

        state.set_client(client.clone()).await;
        let first = state.sync_task_id().await;
        state.set_client(client).await;
        let second = state.sync_task_id().await;

        assert_ne!(first, second, "the second sign-in reused the first loop");
        assert!(state.has_sync_task().await);
    }

    #[tokio::test]
    async fn a_second_login_that_arrives_after_the_first_reuses_it() {
        // Both calls take the gate. The loser finds a client already in place
        // and must not register a second device on the homeserver.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        let state = Arc::new(state);

        let one = {
            let state = state.clone();
            let uri = server.uri();
            tokio::spawn(async move {
                login_for(&state, uri, "bob".to_owned(), "hunter2".to_owned()).await
            })
        };
        let two = {
            let state = state.clone();
            let uri = server.uri();
            tokio::spawn(async move {
                login_for(&state, uri, "bob".to_owned(), "hunter2".to_owned()).await
            })
        };

        let first = one.await.unwrap().unwrap();
        let second = two.await.unwrap().unwrap();

        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.user_id, second.user_id);
    }
}
