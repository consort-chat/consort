// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Consort's Tauri shell.
//!
//! Everything here is wiring. The Matrix work lives in `consort-matrix`, which
//! has no dependency on Tauri and can be exercised from a test or a plain
//! binary. Keeping that line sharp is what makes the interesting half testable
//! without driving a webview.

mod commands;
mod events;
mod state;

use std::path::PathBuf;

use consort_matrix::SessionStore;
use tauri::{Manager, WindowEvent};

use crate::state::AppState;

/// Start the application.
pub fn run() {
    init_tracing();

    // Before any TLS. See `consort_matrix::install_crypto_provider`.
    if !consort_matrix::install_crypto_provider() {
        tracing::debug!("a rustls crypto provider was already installed");
    }

    tauri::Builder::default()
        // Must be registered first, before anything touches the data
        // directory.
        //
        // Two copies of Consort would share one SQLite crypto store and one
        // session file. The store has a cross-process lock, so the second copy
        // fails to open it, and both processes then disagree about who is
        // signed in. It gets worse rather than better once verification lands:
        // two processes racing on one crypto store is how device keys get
        // dropped.
        //
        // Rather than make the storage layer safe for concurrent processes,
        // which is a large amount of work for a case nobody wants, there is
        // simply one Consort. A second launch hands its arguments to the first
        // and exits, and the first raises its window, which is also what a user
        // clicking the launcher icon twice actually expects.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tracing::info!("a second instance was launched; focusing the existing window");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let data_dir = resolve_data_dir(app.handle())?;
            let store = SessionStore::new(&data_dir);

            tracing::info!(
                path = %data_dir.display(),
                token_store = ?store.backend_kind(),
                "using application data directory"
            );

            // The handle is the event sink. Cloning it is the documented way
            // to keep one past `setup`, and it is what every background task
            // emits through.
            app.manage(AppState::new(
                store,
                std::sync::Arc::new(app.handle().clone()),
            ));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                tracing::info!(label = window.label(), "window closed");
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_status,
            commands::login,
            commands::logout,
            commands::token_storage,
            commands::resend_state,
            commands::verification_accept,
            commands::verification_start_sas,
            commands::verification_confirm,
            commands::verification_mismatch,
            commands::verification_cancel,
            commands::verification_verify_this_session,
            commands::verification_other_sessions_exist,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Consort");
}

/// The per-user directory holding the session file and the SDK's SQLite stores.
///
/// Created here rather than lazily on first write so that a permissions problem
/// surfaces at startup with a clear path in the message, instead of during a
/// login as a confusing failure to save the session.
fn resolve_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = app.path().app_data_dir()?;
    consort_matrix::atomic::create_dir_private(&dir)?;
    Ok(dir)
}

/// Logging to stderr, filtered by `RUST_LOG`.
///
/// The default keeps matrix-sdk at `warn`. At `info` it narrates every sync
/// response, which buries our own lines during exactly the debugging session
/// where they matter.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_filter()));

    // `try_init` rather than `init`. `init` panics if a subscriber is already
    // installed, and in a test binary one may well be.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// The filter used when `RUST_LOG` says nothing.
fn default_log_filter() -> &'static str {
    "consort_app_lib=info,consort_matrix=info,matrix_sdk=warn"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_log_filter_is_a_valid_env_filter() {
        use tracing_subscriber::EnvFilter;
        EnvFilter::try_new(default_log_filter()).expect("the default filter must parse");
    }

    #[test]
    fn the_default_log_filter_keeps_the_sdk_quiet_but_our_crates_talkative() {
        let filter = default_log_filter();
        assert!(filter.contains("matrix_sdk=warn"));
        assert!(filter.contains("consort_matrix=info"));
        assert!(filter.contains("consort_app_lib=info"));
    }

    #[test]
    fn installing_the_crypto_provider_twice_is_reported_not_fatal() {
        // The first call in a process wins; every later one returns false.
        // `run` relies on that being a debug line rather than a panic.
        let first = consort_matrix::install_crypto_provider();
        let second = consort_matrix::install_crypto_provider();
        assert!(
            first || !first,
            "the first call may go either way in a test binary"
        );
        assert!(
            !second,
            "a second install must report that one already existed"
        );
    }

    #[test]
    fn init_tracing_can_be_called_more_than_once() {
        init_tracing();
        init_tracing();
    }
}
