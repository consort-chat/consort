// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The thread that owns the microphone.
//!
//! A cpal `Stream` is `!Send` and has to be dropped on the thread that built
//! it, so it gets one. One channel, not two: frames are posted onto the queue
//! the commands arrive on, so there is a single `recv` and no select. A
//! separate frame channel would deadlock on shutdown, because the stream holds
//! a sender and the thread owns the stream.

use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::capture::{AudioCapture, CaptureStream};
use crate::gate::{FRAME_SAMPLES, GateConfig, PreRoll, VoiceGate};
use crate::meter::{Meter, Reading};
use crate::mixing::Voices;
use crate::playback::{AudioPlayback, PlaybackStream};
use crate::tone::Tone;

/// Something the audio thread has to say.
///
/// Serialised internally tagged, matching every other union that crosses the
/// IPC boundary. The wire shape is asserted in `tests/wire.rs`, because
/// nothing in TypeScript would fail to build if it drifted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AudioEvent {
    /// Capture began on this device, which is not always the one asked for.
    Started { device: String },
    /// Nothing is being captured.
    Stopped,
    /// Capture could not begin. The thread is still alive and can be asked
    /// again.
    Failed { error: String },
    /// One update for the level bar.
    Level(Reading),
    /// The test chime began on this output, which is not always the one asked
    /// for.
    ToneStarted { device: String },
    /// The chime is over, whether it finished or was cut short. One event for
    /// both: a button going back to pressable is all that waits on it.
    ToneStopped,
    /// The chime could not begin.
    ToneFailed { error: String },
    /// The call is being played out of this output, which is not always the
    /// one asked for.
    CallAudioStarted { device: String },
    /// The call cannot be played. Reported rather than swallowed: a call that
    /// cannot be heard looks exactly like a call nobody is speaking in, and
    /// somebody would spend an evening blaming the microphone.
    CallAudioFailed { error: String },
    /// The call is no longer being played.
    CallAudioStopped,
    /// The microphone is being played back on this output.
    MonitorStarted { device: String },
    /// The microphone could not be played back. Its own event rather than
    /// [`Self::CallAudioFailed`], which the shell raises a banner for.
    MonitorFailed { error: String },
    /// The microphone is no longer being played back.
    MonitorStopped,
}

/// Who the monitor's own voice is, in the mixer. Never a Matrix ID, and the
/// monitor has a mixer to itself, so nothing can collide with it.
const MONITOR: &str = "you";

/// Where gated audio goes while a call is running.
///
/// `open` is the gate's verdict, so `false` means substituted silence rather
/// than anything said, and the reader can mute a publication instead of
/// guessing from the amplitude. Called on the audio thread once per 10 ms, so
/// it may allocate but must not block: the meter is behind it.
pub type GatedSink = Box<dyn FnMut(&[i16], bool) + Send>;

/// What the thread accepts.
enum Message {
    Start {
        device: Option<String>,
        gate: GateConfig,
    },
    Stop,
    /// Retune the running gate without reopening the device.
    ///
    /// Its own message rather than another `Start`, which would drop the meter
    /// to zero under somebody who is watching it while they tune.
    Retune {
        gate: GateConfig,
    },
    /// One captured frame, posted by the backend's realtime callback.
    Frame(Vec<i16>),
    /// Install a reader for the gate's output, or `None` to remove it.
    ///
    /// On the same channel as everything else, so a sink installed before
    /// `Start` sees the first frame and the ordering holds at both ends.
    Publish(Option<GatedSink>),
    PlayTone {
        device: Option<String>,
    },
    StopTone,
    /// Open an output and play everybody else in the call through it.
    ///
    /// Its own message, not a flag on `Start`: a device change on one end must
    /// not disturb the other.
    PlayCall {
        device: Option<String>,
        voices: Voices,
    },
    StopCall,
    /// Open an output and play this microphone back through it.
    ///
    /// Not a `PlayCall` with a caller-fed mixer, because what goes into it is
    /// the gate's output, which only this thread has.
    StartMonitor {
        device: Option<String>,
    },
    StopMonitor,
    /// The chime handed its last sample to the device, posted by the backend's
    /// realtime callback. Carries which chime, because a second press can be
    /// underway by the time it arrives.
    ToneEnded(u64),
    Shutdown,
}

