// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The call thread, joined to the webview and to the microphone.
//!
//! [`consort_call::CallThread`] owns the call and reports over a tokio
//! channel. Everything here is the short walk from that channel to the event
//! sink, and it is deliberately the same shape as [`crate::audio::AudioBridge`]:
//! a plain thread doing a blocking receive in a loop, and a [`Drop`] that ends
//! the owner before the pump so the pump's channel can close.
//!
//! ## Why the pump is not an async task
//!
//! It could be. Tauri has a multi-threaded runtime and the receiver is tokio's.
//! But a task would be one more thing to abort at sign-out, and the audio side
//! already established the pattern with a thread because its channel is
//! synchronous. One pattern beats two.
//!
//! ## Unlike audio, this is part of the signed-in session
//!
//! The audio thread is deliberately not: a microphone has nothing to do with a
//! Matrix account, and the settings screen works signed out. A call is the
//! opposite. It needs a `Client`, its membership is published under this
//! account's user and device, and a sign-out that left one running would leave
//! a name sitting in a voice channel for whoever is signed in next to find.

use std::thread::JoinHandle;

use consort_call::hearing::Ears;
use consort_call::{CallEvent, CallThread, CallTransport, Microphone};

/// A running call thread, with its events wired to the webview.
pub struct CallBridge {
    /// `Option` only so that [`Drop`] can take it and end the call thread
    /// before joining the pump. Always `Some` otherwise.
    thread: Option<CallThread>,
    pump: Option<JoinHandle<()>>,
}

impl CallBridge {
    /// Start the call thread and the pump that forwards what it says.
    ///
    /// `microphone` is where this session's captured audio comes from and
    /// `ears` is where everybody else's goes. Both are handed in rather than
    /// built here, because both ends outlive any one call.
    ///
    /// `report` is called once per event, on the pump thread. It does two jobs
    /// that have to happen in that order: give the microphone back when the
    /// call ends, and tell the webview. Passing a closure rather than an event
    /// sink is what keeps this module from having to know that a microphone
    /// exists.
    pub fn spawn<T: CallTransport>(
        transport: T,
        microphone: Microphone,
        ears: Ears,
        mut report: impl FnMut(CallEvent) + Send + 'static,
    ) -> Self {
        let (events, mut inbox) = tokio::sync::mpsc::unbounded_channel::<CallEvent>();
        let thread = CallThread::spawn(transport, events, microphone, ears);

        let pump = std::thread::Builder::new()
            .name("consort-call-events".to_owned())
            .spawn(move || {
                // Ends when the call thread drops its sender, which it does on
                // the way out. Nothing else needs to tell it to stop.
                //
                // `blocking_recv` rather than an await: this is a plain thread
                // with no runtime on it, which is the same reason the audio
                // pump is one.
                while let Some(event) = inbox.blocking_recv() {
                    report(event);
                }
            })
            .expect("the operating system refused a thread");

        Self {
            thread: Some(thread),
            pump: Some(pump),
        }
    }

    /// Join the call in `room_id`, leaving whatever call is current first.
    pub fn connect(&self, room_id: String) {
        if let Some(thread) = &self.thread {
            thread.connect(room_id);
        }
    }

    /// Leave the current call, if there is one.
    pub fn disconnect(&self) {
        if let Some(thread) = &self.thread {
            thread.disconnect();
        }
    }

    /// Mute or unmute this session's microphone.
    pub fn set_muted(&self, muted: bool) {
        if let Some(thread) = &self.thread {
            thread.set_muted(muted);
        }
    }

    /// Stop or resume receiving the audio of everybody else in the call.
    pub fn set_deafened(&self, deafened: bool) {
        if let Some(thread) = &self.thread {
            thread.set_deafened(deafened);
        }
    }

    /// Say that nobody is at this computer.
    pub fn set_away(&self, away: bool) {
        if let Some(thread) = &self.thread {
            thread.set_away(away);
        }
    }
}

