// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Application state shared by every Tauri command.

use std::sync::Arc;

use consort_matrix::{
    Client, Connection, Rooms, SessionStore, StopReason, backup, rooms, sync, verification,
};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use consort_audio::{AudioCapture, GateConfig};

use crate::events::{AppEvent, EventSink, LatestSink};
use crate::microphone::Microphone;
use crate::settings::SettingsStore;

/// One background task's handle.
///
/// Six of these now, all owned the same way and all aborted at the same two
/// moments, so the abort-and-replace is written once rather than six times
/// with one of them subtly different.
type TaskSlot = Mutex<Option<JoinHandle<()>>>;

/// Adopt a task, stopping whatever was in the slot before it.
async fn replace_task(slot: &TaskSlot, task: JoinHandle<()>) {
    if let Some(previous) = slot.lock().await.replace(task) {
        previous.abort();
    }
}

/// Whether the slot holds a task that has not stopped.
///
/// Test-only. The application starts and stops these through `set_client` and
/// `clear_client`; this is how a test checks that it did.
#[cfg(test)]
async fn task_running(slot: &TaskSlot) -> bool {
    slot.lock()
        .await
        .as_ref()
        .is_some_and(|task| !task.is_finished())
}

/// Stop the task in the slot, reporting whether there was one.
async fn stop_task(slot: &TaskSlot) -> bool {
    match slot.lock().await.take() {
        Some(task) => {
            task.abort();
            true
        }
        None => false,
    }
}

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
    /// The sync loop.
    ///
    /// Same ownership story as `refresh_task` and for the same reason: it
    /// holds a `Client` and watches a channel belonging to that client, so it
    /// cannot end on its own. One per signed-in session, and never two.
    sync_task: TaskSlot,
    /// The watcher reporting whether this session is verified.
    ///
    /// Separate from the sync loop even though both need a live session,
    /// because they answer different questions and fail independently: the
    /// verification state is read from the crypto store and is known before
    /// the first sync response arrives.
    verification_task: TaskSlot,
    /// The watcher for incoming verification requests.
    ///
    /// The one whose abort does more than stop a loop: it owns a task per
    /// verification flow in progress, and dropping it takes those with it.
    /// Without that, signing out in the middle of an emoji comparison leaves
    /// the previous account's flow running for the life of the process, still
    /// holding the client it was started with.
    flow_task: TaskSlot,
    /// The watcher reporting whether room keys are being backed up.
    ///
    /// A fourth channel rather than a field on the verification one, because
    /// the two answer different questions and one can be true while the other
    /// is not. A verified session with no backup still cannot read a word of
    /// history, and reporting that as part of "verified" would bury it.
    backup_task: TaskSlot,
    /// The watcher reporting what rooms the account is in.
    ///
    /// Driven by the same sync responses as `sync_task` and still its own
    /// task, because the two report different things and the room list has to
    /// say something before the first sync arrives. It reads only the local
    /// store, so an account that has synced before is drawn immediately and
    /// correctly while offline.
    rooms_task: TaskSlot,
    /// The way to start a verification rather than answer one.
    ///
    /// Beside `flow_task` rather than inside it because the two are different
    /// things: one is a task to abort, the other is a channel into it. Set and
    /// cleared in the same two places as the task, and only there.
    initiator: Mutex<Option<verification::Initiator>>,
    /// Where events destined for the webview go.
    ///
    /// A trait object rather than an `AppHandle` so this struct can be built
    /// in a test. See `crate::events::EventSink`. Wrapped so that a webview
    /// which subscribed after these tasks started can ask for the current
    /// state instead of waiting for the next change.
    events: Arc<LatestSink>,
    /// Where the audio choices are written down.
    ///
    /// Beside the session store rather than inside it, because the two have
    /// nothing to do with each other beyond sharing a directory. A settings
    /// file is not a secret, it survives a sign-out, and losing it costs
    /// somebody their thresholds rather than their login.
    settings: SettingsStore,
    /// The audio thread, once anything has asked for it.
    ///
    /// Lazy because opening it costs a thread and, on some backends, a
    /// connection to a sound server, and most sessions never open the settings
    /// screen at all. Kept once created, because a person adjusting a device
    /// picker starts and stops the test repeatedly and the slow part is the
    /// sound card rather than the thread.
    ///
    /// A `std::sync::Mutex` rather than tokio's. Nothing here awaits while
    /// holding it, and the commands that reach it are synchronous.
    microphone: std::sync::Mutex<Option<Microphone>>,
}

