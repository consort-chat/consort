// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Events pushed from Rust to the webview.
//!
//! Until this existed the IPC ran one way: the frontend invoked a command and
//! got an answer. Everything interesting from here on is the other direction,
//! starting with the sync loop's health and, next milestone, verification
//! requests that arrive because somebody pressed a button on their phone.
//!
//! One channel per concern, each carrying a tagged union. Nothing else in the
//! crate writes an event name as a string literal: a name is a contract with
//! `app/src/lib/api.ts` and Tauri validates it at emit time, so a typo is a
//! listener that silently never fires rather than anything that fails to
//! build.

use std::sync::{Arc, Mutex};

use consort_audio::AudioEvent;
use consort_call::{CallEvent, SelfAudio};
use consort_matrix::{CallReadiness, Connection, Flow, KeyBackup, Rooms, SessionVerification};
use serde::Serialize;

/// A join that was not attempted, because it could not have been heard.
///
/// Deliberately not a `CallEvent`. The call thread never produces one: the
/// decision happens before the thread is asked for anything, and putting it in
/// that enum would put a variant in front of every reader of a call's progress
/// that no call ever reaches.
///
/// More importantly it is not a call state. The call channel carries what this
/// session is currently doing, and somebody sitting in one voice channel who
/// clicks a second one and is refused is still sitting in the first. Reported
/// there it would evict the call they are in and draw a client connected to
/// nothing while this process is very much still publishing a membership.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRefused {
    /// The channel that was clicked, so the interface can name it.
    pub room_id: String,
    /// Why, in the two-answer vocabulary the frontend already reads. See
    /// `consort_matrix::CallReadiness`.
    pub readiness: CallReadiness,
}

/// Something the frontend needs to be told about without having asked.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    /// The health of the sync loop changed.
    Connection(Connection),
    /// Whether this session is verified changed.
    Verification(SessionVerification),
    /// Whether this session could be heard in an encrypted call changed.
    ///
    /// Its own channel rather than a reading of
    /// [`Verification`](Self::Verification), because the two answer different
    /// questions and one of them cannot be derived from the other. A session
    /// on an account with no cross-signing identity at all is `Unverified` on
    /// that channel and `NoIdentity` here, and those send a person to two
    /// different places: one is fixed on this device, the other on the
    /// account.
    CallReadiness(CallReadiness),
    /// A verification flow started, moved on, or ended.
    VerificationFlow(Flow),
    /// What is happening to this session's room keys changed.
    KeyBackup(KeyBackup),
    /// The rooms the account is in changed.
    Rooms(Rooms),
    /// The microphone test said something.
    ///
    /// The one channel here that is not state. See
    /// [`is_worth_keeping`](Self::is_worth_keeping).
    Audio(AudioEvent),
    /// This session's voice call started, ended, or would not start.
    Call(CallEvent),
    /// A voice channel was clicked and not joined, because a call from this
    /// session in that room would have been inaudible.
    CallRefused(CallRefused),
    /// This session muted itself, or deafened itself, or stopped.
    ///
    /// A channel of its own rather than another value on [`Call`](Self::Call),
    /// because only one event per channel is kept for a late subscriber. Sent
    /// as a call state, a mute would evict the call it was pressed during, and
    /// a webview that reloaded would come back believing it was in no channel
    /// while this process is very much publishing one.
    SelfAudio(SelfAudio),
    /// Who in the current call is talking right now, by Matrix user ID.
    ///
    /// Its own channel for two reasons, and they pull in opposite directions
    /// from [`SelfAudio`](Self::SelfAudio)'s. This one arrives several times a
    /// second, so it must not sit on the call channel where it would evict the
    /// call state constantly. And unlike a mute it is never replayed: see
    /// [`is_worth_keeping`](Self::is_worth_keeping).
    Speaking(Vec<String>),
}

impl AppEvent {
    /// The channel carrying sync-loop health.
    pub const CONNECTION: &'static str = "connection";
    /// The channel carrying this session's verification state.
    pub const VERIFICATION: &'static str = "verification";
    /// The channel carrying whether a call from this session could be heard.
    pub const CALL_READINESS: &'static str = "call-readiness";
    /// The channel carrying the progress of one verification flow.
    pub const VERIFICATION_FLOW: &'static str = "verification-flow";
    /// The channel carrying whether room keys are being backed up.
    pub const KEY_BACKUP: &'static str = "key-backup";
    /// The channel carrying the whole room list.
    pub const ROOMS: &'static str = "rooms";
    /// The channel carrying the microphone test.
    pub const AUDIO: &'static str = "audio";
    /// The channel carrying this session's voice call.
    pub const CALL: &'static str = "call";
    /// The channel carrying a join that was refused before it was attempted.
    pub const CALL_REFUSED: &'static str = "call-refused";
    /// The channel carrying whether this session is muted or deafened.
    pub const SELF_AUDIO: &'static str = "self-audio";
    /// The channel carrying who in the call is talking.
    pub const SPEAKING: &'static str = "speaking";

