// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Application state shared by every Tauri command.

use consort_matrix::{Client, SessionStore};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

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
    /// Held for the duration of a login or a logout.
    ///
    /// The frontend disables its button while a login is in flight, but the
    /// frontend is not the only thing that can call the command: the webview
    /// can invoke it directly, and a double-submit that slips past the React
    /// state would run two logins concurrently. Two logins means two devices
    /// registered on the homeserver and two writers racing on the session
    /// store, one of which wins arbitrarily.
    ///
    /// A separate mutex from the `RwLock` above because it guards the whole
    /// operation, network round trips included, not just the moment the client
    /// is swapped in.
    auth_gate: Mutex<()>,
    /// The background task that writes rotated tokens back to the store.
    ///
    /// Owned here because it cannot stop by itself. It holds a `Client`, and
    /// the channel it watches belongs to that same client, so the channel
    /// never closes while the task is alive. Without something aborting it, a
    /// sign-out followed by a sign-in leaves the previous account's task
    /// running forever, still holding its client and its SQLite handles.
    refresh_task: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(store: SessionStore) -> Self {
        Self {
            client: RwLock::new(None),
            store,
            auth_gate: Mutex::new(()),
            refresh_task: Mutex::new(None),
        }
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// Serialise sign-in and sign-out against each other.
    ///
    /// Callers hold the returned guard for the whole operation.
    pub async fn lock_auth(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.auth_gate.lock().await
    }

    /// Whether an authentication operation is currently running.
    ///
    /// Test-only. Taking the lock is what enforces exclusion; this just lets a
    /// test observe that it is held without blocking on it.
    #[cfg(test)]
    pub fn auth_in_progress(&self) -> bool {
        self.auth_gate.try_lock().is_err()
    }

    /// The signed-in client, if there is one.
    pub async fn client(&self) -> Option<Client> {
        self.client.read().await.clone()
    }

    /// Adopt a signed-in client and start persisting its token rotations.
    ///
    /// Replaces whatever was there, aborting the previous account's task
    /// first.
    pub async fn set_client(&self, client: Client) {
        *self.client.write().await = Some(client.clone());

        let task = tokio::spawn(consort_matrix::auth::persist_token_refreshes(
            client,
            self.store.clone(),
        ));
        if let Some(previous) = self.refresh_task.lock().await.replace(task) {
            previous.abort();
        }
    }

    /// Forget the client and stop its background task.
    pub async fn clear_client(&self) {
        if let Some(task) = self.refresh_task.lock().await.take() {
            task.abort();
        }
        *self.client.write().await = None;
    }

    /// Whether a token-refresh task is currently running.
    ///
    /// Test-only. The application starts and stops the task through
    /// `set_client` and `clear_client`; this exists so a test can check that
    /// it did.
    #[cfg(test)]
    pub async fn has_refresh_task(&self) -> bool {
        self.refresh_task
            .lock()
            .await
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consort_matrix::secrets::MemoryBackend;
    use std::sync::Arc;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()));
        (dir, AppState::new(store))
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_client() {
        let (_dir, state) = state();
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn clearing_a_client_that_was_never_set_is_fine() {
        let (_dir, state) = state();
        state.clear_client().await;
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn the_auth_gate_is_open_when_nothing_is_happening() {
        let (_dir, state) = state();
        assert!(!state.auth_in_progress());
    }

    #[tokio::test]
    async fn the_auth_gate_reports_itself_held() {
        let (_dir, state) = state();
        let _guard = state.lock_auth().await;
        assert!(state.auth_in_progress());
    }

    #[tokio::test]
    async fn the_auth_gate_reopens_when_the_guard_drops() {
        let (_dir, state) = state();
        {
            let _guard = state.lock_auth().await;
        }
        assert!(!state.auth_in_progress());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_auth_gate_serialises_two_concurrent_callers() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (_dir, state) = state();
        let state = Arc::new(state);
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let state = state.clone();
                let concurrent = concurrent.clone();
                let peak = peak.clone();
                tokio::spawn(async move {
                    let _guard = state.lock_auth().await;
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();

        for task in tasks {
            task.await.unwrap();
        }

        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "two logins ran at the same time"
        );
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_refresh_task() {
        let (_dir, state) = state();
        assert!(!state.has_refresh_task().await);
    }

    #[tokio::test]
    async fn clearing_a_client_with_no_task_running_is_fine() {
        let (_dir, state) = state();
        state.clear_client().await;
        assert!(!state.has_refresh_task().await);
    }

    #[tokio::test]
    async fn the_store_is_the_one_it_was_built_with() {
        let (dir, state) = state();
        assert_eq!(
            state.store().session_file(),
            dir.path().join("session.json")
        );
    }
}