impl AppState {
    pub fn new(store: SessionStore, settings: SettingsStore, events: Arc<dyn EventSink>) -> Self {
        Self {
            client: RwLock::new(None),
            store,
            auth_gate: Mutex::new(()),
            refresh_task: Mutex::new(None),
            sync_task: Mutex::new(None),
            verification_task: Mutex::new(None),
            flow_task: Mutex::new(None),
            backup_task: Mutex::new(None),
            rooms_task: Mutex::new(None),
            initiator: Mutex::new(None),
            events: Arc::new(LatestSink::new(events)),
            settings,
            microphone: std::sync::Mutex::new(None),
        }
    }

    /// Begin the microphone test, opening the audio thread on first use.
    ///
    /// `capture` is a closure rather than a value so that nothing builds a
    /// backend on the calls that do not need one, which is every call after
    /// the first.
    ///
    /// `device` is a name to open, or `None` for whatever the host calls its
    /// default. Resolving a saved choice into one or the other is
    /// `Selection::name_to_open`, and belongs at the call site: this has no
    /// business reading settings.
    pub fn start_microphone(
        &self,
        capture: impl FnOnce() -> Box<dyn AudioCapture>,
        device: Option<String>,
        gate: GateConfig,
    ) {
        let mut slot = self
            .microphone
            .lock()
            .expect("the microphone mutex is never poisoned");
        let microphone =
            slot.get_or_insert_with(|| Microphone::spawn(capture(), self.events.clone()));
        microphone.start(device, gate);
    }

    /// End the microphone test, releasing the device.
    ///
    /// A no-op when nothing was ever started, which is what closing the
    /// settings screen does whether or not anybody pressed the button. The
    /// thread stays alive for next time; only the device is given back.
    pub fn stop_microphone(&self) {
        if let Some(microphone) = self
            .microphone
            .lock()
            .expect("the microphone mutex is never poisoned")
            .as_ref()
        {
            microphone.stop();
        }
    }

