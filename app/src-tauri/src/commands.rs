// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The commands the frontend can invoke.
//!
//! Each one is a thin adapter: translate arguments, call into `consort-matrix`,
//! translate the result. Logic that is not about crossing the JS boundary
//! belongs in `consort-matrix`, where it can be tested without a webview.

use consort_matrix::{Credentials, Profile, auth};
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

/// Called once on startup to decide which screen to show.
///
/// A stored session that fails to restore is treated as signed out rather than
/// as an error. The token may simply have been revoked from another device, and
/// the only useful thing to show a person in that case is the login form.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> Result<SessionStatus, CommandError> {
    if let Some(client) = state.client().await {
        let profile = Profile::from_client(&client).await?;
        return Ok(SessionStatus::SignedIn { profile });
    }

    let stored = match state.store().load() {
        Ok(Some(stored)) => stored,
        Ok(None) => return Ok(SessionStatus::SignedOut),
        Err(error) => {
            // A session file we cannot parse is worse than none: it would fail
            // identically on every launch. Drop it and start clean.
            tracing::warn!(%error, "discarding an unreadable stored session");
            let _ = state.store().clear();
            return Ok(SessionStatus::SignedOut);
        }
    };

    match auth::restore(&stored).await {
        Ok((client, profile)) => {
            state.set_client(client).await;
            Ok(SessionStatus::SignedIn { profile })
        }
        Err(error) => {
            tracing::warn!(%error, "stored session did not restore; signing out");
            let _ = state.store().clear();
            Ok(SessionStatus::SignedOut)
        }
    }
}

/// Sign in with a password.
#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    server: String,
    username: String,
    password: String,
) -> Result<Profile, CommandError> {
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
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), CommandError> {
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
