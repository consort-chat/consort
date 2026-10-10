// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Matrix authentication and session persistence for Consort.
//!
//! Knows nothing about Tauri, the UI or voice, so it can be driven from a
//! plain `main` when something needs reproducing outside the app. The access
//! token goes to the platform keyring, or to an owner-only file when none
//! answers: see [`secrets`]. The room keys go to the SDK's SQLite stores under
//! 32 random bytes kept beside it: see [`store_key`].
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
//! A session whose store key has gone is not a session: [`SessionStore::load`]
//! reports it as signed out, and the next login builds a store from scratch.

pub mod atomic;
pub mod auth;
pub mod backup;
pub mod calls;
pub mod error;
mod media;
pub mod notifications;
pub mod receipts;
pub mod rooms;
pub mod secrets;
pub mod session;
pub mod slash;
pub mod store_key;
pub mod sync;
pub mod timeline;
pub mod verification;

pub use auth::{Credentials, Profile};
pub use backup::KeyBackup;
pub use calls::{CallReadiness, JoinVerdict};
pub use error::{Error, Result};
pub use notifications::Notification;
pub use receipts::count_unread;
pub use rooms::{Channel, ChannelKind, Participant, Rooms, Space};
pub use secrets::{Backend, BackendKind};
pub use session::{KEYRING_SERVICE, SessionStore, StoredSession};
pub use slash::Effect;
pub use store_key::StoreKey;
pub use sync::{Connection, StopReason};
pub use timeline::{Message, MessageKind, ReadOn, Readers, Thread, Timeline, Typing};
pub use verification::{Flow, FlowState, SessionVerification};

// Re-exported so a consumer cannot depend on a different matrix-sdk rev. The
// pin comment in the workspace manifest explains why that would break.
pub use matrix_sdk::Client;

/// Install the process-wide rustls crypto provider; `false` if one was already
/// there. Call once from `main` before any TLS: both `ring` and `aws-lc-rs` are
/// compiled in, and rustls panics on the first connection rather than guess.
pub fn install_crypto_provider() -> bool {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .is_ok()
}
