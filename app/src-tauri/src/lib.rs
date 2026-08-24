// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Consort's Tauri shell.
//!
//! Everything here is wiring. The Matrix work lives in `consort-matrix`, which
//! has no dependency on Tauri and can be exercised from a test or a plain
//! binary. Keeping that line sharp is what makes the interesting half testable
//! without driving a webview.

mod commands;
mod state;

use std::path::PathBuf;

use consort_matrix::SessionStore;
use tauri::Manager;

use crate::state::AppState;

/// Start the application.
pub fn run() {
    init_tracing();

    // Before any TLS. See `consort_matrix::install_crypto_provider`.
    if !consort_matrix::install_crypto_provider() {
        tracing::debug!("a rustls crypto provider was already installed");
    }

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = resolve_data_dir(app.handle())?;
            tracing::info!(path = %data_dir.display(), "using application data directory");
            app.manage(AppState::new(SessionStore::new(data_dir)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_status,
            commands::login,
            commands::logout,
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
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Logging to stderr, filtered by `RUST_LOG`.
///
/// The default keeps matrix-sdk at `warn`. At `info` it narrates every sync
/// response, which buries our own lines during exactly the debugging session
/// where they matter.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("consort_app_lib=info,consort_matrix=info,matrix_sdk=warn")
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
