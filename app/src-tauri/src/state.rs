// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Application state shared by every Tauri command.

use std::sync::Arc;

use consort_call::{CallEvent, CallTransport, Microphone};
use consort_matrix::{
    CallReadiness, Client, Connection, Rooms, SessionStore, StopReason, Timeline, backup, calls,
    rooms, sync, timeline, verification,
};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use consort_audio::{GateConfig, GatedSink, Talking, Voices};

use crate::audio::Backends;
use crate::call::CallBridge;
use crate::ears::speakers;
use crate::events::{AppEvent, CallRefused, EventSink, LatestSink};
use crate::settings::SettingsStore;
use crate::sound::Sound;

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

/// How to open the microphone for a call.
///
/// Built by the command layer, which is the half that knows what devices exist
/// and how to open one, and read on the call thread's pump, which is the half
/// that knows when. Rebuilt on every connect rather than remembered, so a
/// device chosen between two calls is the one the second call opens.
pub struct CallAudio {
    /// The device to open, or `None` for whatever the host calls its default.
    pub device: Option<String>,
    /// The output to play the call out of, or `None` for the host's default.
    ///
    /// Resolved with the input at the same moment and from the same saved
    /// settings, so a call cannot end up listening to one screen's answer and
    /// speaking out of another's.
    pub output: Option<String>,
    pub gate: GateConfig,
    /// The sound card. A closure because the audio thread is built at most
    /// once per process and almost every call finds it already there.
    pub backends: Box<dyn Fn() -> Backends + Send>,
    /// This session's own Matrix user ID.
    ///
    /// Carried because the audio thread is where this session's own ring is
    /// decided, and the frames it decides from say nothing about whose they
    /// are. Read from the signed-in client at the moment of joining, which is
    /// the last layer that has one.
    pub us: String,
}

/// The microphone sink a call is fed from, which is also where this session's
/// own green ring is decided.
///
/// Two jobs on one closure because they are the same frame seen twice, and
/// because this is the only place both are available. The frames are the
/// gate's output, so they are silence while it is shut, which is what lets the
/// ring be measured exactly as everybody else's is rather than from the gate's
/// verdict. That distinction matters with voice activity switched off: the
/// gate then reports every frame open, and a ring drawn from the verdict would
/// simply stay lit.
///
/// It is also the tick. Frames arrive every 10 ms for as long as a call has
/// the microphone, whether or not anybody is saying anything, so this is the
/// one clock in the building that can put a ring out again. Nothing here
/// blocks: `advance` answers `None` on all but a handful of frames a second.
fn speaking_sink(
    queue: Microphone,
    talking: Talking,
    us: String,
    events: Arc<LatestSink>,
) -> GatedSink {
    Box::new(move |samples, open| {
        // Before the tally, because this is the frame's reason for existing.
        queue.offer(samples, open);

        talking.heard(&us, samples);
        if let Some(user_ids) = talking.advance() {
            events.emit(AppEvent::Speaking(user_ids));
        }
    })
}

