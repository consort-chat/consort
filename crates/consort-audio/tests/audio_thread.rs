// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The thread that owns the microphone.
//!
//! It exists because a cpal stream is `!Send`: it cannot be held in shared
//! application state and it cannot cross an await inside a Tauri command. So it
//! gets a thread of its own and a channel in, which is the same shape the
//! MatrixRTC call will need for the same reason.
//!
//! Every test here drives a fake backend that hands over the frames it is told
//! to and records when it was opened and closed. What is being checked is the
//! thread's bookkeeping, not whether a sound card works.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use consort_audio::capture::{AudioCapture, CaptureError, CaptureStream};
use consort_audio::{AudioEvent, AudioThread, FRAME_SAMPLES, GateConfig};

/// Long enough that a loaded machine is not the reason a test fails, short
/// enough that a genuinely stuck thread does not hold the suite up.
const PATIENCE: Duration = Duration::from_secs(5);

/// What the fake backend was asked to do, in order.
type Log = Arc<Mutex<Vec<String>>>;

struct FakeCapture {
    log: Log,
    /// A device name that cannot be opened.
    broken: Option<String>,
    /// How many frames to hand over the moment a stream opens.
    frames: usize,
    /// How long each of those frames is. Anything but `FRAME_SAMPLES` is a
    /// backend misbehaving.
    frame_samples: usize,
}

impl FakeCapture {
    fn new(log: &Log) -> Self {
        Self {
            log: Arc::clone(log),
            broken: None,
            frames: 0,
            frame_samples: FRAME_SAMPLES,
        }
    }
}

struct FakeStream {
    log: Log,
    device: String,
}

impl CaptureStream for FakeStream {
    fn device_name(&self) -> &str {
        &self.device
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("close {}", self.device));
    }
}

impl AudioCapture for FakeCapture {
    fn open(
        &self,
        device: Option<&str>,
        mut on_frame: Box<dyn FnMut(&[i16]) + Send>,
    ) -> Result<Box<dyn CaptureStream>, CaptureError> {
        let device = device.unwrap_or("Default").to_owned();
        self.log.lock().unwrap().push(format!("open {device}"));

        if self.broken.as_deref() == Some(device.as_str()) {
            return Err(CaptureError::NoDevice);
        }

        // Delivered synchronously, so a test never has to guess how long the
        // microphone takes to say something.
        for _ in 0..self.frames {
            on_frame(&vec![6_000i16; self.frame_samples]);
        }

        Ok(Box::new(FakeStream {
            log: Arc::clone(&self.log),
            device,
        }))
    }
}

fn log_of(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

/// Wait for the next event, failing the test rather than hanging forever.
fn next(events: &Receiver<AudioEvent>) -> AudioEvent {
    match events.recv_timeout(PATIENCE) {
        Ok(event) => event,
        Err(RecvTimeoutError::Timeout) => panic!("the audio thread said nothing in {PATIENCE:?}"),
        Err(RecvTimeoutError::Disconnected) => panic!("the audio thread ended without saying so"),
    }
}

/// Wait for the next event that is not a meter reading.
fn next_state_change(events: &Receiver<AudioEvent>) -> AudioEvent {
    loop {
        match next(events) {
            AudioEvent::Level(_) => continue,
            event => return event,
        }
    }
}

fn thread(capture: FakeCapture) -> (AudioThread, Receiver<AudioEvent>) {
    let (sender, events) = std::sync::mpsc::channel();
    (AudioThread::spawn(Box::new(capture), sender), events)
}

#[test]
fn starting_opens_the_requested_device_and_says_so() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.start(Some("Yeti".to_owned()), GateConfig::default());

    assert_eq!(
        next_state_change(&events),
        AudioEvent::Started {
            device: "Yeti".to_owned()
        }
    );
    assert_eq!(log_of(&log), vec!["open Yeti"]);
}

#[test]
fn starting_without_a_device_asks_the_host_for_its_default() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.start(None, GateConfig::default());

    assert_eq!(
        next_state_change(&events),
        AudioEvent::Started {
            device: "Default".to_owned()
        }
    );
}

#[test]
fn stopping_closes_the_stream() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    audio.stop();

    assert_eq!(next_state_change(&events), AudioEvent::Stopped);
    assert_eq!(log_of(&log), vec!["open Yeti", "close Yeti"]);
}