    /// The channel this event goes out on.
    pub fn channel(&self) -> &'static str {
        match self {
            Self::Connection(_) => Self::CONNECTION,
            Self::Verification(_) => Self::VERIFICATION,
            Self::CallReadiness(_) => Self::CALL_READINESS,
            Self::VerificationFlow(_) => Self::VERIFICATION_FLOW,
            Self::KeyBackup(_) => Self::KEY_BACKUP,
            Self::Rooms(_) => Self::ROOMS,
            Self::Audio(_) => Self::AUDIO,
            Self::Call(_) => Self::CALL,
            Self::CallRefused(_) => Self::CALL_REFUSED,
            Self::SelfAudio(_) => Self::SELF_AUDIO,
            Self::Speaking(_) => Self::SPEAKING,
        }
    }

    /// Whether a late subscriber should be caught up on this.
    ///
    /// Four of the five channels carry state: there is always a current
    /// connection, a current verification state, a current answer about room
    /// keys and a current set of rooms, and a webview that missed the last one
    /// is a webview showing the wrong thing until something else happens to
    /// change. The room list is the starkest case, because the thing that
    /// changes it next may be days away: an account that joins no rooms and
    /// leaves none would show an empty shell until it did.
    ///
    /// A flow is state only while it is running. Once it is done or cancelled
    /// it is history, and replaying it on the next mount would put "the emoji
    /// did not match" back on screen for a flow that ended twenty minutes ago.
    /// A flow still in progress is very much worth keeping: the other device
    /// is waiting, and there is no way to ask for the emoji again.
    ///
    /// Audio is never kept. A level reading is a measurement of a moment, and
    /// replaying the last one would draw a moving bar for a microphone that
    /// stopped minutes ago. The other three go the same way because this
    /// channel only speaks while the settings screen is open, and a screen
    /// that reopens starts the test again rather than needing to be told what
    /// happened last time.
    ///
    /// Who is talking is never kept, for the reason a level reading is not. It
    /// describes a moment that has passed by the time anybody reloads, and
    /// replaying it would leave a ring drawn around somebody who stopped
    /// talking before the webview restarted, with nothing to take it off
    /// again if they have since left the call.
    ///
    /// A call is state, all four of it. There is always a current answer to
    /// "am I in a voice channel", and a webview that reloaded mid-call and was
    /// not told would draw a client sitting in no channel while this process
    /// is very much publishing one. `Failed` is kept for the same reason
    /// rather than despite being an incident: it is how the interface says
    /// "not in a call, and here is why", and it is superseded by the next
    /// attempt like every other value on this channel.
    pub fn is_worth_keeping(&self) -> bool {
        match self {
            Self::Connection(_)
            | Self::Verification(_)
            | Self::CallReadiness(_)
            | Self::KeyBackup(_)
            | Self::Rooms(_)
            | Self::Call(_)
            | Self::SelfAudio(_) => true,
            Self::VerificationFlow(flow) => !flow.state.is_final(),
            // A refusal is an incident, not a state. What it reports is
            // already on the `call-readiness` channel as a standing answer,
            // and this only adds "the thing you just clicked". Replayed on a
            // reload it would put a complaint about a click from twenty
            // minutes ago in front of somebody who has since verified.
            Self::Audio(_) | Self::Speaking(_) | Self::CallRefused(_) => false,
        }
    }

    /// The JSON the frontend receives.
    ///
    /// The payload is the contents, not the `AppEvent` around it. The variant
    /// already picked the channel, so repeating it inside the body would give
    /// the frontend a wrapper to unpick for no information.
    pub fn payload(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            Self::Connection(state) => serde_json::to_value(state),
            Self::Verification(state) => serde_json::to_value(state),
            Self::CallReadiness(state) => serde_json::to_value(state),
            Self::VerificationFlow(flow) => serde_json::to_value(flow),
            Self::KeyBackup(state) => serde_json::to_value(state),
            Self::Rooms(rooms) => serde_json::to_value(rooms),
            Self::Audio(event) => serde_json::to_value(event),
            Self::Call(event) => serde_json::to_value(event),
            Self::CallRefused(refusal) => serde_json::to_value(refusal),
            Self::SelfAudio(audio) => serde_json::to_value(audio),
            Self::Speaking(user_ids) => serde_json::to_value(user_ids),
        }
    }
}

/// Somewhere to send events.
///
/// A trait rather than an `AppHandle` so that everything holding one stays
/// testable. `AppHandle` can only be produced by a running Tauri application,
/// and state that needs one is state no test can construct.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: AppEvent);
}