/// The one piece of long-lived state the app has.
///
/// The `Client` is `Send + Sync + Clone`, so it lives in ordinary shared state
/// and commands can hold it across `.await`.
///
/// The call is the exception, and it is why [`CallBridge`] exists rather than
/// a field holding a call. `Call::join` drives `!Send` futures through
/// `spawn_local` and panics outside a `tokio::task::LocalSet`, while Tauri
/// commands run on a multi-thread runtime and require `Send`. So the call
/// lives on a thread of its own and only the handle is here.
pub struct AppState {
    client: RwLock<Option<Client>>,
    /// The attachments the media scheme is holding.
    ///
    /// Here rather than in `media.rs` because a range request arrives outside
    /// any command, so the protocol handler reaches it the same way a command
    /// reaches everything else: through the managed state.
    media: Mutex<crate::media::Cache>,
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
    /// The watcher reporting whether a call from this session could be heard.
    ///
    /// A fifth task rather than a reading taken off `verification_task`,
    /// because `CallReadiness` draws a distinction `SessionVerification`
    /// cannot: an account with no cross-signing identity at all is fixed
    /// somewhere else entirely from a session that merely has not been
    /// verified yet, and the interface has to say which.
    readiness_task: TaskSlot,
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
    /// The microphone, and the record of who currently wants it open.
    ///
    /// An `Arc` because the call thread's pump holds one too: opening and
    /// closing the device is driven by call events, in the order the call
    /// thread produced them, which is the only ordering that cannot race a
    /// click against a join. See [`crate::sound`].
    sound: Arc<Sound>,
    /// The queue carrying captured audio from the audio thread to the call.
    ///
    /// Built once and cloned, because both ends outlive any one call: the
    /// audio thread fills it whenever a call is up, and the call thread drains
    /// it. Empty and harmless the rest of the time.
    microphone: Microphone,
    /// The mixer carrying everybody else's audio from the call to the audio
    /// thread.
    ///
    /// The microphone queue's opposite number, built once and cloned for the
    /// same reason: both ends outlive any one call. The call thread fills it
    /// and the sound card drains it, and it is empty and harmless in between.
    voices: Voices,
    /// Whether a call makes a sound when somebody walks into it.
    ///
    /// Lifted out of the settings file into an atomic because it is read on
    /// the call thread, at the moment a roster changes, and written from
    /// whichever thread saved the settings. Reading the file there would be a
    /// disk touch in the middle of a call for one boolean.
    ///
    /// Seeded from the file at startup and kept in step by
    /// `commands::set_audio_settings_for`, which is the only thing that writes
    /// it.
    chiming: crate::ears::Wanted,
    /// Whether the spoken notifications are switched on, on the same terms as
    /// `chiming` and for the same reasons. A second flag rather than a second
    /// meaning on the first, because the two settings are independent and a
    /// call thread that shared one could not tell them apart.
    speaking: crate::ears::Wanted,
    /// How loud each person in a call should be.
    ///
    /// Held here rather than inside the call, because what somebody chose
    /// about a person is not a fact about the call they happened to choose it
    /// in: it has to survive leaving, rejoining, and the application being shut
    /// for a week. See [`crate::ears::Levels`].
    levels: Arc<crate::ears::Levels>,
    /// Who is currently audible, which is what the green rings are drawn from.
    ///
    /// Held here rather than inside the call because its two writers are on
    /// different threads and neither belongs to the call: everybody else's
    /// frames are tallied beside the mixer, and this session's own are tallied
    /// on the audio thread as they leave. See [`consort_audio::talking`].
    talking: Talking,
    /// How the microphone should be opened for the call currently being
    /// joined. See [`CallAudio`].
    call_audio: Arc<std::sync::Mutex<Option<CallAudio>>>,
    /// The room whose messages are currently being watched, if any.
    ///
    /// One at a time, and replacing it is how a room change is done: dropping
    /// a [`timeline::Watch`] ends its task, so there is no
    /// path that leaves two watchers publishing to one channel.
    ///
    /// A `std::sync::Mutex`, like the call below and for the same reason:
    /// nothing here awaits while holding it.
    timeline: std::sync::Mutex<Option<timeline::Watch>>,
    /// The call thread, once something has asked to join a channel.
    ///
    /// Lazy for the same reason the audio thread is, and more so: it holds a
    /// `Client` and a transport, and most sessions never join a call at all.
    /// Dropped on sign-out, which unwinds the membership.
    ///
    /// A `std::sync::Mutex` rather than tokio's. Nothing here awaits while
    /// holding it, and the commands that reach it are synchronous.
    call: std::sync::Mutex<Option<CallBridge>>,
}

impl AppState {
    pub fn new(store: SessionStore, settings: SettingsStore, events: Arc<dyn EventSink>) -> Self {
        let events = Arc::new(LatestSink::new(events));
        // Read once here rather than defaulted, so a call joined before
        // anybody opens the settings screen already honours what the file
        // says.
        let audio = settings.load().audio;
        let chiming = Arc::new(std::sync::atomic::AtomicBool::new(audio.call_sounds));
        let speaking = Arc::new(std::sync::atomic::AtomicBool::new(audio.call_voices));

        // Likewise read once here. A call joined before anybody opens the
        // settings screen has to be at the volume the file says, and the
        // alternative is a first arrival at full volume followed by a
        // correction, which is precisely the sound somebody turned it down to
        // avoid.
        let voices = Voices::new();
        voices.set_output_level(audio.output_volume);
        voices.set_notification_level(audio.notification_volume);
        let levels = crate::ears::Levels::new(voices.clone(), audio.person_volumes.clone());

        Self {
            client: RwLock::new(None),
            media: Mutex::new(crate::media::Cache::new()),
            store,
            auth_gate: Mutex::new(()),
            refresh_task: Mutex::new(None),
            sync_task: Mutex::new(None),
            verification_task: Mutex::new(None),
            readiness_task: Mutex::new(None),
            flow_task: Mutex::new(None),
            backup_task: Mutex::new(None),
            rooms_task: Mutex::new(None),
            initiator: Mutex::new(None),
            sound: Arc::new(Sound::new(events.clone())),
            events,
            settings,
            microphone: Microphone::new(),
            voices,
            chiming,
            speaking,
            levels,
            talking: Talking::new(),
            call_audio: Arc::new(std::sync::Mutex::new(None)),
            timeline: std::sync::Mutex::new(None),
            call: std::sync::Mutex::new(None),
        }
    }