impl Drop for CallBridge {
    fn drop(&mut self) {
        // Order matters and is the whole reason `thread` is an `Option`.
        // Dropping the call thread unwinds the membership and joins the
        // thread, and only once it has returned is its event sender gone,
        // which is what lets the pump's receive fail and the pump end. Joining
        // the pump first would wait forever.
        drop(self.thread.take());
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::testing::{FakeCallTransport, connected};

    /// Long enough that a loaded machine is not the reason a test fails, short
    /// enough that a genuinely stuck thread does not hold the suite up.
    const PATIENCE: Duration = Duration::from_secs(5);

    const GENERAL: &str = "!general:example.org";

    /// Everything the pump has forwarded, in order.
    #[derive(Clone, Default)]
    struct Heard(Arc<Mutex<Vec<CallEvent>>>);

    impl Heard {
        fn seen(&self) -> Vec<CallEvent> {
            self.0.lock().unwrap().clone()
        }

        /// Block until `predicate` holds of the events so far, or give up.
        fn wait_for(&self, what: &str, predicate: impl Fn(&[CallEvent]) -> bool) -> Vec<CallEvent> {
            let deadline = Instant::now() + PATIENCE;
            while Instant::now() < deadline {
                let seen = self.seen();
                if predicate(&seen) {
                    return seen;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("waited {PATIENCE:?} for {what}; heard {:?}", self.seen());
        }
    }

    fn bridge(transport: FakeCallTransport) -> (CallBridge, Heard) {
        let heard = Heard::default();
        let recorder = heard.clone();
        let bridge = CallBridge::spawn(
            transport,
            Microphone::new(),
            crate::ears::speakers(consort_audio::Voices::new()),
            move |event| recorder.0.lock().unwrap().push(event),
        );
        (bridge, heard)
    }

    #[test]
    fn joining_a_call_reaches_the_webview() {
        let (bridge, heard) = bridge(FakeCallTransport::joining());

        bridge.connect(GENERAL.to_owned());

        let seen = heard.wait_for("the call connecting", |seen| {
            seen.contains(&connected(GENERAL))
        });
        assert_eq!(
            seen.first(),
            Some(&CallEvent::Connecting {
                room_id: GENERAL.to_owned()
            }),
            "{seen:?}"
        );
    }

    #[test]
    fn muting_and_deafening_reach_the_webview() {
        // Both, and both as one `SelfAudio`, because they are two switches over
        // one state: the mute that deafening implies has to arrive with the
        // deafen rather than as a second event that could be missed on its own.
        let (bridge, heard) = bridge(FakeCallTransport::joining());
        bridge.connect(GENERAL.to_owned());
        heard.wait_for("the call connecting", |seen| {
            seen.contains(&connected(GENERAL))
        });

        bridge.set_muted(true);
        bridge.set_deafened(true);

        let seen = heard.wait_for("both switches", |seen| {
            seen.contains(&CallEvent::SelfAudio(consort_call::SelfAudio {
                muted: true,
                deafened: true,
                away: false,
            }))
        });
        assert!(
            seen.contains(&CallEvent::SelfAudio(consort_call::SelfAudio {
                muted: true,
                deafened: false,
                away: false,
            })),
            "the mute on its own never arrived: {seen:?}"
        );
    }

    #[test]
    fn a_bridge_whose_thread_has_gone_takes_a_press_without_complaint() {
        // What a stray click on a control that outlived its call is. The
        // thread is only gone once the handle is on its way out, so there is
        // nobody left for an error to reach.
        let (mut bridge, _heard) = bridge(FakeCallTransport::joining());
        drop(bridge.thread.take());

        bridge.set_muted(true);
        bridge.set_deafened(true);
        bridge.disconnect();
    }

    #[test]
    fn a_join_that_fails_reaches_the_webview_carrying_a_sentence() {
        // The common case on a real deployment, not an edge one: sync has not
        // delivered the room, or there is no voice server. All of it has to
        // arrive as something to put on screen.
        let (bridge, heard) = bridge(FakeCallTransport::refusing());

        bridge.connect(GENERAL.to_owned());

        let seen = heard.wait_for("the failure", |seen| {
            seen.iter()
                .any(|event| matches!(event, CallEvent::Failed { .. }))
        });
        let Some(CallEvent::Failed { room_id, error }) = seen
            .iter()
            .find(|event| matches!(event, CallEvent::Failed { .. }))
        else {
            unreachable!()
        };
        assert_eq!(room_id, GENERAL);
        assert!(error.contains(GENERAL), "{error}");
    }

    #[test]
    fn leaving_reaches_the_webview() {
        let (bridge, heard) = bridge(FakeCallTransport::joining());
        bridge.connect(GENERAL.to_owned());
        heard.wait_for("the call connecting", |seen| {
            seen.contains(&connected(GENERAL))
        });

        bridge.disconnect();

        heard.wait_for("the call ending", |seen| {
            seen.contains(&CallEvent::Disconnected)
        });
    }

    #[test]
    fn dropping_the_bridge_ends_both_threads() {
        // The test that would hang rather than fail if the drop order were
        // wrong. The pump only ends once the call thread has dropped its
        // sender, and the call thread is what unwinds the membership, so
        // joining the pump first would wait for something waiting on it.
        let (bridge, heard) = bridge(FakeCallTransport::joining());
        bridge.connect(GENERAL.to_owned());
        heard.wait_for("the call connecting", |seen| {
            seen.contains(&connected(GENERAL))
        });

        drop(bridge);
    }

    #[test]
    fn a_bridge_nobody_asked_anything_of_still_shuts_down() {
        let (bridge, heard) = bridge(FakeCallTransport::joining());

        drop(bridge);

        assert!(heard.seen().is_empty(), "an idle call said something");
    }
}
