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

use consort_matrix::{Connection, Flow, KeyBackup, SessionVerification};

/// Something the frontend needs to be told about without having asked.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    /// The health of the sync loop changed.
    Connection(Connection),
    /// Whether this session is verified changed.
    Verification(SessionVerification),
    /// A verification flow started, moved on, or ended.
    VerificationFlow(Flow),
    /// What is happening to this session's room keys changed.
    KeyBackup(KeyBackup),
}

impl AppEvent {
    /// The channel carrying sync-loop health.
    pub const CONNECTION: &'static str = "connection";
    /// The channel carrying this session's verification state.
    pub const VERIFICATION: &'static str = "verification";
    /// The channel carrying the progress of one verification flow.
    pub const VERIFICATION_FLOW: &'static str = "verification-flow";
    /// The channel carrying whether room keys are being backed up.
    pub const KEY_BACKUP: &'static str = "key-backup";

    /// The channel this event goes out on.
    pub fn channel(&self) -> &'static str {
        match self {
            Self::Connection(_) => Self::CONNECTION,
            Self::Verification(_) => Self::VERIFICATION,
            Self::VerificationFlow(_) => Self::VERIFICATION_FLOW,
            Self::KeyBackup(_) => Self::KEY_BACKUP,
        }
    }

    /// Whether a late subscriber should be caught up on this.
    ///
    /// Three of the four channels carry state: there is always a current
    /// connection, a current verification state and a current answer about
    /// room keys, and a webview that missed the last one is a webview showing
    /// the wrong thing until something else happens to change.
    ///
    /// A flow is state only while it is running. Once it is done or cancelled
    /// it is history, and replaying it on the next mount would put "the emoji
    /// did not match" back on screen for a flow that ended twenty minutes ago.
    /// A flow still in progress is very much worth keeping: the other device
    /// is waiting, and there is no way to ask for the emoji again.
    pub fn is_worth_keeping(&self) -> bool {
        match self {
            Self::Connection(_) | Self::Verification(_) | Self::KeyBackup(_) => true,
            Self::VerificationFlow(flow) => !flow.state.is_final(),
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
            Self::VerificationFlow(flow) => serde_json::to_value(flow),
            Self::KeyBackup(state) => serde_json::to_value(state),
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
    use consort_matrix::{FlowState, StopReason};

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