/// The real one.
///
/// Generic over the runtime rather than fixed to `Wry`, so the same code the
/// application runs is the code the test below exercises on Tauri's headless
/// mock runtime. Written for `AppHandle` alone it would be untestable, and
/// "it compiles" is not coverage for the one function that puts an event on
/// the wire.
///
/// Failures are logged rather than propagated. An event that could not be
/// delivered is a webview that is closing or already gone, and there is
/// nothing the caller, usually a background task, could usefully do about it.
impl<R: tauri::Runtime> EventSink for tauri::AppHandle<R> {
    fn emit(&self, event: AppEvent) {
        let channel = event.channel();

        let payload = match event.payload() {
            Ok(payload) => payload,
            Err(error) => {
                tracing::error!(%error, channel, "could not serialise an event");
                return;
            }
        };

        if let Err(error) = tauri::Emitter::emit(self, channel, payload) {
            tracing::warn!(%error, channel, "could not deliver an event to the webview");
        }
    }
}

/// Remembers the last event on each channel so a late subscriber can catch up.
///
/// The webview is almost always the late one. Background tasks start the
/// moment a session exists, which on a restored session is inside Tauri's
/// `setup`, before any JavaScript has run. By the time the signed-in screen
/// mounts and subscribes, the states it needs have already been published to
/// nobody, and every channel here is a state channel rather than a stream of
/// incidents: missing "live" does not mean missing one message, it means
/// sitting on "connecting" until something else happens to change.
///
/// So the frontend subscribes and then asks to be caught up, and gets the
/// current state through the same channel and the same handler as every later
/// change. The alternative is a getter command per channel, which is a second
/// wire format and a second code path in the UI for the same information.
///
/// Only the most recent event per channel, and in the order the channels first
/// spoke. Replaying a history would walk the interface back through states it
/// has already left.
pub struct LatestSink {
    inner: Arc<dyn EventSink>,
    latest: Mutex<Vec<AppEvent>>,
}

impl LatestSink {
    pub fn new(inner: Arc<dyn EventSink>) -> Self {
        Self {
            inner,
            latest: Mutex::new(Vec::new()),
        }
    }

    /// Send the current state of every channel again.
    ///
    /// A no-op before anything has been published, which is the signed-out
    /// case: nothing has happened, so there is nothing to catch up on.
    pub fn resend(&self) {
        let events = self
            .latest
            .lock()
            .expect("the latest-event mutex is never poisoned")
            .clone();
        for event in events {
            self.inner.emit(event);
        }
    }
}

impl EventSink for LatestSink {
    fn emit(&self, event: AppEvent) {
        {
            let mut latest = self
                .latest
                .lock()
                .expect("the latest-event mutex is never poisoned");
            let held = latest
                .iter()
                .position(|held| held.channel() == event.channel());

            match (held, event.is_worth_keeping()) {
                (Some(at), true) => latest[at] = event.clone(),
                (Some(at), false) => {
                    // Not merely skipped. Whatever this channel said before is
                    // superseded by an ending, and leaving the earlier state
                    // in place would resend a flow that has since finished.
                    latest.remove(at);
                }
                (None, true) => latest.push(event.clone()),
                (None, false) => {}
            }
        }
        self.inner.emit(event);
    }
}

/// A sink that remembers instead of sending.
///
/// Test-only, and deliberately in this module rather than in each test file:
/// every test that exercises a background task needs one, and three copies of
/// it would drift.
#[cfg(test)]
#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<AppEvent>>,
}

#[cfg(test)]
impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything emitted so far, oldest first.
    pub fn events(&self) -> Vec<AppEvent> {
        self.events.lock().unwrap().clone()
    }

    /// The most recent connection state, if any.
    pub fn last_connection(&self) -> Option<Connection> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match event {
                AppEvent::Connection(state) => Some(state.clone()),
                _ => None,
            })
    }

    /// The most recent key backup state, if any.
    pub fn last_key_backup(&self) -> Option<KeyBackup> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match event {
                AppEvent::KeyBackup(state) => Some(*state),
                _ => None,
            })
    }

    /// The most recent room list, if any.
    pub fn last_rooms(&self) -> Option<Rooms> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match event {
                AppEvent::Rooms(rooms) => Some(rooms.clone()),
                _ => None,
            })
    }

    /// The most recent verification state, if any.
    pub fn last_verification(&self) -> Option<SessionVerification> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match event {
                AppEvent::Verification(state) => Some(*state),
                _ => None,
            })
    }
}

