// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Stand-ins shared by the tests in this crate.
//!
//! Here rather than in each test module for the reason
//! [`crate::events::RecordingSink`] is: three modules need a sound card that is
//! not one, and three copies of it would drift until a test passed for a
//! reason its neighbour did not.
//!
//! Nothing here is compiled into the application.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use consort_audio::{
    AudioCapture, AudioDevices, AudioEvent, AudioPlayback, CaptureError, CaptureStream, Device,
    Direction, FRAME_SAMPLES, FrameSink, PlaybackError, PlaybackStream, Tone, ToneEnded,
};

use crate::audio::Backends;
use crate::events::{AppEvent, EventSink};

/// Long enough that a loaded machine is not the reason a test fails, short
/// enough that a genuinely stuck thread does not hold the suite up.
pub const PATIENCE: Duration = Duration::from_secs(5);

/// A microphone that hands over one loud frame and never touches hardware.
///
/// Synchronously, from inside `open`, so no test has to guess how long a real
/// device takes to say something.
pub struct FakeCapture;

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
        on_frame(&vec![9_000i16; FRAME_SAMPLES]);
        Ok(Box::new(FakeStream {
            device: device.unwrap_or("Default").to_owned(),
        }))
    }
}

/// Speakers that make no sound.
pub struct FakePlayback;

struct FakeTone;

impl PlaybackStream for FakeTone {
    fn device_name(&self) -> &str {
        "Speakers"
    }
}

impl AudioPlayback for FakePlayback {
    fn play(
        &self,
        _device: Option<&str>,
        _tone: Tone,
        _on_end: ToneEnded,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError> {
        Ok(Box::new(FakeTone))
    }
}

/// A machine with one microphone and one pair of speakers.
pub struct FakeDevices;

impl AudioDevices for FakeDevices {
    fn enumerate(&self, direction: Direction) -> Vec<Device> {
        let name = match direction {
            Direction::Input => "Yeti",
            Direction::Output => "Headphones",
        };
        vec![Device {
            name: name.to_owned(),
            is_default: true,
        }]
    }
}

/// A sound card that is not one.
pub fn fake_backends() -> Backends {
    Backends {
        capture: Box::new(FakeCapture),
        playback: Box::new(FakePlayback),
    }
}

/// Everything the audio thread has said, waitable.
#[derive(Default)]
pub struct HeardAudio(Mutex<Vec<AudioEvent>>);

impl HeardAudio {
    /// How many times a device has been opened.
    ///
    /// The number the microphone-sharing tests are about. Every extra one is a
    /// hole in what a peer hears, and a missing one is somebody nobody can
    /// hear at all.
    pub fn opens(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, AudioEvent::Started { .. }))
            .count()
    }

    /// How many times a device has been given back.
    pub fn stops(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, AudioEvent::Stopped))
            .count()
    }

    /// Block until `predicate` holds, or give up and say what was heard.
    pub fn wait_for(&self, what: &str, predicate: impl Fn(&Self) -> bool) {
        wait_for(what, || predicate(self), || format!("{:?}", self.0.lock()));
    }
}

impl EventSink for HeardAudio {
    fn emit(&self, event: AppEvent) {
        let AppEvent::Audio(event) = event else {
            panic!("the audio thread emitted something that was not an audio event");
        };
        self.0.lock().unwrap().push(event);
    }
}

/// Block until `predicate` holds, or panic saying what `context` reports.
///
/// Polling rather than a channel because what is being waited for is a thread
/// having got round to something, which nothing signals.
pub fn wait_for(what: &str, predicate: impl Fn() -> bool, context: impl Fn() -> String) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("waited {PATIENCE:?} for {what}; saw {}", context());
}

