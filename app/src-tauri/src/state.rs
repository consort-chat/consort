// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Application state shared by every Tauri command.

use consort_matrix::{Client, SessionStore};
use tokio::sync::RwLock;

/// The one piece of long-lived state the app has.
///
/// The `Client` is `Send + Sync + Clone`, so it lives in ordinary shared state
/// and commands can hold it across `.await`.
///
/// That stops being true when the voice layer arrives. `Call::join` drives
/// `!Send` futures through `spawn_local` and panics outside a
/// `tokio::task::LocalSet`, while Tauri commands run on a multi-thread runtime
/// and require `Send`. The call cannot live in this struct. It belongs behind a
/// dedicated thread owning a current-thread runtime plus a `LocalSet`, reached
/// through a command channel, with only the channel handle stored here.
///
/// Recorded now because the shape of that constraint is easy to discover the
/// expensive way, halfway through wiring the call into a command that will
/// never compile.
pub struct AppState {
    client: RwLock<Option<Client>>,
    store: SessionStore,
}

impl AppState {
    pub fn new(store: SessionStore) -> Self {
        Self {
            client: RwLock::new(None),
            store,
        }
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// The signed-in client, if there is one.
    pub async fn client(&self) -> Option<Client> {
        self.client.read().await.clone()
    }

    pub async fn set_client(&self, client: Client) {
        *self.client.write().await = Some(client);
    }

    pub async fn clear_client(&self) {
        *self.client.write().await = None;
    }
}
