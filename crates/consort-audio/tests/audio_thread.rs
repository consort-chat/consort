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
use consort_audio::playback::{AudioPlayback, PlaybackError, PlaybackStream, ToneEnded};
use consort_audio::{
    AudioEvent, AudioThread, FRAME_SAMPLES, GateConfig, GatedSink, PRE_ROLL_FRAMES, Tone, Voices,
};

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

/// A pair of speakers that never make a sound.
///
/// The chime itself is `tests/tone.rs`. What is being checked here is the
/// bookkeeping around it: which device was opened, when it was closed, and
/// what happens when two presses overlap.
struct FakePlayback {
    log: Log,
    /// A device name that cannot be opened.
    broken: Option<String>,
    /// Every `on_end` handed over so far, so a test can decide when a chime
    /// finishes rather than racing a real one.
    endings: Arc<Mutex<Vec<ToneEnded>>>,
    /// The voices the call output was opened on, so a test can put audio into
    /// the far end and see it arrive.
    voices: Arc<Mutex<Option<Voices>>>,
}

impl FakePlayback {
    fn new(log: &Log) -> Self {
        Self {
            log: Arc::clone(log),
            broken: None,
            endings: Arc::default(),
            voices: Arc::default(),
        }
    }

    /// Tell the thread that the `nth` chime opened has finished playing.
    fn finish(endings: &Arc<Mutex<Vec<ToneEnded>>>, nth: usize) {
        let mut held = endings.lock().unwrap();
        assert!(held.len() > nth, "only {} chimes have opened", held.len());
        held[nth]();
    }
}

struct FakeTone {
    log: Log,
    device: String,
}

impl PlaybackStream for FakeTone {
    fn device_name(&self) -> &str {
        &self.device
    }
}

impl Drop for FakeTone {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(format!("silence {}", self.device));
    }
}

impl AudioPlayback for FakePlayback {
    fn play(
        &self,
        device: Option<&str>,
        _tone: Tone,
        on_end: ToneEnded,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError> {
        let device = device.unwrap_or("Default Out").to_owned();
        self.log.lock().unwrap().push(format!("play {device}"));

        if self.broken.as_deref() == Some(device.as_str()) {
            return Err(PlaybackError::NoDevice);
        }

        self.endings.lock().unwrap().push(on_end);
        Ok(Box::new(FakeTone {
            log: Arc::clone(&self.log),
            device,
        }))
    }

    fn play_call(
        &self,
        device: Option<&str>,
        voices: Voices,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError> {
        let device = device.unwrap_or("Default Out").to_owned();
        self.log.lock().unwrap().push(format!("call {device}"));

        if self.broken.as_deref() == Some(device.as_str()) {
            return Err(PlaybackError::NoDevice);
        }

        // Held so the test can see what the thread would be playing.
        *self.voices.lock().unwrap() = Some(voices);
        Ok(Box::new(FakeTone {
            log: Arc::clone(&self.log),
            device,
        }))
    }
}

fn thread(capture: FakeCapture) -> (AudioThread, Receiver<AudioEvent>) {
    let log = Arc::clone(&capture.log);
    let (audio, events, _) = thread_with(capture, FakePlayback::new(&log));
    (audio, events)
}

type Endings = Arc<Mutex<Vec<ToneEnded>>>;

fn thread_with(
    capture: FakeCapture,
    playback: FakePlayback,
) -> (AudioThread, Receiver<AudioEvent>, Endings) {
    let (sender, events) = std::sync::mpsc::channel();
    let endings = Arc::clone(&playback.endings);
    (
        AudioThread::spawn(Box::new(capture), Box::new(playback), sender),
        events,
        endings,
    )
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

// The test tone. An input can be verified by talking; an output cannot be
// verified by anything unless something plays, so this is the only evidence
// the output picker will ever have.

#[test]
fn playing_the_tone_opens_the_chosen_output_and_says_so() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.play_tone(Some("Headphones".to_owned()));

    assert_eq!(
        next_state_change(&events),
        AudioEvent::ToneStarted {
            device: "Headphones".to_owned()
        }
    );
    assert_eq!(log_of(&log), vec!["play Headphones"]);
}

#[test]
fn playing_without_a_device_asks_the_host_for_its_default() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.play_tone(None);

    assert_eq!(
        next_state_change(&events),
        AudioEvent::ToneStarted {
            device: "Default Out".to_owned()
        }
    );
}

#[test]
fn the_chime_closes_the_device_when_it_finishes() {
    // Nothing else will. The stream is what holds the output open, and a
    // stream left behind after a 320 ms sound is an application that has
    // quietly taken the speakers for the rest of the session.
    let log = Log::default();
    let (audio, events, endings) = thread_with(FakeCapture::new(&log), FakePlayback::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    FakePlayback::finish(&endings, 0);

    assert_eq!(next_state_change(&events), AudioEvent::ToneStopped);
    assert_eq!(log_of(&log), vec!["play Headphones", "silence Headphones"]);
}

#[test]
fn stopping_the_tone_early_closes_the_device() {
    // What closing the settings screen does, mid-chime.
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    audio.stop_tone();

    assert_eq!(next_state_change(&events), AudioEvent::ToneStopped);
    assert_eq!(log_of(&log), vec!["play Headphones", "silence Headphones"]);
}

#[test]
fn stopping_a_tone_that_is_not_playing_is_not_an_error() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));

    audio.stop_tone();

    assert_eq!(next_state_change(&events), AudioEvent::ToneStopped);
    assert!(log_of(&log).is_empty(), "nothing opened, so nothing closes");
}