#[cfg(test)]
impl EventSink for RecordingSink {
    fn emit(&self, event: AppEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consort_matrix::{Channel, ChannelKind, FlowState, Participant, Space, StopReason};

    #[test]
    fn a_connection_event_goes_out_on_the_connection_channel() {
        let event = AppEvent::Connection(Connection::Live);

        assert_eq!(event.channel(), AppEvent::CONNECTION);
    }

    #[test]
    fn the_payload_is_the_state_itself_and_not_a_wrapper_around_it() {
        // api.ts destructures `state` off the payload directly. Serialising
        // the `AppEvent` instead of its contents would deliver
        // `{"Connection": {...}}` and match nothing.
        let event = AppEvent::Connection(Connection::Offline {
            attempt: 2,
            retry_in_seconds: 4,
        });

        let payload = event.payload().unwrap();

        assert_eq!(payload.get("state").unwrap(), "offline");
        assert_eq!(payload.get("attempt").unwrap(), 2);
        assert!(
            payload.get("Connection").is_none(),
            "the variant name leaked into the wire format: {payload}"
        );
    }

    #[test]
    fn every_event_has_a_channel_name_tauri_will_accept() {
        // Tauri rejects an event name containing anything outside
        // `[a-zA-Z0-9-/:_]`, and it rejects it at emit time rather than at
        // compile time, so a typo here is a listener that silently never
        // fires.
        let events = [
            AppEvent::Connection(Connection::Connecting),
            AppEvent::Connection(Connection::Stopped {
                reason: StopReason::SignedOut,
            }),
            AppEvent::Verification(SessionVerification::Unknown),
            AppEvent::KeyBackup(KeyBackup::Enabled),
            AppEvent::Rooms(a_room_list()),
        ];

        for event in events {
            let name = event.channel();
            assert!(!name.is_empty(), "{event:?}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-/:_".contains(c)),
                "{name} is not a usable event name"
            );
        }
    }

    #[test]
    fn every_state_can_be_turned_into_a_payload() {
        let events = [
            AppEvent::Connection(Connection::Connecting),
            AppEvent::Connection(Connection::Live),
            AppEvent::Connection(Connection::Offline {
                attempt: 1,
                retry_in_seconds: 1,
            }),
            AppEvent::Connection(Connection::Stopped {
                reason: StopReason::Failed,
            }),
            AppEvent::Verification(SessionVerification::Unknown),
            AppEvent::Verification(SessionVerification::Verified),
            AppEvent::Verification(SessionVerification::Unverified),
            AppEvent::KeyBackup(KeyBackup::Missing),
            AppEvent::Rooms(Rooms::default()),
            AppEvent::Rooms(a_room_list()),
        ];

        for event in events {
            event
                .payload()
                .unwrap_or_else(|error| panic!("{event:?} would not serialise: {error}"));
        }
    }

    /// The real sink, on Tauri's headless mock runtime.
    ///
    /// Everything else in this file is about the shape of an event. This is
    /// the only test that shows one actually leaving the process, which is
    /// the part a channel-name typo breaks and nothing else would catch.
    mod through_a_real_app_handle {
        use super::*;
        use std::sync::mpsc;
        use tauri::Listener;

        #[test]
        fn an_event_reaches_a_listener_on_the_channel_it_named() {
            let app = tauri::test::mock_builder()
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .expect("the mock runtime builds without a display");
            let handle = app.handle().clone();

            let (sender, received) = mpsc::channel();
            handle.listen(AppEvent::CONNECTION, move |event| {
                let _ = sender.send(event.payload().to_owned());
            });

            EventSink::emit(&handle, AppEvent::Connection(Connection::Live));

            let payload = received
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("nothing arrived on the connection channel");
            let parsed: Connection = serde_json::from_str(&payload).unwrap();
            assert_eq!(parsed, Connection::Live);
        }

        #[test]
        fn emitting_with_nobody_listening_is_not_a_failure() {
            // Which is the normal case during startup and shutdown: the sync
            // loop reports before the webview has subscribed, and again after
            // it has gone.
            let app = tauri::test::mock_builder()
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();

            EventSink::emit(
                app.handle(),
                AppEvent::Connection(Connection::Stopped {
                    reason: StopReason::Failed,
                }),
            );
        }
    }

    #[test]
    fn a_recording_sink_keeps_what_it_was_given_in_order() {
        let sink = RecordingSink::new();

        sink.emit(AppEvent::Connection(Connection::Connecting));
        sink.emit(AppEvent::Connection(Connection::Live));

        assert_eq!(
            sink.events(),
            vec![
                AppEvent::Connection(Connection::Connecting),
                AppEvent::Connection(Connection::Live),
            ]
        );
    }

    #[test]
    fn a_recording_sink_reports_the_last_connection_state() {
        let sink = RecordingSink::new();

        sink.emit(AppEvent::Connection(Connection::Connecting));
        sink.emit(AppEvent::Connection(Connection::Live));

        assert_eq!(sink.last_connection(), Some(Connection::Live));
    }

    #[test]
    fn a_recording_sink_that_saw_nothing_reports_nothing() {
        assert_eq!(RecordingSink::new().last_connection(), None);
    }

