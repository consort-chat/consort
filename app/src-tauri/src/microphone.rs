// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The microphone test, joined to the webview.
//!
//! [`consort_audio::AudioThread`] already owns the sound card and reports over
//! a `std::sync::mpsc` channel. Everything in this file is the short walk from
//! that channel to the event sink: a plain thread doing a blocking `recv` in a
//! loop, because the channel is synchronous and wrapping it in an async task
//! would mean either polling it or bringing in a second channel type to bridge
//! the two.
//!
//! Deliberately not part of the signed-in session. The settings screen works
//! signed out, the microphone has nothing to do with a Matrix account, and
//! tying the two together would mean a sign-out silently killed a level meter
//! somebody was in the middle of reading.

use std::sync::Arc;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;

use consort_audio::{AudioCapture, AudioEvent, AudioThread, GateConfig};

use crate::events::{AppEvent, EventSink};

/// A running audio thread, with its events wired to the webview.
///
/// Held for as long as the application runs rather than per test, because
/// opening and closing the sound card is the slow part and a person adjusting
/// a device picker does it repeatedly. Idle until [`start`](Self::start).
pub struct Microphone {
    /// `Option` only so that [`Drop`] can take it and end the audio thread
    /// before joining the pump. Always `Some` otherwise.
    thread: Option<AudioThread>,
    pump: Option<JoinHandle<()>>,
}

impl Microphone {
    /// Start the audio thread and the pump that forwards what it says.
    pub fn spawn(capture: Box<dyn AudioCapture>, sink: Arc<dyn EventSink>) -> Self {
        let (events, inbox) = channel::<AudioEvent>();
        let thread = AudioThread::spawn(capture, events);

        let pump = std::thread::Builder::new()
            .name("consort-audio-events".to_owned())
            .spawn(move || {
                // Ends when the audio thread drops its sender, which it does
                // on the way out. Nothing else needs to tell it to stop.
                while let Ok(event) = inbox.recv() {
                    sink.emit(AppEvent::Audio(event));
                }
            })
            .expect("the operating system refused a thread");

        Self {
            thread: Some(thread),
            pump: Some(pump),
        }
    }

    /// Begin capturing from `device`, or from the host's default.
    pub fn start(&self, device: Option<String>, gate: GateConfig) {
        if let Some(thread) = &self.thread {
            thread.start(device, gate);
        }
    }

    /// Stop capturing, releasing the device.
    pub fn stop(&self) {
        if let Some(thread) = &self.thread {
            thread.stop();
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        // Order matters and is the whole reason `thread` is an `Option`.
        // Dropping the audio thread joins it, and only once it has returned is
        // its event sender gone, which is what lets the pump's `recv` fail and
        // the pump end. Joining the pump first would wait forever.
        drop(self.thread.take());
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use consort_audio::{CaptureError, CaptureStream, FRAME_SAMPLES, FrameSink};

    use crate::events::RecordingSink;

    /// Long enough that a loaded machine is not the reason a test fails, short
    /// enough that a genuinely stuck thread does not hold the suite up.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// A backend that hands over the frames it is told to and never touches
    /// hardware.
    struct FakeCapture {
        frames: usize,
        broken: bool,
    }

    struct FakeStream {
        device: String,
    }

    impl CaptureStream for FakeStream {
        fn device_name(&self) -> &str {
            &self.device
        }
    }

    impl AudioCapture for FakeCapture {
        fn open(
            &self,
            device: Option<&str>,
            mut on_frame: FrameSink,
        ) -> Result<Box<dyn CaptureStream>, CaptureError> {
            if self.broken {
                return Err(CaptureError::NoDevice);
            }
            // Delivered synchronously, so no test has to guess how long a
            // microphone takes to say something.
            for _ in 0..self.frames {
                on_frame(&vec![6_000i16; FRAME_SAMPLES]);
            }
            Ok(Box::new(FakeStream {
                device: device.unwrap_or("Default").to_owned(),
            }))
        }
    }

    /// A sink that can be waited on, wrapping the recording one.
    struct Waitable {
        inner: Mutex<Vec<AudioEvent>>,
    }

    impl Waitable {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                inner: Mutex::new(Vec::new()),
            })
        }

        /// Everything seen so far.
        fn seen(&self) -> Vec<AudioEvent> {
            self.inner.lock().unwrap().clone()
        }