#[test]
fn pressing_twice_replaces_the_chime_rather_than_layering_it() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    assert_eq!(
        log_of(&log),
        vec!["play Headphones", "silence Headphones", "play Headphones"],
        "two chimes at once is twice the volume and half the information"
    );
}

#[test]
fn a_chime_that_finishes_late_does_not_silence_the_one_that_replaced_it() {
    // The race that makes the button feel broken. Press it, press it again
    // before the first has finished, and the first one's "I am done" arrives
    // while the second is playing. Acting on it cuts the second chime off and
    // leaves the button saying nothing is playing while something is.
    let log = Log::default();
    let (audio, events, endings) = thread_with(FakeCapture::new(&log), FakePlayback::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    FakePlayback::finish(&endings, 0);

    assert_eq!(
        events.recv_timeout(Duration::from_millis(200)),
        Err(RecvTimeoutError::Timeout),
        "the stale ending stopped the chime that replaced it"
    );
    // And the live one still ends when it is actually over.
    FakePlayback::finish(&endings, 1);
    assert_eq!(next_state_change(&events), AudioEvent::ToneStopped);
}

#[test]
fn an_ending_that_arrives_after_a_stop_is_ignored() {
    // Same race, the other way round: the chime is stopped by hand and its
    // callback reports the end afterwards. Two `ToneStopped` events would put
    // the button back to idle twice, which is harmless, and would also close
    // whatever started in between, which is not.
    let log = Log::default();
    let (audio, events, endings) = thread_with(FakeCapture::new(&log), FakePlayback::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);
    audio.stop_tone();
    next_state_change(&events);

    FakePlayback::finish(&endings, 0);

    assert_eq!(
        events.recv_timeout(Duration::from_millis(200)),
        Err(RecvTimeoutError::Timeout),
        "a stopped chime reported its end and was believed twice"
    );
}

#[test]
fn an_output_that_will_not_open_is_reported_without_killing_the_thread() {
    let log = Log::default();
    let mut playback = FakePlayback::new(&log);
    playback.broken = Some("Headphones".to_owned());
    let (audio, events, _) = thread_with(FakeCapture::new(&log), playback);

    audio.play_tone(Some("Headphones".to_owned()));

    match next_state_change(&events) {
        AudioEvent::ToneFailed { error } => assert!(!error.is_empty(), "the reason has to survive"),
        other => panic!("expected a failure, got {other:?}"),
    }

    // Still alive, and the microphone it also owns is unharmed.
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    assert_eq!(
        next_state_change(&events),
        AudioEvent::Started {
            device: "Yeti".to_owned()
        }
    );
}

#[test]
fn the_chime_and_the_microphone_do_not_disturb_each_other() {
    // They share a thread, which is the only place a cpal stream can live, so
    // the obvious bug is one of them tearing down the other.
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.start(Some("Yeti".to_owned()), GateConfig::default());
    next_state_change(&events);

    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);
    audio.stop_tone();
    next_state_change(&events);

    assert_eq!(
        log_of(&log),
        vec!["open Yeti", "play Headphones", "silence Headphones"],
        "the microphone was disturbed by the test tone"
    );
    drop(audio);
}