    /// Begin the microphone test, opening the audio thread on first use.
    ///
    /// `device` is a name to open, or `None` for whatever the host calls its
    /// default. Resolving a saved choice into one or the other is
    /// `Selection::name_to_open`, and belongs at the call site: this has no
    /// business reading settings.
    pub fn start_microphone(
        &self,
        backends: impl FnOnce() -> Backends,
        device: Option<String>,
        gate: GateConfig,
    ) {
        self.sound.start_test(backends, device, gate);
    }

    /// End the microphone test, releasing the device.
    ///
    /// A no-op when nothing was ever started, which is what closing the
    /// settings screen does whether or not anybody pressed the button. The
    /// thread stays alive for next time, and so does the device when a call
    /// still wants it.
    pub fn stop_microphone(&self) {
        self.sound.stop_test();
    }

    /// Retune the running gate, leaving the device open.
    ///
    /// A no-op when nothing is running, because the tuning is handed to
    /// [`start_microphone`](Self::start_microphone) anyway and there is
    /// nothing here to remember it with.
    pub fn retune_gate(&self, gate: GateConfig) {
        self.sound.retune(gate);
    }

    /// Play the test chime, opening the audio thread on first use.
    ///
    /// The output half of what `start_microphone` does for the input half, and
    /// through the same thread: a cpal stream is `!Send` in either direction.
    pub fn play_test_tone(&self, backends: impl FnOnce() -> Backends, device: Option<String>) {
        self.sound.play_tone(backends, device);
    }

    /// Cut the chime short, releasing the output.
    pub fn stop_test_tone(&self) {
        self.sound.stop_tone();
    }

    /// Play the microphone back through the chosen output.
    pub fn start_monitor(
        &self,
        backends: impl FnOnce() -> Backends,
        device: Option<String>,
        gate: GateConfig,
        output: Option<String>,
    ) {
        self.sound.start_monitor(backends, device, gate, output);
    }

    /// Stop playing the microphone back, leaving the microphone open.
    pub fn stop_monitor(&self) {
        self.sound.stop_monitor();
    }

    /// Join the voice channel in `room_id`, starting the call thread on first
    /// use.
    ///
    /// `transport` is a closure because it is only wanted once: the thread
    /// outlives any one call, and building a second transport for a session
    /// that already has one would be building a second call.
    pub fn connect_call<T: CallTransport>(
        &self,
        room_id: String,
        transport: impl FnOnce() -> T,
        audio: CallAudio,
    ) {
        *self.locked_call_audio() = Some(audio);

        let mut slot = self.locked_call();
        let bridge = slot.get_or_insert_with(|| {
            CallBridge::spawn(
                transport(),
                self.microphone.clone(),
                speakers(
                    self.voices.clone(),
                    self.chiming.clone(),
                    self.speaking.clone(),
                    self.levels.clone(),
                    self.talking.clone(),
                ),
                self.call_reporter(),
            )
        });
        bridge.connect(room_id);
    }

    /// Say that a join will not be attempted, and why.
    ///
    /// On the call channel rather than as an error from the command, because
    /// this is the answer to "what is my call doing" and the interface reads
    /// that in exactly one place. A refusal returned as a command error would
    /// be a second thing to render, in a second component, saying something
    /// about the same call.
    ///
    /// It starts no call thread, which is the point. Nothing is opened, no
    /// membership is published, and a session that is already in a call
    /// elsewhere stays in it: refusing a new channel must not evict the
    /// channel somebody is currently talking in.
    pub fn refuse_call(&self, room_id: String, readiness: CallReadiness) {
        tracing::info!(
            %room_id,
            ?readiness,
            "refusing to join an encrypted call this session cannot be heard in"
        );
        self.events
            .emit(AppEvent::CallRefused(CallRefused { room_id, readiness }));
    }

    /// Leave the voice channel, if this session is in one.
    ///
    /// A no-op when no call was ever started, which is what a stray click on a
    /// disconnect control that outlived its call is.
    ///
    /// The microphone is not given back here. That happens when the call
    /// thread reports the call over, which is the only ordering that cannot
    /// close the device out from under a channel somebody clicked immediately
    /// afterwards. See [`Self::call_reporter`].
    pub fn disconnect_call(&self) {
        if let Some(bridge) = self.locked_call().as_ref() {
            bridge.disconnect();
        }
    }

    /// Mute or unmute this session's microphone.
    ///
    /// A no-op before the first call of the session, when there is no thread to
    /// remember it. That is not a gap: the controls are drawn inside the call
    /// panel, so there is nothing to press until a call exists. Once one has,
    /// the thread outlives it and carries the state across channel switches.
    pub fn set_call_muted(&self, muted: bool) {
        if let Some(bridge) = self.locked_call().as_ref() {
            bridge.set_muted(muted);
        }
    }

    /// Say that nobody is at this computer.
    ///
    /// Mutes and does not deafen, which is the whole difference from the
    /// button below it. A no-op before the first call of the session, like the
    /// other two and for the same reason.
    pub fn set_call_away(&self, away: bool) {
        if let Some(bridge) = self.locked_call().as_ref() {
            bridge.set_away(away);
        }
    }