        /// Block until `predicate` holds of the events so far, or give up.
        fn wait_for(
            &self,
            what: &str,
            predicate: impl Fn(&[AudioEvent]) -> bool,
        ) -> Vec<AudioEvent> {
            let deadline = Instant::now() + PATIENCE;
            while Instant::now() < deadline {
                let seen = self.seen();
                if predicate(&seen) {
                    return seen;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("waited {PATIENCE:?} for {what}; saw {:?}", self.seen());
        }
    }

    impl EventSink for Waitable {
        fn emit(&self, event: AppEvent) {
            let AppEvent::Audio(event) = event else {
                panic!("the microphone emitted something that was not an audio event");
            };
            self.inner.lock().unwrap().push(event);
        }
    }

    fn microphone(frames: usize, sink: Arc<Waitable>) -> Microphone {
        Microphone::spawn(
            Box::new(FakeCapture {
                frames,
                broken: false,
            }),
            sink,
        )
    }

    #[test]
    fn opening_a_device_reaches_the_webview_and_names_it() {
        let sink = Waitable::new();
        let microphone = microphone(0, sink.clone());

        microphone.start(Some("Yeti".to_owned()), GateConfig::default());

        let seen = sink.wait_for("the started event", |seen| !seen.is_empty());
        assert_eq!(
            seen[0],
            AudioEvent::Started {
                device: "Yeti".to_owned()
            }
        );
    }

    #[test]
    fn asking_for_no_device_in_particular_opens_the_hosts_default() {
        // The first-run path. Nothing saved means nothing is named, and the
        // backend picks.
        let sink = Waitable::new();
        let microphone = microphone(0, sink.clone());

        microphone.start(None, GateConfig::default());

        let seen = sink.wait_for("the started event", |seen| !seen.is_empty());
        assert_eq!(
            seen[0],
            AudioEvent::Started {
                device: "Default".to_owned()
            }
        );
    }

    #[test]
    fn level_readings_reach_the_webview() {
        // Five frames is one reading: the meter batches at 50 ms so that a
        // moving bar costs twenty events a second rather than a hundred.
        let sink = Waitable::new();
        let microphone = microphone(5, sink.clone());

        microphone.start(None, GateConfig::default());

        let seen = sink.wait_for("a level reading", |seen| {
            seen.iter()
                .any(|event| matches!(event, AudioEvent::Level(_)))
        });
        let Some(AudioEvent::Level(reading)) = seen
            .iter()
            .find(|event| matches!(event, AudioEvent::Level(_)))
        else {
            unreachable!()
        };
        assert!(
            reading.level > 0.0,
            "a frame of real signal metered as silence: {reading:?}"
        );
    }

    #[test]
    fn a_failure_to_open_reaches_the_webview_rather_than_being_swallowed() {
        // The common case on a real desktop, not an edge one: the device is
        // held by something else, or it went away between the list being drawn
        // and the button being pressed.
        let sink = Waitable::new();
        let microphone = Microphone::spawn(
            Box::new(FakeCapture {
                frames: 0,
                broken: true,
            }),
            sink.clone(),
        );

        microphone.start(None, GateConfig::default());

        let seen = sink.wait_for("the failure", |seen| !seen.is_empty());
        assert!(
            matches!(&seen[0], AudioEvent::Failed { error } if !error.is_empty()),
            "expected a failure carrying a sentence, got {:?}",
            seen[0]
        );
    }

    #[test]
    fn stopping_reaches_the_webview() {
        let sink = Waitable::new();
        let microphone = microphone(0, sink.clone());
        microphone.start(None, GateConfig::default());
        sink.wait_for("the started event", |seen| !seen.is_empty());

        microphone.stop();

        sink.wait_for("the stopped event", |seen| {
            seen.contains(&AudioEvent::Stopped)
        });
    }

    #[test]
    fn dropping_the_microphone_ends_both_threads() {
        // The test that would hang rather than fail if the drop order were
        // wrong. The pump only ends once the audio thread has dropped its
        // sender, and the audio thread is what holds the device open, so
        // joining the pump first would wait for something that is waiting for
        // it.
        let sink = Waitable::new();
        let microphone = microphone(0, sink.clone());
        microphone.start(None, GateConfig::default());
        sink.wait_for("the started event", |seen| !seen.is_empty());

        drop(microphone);
    }

    #[test]
    fn a_microphone_that_was_never_started_still_shuts_down() {
        let sink = Waitable::new();

        drop(microphone(0, sink.clone()));

        assert!(sink.seen().is_empty(), "an idle microphone said something");
    }

    #[test]
    fn everything_it_says_goes_out_on_the_audio_channel() {
        // The sink above asserts the variant. This one asserts the wire name,
        // which is what a listener in `api.ts` actually subscribes to.
        let sink = Arc::new(RecordingSink::new());
        let microphone = Microphone::spawn(
            Box::new(FakeCapture {
                frames: 5,
                broken: false,
            }),
            sink.clone(),
        );

        microphone.start(None, GateConfig::default());
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline && sink.events().len() < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }

        let events = sink.events();
        assert!(
            events.len() >= 2,
            "expected a start and a reading: {events:?}"
        );
        for event in events {
            assert_eq!(event.channel(), AppEvent::AUDIO);
        }
    }
}