#[test]
fn dropping_the_handle_silences_a_chime_that_is_still_playing() {
    let log = Log::default();
    let (audio, events) = thread(FakeCapture::new(&log));
    audio.play_tone(Some("Headphones".to_owned()));
    next_state_change(&events);

    drop(audio);

    assert_eq!(log_of(&log), vec!["play Headphones", "silence Headphones"]);
}

// Where the gate's output goes. Until a call is up it goes nowhere, which is
// what it did for the whole of the settings work; these are about the reader
// that arrives with one.

/// Every frame handed to an installed sink, with the gate's verdict.
type Published = Arc<Mutex<Vec<(Vec<i16>, bool)>>>;

fn recording() -> (Published, GatedSink) {
    let published: Published = Published::default();
    let recorded = Arc::clone(&published);
    (
        published,
        Box::new(move |samples: &[i16], open: bool| {
            recorded.lock().unwrap().push((samples.to_vec(), open));
        }),
    )
}

/// Wait for the capture to open, then close it, and answer when it has.
///
/// Deterministic without a sleep, and the order matters. The fake backend
/// posts its frames onto the same channel the commands arrive on, from inside
/// `open`, so they are only queued behind a `Stop` if the `Stop` is sent after
/// `Started` has been seen. Sending both at once discards every frame instead.
fn capture_a_device(audio: &AudioThread, events: &Receiver<AudioEvent>, device: &str) {
    audio.start(Some(device.to_owned()), GateConfig::default());
    assert!(matches!(
        next_state_change(events),
        AudioEvent::Started { .. }
    ));
}

fn stop_capturing(audio: &AudioThread, events: &Receiver<AudioEvent>) {
    audio.stop();
    assert_eq!(next_state_change(events), AudioEvent::Stopped);
}

/// Run one device's worth of frames through a thread and collect what a sink
/// installed on it saw.
fn published(capture: FakeCapture, install: bool) -> Vec<(Vec<i16>, bool)> {
    let (published, sink) = recording();
    let (audio, events) = thread(capture);

    if install {
        audio.publish_to(sink);
    }
    capture_a_device(&audio, &events, "Yeti");
    stop_capturing(&audio, &events);
    drop(audio);

    published.lock().unwrap().clone()
}

#[test]
fn every_gated_frame_reaches_the_sink_bar_the_ones_still_in_the_pre_roll() {
    let mut capture = FakeCapture::new(&Log::default());
    capture.frames = 40;

    let frames = published(capture, true);

    assert_eq!(
        frames.len(),
        40 - PRE_ROLL_FRAMES,
        "the publication runs a pre-roll behind the microphone, so the last \
         few frames of a capture are still in the line when it stops"
    );
    assert!(
        frames
            .iter()
            .all(|(samples, _)| samples.len() == FRAME_SAMPLES),
        "a publication only accepts whole frames"
    );
}

#[test]
fn nothing_is_published_until_a_sink_is_installed() {
    // The state before a call, and the state this spent the whole of the
    // settings work in. An idle Consort must do no more work than it did then.
    let mut capture = FakeCapture::new(&Log::default());
    capture.frames = 40;

    assert!(published(capture, false).is_empty());
}

#[test]
fn what_goes_out_is_the_gate_s_output_and_not_the_raw_capture() {
    // The difference is the whole point of the gate. The fake microphone hands
    // over one constant value, and the denoiser cannot leave it that way.
    let mut capture = FakeCapture::new(&Log::default());
    capture.frames = 40;

    let frames = published(capture, true);

    assert!(
        frames
            .iter()
            .any(|(samples, _)| samples.iter().any(|sample| *sample != 6_000)),
        "the raw capture reached the sink untouched"
    );
}

#[test]
fn the_warm_up_frame_goes_out_silent_however_the_gate_ends_up_marking_it() {
    // RNNoise fades in over its first output, so that frame is a ramp rather
    // than anything anybody said. It used to be safe to leave it alone because
    // the gate is always shut on it, but the pre-roll reaches backwards: a gate
    // that opens in the first 30 ms marks it open on the way past.
    let mut capture = FakeCapture::new(&Log::default());
    capture.frames = 40;

    let frames = published(capture, true);

    let (first, open) = &frames[0];
    assert!(open, "this capture opens the gate inside the pre-roll");
    assert!(
        first.iter().all(|sample| *sample == 0),
        "the fade-in ramp was published as the first thing a listener hears"
    );
}