    /// Stop or resume receiving the audio of everybody else in the call.
    pub fn set_call_deafened(&self, deafened: bool) {
        if let Some(bridge) = self.locked_call().as_ref() {
            bridge.set_deafened(deafened);
        }
    }

    /// What to do with everything the call thread says.
    ///
    /// Two jobs in one closure, and the order inside it is the point. The
    /// microphone follows the call rather than the click: a `Connecting`
    /// opens it and an ending gives it back, both on the pump thread, in the
    /// order the call thread produced them.
    ///
    /// Driving it from the commands instead would race. Leaving a channel and
    /// immediately clicking another one issues both commands before the first
    /// one's `Disconnected` has been handled, and a release running after the
    /// second acquire closes the device on a call that is starting.
    fn call_reporter(&self) -> impl FnMut(CallEvent) + Send + 'static {
        let sound = self.sound.clone();
        let events = self.events.clone();
        let microphone = self.microphone.clone();
        let voices = self.voices.clone();
        let call_audio = self.call_audio.clone();
        let talking = self.talking.clone();

        move |event| {
            match &event {
                CallEvent::Connecting { .. } => {
                    if let Some(audio) = call_audio
                        .lock()
                        .expect("the call audio mutex is never poisoned")
                        .as_ref()
                    {
                        let queue = microphone.clone();
                        sound.start_call(
                            || (audio.backends)(),
                            audio.device.clone(),
                            audio.gate,
                            speaking_sink(queue, talking.clone(), audio.us.clone(), events.clone()),
                            audio.output.clone(),
                            voices.clone(),
                        );
                    }
                }
                CallEvent::Connected { .. } => {}
                CallEvent::Disconnected | CallEvent::Failed { .. } => {
                    sound.stop_call();
                    // The tally is ticked by the microphone, and the line
                    // above is what stops it. Without this whoever was talking
                    // when the call ended stays lit until the next one.
                    if let Some(user_ids) = talking.quiet() {
                        events.emit(AppEvent::Speaking(user_ids));
                    }
                }
                // Split onto its own channel here rather than being given one
                // by the call thread, which has one way out and no reason to
                // know how the webview is wired. See `AppEvent::SelfAudio` for
                // why it cannot travel with the call.
                CallEvent::SelfAudio(audio) => {
                    events.emit(AppEvent::SelfAudio(*audio));
                    return;
                }
            }

            events.emit(AppEvent::Call(event));
        }
    }

    /// Watch `room_id`'s messages, replacing whatever room was being watched.
    ///
    /// Idempotent for the room already open, which matters because the shell
    /// re-selects a channel for reasons that are not a click: a room list
    /// arriving re-derives the selection. Restarting the watcher for one would
    /// throw away every page somebody had scrolled back through.
    ///
    /// A no-op while signed out. There is nothing to read a room out of, and
    /// the interface that would draw it is not on screen.
    pub async fn open_room(&self, room_id: String) {
        if self
            .locked_timeline()
            .as_ref()
            .is_some_and(|watch| watch.room_id() == room_id)
        {
            return;
        }

        let Some(client) = self.client().await else {
            return;
        };

        let events = self.events.clone();
        let for_threads = self.events.clone();
        // Assigned rather than pushed, so the previous watcher is dropped, and
        // therefore aborted, by the assignment itself.
        *self.locked_timeline() = Some(timeline::watch(
            client,
            &room_id,
            move |timeline| events.emit(AppEvent::Timeline(timeline)),
            move |thread| for_threads.emit(AppEvent::Thread(thread.map(Box::new))),
        ));
    }

    /// Stop watching whatever room was open, and say so.
    ///
    /// The parting word is the point. This channel keeps its latest value for
    /// a late subscriber, and what it is keeping is somebody's conversation:
    /// without an empty one to replace it, closing a room leaves it waiting
    /// for whatever asks to be caught up next.
    ///
    /// Also called on sign-out, on the same terms as every other task the
    /// session owns: the watcher holds a `Client`, so leaving one running
    /// would keep the previous account's client alive behind the next login.
    pub fn close_room(&self) {
        if self.locked_timeline().take().is_some() {
            self.events.emit(AppEvent::Timeline(Timeline::default()));
            // The thread went with the watcher that owned it, and a panel left
            // on screen would be showing a conversation from a room nobody has
            // open.
            self.events.emit(AppEvent::Thread(None));
        }
    }

    /// Ask the open room for a page of older messages.
    ///
    /// A no-op when no room is open, which is what a scroll landing at the
    /// same moment as a room change is.
    pub fn earlier_messages(&self) {
        if let Some(watch) = self.locked_timeline().as_ref() {
            watch.earlier();
        }
    }

    /// Open the thread hanging from `root_id`, or shut whichever is open.
    ///
    /// A no-op when no room is open. The panel belongs to the room's watcher,
    /// so there is nothing to open it against and nothing on screen to draw
    /// it beside.
    pub fn open_thread(&self, root_id: Option<String>) {
        if let Some(watch) = self.locked_timeline().as_ref() {
            watch.open_thread(root_id);
        }
    }

    fn locked_timeline(&self) -> std::sync::MutexGuard<'_, Option<timeline::Watch>> {
        self.timeline
            .lock()
            .expect("the timeline mutex is never poisoned")
    }

    fn locked_call(&self) -> std::sync::MutexGuard<'_, Option<CallBridge>> {
        self.call.lock().expect("the call mutex is never poisoned")
    }

    fn locked_call_audio(&self) -> std::sync::MutexGuard<'_, Option<CallAudio>> {
        self.call_audio
            .lock()
            .expect("the call audio mutex is never poisoned")
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

    /// Switch the join and leave sounds on or off for a call in progress.
    ///
    /// Beside `settings()` rather than derived from it, because the call
    /// thread cannot read a file at the moment a roster changes. Called by the
    /// command that saves the settings, right after it saves them, so that
    /// what is on disk and what a call is doing can never disagree.
    pub fn set_call_sounds(&self, wanted: bool) {
        self.chiming
            .store(wanted, std::sync::atomic::Ordering::Relaxed);
    }

    /// Switch the spoken notifications on or off for a call in progress.
    ///
    /// Separate from [`set_call_sounds`](Self::set_call_sounds) rather than a
    /// second argument to it, because they are two settings and a caller that
    /// had to pass both would be one refactor away from passing the same value
    /// twice.
    pub fn set_call_voices(&self, wanted: bool) {
        self.speaking
            .store(wanted, std::sync::atomic::Ordering::Relaxed);
    }

    /// Set how loud a call and its notifications should be, as percentages.
    ///
    /// Beside `settings()` for the same reason the two switches above are: the
    /// mixer is read inside the device callback and cannot go and read a file,
    /// and the two must not be able to disagree.
    pub fn set_volumes(&self, output: u8, notifications: u8) {
        self.voices.set_output_level(output);
        self.voices.set_notification_level(notifications);
    }

    /// How loud each person should be, for the command that changes one of
    /// them and for the call that has to apply them.
    pub fn levels(&self) -> &Arc<crate::ears::Levels> {
        &self.levels
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

    /// The attachments held for the media scheme.
    pub fn media_cache(&self) -> &Mutex<crate::media::Cache> {
        &self.media
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

        // Whether this session could be heard in an encrypted call, for as
        // long as the session lasts rather than once at startup.
        //
        // Once was wrong in the direction that matters. Every session begins
        // unverified and the whole point of verifying one is that calls start
        // working afterwards, so an answer taken at startup and kept would
        // lock somebody out of every call until they restarted the
        // application, which is worse than the failure the answer exists to
        // prevent.
        let events = self.events.clone();
        replace_task(
            &self.readiness_task,
            calls::watch_readiness(client.clone(), move |state| {
                events.emit(AppEvent::CallReadiness(state));
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
        // First, and before the client it was built on goes. Dropping the
        // bridge unwinds the membership and waits for it, so a sign-out does
        // not leave this account's name sitting in a voice channel for
        // whoever signs in next to find.
        //
        // Dropped by name rather than at the end of an expression, because
        // when it happens is load bearing: the drop joins the pump, so
        // everything after it runs with no more call events on their way. A
        // queued `Connecting` handled afterwards would reopen the microphone
        // in the middle of a sign-out.
        let bridge = self.locked_call().take();
        let was_in_a_call = bridge.is_some();
        drop(bridge);

        // The call thread deliberately says nothing on its shutdown path, so
        // the two things a `Disconnected` would have done are done here: the
        // microphone goes back, and the webview is told, so a stale
        // "connected" is not what the next sign-in is caught up with.
        if was_in_a_call {
            self.sound.stop_call();
            self.events.emit(AppEvent::Call(CallEvent::Disconnected));
        }
        *self.locked_call_audio() = None;

        stop_task(&self.refresh_task).await;

        // No parting word for either verification channel. There is nothing
        // left to say about a session that has gone, and the next sign-in
        // publishes its own state as soon as it has one. A flow that was
        // halfway through is over rather than cancelled by anybody, and
        // announcing a cancellation nobody performed would be a lie about
        // whose decision it was.
        stop_task(&self.verification_task).await;
        stop_task(&self.readiness_task).await;
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

        // The open room goes the same way, and more urgently: what it retains
        // is the previous account's conversation, in full, sitting on a
        // retained channel waiting for the next webview to ask.
        self.close_room();

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

    /// Whether the microphone is open for anything.
    ///
    /// Test-only, and the one thing about the call wiring that is otherwise
    /// only observable by listening to a sound card.
    #[cfg(test)]
    pub fn microphone_open(&self) -> bool {
        self.sound.capturing()
    }

    /// Whether a call thread has been started.
    ///
    /// Test-only. Distinguishes "never joined a call" from "joined one and
    /// left", which `microphone_open` cannot.
    #[cfg(test)]
    pub fn has_call_thread(&self) -> bool {
        self.locked_call().is_some()
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
    use crate::testing::{FakeCallTransport, fake_backends, wait_for};
    use consort_matrix::StopReason;
    use consort_matrix::secrets::MemoryBackend;
    use std::sync::Arc;

    const GENERAL: &str = "!general:example.org";

    fn call_audio() -> CallAudio {
        CallAudio {
            device: Some("Yeti".to_owned()),
            output: Some("Headphones".to_owned()),
            gate: GateConfig::default(),
            backends: Box::new(fake_backends),
            us: "@ada:example.org".to_owned(),
        }
    }

    /// Join `room_id`, with a transport that works or one that does not.
    fn join(state: &AppState, room_id: &str, joins: bool) {
        state.connect_call(
            room_id.to_owned(),
            move || {
                if joins {
                    FakeCallTransport::joining()
                } else {
                    FakeCallTransport::refusing()
                }
            },
            call_audio(),
        );
    }

    /// Block until the call channel has said `state`, or give up.
    ///
    /// The call thread and its pump are both threads, so everything the call
    /// wiring does arrives after the call that asked for it has returned.
    fn until_call(sink: &Arc<RecordingSink>, tag: &str) {
        wait_for(
            &format!("the call to say {tag}"),
            || last_call_state(sink).as_deref() == Some(tag),
            || format!("{:?}", last_call_state(sink)),
        );
    }

    /// The most recent thing said on the call channel, by its wire tag.
    fn last_call_state(sink: &Arc<RecordingSink>) -> Option<String> {
        sink.events().iter().rev().find_map(|event| match event {
            AppEvent::Call(call) => Some(
                serde_json::to_value(call).ok()?["state"]
                    .as_str()?
                    .to_owned(),
            ),
            _ => None,
        })
    }

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

    /// Joining and leaving a voice channel, and what the microphone does.
    mod calls {
        use super::*;

        #[test]
        fn a_fresh_state_is_in_no_call_and_holds_no_microphone() {
            let (_dir, state, sink) = state();

            assert!(!state.has_call_thread());
            assert!(!state.microphone_open());
            assert!(sink.events().is_empty());
        }

        #[test]
        fn joining_a_channel_opens_the_microphone_and_tells_the_webview() {
            let (_dir, state, sink) = state();

            join(&state, GENERAL, true);

            until_call(&sink, "connected");
            assert!(
                state.microphone_open(),
                "the call connected with nothing capturing"
            );
        }

        #[test]
        fn leaving_gives_the_microphone_back() {
            let (_dir, state, sink) = state();
            join(&state, GENERAL, true);
            until_call(&sink, "connected");

            state.disconnect_call();

            until_call(&sink, "disconnected");
            wait_for(
                "the microphone to be given back",
                || !state.microphone_open(),
                || "still open".to_owned(),
            );
        }

        #[test]
        fn a_join_that_fails_gives_the_microphone_back_too() {
            // Otherwise a channel that cannot be joined holds the sound card
            // open for the rest of the session, and the only sign of it is a
            // microphone light nobody can turn off.
            let (_dir, state, sink) = state();

            join(&state, GENERAL, false);

            until_call(&sink, "failed");
            wait_for(
                "the microphone to be given back",
                || !state.microphone_open(),
                || "still open".to_owned(),
            );
        }

        #[test]
        fn moving_between_channels_keeps_the_microphone_open_throughout() {
            // The reason the microphone follows the call events rather than
            // the clicks. Both orderings connect in the end; only this one
            // does it without closing the sound card in between.
            let (_dir, state, sink) = state();
            join(&state, GENERAL, true);
            until_call(&sink, "connected");

            join(&state, "!music:example.org", true);

            until_call(&sink, "connected");
            assert!(state.microphone_open());
            assert_eq!(
                sink.events()
                    .iter()
                    .filter(|event| matches!(event, AppEvent::Call(CallEvent::Disconnected)))
                    .count(),
                0,
                "a channel change reported the call as over"
            );
        }

        #[test]
        fn who_is_in_the_channel_reaches_the_webview_with_the_call() {
            // One state rather than two channels. A reader that keeps the
            // latest thing said about the call then has the roster too, and
            // one that missed a roster change has not also lost track of
            // whether it is in a call.
            let (_dir, state, sink) = state();
            let transport = FakeCallTransport::joining();
            transport.set_roster(vec![consort_matrix::Participant::named(
                "@ada:example.org",
                "Ada",
            )]);

            state.connect_call(GENERAL.to_owned(), move || transport, call_audio());

            until_call(&sink, "connected");
            let Some(AppEvent::Call(CallEvent::Connected { participants, .. })) = sink
                .events()
                .into_iter()
                .rev()
                .find(|event| matches!(event, AppEvent::Call(CallEvent::Connected { .. })))
            else {
                panic!("the call never connected: {:?}", sink.events());
            };
            assert_eq!(
                participants
                    .iter()
                    .map(|person| person.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["Ada"]
            );
        }

        #[test]
        fn why_a_call_cannot_be_heard_reaches_the_webview_too() {
            // The failure with no other symptom: the membership published, the
            // roster is right, packets are flowing, and neither side can
            // decrypt a word. If this does not reach the webview then nothing
            // does, because everything else says the call is working.
            let (_dir, state, sink) = state();
            let transport = FakeCallTransport::joining();
            transport.set_trouble(Some("nobody can hear you"));

            state.connect_call(GENERAL.to_owned(), move || transport, call_audio());

            until_call(&sink, "connected");
            let Some(AppEvent::Call(CallEvent::Connected { trouble, .. })) = sink
                .events()
                .into_iter()
                .rev()
                .find(|event| matches!(event, AppEvent::Call(CallEvent::Connected { .. })))
            else {
                panic!("the call never connected: {:?}", sink.events());
            };
            assert_eq!(trouble.as_deref(), Some("nobody can hear you"));
        }

        #[test]
        fn a_refused_join_starts_nothing() {
            // The whole point of asking before the join rather than after it.
            // Nothing is opened, no membership is published, and there is no
            // thread left behind holding a device.
            let (_dir, state, sink) = state();

            state.refuse_call(GENERAL.to_owned(), CallReadiness::SessionUnverified);

            assert!(!state.has_call_thread());
            assert!(
                sink.events()
                    .iter()
                    .all(|event| !matches!(event, AppEvent::Call(_))),
                "a refusal spoke on the call channel: {:?}",
                sink.events()
            );
        }

        #[test]
        fn a_refused_join_says_which_room_and_which_failure() {
            let (_dir, state, sink) = state();

            state.refuse_call(GENERAL.to_owned(), CallReadiness::NoIdentity);

            let Some(AppEvent::CallRefused(refusal)) = sink
                .events()
                .into_iter()
                .find(|event| matches!(event, AppEvent::CallRefused(_)))
            else {
                panic!("no refusal reached the webview: {:?}", sink.events());
            };
            assert_eq!(refusal.room_id, GENERAL);
            // Which one it was, not merely that it was one. The two are
            // cleared in two different places and the interface has to say
            // which.
            assert_eq!(refusal.readiness, CallReadiness::NoIdentity);
        }

        #[test]
        fn refusing_one_channel_does_not_evict_the_call_in_another() {
            // The reason a refusal is not a call state. Somebody sitting in a
            // voice channel who clicks a second one and is refused is still
            // sitting in the first, and an interface told otherwise would draw
            // a client connected to nothing while this process is publishing a
            // membership.
            let (_dir, state, sink) = state();
            state.connect_call(GENERAL.to_owned(), FakeCallTransport::joining, call_audio());
            until_call(&sink, "connected");

            state.refuse_call(
                "!lounge:example.org".to_owned(),
                CallReadiness::SessionUnverified,
            );

            let latest = sink
                .events()
                .into_iter()
                .rev()
                .find_map(|event| match event {
                    AppEvent::Call(call) => Some(call),
                    _ => None,
                })
                .expect("the call channel said nothing at all");
            assert!(
                matches!(latest, CallEvent::Connected { ref room_id, .. } if room_id == GENERAL),
                "the refusal changed what call this session is in: {latest:?}"
            );
        }

        #[test]
        fn a_refusal_is_not_replayed_to_a_webview_that_reloaded() {
            // It is an incident, not a state. What it reports is already a
            // standing answer on the readiness channel; this only adds "the
            // thing you just clicked", and a click from twenty minutes ago is
            // not news to somebody who has since verified.
            assert!(
                !AppEvent::CallRefused(CallRefused {
                    room_id: GENERAL.to_owned(),
                    readiness: CallReadiness::SessionUnverified,
                })
                .is_worth_keeping()
            );
        }

        #[test]
        fn a_second_call_reuses_the_first_one_s_thread() {
            // A transport per call would be a `Call::join` per call from a
            // client that already has one, and a second thread nobody ends.
            let (_dir, state, sink) = state();
            join(&state, GENERAL, true);
            until_call(&sink, "connected");

            // Refuses everything. If this were the transport that got used,
            // the join below would fail.
            join(&state, "!music:example.org", false);

            until_call(&sink, "connected");
        }

        #[tokio::test]
        async fn signing_out_leaves_the_call_and_says_so() {
            // The call thread deliberately says nothing on its shutdown path,
            // so this is where the two things a `Disconnected` would have done
            // get done: the device goes back, and the webview is told, so the
            // next sign-in is not caught up with a call that ended with
            // somebody else's session.
            let (_dir, state, sink) = state();
            join(&state, GENERAL, true);
            until_call(&sink, "connected");

            state.clear_client().await;

            assert!(!state.has_call_thread());
            assert!(!state.microphone_open());
            assert_eq!(last_call_state(&sink).as_deref(), Some("disconnected"));
        }

        #[tokio::test]
        async fn signing_out_of_a_session_that_never_joined_a_call_says_nothing_about_one() {
            // The ordinary sign-out. Announcing a disconnection to somebody
            // who was never in a channel is a notification about nothing.
            let (_dir, state, sink) = state();

            state.clear_client().await;

            assert_eq!(last_call_state(&sink), None);
        }

        #[test]
        fn leaving_a_call_that_was_never_joined_does_nothing() {
            // A disconnect control that outlived its call, which is what a
            // stale webview clicking through a resent state looks like.
            let (_dir, state, sink) = state();

            state.disconnect_call();

            assert!(!state.has_call_thread());
            assert_eq!(last_call_state(&sink), None);
        }
    }
}

/// The green rings, which are decided here rather than asked of the SFU.
#[cfg(test)]
mod rings {
    use super::*;
    use crate::events::RecordingSink;

    const US: &str = "@ada:example.org";

    /// A frame loud enough to be somebody talking.
    fn loud() -> Vec<i16> {
        vec![8_000; consort_audio::FRAME_SAMPLES]
    }

    /// A frame the gate has shut on, which is what a call is published while
    /// nobody is talking.
    fn shut() -> Vec<i16> {
        vec![0; consort_audio::FRAME_SAMPLES]
    }

    fn sink() -> (GatedSink, Talking, Arc<RecordingSink>, Microphone) {
        let recorder = Arc::new(RecordingSink::new());
        let events = Arc::new(LatestSink::new(recorder.clone()));
        let talking = Talking::new();
        let queue = Microphone::new();
        (
            speaking_sink(queue.clone(), talking.clone(), US.to_owned(), events),
            talking,
            recorder,
            queue,
        )
    }

    /// Every set of speakers the webview was told about, in order.
    fn told(recorder: &RecordingSink) -> Vec<Vec<String>> {
        recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                AppEvent::Speaking(user_ids) => Some(user_ids),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn our_own_ring_lights_on_the_first_audible_frame() {
        // One frame is 10 ms. The SFU's detector needed several hundred, and
        // reaching that threshold from a desk microphone meant leaning into
        // it, which is the complaint this replaces.
        let (mut sink, _talking, recorder, _queue) = sink();

        sink(&loud(), true);

        assert_eq!(told(&recorder), vec![vec![US.to_owned()]]);
    }

    #[test]
    fn a_gated_shut_frame_lights_nothing() {
        // Measured on the samples rather than on the gate's verdict, so this
        // holds whether or not voice activity is switched on: with it off the
        // gate reports every frame open and these samples would still be
        // silence.
        let (mut sink, _talking, recorder, _queue) = sink();

        sink(&shut(), true);

        assert!(told(&recorder).is_empty());
    }

    #[test]
    fn talking_continuously_says_so_once() {
        // A hundred frames a second, and the interface needs to hear about one
        // of them. Emitting per frame would be a hundred IPC messages a second
        // saying what the last one said.
        let (mut sink, _talking, recorder, _queue) = sink();

        for _ in 0..50 {
            sink(&loud(), true);
        }

        assert_eq!(told(&recorder), vec![vec![US.to_owned()]]);
    }

    #[test]
    fn stopping_puts_the_ring_out() {
        let (mut sink, _talking, recorder, _queue) = sink();
        sink(&loud(), true);

        for _ in 0..consort_audio::HOLD_FRAMES {
            sink(&shut(), false);
        }

        assert_eq!(
            told(&recorder),
            vec![vec![US.to_owned()], Vec::new()],
            "the ring never went out"
        );
    }

    #[test]
    fn somebody_else_talking_reaches_the_webview_on_our_tick() {
        // The two halves are written from different threads and read from one.
        // This session's own capture is the clock, because it runs whether or
        // not anybody is saying anything, which is what lets a ring go out.
        let (mut sink, talking, recorder, _queue) = sink();
        talking.heard("@bob:example.org", &loud());

        sink(&shut(), false);

        assert_eq!(told(&recorder), vec![vec!["@bob:example.org".to_owned()]]);
    }

    #[tokio::test]
    async fn the_frames_still_reach_the_call() {
        // The tally is beside the queue, not in front of it. A ring that cost
        // the call its audio would be the worst possible trade.
        let (mut sink, _talking, _recorder, queue) = sink();

        sink(&loud(), true);

        assert_eq!(queue.next().await.samples, loud());
    }
}