    /// Send the current state of every push channel again.
    ///
    /// The frontend calls this once it has subscribed. Without it the states
    /// published while the webview was still loading are lost, and the
    /// interface sits on its initial guess until something happens to change.
    pub fn resend_state(&self) {
        self.events.resend();
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn settings(&self) -> &SettingsStore {
        &self.settings
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

    /// Adopt a signed-in client, and start the background work that goes with
    /// one: persisting token rotations, and syncing.
    ///
    /// Replaces whatever was there, aborting the previous account's tasks
    /// first.
    pub async fn set_client(&self, client: Client) {
        *self.client.write().await = Some(client.clone());

        replace_task(
            &self.refresh_task,
            tokio::spawn(consort_matrix::auth::persist_token_refreshes(
                client.clone(),
                self.store.clone(),
            )),
        )
        .await;

        let events = self.events.clone();
        replace_task(
            &self.sync_task,
            sync::start(client.clone(), move |state| {
                events.emit(AppEvent::Connection(state));
            }),
        )
        .await;

        let events = self.events.clone();
        replace_task(
            &self.verification_task,
            verification::watch(client.clone(), move |state| {
                events.emit(AppEvent::Verification(state));
            }),
        )
        .await;

        let events = self.events.clone();
        replace_task(
            &self.backup_task,
            backup::watch(client.clone(), move |state| {
                events.emit(AppEvent::KeyBackup(state));
            }),
        )
        .await;

        let events = self.events.clone();
        replace_task(
            &self.rooms_task,
            rooms::watch(client.clone(), move |list| {
                events.emit(AppEvent::Rooms(list));
            }),
        )
        .await;

        let events = self.events.clone();
        let (flow_task, initiator) = verification::supervise(client, move |flow| {
            events.emit(AppEvent::VerificationFlow(flow));
        });
        replace_task(&self.flow_task, flow_task).await;
        *self.initiator.lock().await = Some(initiator);
    }

    /// Ask this account's other sessions to verify this one.
    ///
    /// Goes through the initiator rather than the client, because a flow this
    /// session starts has to be owned by the same set as one that arrives.
    /// Nothing echoes our own request back to us, so the supervising task
    /// would otherwise never hear about it and the interface would show
    /// nothing at all.
    pub async fn verify_this_session(&self) -> Result<(), consort_matrix::Error> {
        match self.initiator.lock().await.as_ref() {
            Some(initiator) => initiator.verify_this_session().await,
            None => Err(consort_matrix::Error::NotLoggedIn),
        }
    }

    /// Forget the client and stop its background tasks.
    pub async fn clear_client(&self) {
        stop_task(&self.refresh_task).await;

        // No parting word for either verification channel. There is nothing
        // left to say about a session that has gone, and the next sign-in
        // publishes its own state as soon as it has one. A flow that was
        // halfway through is over rather than cancelled by anybody, and
        // announcing a cancellation nobody performed would be a lie about
        // whose decision it was.
        stop_task(&self.verification_task).await;
        stop_task(&self.flow_task).await;
        stop_task(&self.backup_task).await;
        *self.initiator.lock().await = None;

        // The room list does get a parting word, unlike the two verification
        // channels, because the last one is retained for a late subscriber and
        // it names somebody's rooms. Signing in as a second account would
        // otherwise show the first account's spaces for the moment between the
        // webview asking to be caught up and the new watcher's first report.
        if stop_task(&self.rooms_task).await {
            self.events.emit(AppEvent::Rooms(Rooms::default()));
        }

        // Aborting the sync task means it never runs its own final report, so
        // the last thing the frontend heard was whatever the loop was doing
        // when the user pressed sign out. Say what happened instead of leaving
        // a stale "live" behind.
        //
        // Only when there was a loop to stop. Startup calls this after a
        // restore that did not work, and announcing a sign-out to somebody who
        // was never signed in is a notification about nothing.
        if stop_task(&self.sync_task).await {
            self.events.emit(AppEvent::Connection(Connection::Stopped {
                reason: StopReason::SignedOut,
            }));
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
        task_running(&self.refresh_task).await
    }

    /// Whether a sync loop is currently running.
    ///
    /// Test-only, for the same reason as `has_refresh_task`.
    #[cfg(test)]
    pub async fn has_sync_task(&self) -> bool {
        task_running(&self.sync_task).await
    }

    /// Whether a room list watcher is currently running.
    ///
    /// Test-only, for the same reason as `has_refresh_task`.
    #[cfg(test)]
    pub async fn has_rooms_task(&self) -> bool {
        task_running(&self.rooms_task).await
    }

    /// Whether a verification watcher is currently running.
    ///
    /// Test-only, for the same reason as `has_refresh_task`.
    #[cfg(test)]
    pub async fn has_verification_task(&self) -> bool {
        task_running(&self.verification_task).await
    }

    /// Whether the watcher for incoming verification requests is running.
    ///
    /// Test-only, for the same reason as `has_refresh_task`.
    #[cfg(test)]
    pub async fn has_flow_task(&self) -> bool {
        task_running(&self.flow_task).await
    }

    /// Whether the key backup watcher is running.
    ///
    /// Test-only, for the same reason as `has_refresh_task`.
    #[cfg(test)]
    pub async fn has_backup_task(&self) -> bool {
        task_running(&self.backup_task).await
    }

    /// Which task is currently the sync loop.
    ///
    /// Test-only. Enough to tell "the same loop is still running" from "a new
    /// one replaced it", which `has_sync_task` cannot.
    #[cfg(test)]
    pub async fn sync_task_id(&self) -> Option<tokio::task::Id> {
        self.sync_task.lock().await.as_ref().map(|task| task.id())
    }

    /// Install a stand-in for the sync loop.
    ///
    /// Test-only. `clear_client` behaves differently depending on whether a
    /// loop was running, and reaching that branch otherwise needs a real
    /// `Client`, which needs a homeserver. The task never finishes, which is
    /// what a real sync loop does too.
    #[cfg(test)]
    pub async fn pretend_to_be_signed_in(&self) {
        *self.sync_task.lock().await = Some(tokio::spawn(std::future::pending()));
        *self.verification_task.lock().await = Some(tokio::spawn(std::future::pending()));
        *self.flow_task.lock().await = Some(tokio::spawn(std::future::pending()));
        *self.backup_task.lock().await = Some(tokio::spawn(std::future::pending()));
        *self.rooms_task.lock().await = Some(tokio::spawn(std::future::pending()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RecordingSink;
    use consort_matrix::StopReason;
    use consort_matrix::secrets::MemoryBackend;
    use std::sync::Arc;

    fn state() -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()));
        let sink = Arc::new(RecordingSink::new());
        let settings = SettingsStore::at(dir.path());
        (dir, AppState::new(store, settings, sink.clone()), sink)
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_client() {
        let (_dir, state, _sink) = state();
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn clearing_a_client_that_was_never_set_is_fine() {
        let (_dir, state, _sink) = state();
        state.clear_client().await;
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn the_auth_gate_is_open_when_nothing_is_happening() {
        let (_dir, state, _sink) = state();
        assert!(!state.auth_in_progress());
    }

    #[tokio::test]
    async fn the_auth_gate_reports_itself_held() {
        let (_dir, state, _sink) = state();
        let _guard = state.lock_auth().await;
        assert!(state.auth_in_progress());
    }

    #[tokio::test]
    async fn the_auth_gate_reopens_when_the_guard_drops() {
        let (_dir, state, _sink) = state();
        {
            let _guard = state.lock_auth().await;
        }
        assert!(!state.auth_in_progress());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_auth_gate_serialises_two_concurrent_callers() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let (_dir, state, _sink) = state();
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
        let (_dir, state, _sink) = state();
        assert!(!state.has_refresh_task().await);
    }

    #[tokio::test]
    async fn clearing_a_client_with_no_task_running_is_fine() {
        let (_dir, state, _sink) = state();
        state.clear_client().await;
        assert!(!state.has_refresh_task().await);
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_sync_task() {
        let (_dir, state, _sink) = state();
        assert!(!state.has_sync_task().await);
    }

    #[tokio::test]
    async fn clearing_a_client_with_no_sync_task_running_is_fine() {
        let (_dir, state, _sink) = state();
        state.clear_client().await;
        assert!(!state.has_sync_task().await);
    }

    #[tokio::test]
    async fn signing_out_tells_the_frontend_the_connection_stopped() {
        // The sync task is aborted rather than allowed to finish, so it never
        // gets to report anything itself. Without this the last thing the UI
        // heard was "live", and it would still be saying so on the login
        // screen.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert_eq!(
            sink.last_connection(),
            Some(Connection::Stopped {
                reason: StopReason::SignedOut
            })
        );
    }

    #[tokio::test]
    async fn clearing_a_state_that_was_never_signed_in_says_nothing() {
        // Startup calls this on a failed restore. Announcing a sign-out to a
        // user who was never signed in is a notification about nothing.
        let (_dir, state, sink) = state();

        state.clear_client().await;

        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_verification_watcher() {
        let (_dir, state, _sink) = state();
        assert!(!state.has_verification_task().await);
    }

    #[tokio::test]
    async fn signing_out_stops_the_verification_watcher() {
        // It holds a `Client`, so a watcher left running keeps the previous
        // account's SQLite handles open for the life of the process.
        let (_dir, state, _sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert!(!state.has_verification_task().await);
    }

    #[tokio::test]
    async fn signing_out_says_nothing_about_verification() {
        // There is no honest thing to say. The session is gone, so it is
        // neither verified nor unverified, and the next sign-in publishes its
        // own answer as soon as it has one.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert_eq!(sink.last_verification(), None);
    }

    #[tokio::test]
    async fn a_fresh_state_watches_for_no_verification_requests() {
        let (_dir, state, _sink) = state();
        assert!(!state.has_flow_task().await);
    }

    #[tokio::test]
    async fn signing_out_stops_watching_for_verification_requests() {
        // Stronger than the other three. This task owns every flow task it
        // started, each of which holds the `Client` and watches a stream
        // belonging to that same client, so nothing else can end them.
        let (_dir, state, _sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert!(!state.has_flow_task().await);
    }

    #[tokio::test]
    async fn a_fresh_state_watches_nothing_about_key_backup() {
        let (_dir, state, _sink) = state();
        assert!(!state.has_backup_task().await);
    }

    #[tokio::test]
    async fn signing_out_stops_the_key_backup_watcher() {
        // Same as the other three: it holds a `Client` and watches a stream
        // belonging to it, so nothing else can end it.
        let (_dir, state, _sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert!(!state.has_backup_task().await);
    }

    #[tokio::test]
    async fn signing_out_says_nothing_about_key_backup() {
        // Same reasoning as the verification state. Nothing true is left to
        // say about the keys of a session that has gone, and "your messages
        // are not backed up" is the wrong last word to leave on screen.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert_eq!(sink.last_key_backup(), None);
    }

    #[tokio::test]
    async fn a_fresh_state_has_no_room_list_watcher() {
        let (_dir, state, _sink) = state();
        assert!(!state.has_rooms_task().await);
    }

    #[tokio::test]
    async fn signing_out_stops_the_room_list_watcher() {
        let (_dir, state, _sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert!(!state.has_rooms_task().await);
    }

    #[tokio::test]
    async fn signing_out_empties_the_room_list() {
        // The one channel that does get a parting word, and the reason is the
        // catch-up. The last room list is retained for a webview that
        // subscribes late, and it names somebody's rooms. Left in place,
        // signing in as a second account shows the first account's spaces
        // until the new watcher gets its first report out.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert_eq!(sink.last_rooms(), Some(Rooms::default()));
    }

    #[tokio::test]
    async fn signing_out_says_nothing_about_a_flow() {
        // Same reasoning as the verification state: there is nothing to say
        // about a session that has gone, and a flow it was halfway through is
        // over rather than cancelled by anybody.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;

        state.clear_client().await;

        assert!(
            !sink
                .events()
                .iter()
                .any(|event| event.channel() == AppEvent::VERIFICATION_FLOW)
        );
    }

    #[tokio::test]
    async fn asking_to_be_caught_up_repeats_the_current_state() {
        // The webview subscribes whenever its JavaScript gets there, which on
        // a restored session is long after the background tasks published
        // their first states. Without this it never hears them.
        let (_dir, state, sink) = state();
        state.pretend_to_be_signed_in().await;
        state.clear_client().await;

        state.resend_state();

        let stopped = Connection::Stopped {
            reason: StopReason::SignedOut,
        };
        assert_eq!(
            sink.events(),
            vec![
                AppEvent::Rooms(Rooms::default()),
                AppEvent::Connection(stopped.clone()),
                AppEvent::Rooms(Rooms::default()),
                AppEvent::Connection(stopped),
            ],
            "both channels should be caught up, in the order they first spoke"
        );
    }

    #[tokio::test]
    async fn catching_up_a_state_that_has_said_nothing_stays_quiet() {
        let (_dir, state, sink) = state();

        state.resend_state();

        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn the_store_is_the_one_it_was_built_with() {
        let (dir, state, _sink) = state();
        assert_eq!(
            state.store().session_file(),
            dir.path().join("session.json")
        );
    }
}