    /// A room list with one space holding one voice channel.
    fn a_room_list() -> Rooms {
        Rooms {
            spaces: vec![
                Space {
                    id: "home".to_owned(),
                    name: "Home".to_owned(),
                    avatar: None,
                    channels: Vec::new(),
                },
                Space {
                    id: "!space:example.org".to_owned(),
                    name: "Kahu HQ".to_owned(),
                    avatar: Some("mxc://example.org/abc".to_owned()),
                    channels: vec![Channel {
                        id: "!lounge:example.org".to_owned(),
                        name: Some("Lounge".to_owned()),
                        kind: ChannelKind::Voice,
                        avatar: None,
                        joined: true,
                        participants: vec![Participant::named("@ada:example.org", "Ada")],
                    }],
                },
            ],
        }
    }

    #[test]
    fn a_room_list_goes_out_on_its_own_channel() {
        let event = AppEvent::Rooms(a_room_list());

        assert_eq!(event.channel(), AppEvent::ROOMS);
        assert_ne!(AppEvent::ROOMS, AppEvent::CONNECTION);
        assert_ne!(AppEvent::ROOMS, AppEvent::KEY_BACKUP);
    }

    #[test]
    fn the_room_list_payload_is_the_tree_itself() {
        let event = AppEvent::Rooms(a_room_list());

        let payload = event.payload().unwrap();

        assert_eq!(payload["spaces"][0]["id"], "home");
        assert_eq!(payload["spaces"][1]["channels"][0]["kind"], "voice");
        assert_eq!(
            payload["spaces"][1]["channels"][0]["participants"][0]["name"], "Ada",
            "who is in a voice channel has to survive the trip to the webview"
        );
        assert!(
            payload.get("Rooms").is_none(),
            "the variant name leaked into the wire format: {payload}"
        );
    }

    #[test]
    fn a_room_list_is_worth_replaying_to_a_late_subscriber() {
        // The one channel where the next change may be days away. An account
        // that joins no rooms and leaves none would sit on an empty shell
        // until it did.
        assert!(AppEvent::Rooms(a_room_list()).is_worth_keeping());
    }

    #[test]
    fn a_recording_sink_reports_the_last_room_list() {
        let sink = RecordingSink::new();

        sink.emit(AppEvent::Rooms(Rooms::default()));
        sink.emit(AppEvent::Rooms(a_room_list()));

        assert_eq!(sink.last_rooms(), Some(a_room_list()));
    }

    #[test]
    fn a_verification_event_goes_out_on_its_own_channel() {
        let event = AppEvent::Verification(SessionVerification::Unverified);

        assert_eq!(event.channel(), AppEvent::VERIFICATION);
        assert_ne!(AppEvent::VERIFICATION, AppEvent::CONNECTION);
    }

    #[test]
    fn the_verification_payload_is_the_state_itself() {
        let event = AppEvent::Verification(SessionVerification::Unverified);

        let payload = event.payload().unwrap();

        assert_eq!(payload.get("state").unwrap(), "unverified");
        assert!(
            payload.get("Verification").is_none(),
            "the variant name leaked into the wire format: {payload}"
        );
    }

    /// A flow in the given state, for the tests that only care about that.
    fn a_flow(state: FlowState) -> Flow {
        Flow {
            flow_id: "the-only-flow".to_owned(),
            other_user_id: "@bob:example.org".to_owned(),
            is_self_verification: true,
            we_started: false,
            state,
        }
    }

    #[test]
    fn a_verification_flow_goes_out_on_its_own_channel() {
        let event = AppEvent::VerificationFlow(a_flow(FlowState::Requested));

        assert_eq!(event.channel(), AppEvent::VERIFICATION_FLOW);
        assert_ne!(AppEvent::VERIFICATION_FLOW, AppEvent::VERIFICATION);
    }

    #[test]
    fn the_flow_payload_is_the_flow_itself() {
        let event = AppEvent::VerificationFlow(a_flow(FlowState::Requested));

        let payload = event.payload().unwrap();

        assert_eq!(payload.get("flowId").unwrap(), "the-only-flow");
        assert_eq!(payload["state"]["kind"], "requested");
        // The direction reaches the webview too. It decides both the sentence
        // and which buttons are drawn, so a payload without it renders a flow
        // the wrong way round.
        assert_eq!(payload["weStarted"], false);
    }

    /// The microphone test's own channel.
    mod audio {
        use super::*;
        use consort_audio::Reading;

        fn a_reading() -> Reading {
            // Both exactly representable in f32, so the JSON comparison
            // below is about the shape rather than about widening 0.42 to
            // 0.41999998688697815.
            Reading {
                level: 0.5,
                probability: 0.75,
                open: true,
            }
        }

        #[test]
        fn an_audio_event_goes_out_on_its_own_channel() {
            let event = AppEvent::Audio(AudioEvent::Level(a_reading()));

            assert_eq!(event.channel(), AppEvent::AUDIO);
            assert_ne!(AppEvent::AUDIO, AppEvent::CONNECTION);
            assert_ne!(AppEvent::AUDIO, AppEvent::ROOMS);
        }

