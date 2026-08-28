// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! One microphone, two things that want it.
//!
//! Until calls existed the settings screen was the only thing that ever opened
//! the sound card, so opening and closing it could be the same pair of
//! commands the screen already had. It is not any more. A call wants the same
//! device, tuned the same way, and the two overlap constantly: adjusting your
//! microphone while you are in a voice channel is the ordinary reason to open
//! that screen at all.
//!
//! What that costs, without something in the middle, is one specific bug:
//! closing the settings screen during a call stops the capture, the call keeps
//! publishing an empty stream, and the first anyone knows is a person nobody
//! can hear. So the device is opened for a named reason and closed only when
//! no reason is left.
//!
//! [`Sound`] is also where the audio thread is built, lazily. Most sessions
//! never touch either half of this, and a thread plus a connection to a sound
//! server is not worth spending on a session that opens neither the settings
//! screen nor a call.

use std::sync::{Arc, Mutex};

use consort_audio::{GateConfig, GatedSink, Voices};

use crate::audio::{AudioBridge, Backends};
use crate::events::EventSink;

/// What the microphone was last asked to open, and who is still asking.
///
/// The request is held so that a second reason to open a device that is
/// already open is not a reason to close and reopen it. That reopening is
/// audible: it is a gap in what a peer hears, in answer to somebody opening a
/// settings screen.
#[derive(Default)]
struct Demand {
    /// The settings screen has it open.
    test: bool,
    /// A call has it open.
    call: bool,
    /// The `(device, gate)` the capture was last started with, or `None` when
    /// nothing is asking for one.
    open_with: Option<(Option<String>, GateConfig)>,
}

impl Demand {
    fn wanted(&self) -> bool {
        self.test || self.call
    }
}

/// The machine's audio, opened on demand and shared.
pub struct Sound {
    /// `None` until the first thing wants it. See the module documentation.
    ///
    /// A `std::sync::Mutex` rather than tokio's: nothing here awaits, and every
    /// command that reaches it is synchronous.
    bridge: Mutex<Option<AudioBridge>>,
    demand: Mutex<Demand>,
    events: Arc<dyn EventSink>,
}

impl Sound {
    pub fn new(events: Arc<dyn EventSink>) -> Self {
        Self {
            bridge: Mutex::new(None),
            demand: Mutex::new(Demand::default()),
            events,
        }
    }

    /// Open the microphone for the settings screen's level meter.
    pub fn start_test(
        &self,
        backends: impl FnOnce() -> Backends,
        device: Option<String>,
        gate: GateConfig,
    ) {
        self.open(backends, device, gate, |demand| demand.test = true);
    }

    /// The settings screen is done with the microphone.
    ///
    /// Leaves it open if a call still wants it, which is the whole reason this
    /// file exists.
    pub fn stop_test(&self) {
        self.close(|demand| demand.test = false);
    }

    /// Open the microphone for a call, sending what the gate produces to
    /// `sink`.
    ///
    /// The sink is installed whether or not the device had to be opened. A
    /// call starting while the settings screen already has the microphone is
    /// the case that would otherwise publish nothing at all.
    pub fn start_call(
        &self,
        backends: impl FnOnce() -> Backends,
        device: Option<String>,
        gate: GateConfig,
        sink: GatedSink,
        output: Option<String>,
        voices: Voices,
    ) {
        self.open(backends, device, gate, |demand| demand.call = true);
        self.with_running(|bridge| {
            bridge.publish_to(sink);
            // No demand tracking on this one, unlike the microphone. Two
            // things fight over an input; an output is opened a second time
            // without complaint, which is what lets somebody test their
            // speakers during a call.
            //
            // Started even when the capture above failed. A broken microphone
            // is a reason to be unable to speak, not a reason to be unable to
            // hear, and the two devices are not usually even the same one.
            bridge.play_call(output, voices);
        });
    }

    /// The call is over.
    ///
    /// Stops the frames before releasing the device, so nothing is still being
    /// pushed into a call that has gone.
    pub fn stop_call(&self) {
        self.with_running(|bridge| {
            bridge.stop_publishing();
            bridge.stop_call();
        });
        self.close(|demand| demand.call = false);
    }