#[test]
fn stopping_when_nothing_is_running_is_not_an_error() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.stop();

    assert_eq!(
        next_state_change(&events),
        AudioEvent::Stopped,
        "the answer to \"stop\" is \"stopped\" whether or not it was running"
    );
    assert!(
        log_of(&log).is_empty(),
        "nothing was opened, so nothing closes"
    );
}

#[test]
fn switching_device_closes_the_old_stream_before_opening_the_new_one() {
    // Both open at once means two claims on the sound card, and on a device
    // that only allows one the second fails for a reason nothing explains.
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    audio.start(Some("Webcam".to_owned()), GateConfig::default());
    next_state_change(&events);

    assert_eq!(
        log_of(&log),
        vec!["open Yeti", "close Yeti", "open Webcam"],
        "the old stream has to be gone before the new one is asked for"
    );
}

#[test]
fn a_device_that_will_not_open_is_reported_without_killing_the_thread() {
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.broken = Some("Yeti".to_owned());
    let (audio, events) = thread(capture);

    audio.start(Some("Yeti".to_owned()), GateConfig::default());

    match next_state_change(&events) {
        AudioEvent::Failed { error } => assert!(!error.is_empty(), "the reason has to survive"),
        other => panic!("expected a failure, got {other:?}"),
    }

    // Still alive: the whole point of reporting rather than panicking is that
    // the next attempt can work.
    audio.start(Some("Webcam".to_owned()), GateConfig::default());
    assert_eq!(
        next_state_change(&events),
        AudioEvent::Started {
            device: "Webcam".to_owned()
        }
    );
}

#[test]
fn a_failed_start_leaves_nothing_running() {
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.broken = Some("Webcam".to_owned());
    let (audio, events) = thread(capture);
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    audio.start(Some("Webcam".to_owned()), GateConfig::default());
    next_state_change(&events);

    assert_eq!(
        log_of(&log),
        vec!["open Yeti", "close Yeti", "open Webcam"],
        "the working stream is still torn down first; a failed switch must not \
         leave the previous microphone live while the screen says otherwise"
    );
}

#[test]
fn dropping_the_handle_stops_the_thread_and_the_stream() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    drop(audio);

    // The event channel closing is how a caller learns the thread has gone.
    while let Ok(event) = events.recv_timeout(PATIENCE) {
        let _ = event;
    }
    assert_eq!(log_of(&log), vec!["open Yeti", "close Yeti"]);
}

#[test]
fn captured_frames_reach_the_gate_and_come_back_as_readings() {
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    // Enough to clear the warm-up frame and fill a meter batch.
    capture.frames = 40;
    let (audio, events) = thread(capture);

    audio.start(Some("Yeti".to_owned()), GateConfig::default());

    let mut readings = 0;
    let deadline = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < deadline && readings == 0 {
        if let Ok(AudioEvent::Level(reading)) = events.recv_timeout(Duration::from_millis(200)) {
            assert!(
                (0.0..=1.0).contains(&reading.probability),
                "a probability outside [0, 1] is not one: {reading:?}"
            );
            readings += 1;
        }
    }
    assert!(readings > 0, "40 frames of audio produced no meter reading");
    drop(audio);
}

#[test]
fn a_frame_of_the_wrong_length_is_discarded_rather_than_fatal() {
    // `Frames` only ever emits whole frames, so this is a backend doing
    // something unexpected. The gate would panic on it, and a panicked audio
    // thread takes the microphone with it and reports nothing.
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.frames = 40;
    capture.frame_samples = FRAME_SAMPLES - 1;
    let (audio, events) = thread(capture);

    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    assert_eq!(
        next_state_change(&events),
        AudioEvent::Started {
            device: "Yeti".to_owned()
        }
    );

    // Still answering, which it would not be if it had died on the first frame.
    audio.stop();
    assert_eq!(next_state_change(&events), AudioEvent::Stopped);
}

#[test]
fn the_thread_gives_up_when_nobody_is_listening_any_more() {
    // The window closed and the receiver went with it. Carrying on would hold
    // the microphone open for a screen that no longer exists.
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    drop(events);
    audio.start(Some("Webcam".to_owned()), GateConfig::default());
    drop(audio);

    let entries = log_of(&log);
    assert!(
        entries.contains(&"close Yeti".to_owned()),
        "the microphone was left open: {entries:?}"
    );
}