        #[test]
        fn the_payload_is_the_audio_event_itself() {
            let event = AppEvent::Audio(AudioEvent::Level(a_reading()));

            let payload = event.payload().unwrap();

            assert_eq!(payload["state"], "level");
            assert_eq!(payload["level"], 0.5);
            assert!(
                payload.get("Audio").is_none(),
                "the variant name leaked into the wire format: {payload}"
            );
        }

        #[test]
        fn every_audio_state_can_be_turned_into_a_payload() {
            let events = [
                AudioEvent::Started {
                    device: "Yeti".to_owned(),
                },
                AudioEvent::Stopped,
                AudioEvent::Failed {
                    error: "busy".to_owned(),
                },
                AudioEvent::Level(a_reading()),
            ];

            for event in events {
                let event = AppEvent::Audio(event);
                event
                    .payload()
                    .unwrap_or_else(|error| panic!("{event:?} would not serialise: {error}"));
            }
        }

        #[test]
        fn nothing_about_the_microphone_is_ever_replayed() {
            // A level is not state. Replaying the last one to a webview that
            // remounted would draw a moving bar for a stream that stopped
            // minutes ago, which is the exact opposite of what the replay
            // exists for. The rest goes the same way for the same reason:
            // this channel only speaks while a settings screen is open and
            // listening, and a screen that reopens restarts the test.
            for event in [
                AudioEvent::Started {
                    device: "Yeti".to_owned(),
                },
                AudioEvent::Stopped,
                AudioEvent::Failed {
                    error: "busy".to_owned(),
                },
                AudioEvent::Level(a_reading()),
            ] {
                assert!(
                    !AppEvent::Audio(event.clone()).is_worth_keeping(),
                    "{event:?} would be replayed to a late subscriber"
                );
            }
        }

        #[test]
        fn a_level_reading_does_not_disturb_the_channels_that_are_replayed() {
            // The retention list is walked by channel name. An audio event
            // that matched a held entry would evict it, and a microphone test
            // would silently cost the next remount its room list.
            let inner = Arc::new(RecordingSink::new());
            let latest = LatestSink::new(inner.clone());
            latest.emit(AppEvent::Connection(Connection::Live));
            latest.emit(AppEvent::Rooms(a_room_list()));

            latest.emit(AppEvent::Audio(AudioEvent::Level(a_reading())));
            let before = inner.events().len();
            latest.resend();

            let resent: Vec<&'static str> = inner.events()[before..]
                .iter()
                .map(|event| event.channel())
                .collect();
            assert_eq!(resent, vec![AppEvent::CONNECTION, AppEvent::ROOMS]);
        }
    }

    /// The voice call, on its own channel.
    mod calls {
        use super::*;

        fn in_general() -> CallEvent {
            CallEvent::Connected {
                room_id: "!general:example.org".to_owned(),
                participants: vec![consort_matrix::Participant::named(
                    "@bob:example.org",
                    "Bob",
                )],
                trouble: Some("nobody can hear you".to_owned()),
            }
        }

        #[test]
        fn who_is_talking_does_not_travel_on_the_channel_the_call_is_on() {
            // The same reason a mute does not, only more so: this arrives
            // several times a second, so on the call channel it would evict
            // the call state constantly rather than occasionally.
            let talking = AppEvent::Speaking(vec!["@ada:example.org".to_owned()]);

            assert_eq!(talking.channel(), AppEvent::SPEAKING);
            assert_ne!(AppEvent::SPEAKING, AppEvent::CALL);
            assert_ne!(AppEvent::SPEAKING, AppEvent::SELF_AUDIO);
        }

        #[test]
        fn who_is_talking_is_not_kept_for_a_late_subscriber() {
            // Unlike a mute, which is state. This describes a moment that has
            // passed by the time anybody reloads, and replaying it would draw
            // a ring around somebody who stopped talking before the webview
            // restarted, with nothing to take it off again.
            assert!(!AppEvent::Speaking(vec!["@ada:example.org".to_owned()]).is_worth_keeping());
        }

        #[test]
        fn the_speaking_payload_is_the_bare_list_of_people() {
            let payload = AppEvent::Speaking(vec!["@ada:example.org".to_owned()])
                .payload()
                .unwrap();

            assert_eq!(payload, serde_json::json!(["@ada:example.org"]));
        }

        #[test]
        fn a_mute_does_not_travel_on_the_channel_the_call_is_on() {
            // Only the last event per channel is replayed to a webview that
            // reloaded. Sharing would mean a mute evicting the call it was
            // pressed during, and a client coming back believing it is in no
            // channel while this process is publishing one.
            let muted = AppEvent::SelfAudio(SelfAudio {
                muted: true,
                deafened: false,
            });

            assert_eq!(muted.channel(), AppEvent::SELF_AUDIO);
            assert_ne!(AppEvent::SELF_AUDIO, AppEvent::CALL);
        }

