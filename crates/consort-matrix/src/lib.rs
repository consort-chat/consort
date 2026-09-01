// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Matrix authentication and session persistence for Consort.
//!
//! This crate deliberately knows nothing about Tauri, about the UI, or about
//! voice. It is the piece that can be tested with `cargo test` and driven from
//! a plain `main` when something needs reproducing outside the app.
//!
//! ```no_run
//! use consort_matrix::{Credentials, SessionStore, auth};
//!
//! # async fn example() -> consort_matrix::Result<()> {
//! let store = SessionStore::new("/tmp/consort");
//!
//! // Returning users: no password needed.
//! let (client, profile) = match store.load()? {
//!     Some(stored) => auth::restore(&stored).await?,
//!     None => {
//!         auth::login(&store, &Credentials {
//!             server: "example.org".to_owned(),
//!             username: "bob".to_owned(),
//!             password: "hunter2".to_owned(),
//!         })
//!         .await?
//!     }
//! };
//!
//! println!("signed in as {}", profile.user_id);
//! # let _ = client;
//! # Ok(())
//! # }
//! ```
//!
//! ## Where the access token goes
//!
//! [`SessionStore::new`] puts it in the platform keyring: Secret Service on
//! Linux and the BSDs, the Credential Manager on Windows, Keychain on macOS.
//! When no keyring answers, and on a bare window manager or in a container
//! none will, it falls back to an owner-only file and says so through
//! [`SessionStore::backend_kind`]. See [`secrets`] for why that fallback
//! exists rather than a hard failure.
//!
//! ## Where the room keys go
//!
//! Into the same SQLite databases the SDK keeps everything else in, encrypted
//! with 32 random bytes that go to the secret store beside the access token.
//! See [`store_key`] for why a key rather than a passphrase, and for what that
//! does and does not protect. A session whose key has gone is not a session:
//! [`SessionStore::load`] reports it as signed out, and the next login builds a
//! store from scratch.
//!
//! ## The rustls provider
//!
//! Nothing here installs a rustls `CryptoProvider`, but something must, exactly
//! once per process, before the first TLS connection. It is the binary's job,
//! not a library's. See [`install_crypto_provider`] for why it is not automatic.

pub mod atomic;
pub mod auth;
pub mod backup;
pub mod calls;
pub mod error;
pub mod rooms;
pub mod secrets;
pub mod session;
pub mod store_key;
pub mod sync;
pub mod verification;

pub use auth::{Credentials, Profile};
pub use backup::KeyBackup;
pub use calls::{CallReadiness, JoinVerdict};
pub use error::{Error, Result};
pub use rooms::{Channel, ChannelKind, Participant, Rooms, Space};
pub use secrets::{Backend, BackendKind};
pub use session::{KEYRING_SERVICE, SessionStore, StoredSession};
pub use store_key::StoreKey;
pub use sync::{Connection, StopReason};
pub use verification::{Flow, FlowState, SessionVerification};

// Re-exported so a consumer holding a `Client` needs only this crate as a
// dependency, and cannot accidentally depend on a *different* matrix-sdk rev.
// The pin comment in the workspace manifest explains why that would break.
pub use matrix_sdk::Client;

/// Install the process-wide rustls crypto provider.
///
/// Call once, before any TLS happens, from `main`.
///
/// This is not done automatically on first use because the choice is
/// process-global and belongs to the binary. It matters more than it looks:
/// once the voice layer lands, the dependency graph enables *both* rustls
/// backends, `ring` by way of livekit and reqwest, and `aws-lc-rs` by way of
/// matrix-sdk. With two compiled in, rustls refuses to guess and panics on the
/// first connection instead. `aws-lc-rs` is the one selected here because it is
/// what matrix-sdk expects by default.
///
/// Returns `false` if a provider was already installed, which is not an error:
/// it means something else got there first, and the process has exactly one
/// provider either way.
pub fn install_crypto_provider() -> bool {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .is_ok()
}