/// A handle on the audio thread.
///
/// Dropping it stops the thread and closes the microphone.
pub struct AudioThread {
    commands: Sender<Message>,
    join: Option<JoinHandle<()>>,
}

impl AudioThread {
    /// Start the thread. It idles until told to [`start`](Self::start).
    pub fn spawn(
        capture: Box<dyn AudioCapture>,
        playback: Box<dyn AudioPlayback>,
        events: Sender<AudioEvent>,
    ) -> Self {
        let (commands, inbox) = channel::<Message>();
        let frames = commands.clone();
        let join = std::thread::Builder::new()
            .name("consort-audio".to_owned())
            .spawn(move || run(capture, playback, inbox, frames, events))
            .expect("the operating system refused a thread");

        Self {
            commands,
            join: Some(join),
        }
    }

    /// Begin capturing from `device`, or from the host's default.
    ///
    /// Replaces whatever was running. Answered with `Started` or `Failed`.
    pub fn start(&self, device: Option<String>, gate: GateConfig) {
        self.send(Message::Start { device, gate });
    }

    /// Stop capturing. Answered with `Stopped` whether or not anything was
    /// running.
    pub fn stop(&self) {
        self.send(Message::Stop);
    }

    /// Change the running gate's tuning, leaving the device open.
    ///
    /// Ignored when nothing is capturing, since [`start`](Self::start) takes
    /// the tuning anyway. Answered with nothing: the meter is the answer.
    pub fn retune(&self, gate: GateConfig) {
        self.send(Message::Retune { gate });
    }

    /// Send every gated frame to `sink` from now on, replacing any current
    /// one. Answered with nothing: whether audio reaches anybody is a question
    /// about the call.
    pub fn publish_to(&self, sink: GatedSink) {
        self.send(Message::Publish(Some(sink)));
    }

    /// Stop sending gated frames anywhere.
    ///
    /// Called when the call ends. A sink left installed is work done for
    /// nobody, onto a queue nothing drains, logged as dropped frames.
    pub fn stop_publishing(&self) {
        self.send(Message::Publish(None));
    }

    /// Play the test chime through `device`, or through the host's default.
    ///
    /// Replaces whatever was playing. Answered with `ToneStarted` or
    /// `ToneFailed`, and later with `ToneStopped`.
    pub fn play_tone(&self, device: Option<String>) {
        self.send(Message::PlayTone { device });
    }

    /// Cut the chime short. Answered with `ToneStopped` either way.
    pub fn stop_tone(&self) {
        self.send(Message::StopTone);
    }

    /// Start playing `voices` out of `device`, or out of the host's default.
    ///
    /// Replaces whatever was playing. Answered with `CallAudioStarted` or
    /// `CallAudioFailed`.
    pub fn play_call(&self, device: Option<String>, voices: Voices) {
        self.send(Message::PlayCall { device, voices });
    }

    /// Stop playing the call and give the output back.
    ///
    /// Answered with `CallAudioStopped` whether or not anything was playing.
    pub fn stop_call(&self) {
        self.send(Message::StopCall);
    }

    /// Play this microphone back, so somebody can hear what they are sending.
    ///
    /// The gate's own output: denoised, gated, pre-roll and all. That is why
    /// it is not simply a second capture stream.
    pub fn start_monitor(&self, device: Option<String>) {
        self.send(Message::StartMonitor { device });
    }

    /// Stop playing this microphone back.
    pub fn stop_monitor(&self) {
        self.send(Message::StopMonitor);
    }