        #[test]
        fn a_mute_is_kept_for_a_late_subscriber() {
            // It is state, like the call it accompanies. A webview that
            // reloaded while muted and was not told would draw a live
            // microphone for a session that is not sending anything.
            assert!(
                AppEvent::SelfAudio(SelfAudio {
                    muted: true,
                    deafened: false,
                })
                .is_worth_keeping()
            );
        }

        #[test]
        fn the_mute_payload_is_the_two_flags_and_nothing_around_them() {
            let payload = AppEvent::SelfAudio(SelfAudio {
                muted: false,
                deafened: true,
            })
            .payload()
            .unwrap();

            assert_eq!(payload["muted"], false);
            assert_eq!(payload["deafened"], true);
            assert!(
                payload.get("state").is_none(),
                "the channel already picked the variant, so a tag inside it is \
                 a wrapper the frontend has to unpick for no information"
            );
        }

        #[test]
        fn a_call_event_goes_out_on_its_own_channel() {
            let event = AppEvent::Call(in_general());

            assert_eq!(event.channel(), AppEvent::CALL);
            assert_ne!(AppEvent::CALL, AppEvent::AUDIO);
            assert_ne!(AppEvent::CALL, AppEvent::CONNECTION);
        }

        #[test]
        fn the_payload_is_the_call_event_itself() {
            // `connection` is the sync loop and `call` is a voice channel.
            // They are two different things that both mean "connected", and
            // the wire format is where that stops being a naming opinion and
            // starts being something the frontend switches on.
            let event = AppEvent::Call(in_general());

            let payload = event.payload().unwrap();

            assert_eq!(payload["state"], "connected");
            assert_eq!(payload["roomId"], "!general:example.org");
            // The roster rides on the state rather than a channel of its own,
            // so the two cannot arrive out of step with each other.
            assert_eq!(payload["participants"][0]["name"], "Bob");
            assert_eq!(payload["trouble"], "nobody can hear you");
            assert!(
                payload.get("Call").is_none(),
                "the variant name leaked into the wire format: {payload}"
            );
        }

        #[test]
        fn every_call_state_can_be_turned_into_a_payload() {
            for event in every_call_state() {
                let event = AppEvent::Call(event);
                event
                    .payload()
                    .unwrap_or_else(|error| panic!("{event:?} would not serialise: {error}"));
            }
        }

        #[test]
        fn every_call_state_is_replayed_to_a_late_subscriber() {
            // Including the failure. A webview that reloads is entitled to
            // find out that it is in a voice channel, and equally entitled to
            // find out that the last attempt did not work: without the second,
            // a reload after a failed join draws a client idling in no channel
            // with nothing to say about why.
            for event in every_call_state() {
                assert!(
                    AppEvent::Call(event.clone()).is_worth_keeping(),
                    "{event:?} would be lost by a webview that reloaded"
                );
            }
        }

        #[test]
        fn the_call_a_late_subscriber_is_told_about_is_the_current_one() {
            // One slot per channel, so the join that failed before the one
            // that worked is not replayed alongside it.
            let inner = Arc::new(RecordingSink::new());
            let latest = LatestSink::new(inner.clone());
            latest.emit(AppEvent::Call(CallEvent::Failed {
                room_id: "!music:example.org".to_owned(),
                error: "no voice server".to_owned(),
            }));
            latest.emit(AppEvent::Call(in_general()));

            let before = inner.events().len();
            latest.resend();

            assert_eq!(inner.events()[before..], [AppEvent::Call(in_general())]);
        }

        fn every_call_state() -> Vec<CallEvent> {
            vec![
                CallEvent::Connecting {
                    room_id: "!general:example.org".to_owned(),
                },
                in_general(),
                CallEvent::Connected {
                    room_id: "!general:example.org".to_owned(),
                    participants: Vec::new(),
                    trouble: None,
                },
                CallEvent::Disconnected,
                CallEvent::Failed {
                    room_id: "!general:example.org".to_owned(),
                    error: "sync has not caught up".to_owned(),
                },
            ]
        }
    }

    /// Which channels are worth catching a late subscriber up on.
    mod retention {
        use super::*;

        #[test]
        fn a_state_channel_is_always_worth_repeating() {
            assert!(AppEvent::Connection(Connection::Live).is_worth_keeping());
            assert!(AppEvent::Verification(SessionVerification::Unverified).is_worth_keeping());
        }

        #[test]
        fn a_flow_in_progress_is_worth_repeating() {
            for state in [
                FlowState::Requested,
                FlowState::Ready,
                FlowState::Waiting,
                FlowState::Confirmed,
            ] {
                assert!(
                    AppEvent::VerificationFlow(a_flow(state.clone())).is_worth_keeping(),
                    "{state:?}"
                );
            }
        }