#[test]
fn removing_the_sink_stops_the_frames() {
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.frames = 40;
    let (published, sink) = recording();
    let (audio, events) = thread(capture);

    audio.publish_to(sink);
    capture_a_device(&audio, &events, "Yeti");
    stop_capturing(&audio, &events);
    let during = published.lock().unwrap().len();
    assert_eq!(
        during,
        40 - PRE_ROLL_FRAMES,
        "nothing was published while the call was up"
    );

    audio.stop_publishing();
    capture_a_device(&audio, &events, "Yeti");
    stop_capturing(&audio, &events);

    assert_eq!(
        published.lock().unwrap().len(),
        during,
        "frames kept going out after the call ended"
    );
}

#[test]
fn changing_microphone_mid_call_keeps_publishing() {
    // The sink outlives the device, which is why it is not held inside
    // whatever is currently capturing. Somebody unplugging a headset during a
    // call must not go silent for the rest of it.
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.frames = 40;
    let (published, sink) = recording();
    let (audio, events) = thread(capture);

    audio.publish_to(sink);
    capture_a_device(&audio, &events, "Yeti");
    capture_a_device(&audio, &events, "Webcam");
    stop_capturing(&audio, &events);

    assert_eq!(
        published.lock().unwrap().len(),
        2 * (40 - PRE_ROLL_FRAMES),
        "each device runs its own pre-roll, because publishing 30 ms captured \
         from the microphone somebody just switched away from is worse than \
         the gap"
    );
}

#[test]
fn the_monitor_plays_the_microphone_back_through_an_output() {
    let log = Log::default();
    let mut capture = FakeCapture::new(&log);
    capture.frames = 40;
    let playback = FakePlayback::new(&log);
    let voices = Arc::clone(&playback.voices);
    let (audio, events, _) = thread_with(capture, playback);

    audio.start_monitor(Some("Speakers".to_owned()));
    assert!(matches!(
        next_state_change(&events),
        AudioEvent::MonitorStarted { device } if device == "Speakers"
    ));

    capture_a_device(&audio, &events, "Yeti");

    let held = voices
        .lock()
        .unwrap()
        .clone()
        .expect("an output was opened");
    let deadline = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < deadline && held.waiting("you") == 0 {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        held.waiting("you") > 0,
        "what somebody hears has to be what the gate is putting out, or the \
         button is a different question from the one it asks"
    );
}

#[test]
fn the_monitor_is_a_stream_of_its_own_rather_than_the_call_s() {
    // Monitoring during a call would otherwise put one voice into everybody
    // else's mix, which is the opposite of what the button is for.
    let log = Log::default();
    let (audio, events, _) = thread_with(FakeCapture::new(&log), FakePlayback::new(&log));

    audio.play_call(Some("Speakers".to_owned()), Voices::new());
    next_state_change(&events);
    audio.start_monitor(Some("Speakers".to_owned()));
    next_state_change(&events);
    audio.stop_monitor();
    assert!(matches!(
        next_state_change(&events),
        AudioEvent::MonitorStopped
    ));

    // The call's output is still open: only the monitor's was given up.
    assert_eq!(
        log_of(&log)
            .iter()
            .filter(|line| line.as_str() == "silence Speakers")
            .count(),
        1,
        "stopping the monitor must not take the call's output with it"
    );
}

#[test]
fn a_monitor_that_cannot_open_says_so_rather_than_going_quiet() {
    let log = Log::default();
    let mut playback = FakePlayback::new(&log);
    playback.broken = Some("Broken".to_owned());
    let (audio, events, _) = thread_with(FakeCapture::new(&log), playback);

    audio.start_monitor(Some("Broken".to_owned()));

    assert!(matches!(
        next_state_change(&events),
        AudioEvent::MonitorFailed { .. }
    ));
}

#[test]
fn asking_to_monitor_twice_leaves_one_stream_open() {
    let log = Log::default();
    let (audio, events, _) = thread_with(FakeCapture::new(&log), FakePlayback::new(&log));

    audio.start_monitor(Some("Speakers".to_owned()));
    next_state_change(&events);
    audio.start_monitor(Some("Speakers".to_owned()));
    next_state_change(&events);
    audio.stop_monitor();
    next_state_change(&events);

    assert_eq!(
        log_of(&log),
        vec![
            "call Speakers",
            "silence Speakers",
            "call Speakers",
            "silence Speakers"
        ],
        "the first stream has to be given up before the second is opened, or \
         a device that allows one claim refuses the second for no visible \
         reason"
    );
}