    fn send(&self, message: Message) {
        // A closed channel means the thread panicked. The caller finds out
        // when the event channel closes.
        let _ = self.commands.send(message);
    }
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        // Explicit rather than relying on the channel closing: the running
        // stream holds a sender, and the thread is what drops the stream.
        self.send(Message::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// What is running right now, if anything.
struct Running {
    /// Held only to keep the microphone open; dropping it closes it.
    _stream: Box<dyn CaptureStream>,
    gate: VoiceGate,
    meter: Meter,
    /// The gate's output buffer, reused rather than allocated per frame.
    gated: Vec<i16>,
    /// Holds the gate's output back far enough for an opening edge to reach
    /// the frames that caused it. Inside `Running` so a device change starts
    /// a new line, rather than publishing 30 ms from the old microphone.
    pre_roll: PreRoll,
}

fn run(
    capture: Box<dyn AudioCapture>,
    playback: Box<dyn AudioPlayback>,
    inbox: std::sync::mpsc::Receiver<Message>,
    frames: Sender<Message>,
    events: Sender<AudioEvent>,
) {
    let mut running: Option<Running> = None;
    // Outside `Running`: a call outlives a device change, and reopening the
    // microphone must not silently stop feeding it.
    let mut publishing: Option<GatedSink> = None;
    // Held only to keep the output open; dropping it silences the chime.
    let mut tone: Option<Box<dyn PlaybackStream>> = None;
    // The same, for the call. A second stream on (usually) the same device, so
    // the test button neither interrupts a call nor is interrupted by one.
    let mut call: Option<Box<dyn PlaybackStream>> = None;
    // A third, for playing the microphone back. Not the call's, which would
    // put one voice into everybody else's mix.
    let mut monitor: Option<(Box<dyn PlaybackStream>, Voices)> = None;
    // Which chime is playing. Bumped on every start and stop, so an ending
    // reported by a replaced chime carries a number nothing matches.
    let mut chime: u64 = 0;

    while let Ok(message) = inbox.recv() {
        match message {
            Message::Start { device, gate } => {
                // Always torn down before the new one is opened: two claims on
                // a device that allows one fails for a reason nothing
                // explains. Runs even when the new stream then fails, so the
                // old microphone cannot stay live behind a screen saying it is
                // not.
                running = None;

                let post = frames.clone();
                let opened = capture.open(
                    device.as_deref(),
                    Box::new(move |frame| {
                        let _ = post.send(Message::Frame(frame.to_vec()));
                    }),
                );

                match opened {
                    Ok(stream) => {
                        let event = AudioEvent::Started {
                            device: stream.device_name().to_owned(),
                        };
                        running = Some(Running {
                            _stream: stream,
                            gate: VoiceGate::new(gate),
                            meter: Meter::new(),
                            gated: vec![0; FRAME_SAMPLES],
                            pre_roll: PreRoll::default(),
                        });
                        if events.send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if events
                            .send(AudioEvent::Failed {
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }

            Message::Stop => {
                running = None;
                if events.send(AudioEvent::Stopped).is_err() {
                    break;
                }
            }

            Message::Publish(sink) => {
                publishing = sink;
            }

            Message::PlayCall { device, voices } => {
                // Dropped first, for the reason `Start` gives.
                call = None;

                match playback.play_call(device.as_deref(), voices) {
                    Ok(stream) => {
                        let event = AudioEvent::CallAudioStarted {
                            device: stream.device_name().to_owned(),
                        };
                        call = Some(stream);
                        if events.send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if events
                            .send(AudioEvent::CallAudioFailed {
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }

            Message::StopCall => {
                if let Some(stream) = call.take() {
                    tracing::info!(device = %stream.device_name(), "gave the call output back");
                }
                if events.send(AudioEvent::CallAudioStopped).is_err() {
                    break;
                }
            }

            Message::StartMonitor { device } => {
                // Dropped first, for the reason `Start` gives.
                monitor = None;

                let voices = Voices::new();
                match playback.play_call(device.as_deref(), voices.clone()) {
                    Ok(stream) => {
                        let event = AudioEvent::MonitorStarted {
                            device: stream.device_name().to_owned(),
                        };
                        monitor = Some((stream, voices));
                        if events.send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if events
                            .send(AudioEvent::MonitorFailed {
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }

            Message::StopMonitor => {
                if let Some((stream, _)) = monitor.take() {
                    tracing::info!(device = %stream.device_name(), "stopped playing the microphone back");
                }
                if events.send(AudioEvent::MonitorStopped).is_err() {
                    break;
                }
            }

            Message::Retune { gate } => {
                if let Some(state) = running.as_mut() {
                    state.gate.retune(gate);
                }
            }

            Message::PlayTone { device } => {
                // Torn down first, like the microphone: two chimes at once is
                // twice the volume and half the information.
                tone = None;
                chime += 1;
                let mine = chime;
                let post = frames.clone();
                let played = playback.play(
                    device.as_deref(),
                    Tone::check(),
                    Box::new(move || {
                        let _ = post.send(Message::ToneEnded(mine));
                    }),
                );

                match played {
                    Ok(stream) => {
                        let event = AudioEvent::ToneStarted {
                            device: stream.device_name().to_owned(),
                        };
                        tone = Some(stream);
                        if events.send(event).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if events
                            .send(AudioEvent::ToneFailed {
                                error: error.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }

            Message::StopTone => {
                tone = None;
                chime += 1;
                if events.send(AudioEvent::ToneStopped).is_err() {
                    break;
                }
            }

            Message::ToneEnded(which) => {
                if which != chime {
                    // A replaced chime reporting its end. Believing it would
                    // silence whatever is playing now.
                    continue;
                }
                tone = None;
                chime += 1;
                if events.send(AudioEvent::ToneStopped).is_err() {
                    break;
                }
            }

            Message::Frame(frame) => {
                let Some(state) = running.as_mut() else {
                    // A frame from a stream that has just been torn down. There
                    // is nothing to measure it against any more.
                    continue;
                };
                if frame.len() != FRAME_SAMPLES {
                    // `Frames` only emits whole frames, so this is a backend
                    // misbehaving. `VoiceGate::process` would panic, and a
                    // panicked audio thread takes the microphone with it.
                    tracing::warn!(
                        samples = frame.len(),
                        expected = FRAME_SAMPLES,
                        "discarding a capture frame of the wrong length"
                    );
                    continue;
                }

                // Ungated, because the pre-roll decides what to silence and
                // cannot reopen what it was handed as zeroes.
                let decision = state.gate.process_ungated(&frame, &mut state.gated);

                // Before the meter, because this is the frame's reason for
                // existing. What goes out is 30 ms behind the capture and the
                // meter below is not, deliberately: a bar lagging a person's
                // own voice is what they would notice.
                if let Some((published, open)) = state.pre_roll.step(&state.gated, decision) {
                    if let Some(sink) = publishing.as_mut() {
                        sink(published, open);
                    }
                    // Only while the gate is open, so the monitor answers what
                    // leaves this machine rather than what the microphone
                    // hears.
                    if open && let Some((_, voices)) = monitor.as_ref() {
                        voices.hear(MONITOR, published);
                    }
                }

                // Metered on the captured frame, not the gate's output: a bar
                // that reads zero while the gate is shut cannot tell a dead
                // microphone from one the model is not scoring as speech.
                if let Some(reading) = state.meter.fold(decision, &frame)
                    && events.send(AudioEvent::Level(reading)).is_err()
                {
                    break;
                }
            }

            Message::Shutdown => break,
        }
    }

    // Explicit, so both devices are closed before the event channel is. A
    // caller watching for the channel to close then knows they are free.
    drop(running);
    drop(tone);
    drop(publishing);
}