        #[test]
        fn a_flow_that_is_over_is_not() {
            assert!(!AppEvent::VerificationFlow(a_flow(FlowState::Done)).is_worth_keeping());
        }
    }

    /// Catching up a webview that subscribed late.
    ///
    /// Everything here exists because of one ordering problem: the background
    /// tasks start when the session does, and the webview subscribes whenever
    /// its JavaScript gets around to it. Whoever loses that race, and it is
    /// usually the webview, misses every state published before it arrived.
    mod remembering_the_last_state {
        use super::*;

        fn sink() -> (Arc<RecordingSink>, LatestSink) {
            let inner = Arc::new(RecordingSink::new());
            (inner.clone(), LatestSink::new(inner.clone()))
        }

        #[test]
        fn everything_still_goes_straight_through() {
            let (inner, latest) = sink();

            latest.emit(AppEvent::Connection(Connection::Live));

            assert_eq!(inner.events(), vec![AppEvent::Connection(Connection::Live)]);
        }

        #[test]
        fn resending_repeats_the_last_state_on_every_channel() {
            let (inner, latest) = sink();
            latest.emit(AppEvent::Connection(Connection::Live));
            latest.emit(AppEvent::Verification(SessionVerification::Unverified));

            latest.resend();

            assert_eq!(
                inner.events(),
                vec![
                    AppEvent::Connection(Connection::Live),
                    AppEvent::Verification(SessionVerification::Unverified),
                    AppEvent::Connection(Connection::Live),
                    AppEvent::Verification(SessionVerification::Unverified),
                ]
            );
        }

        #[test]
        fn only_the_most_recent_state_on_a_channel_comes_back() {
            // Replaying the history would walk the interface back through
            // "connecting" and land on "live" only at the end.
            let (inner, latest) = sink();
            latest.emit(AppEvent::Connection(Connection::Connecting));
            latest.emit(AppEvent::Connection(Connection::Live));

            latest.resend();

            assert_eq!(
                inner.events().last(),
                Some(&AppEvent::Connection(Connection::Live))
            );
            assert_eq!(inner.events().len(), 3);
        }

        #[test]
        fn resending_before_anything_happened_says_nothing() {
            // The state of a signed-out app. A webview asking to be caught up
            // should not be told a connection stopped that never started.
            let (inner, latest) = sink();

            latest.resend();

            assert!(inner.events().is_empty());
        }

        #[test]
        fn a_flow_still_in_progress_comes_back() {
            // A verification the user is halfway through is exactly the thing
            // a remount must not lose: the emoji are on screen, the other
            // device is waiting, and there is no way to ask for them again.
            let (inner, latest) = sink();
            latest.emit(AppEvent::VerificationFlow(a_flow(FlowState::Requested)));

            latest.resend();

            assert_eq!(
                inner.events().last(),
                Some(&AppEvent::VerificationFlow(a_flow(FlowState::Requested)))
            );
        }

        #[test]
        fn a_flow_that_has_ended_does_not_come_back() {
            // The other half of the same rule. A flow is state while it is
            // running and history once it is over, and resurrecting "the emoji
            // did not match" on the next mount reports a failure that happened
            // twenty minutes ago as though it had just happened.
            let (inner, latest) = sink();
            latest.emit(AppEvent::VerificationFlow(a_flow(FlowState::Requested)));
            latest.emit(AppEvent::VerificationFlow(a_flow(FlowState::Done)));
            let before = inner.events().len();

            latest.resend();

            assert_eq!(inner.events().len(), before, "a finished flow was resent");
        }

        #[test]
        fn a_flow_ending_does_not_disturb_the_other_channels() {
            let (inner, latest) = sink();
            latest.emit(AppEvent::Connection(Connection::Live));
            latest.emit(AppEvent::VerificationFlow(a_flow(FlowState::Done)));

            latest.resend();

            assert_eq!(
                inner.events().last(),
                Some(&AppEvent::Connection(Connection::Live))
            );
        }

        #[test]
        fn channels_come_back_in_the_order_they_first_spoke() {
            // Not a detail of taste: the frontend applies these in the order
            // it receives them, and a test that asserts on a set rather than a
            // sequence stops catching reordering.
            let (inner, latest) = sink();
            latest.emit(AppEvent::Verification(SessionVerification::Unknown));
            latest.emit(AppEvent::Connection(Connection::Connecting));
            latest.emit(AppEvent::Verification(SessionVerification::Verified));

            latest.resend();

            let resent: Vec<&'static str> = inner.events()[3..]
                .iter()
                .map(|event| event.channel())
                .collect();
            assert_eq!(resent, vec![AppEvent::VERIFICATION, AppEvent::CONNECTION]);
        }
    }
}