    /// Retune the running gate, leaving the device open.
    ///
    /// A no-op when nothing is running, because the tuning is handed to
    /// whichever `start_` opened the device anyway and there is nothing here to
    /// remember it with.
    pub fn retune(&self, gate: GateConfig) {
        self.with_running(|bridge| bridge.retune(gate));
    }

    /// Play the test chime, opening the audio thread on first use.
    ///
    /// No demand tracking. A chime is a third of a second and releases its own
    /// output; it is the input that two things fight over.
    pub fn play_tone(&self, backends: impl FnOnce() -> Backends, device: Option<String>) {
        self.with_bridge(backends, |bridge| bridge.play_tone(device));
    }

    /// Cut the chime short, releasing the output.
    pub fn stop_tone(&self) {
        self.with_running(AudioBridge::stop_tone);
    }

    /// Record a reason to have the microphone open, and open it if what is
    /// being asked for is not already running.
    ///
    /// The skip is the point. `start_test` and `start_call` resolve the same
    /// saved settings, so the second of them to arrive is almost always asking
    /// for exactly what is already open, and restarting the capture for it
    /// would be a hole in what a peer hears.
    ///
    /// Both locks are taken here rather than at the call site so that two
    /// things asking at once cannot interleave into a device that is running
    /// with one request and recorded as running with the other.
    fn open(
        &self,
        backends: impl FnOnce() -> Backends,
        device: Option<String>,
        gate: GateConfig,
        want: impl FnOnce(&mut Demand),
    ) {
        let mut demand = self.demand();
        want(&mut demand);

        let request = (device, gate);
        if demand.open_with.as_ref() == Some(&request) {
            return;
        }
        demand.open_with = Some(request.clone());

        let (device, gate) = request;
        self.with_bridge(backends, |bridge| bridge.start(device, gate));
    }

    /// Withdraw a reason, closing the device once none is left.
    fn close(&self, give_up: impl FnOnce(&mut Demand)) {
        let mut demand = self.demand();
        give_up(&mut demand);

        if demand.wanted() {
            return;
        }
        demand.open_with = None;

        self.with_running(AudioBridge::stop);
    }

    /// Do something with the audio thread, building it if this is the first
    /// thing to want it.
    ///
    /// `backends` is a closure rather than a value so that nothing constructs a
    /// sound backend on the calls that do not need one, which is every call
    /// after the first.
    fn with_bridge(&self, backends: impl FnOnce() -> Backends, act: impl FnOnce(&AudioBridge)) {
        let mut slot = self.bridge();
        let bridge =
            slot.get_or_insert_with(|| AudioBridge::spawn(backends(), self.events.clone()));
        act(bridge);
    }

    /// Do something with the audio thread only if it already exists.
    ///
    /// Every "stop" is this. Building a thread and a connection to a sound
    /// server in order to tell it to stop doing something it was never doing is
    /// the wrong shape, and closing the settings screen does exactly that
    /// whether or not anybody touched anything.
    fn with_running(&self, act: impl FnOnce(&AudioBridge)) {
        if let Some(bridge) = self.bridge().as_ref() {
            act(bridge);
        }
    }