/// A gated sink that counts frames rather than doing anything with them.
pub fn counting_sink() -> (
    Arc<std::sync::atomic::AtomicUsize>,
    consort_audio::GatedSink,
) {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mine = count.clone();
    (
        count,
        Box::new(move |_samples, _open| {
            mine.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    )
}

/// How many frames a [`counting_sink`] has seen.
pub fn frames(count: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
    count.load(std::sync::atomic::Ordering::Relaxed)
}

/// Block until `produce` returns something, or panic.
///
/// The variant for a value rather than a condition: what a test wants next is
/// usually the thing it was waiting for.
pub fn wait_for_value<T>(what: &str, produce: impl Fn() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if let Some(value) = produce() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("waited {PATIENCE:?} for {what}");
}

/// A call transport that joins whatever it is asked for, or refuses
/// everything.
///
/// Here rather than in each test module for the same reason the sound card
/// above is: `crate::call` and `crate::state` both need one, and two copies
/// would drift until one of them was passing for a reason the other was not.
pub struct FakeCallTransport {
    joins: bool,
    /// Who is in every call this transport hands out. Shared, because these
    /// tests never have two calls at once and a channel per room would be
    /// machinery for nobody.
    roster: tokio::sync::watch::Sender<Standing>,
}

/// What a fake call currently is: who is in it, and what is wrong with it.
type Standing = (Vec<consort_matrix::Participant>, Option<String>);

impl FakeCallTransport {
    /// A transport whose calls all work.
    pub fn joining() -> Self {
        Self {
            joins: true,
            roster: tokio::sync::watch::channel((Vec::new(), None)).0,
        }
    }

    /// Put `people` in the calls this hands out.
    ///
    /// Before a join to seed the channel, or during one to make somebody
    /// arrive or leave. `send_replace` rather than `send`, because a `watch`
    /// with nothing subscribed refuses an ordinary send and seeding happens
    /// before anything has subscribed.
    pub fn set_roster(&self, people: Vec<consort_matrix::Participant>) {
        self.roster.send_modify(|standing| standing.0 = people);
    }

    /// Say what is wrong with the calls this hands out, or that nothing is.
    pub fn set_trouble(&self, trouble: Option<&str>) {
        self.roster
            .send_modify(|standing| standing.1 = trouble.map(str::to_owned));
    }

    /// A transport that will not join anything, the way a room sync has not
    /// delivered will not.
    pub fn refusing() -> Self {
        Self {
            joins: false,
            ..Self::joining()
        }
    }
}

pub struct FakeCallSession {
    roster: tokio::sync::watch::Sender<Standing>,
}

pub struct FakeCallTrack;

impl consort_call::PublishedAudio for FakeCallTrack {
    async fn send(&self, _samples: Vec<i16>) -> Result<(), consort_call::CallFailure> {
        Ok(())
    }
}

pub struct FakeCallRoster(tokio::sync::watch::Receiver<Standing>);

impl consort_call::Roster for FakeCallRoster {
    async fn now(&self) -> Vec<consort_matrix::Participant> {
        self.0.borrow().0.clone()
    }

    fn trouble(&self) -> Option<String> {
        self.0.borrow().1.clone()
    }

    async fn changed(&mut self) -> bool {
        self.0.changed().await.is_ok()
    }
}

impl consort_call::CallSession for FakeCallSession {
    type Track = FakeCallTrack;
    type Roster = FakeCallRoster;

    async fn publish_microphone(&self) -> Result<Self::Track, consort_call::CallFailure> {
        Ok(FakeCallTrack)
    }

    fn roster(&self) -> Self::Roster {
        FakeCallRoster(self.roster.subscribe())
    }

    async fn leave(self) -> Result<(), consort_call::CallFailure> {
        Ok(())
    }
}

impl consort_call::CallTransport for FakeCallTransport {
    type Session = FakeCallSession;

    async fn join(&self, room_id: &str) -> Result<Self::Session, consort_call::CallFailure> {
        if self.joins {
            Ok(FakeCallSession {
                roster: self.roster.clone(),
            })
        } else {
            Err(consort_call::CallFailure::UnknownRoom {
                room_id: room_id.to_owned(),
            })
        }
    }
}

/// The `Connected` a test expects, with nobody in the channel.
pub fn connected(room_id: &str) -> consort_call::CallEvent {
    consort_call::CallEvent::Connected {
        room_id: room_id.to_owned(),
        participants: Vec::new(),
        trouble: None,
    }
}