    fn bridge(&self) -> std::sync::MutexGuard<'_, Option<AudioBridge>> {
        self.bridge
            .lock()
            .expect("the audio mutex is never poisoned")
    }

    fn demand(&self) -> std::sync::MutexGuard<'_, Demand> {
        self.demand
            .lock()
            .expect("the audio demand mutex is never poisoned")
    }

    /// Whether the microphone is currently open for anything.
    ///
    /// Test-only. The demand is the thing this module exists to get right, and
    /// it is otherwise only observable by listening to a sound card.
    #[cfg(test)]
    pub fn capturing(&self) -> bool {
        self.demand().open_with.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::testing::{HeardAudio, counting_sink, fake_backends, frames};

    fn sound() -> (Sound, Arc<HeardAudio>) {
        let heard = Arc::new(HeardAudio::default());
        (Sound::new(heard.clone()), heard)
    }

    fn yeti() -> Option<String> {
        Some("Yeti".to_owned())
    }

    /// Long enough for a wrong implementation to be wrong.
    ///
    /// Used only where the assertion is that nothing happened, which has
    /// nothing to wait for.
    const ENOUGH_TO_GO_WRONG: Duration = Duration::from_millis(50);

    #[test]
    fn a_call_opens_the_speakers_as_well_as_the_microphone() {
        // The bug the whole incoming-audio path exists to fix. Every layer
        // under this reported success while nothing was ever opened to play
        // the call out of, so the call was silent and nothing said why.
        let (sound, heard) = sound();
        let (_count, sink) = counting_sink();

        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            Some("Headphones".to_owned()),
            Voices::new(),
        );

        heard.wait_for("the speakers opening", |heard| heard.call_outputs() == 1);
    }

    #[test]
    fn ending_a_call_gives_the_speakers_back() {
        let (sound, heard) = sound();
        let (_count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            Some("Headphones".to_owned()),
            Voices::new(),
        );
        heard.wait_for("the speakers opening", |heard| heard.call_outputs() == 1);

        sound.stop_call();

        heard.wait_for("the speakers closing", |heard| {
            heard.call_outputs_stopped() == 1
        });
    }

    #[test]
    fn testing_the_speakers_during_a_call_does_not_take_them_from_it() {
        // Unlike the microphone, which two things fight over, an output can be
        // opened twice. Refusing the chime here would refuse it at the one
        // moment it is most useful: when somebody cannot hear the call.
        let (sound, heard) = sound();
        let (_count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            Some("Headphones".to_owned()),
            Voices::new(),
        );
        heard.wait_for("the speakers opening", |heard| heard.call_outputs() == 1);

        sound.play_tone(fake_backends, Some("Headphones".to_owned()));

        std::thread::sleep(ENOUGH_TO_GO_WRONG);
        assert_eq!(
            heard.call_outputs_stopped(),
            0,
            "the chime took the output away from the call"
        );
    }

    #[test]
    fn a_microphone_nobody_has_asked_for_is_not_open() {
        let (sound, heard) = sound();

        assert!(!sound.capturing());
        assert_eq!(heard.opens(), 0);
    }

    #[test]
    fn the_settings_screen_opens_and_closes_the_microphone_on_its_own() {
        let (sound, heard) = sound();

        sound.start_test(fake_backends, yeti(), GateConfig::default());
        heard.wait_for("the device opening", |heard| heard.opens() == 1);
        assert!(sound.capturing());

        sound.stop_test();

        heard.wait_for("the device closing", |heard| heard.stops() == 1);
        assert!(!sound.capturing());
    }

    #[test]
    fn closing_the_settings_screen_during_a_call_leaves_the_microphone_open() {
        // The bug this module exists to prevent, and it is silent: the call
        // keeps publishing, the frames stop arriving, and the first anybody
        // knows is a person nobody can hear.
        let (sound, heard) = sound();
        let (count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        sound.start_test(fake_backends, yeti(), GateConfig::default());
        sound.stop_test();

        assert!(sound.capturing(), "the call lost its microphone");
        assert_eq!(heard.stops(), 0);
        // Waited for rather than asserted outright. Frames reach a sink from
        // the audio thread, several of them behind the gate's pre-roll, so
        // reading the count the instant the settings screen closes is reading
        // it before the answer exists.
        crate::testing::wait_for(
            "a frame after the settings screen closed",
            || frames(&count) > 0,
            || "none".to_owned(),
        );
    }

    #[test]
    fn a_call_ending_while_the_settings_screen_is_open_leaves_the_microphone_open() {
        // The mirror image, and the one somebody actually sees: the level
        // meter is in front of them, and hanging up should not stop it.
        let (sound, heard) = sound();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        let (_count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        sound.stop_call();

        assert!(sound.capturing(), "the meter lost its microphone");
        assert_eq!(heard.stops(), 0);
    }

    #[test]
    fn the_device_closes_once_the_last_reason_for_it_goes() {
        let (sound, heard) = sound();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        let (_count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        sound.stop_call();
        sound.stop_test();

        heard.wait_for("the device closing", |heard| heard.stops() == 1);
        assert!(!sound.capturing());
    }

    #[test]
    fn a_second_reason_to_open_a_device_already_open_does_not_reopen_it() {
        // Audible if it goes wrong. Opening the settings screen mid-call would
        // close and reopen the sound card, which is a hole in what everybody
        // else in the channel hears, in answer to somebody looking at a
        // picker.
        let (sound, heard) = sound();
        let (_count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        sound.start_test(fake_backends, yeti(), GateConfig::default());

        std::thread::sleep(ENOUGH_TO_GO_WRONG);
        assert_eq!(heard.opens(), 1);
        assert_eq!(heard.stops(), 0);
    }

    #[test]
    fn asking_for_a_different_device_does_reopen_it() {
        // The other half of the skip. A choice that changed has to reach the
        // sound card, or the picker is a control that does nothing.
        let (sound, heard) = sound();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        sound.start_test(
            fake_backends,
            Some("Webcam".to_owned()),
            GateConfig::default(),
        );

        heard.wait_for("the second device opening", |heard| heard.opens() == 2);
    }

    #[test]
    fn a_retuned_gate_reopens_nothing_but_is_not_mistaken_for_the_same_request() {
        // Two claims in one. `retune` leaves the device alone, which is what
        // lets somebody drag a threshold while watching the meter. A later
        // `start` carrying that tuning is a different request from the one
        // before it, and must not be skipped.
        let (sound, heard) = sound();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        let louder = GateConfig {
            open_at: 0.9,
            ..GateConfig::default()
        };
        sound.retune(louder);
        std::thread::sleep(ENOUGH_TO_GO_WRONG);
        assert_eq!(heard.opens(), 1);

        sound.start_test(fake_backends, yeti(), louder);

        heard.wait_for("the retuned device opening", |heard| heard.opens() == 2);
    }

    #[test]
    fn a_call_starting_while_the_meter_is_running_still_gets_the_frames() {
        // The case the skip could break. Nothing needs opening, so the sink
        // has to be installed on the device that is already running rather
        // than as part of opening one.
        let (sound, heard) = sound();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        heard.wait_for("the device opening", |heard| heard.opens() == 1);

        let (count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );

        // The fake hands over its frame from inside `open`, so the running
        // device has already delivered everything it is going to. Changing
        // device is how this test gets one more frame to observe, and the
        // frame it observes is one the call was never opened for.
        sound.start_test(
            fake_backends,
            Some("Webcam".to_owned()),
            GateConfig::default(),
        );

        crate::testing::wait_for(
            "a frame after the sink was installed",
            || frames(&count) > 0,
            || "none".to_owned(),
        );
    }

    #[test]
    fn the_call_stops_getting_frames_when_it_ends() {
        let (sound, heard) = sound();
        let (count, sink) = counting_sink();
        sound.start_call(
            fake_backends,
            yeti(),
            GateConfig::default(),
            sink,
            None,
            Voices::new(),
        );
        heard.wait_for("the device opening", |heard| heard.opens() == 1);
        crate::testing::wait_for("a frame", || frames(&count) > 0, || "none".to_owned());

        sound.stop_call();
        sound.start_test(fake_backends, yeti(), GateConfig::default());
        heard.wait_for("the device opening again", |heard| heard.opens() == 2);
        let after = frames(&count);
        std::thread::sleep(ENOUGH_TO_GO_WRONG);

        assert_eq!(
            frames(&count),
            after,
            "audio was still going somewhere after the call ended"
        );
    }

    #[test]
    fn a_chime_needs_no_microphone_and_leaves_none_open() {
        // The output half has no demand tracking, on purpose: a chime is a
        // third of a second and gives its own output back. What it must not do
        // is make the input look wanted.
        let (sound, _heard) = sound();

        sound.play_tone(fake_backends, None);
        sound.stop_tone();

        assert!(!sound.capturing());
    }

    #[test]
    fn stopping_something_that_was_never_started_is_not_an_error() {
        // Every one of these is a real path. Closing the settings screen does
        // the first whether or not anybody pressed anything, and a sign-out
        // with no call does the second.
        let (sound, heard) = sound();

        sound.stop_test();
        sound.stop_call();
        sound.stop_tone();

        assert!(!sound.capturing());
        assert_eq!(heard.opens(), 0);
    }
}
