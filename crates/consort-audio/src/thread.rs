// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The thread that owns the microphone.
//!
//! A cpal `Stream` is `!Send`. It cannot live in shared application state, it
//! cannot be held across an await inside a Tauri command, and it has to be
//! dropped on the thread that built it. So it gets a thread of its own, and
//! everything else reaches it through a channel.
//!
//! That is the same shape `Call::join` will need when the MatrixRTC layer
//! arrives, for the same reason and with the same constraint written down in
//! `app/src-tauri/src/state.rs`. Building it here, around a meter that is easy
//! to reason about, is much cheaper than discovering its shape halfway through
//! wiring up a call.
//!
//! One channel, not two. The capture callback posts frames onto the same queue
//! the commands arrive on, so the thread has a single `recv` and no select. A
//! separate frame channel would need one, and would also deadlock on shutdown:
//! the stream holds a sender, the thread owns the stream, and neither would let
//! go first.

use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::capture::{AudioCapture, CaptureStream};
use crate::gate::{FRAME_SAMPLES, GateConfig, VoiceGate};
use crate::meter::{Meter, Reading};
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
    /// both, because the only thing waiting on it is a button that needs to go
    /// back to being pressable.
    ToneStopped,
    /// The chime could not begin.
    ToneFailed { error: String },
}

/// Where gated audio goes while a call is running.
///
/// A closure rather than a trait, matching [`crate::FrameSink`] on the way in,
/// and for the same reason: the one thing on the other side of it is a queue,
/// and a trait would put a name for that queue in this crate. Nothing here
/// should know that calls exist.
///
/// `open` is the gate's verdict for this frame. `false` means the samples are
/// the silence the gate substituted rather than anything anybody said, which
/// is what lets the reader mute a publication instead of guessing from the
/// amplitude.
///
/// Called on the audio thread, once per frame, so once per 10 ms. Not on the
/// realtime callback, so it may allocate, but it must not block: everything
/// else this thread does is behind it, including the meter.
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
    /// Its own message rather than another `Start`. Somebody moving a
    /// threshold or turning voice activity off is watching the meter while
    /// they do it, and reopening the sound card under them drops the bar to
    /// zero for a moment and re-announces the device.
    Retune {
        gate: GateConfig,
    },
    /// One captured frame, posted by the backend's realtime callback.
    Frame(Vec<i16>),
    /// Install a reader for the gate's output, or `None` to remove the one
    /// installed.
    ///
    /// Carried on the same channel as everything else, so a sink installed
    /// before `Start` sees the first frame and one removed after `Stop` cannot
    /// see a frame that arrived in between.
    Publish(Option<GatedSink>),
    PlayTone {
        device: Option<String>,
    },
    StopTone,
    /// The chime handed its last sample to the device, posted by the backend's
    /// realtime callback.
    ///
    /// Carries which chime, because a second press can be underway by the time
    /// this arrives and acting on a stale one would cut the new chime off.
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
    /// Silently ignored when nothing is capturing: the tuning is passed to
    /// [`start`](Self::start) anyway, so there is nothing to remember here.
    /// Answered with nothing, because the meter is already the answer.
    pub fn retune(&self, gate: GateConfig) {
        self.send(Message::Retune { gate });
    }

    /// Send every gated frame to `sink` from now on, replacing any current
    /// one.
    ///
    /// Answered with nothing. Whether audio is reaching anybody is a question
    /// about the call, not about this thread, and the call is what can answer
    /// it.
    pub fn publish_to(&self, sink: GatedSink) {
        self.send(Message::Publish(Some(sink)));
    }

    /// Stop sending gated frames anywhere.
    ///
    /// Called when the call ends. Leaving a sink installed past the end of a
    /// call is not harmful, but it is work done for nobody, and on a queue
    /// nothing is draining it is work that shows up in the log as dropped
    /// frames.
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

    fn send(&self, message: Message) {
        // A closed channel means the thread is already gone, which is only
        // reachable if it panicked. Nothing useful can be done about it from
        // here, and the caller finds out when the event channel closes.
        let _ = self.commands.send(message);
    }
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        // An explicit shutdown rather than relying on the channel closing: the
        // running stream holds a sender too, and the thread is what drops the
        // stream, so waiting for the last sender would wait forever.
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
}

fn run(
    capture: Box<dyn AudioCapture>,
    playback: Box<dyn AudioPlayback>,
    inbox: std::sync::mpsc::Receiver<Message>,
    frames: Sender<Message>,
    events: Sender<AudioEvent>,
) {
    let mut running: Option<Running> = None;
    // Where the gate's output goes, when anybody wants it. Outside `Running`
    // on purpose: a call outlives a device change, and reopening the
    // microphone must not silently stop feeding it.
    let mut publishing: Option<GatedSink> = None;
    // Held only to keep the output open; dropping it silences the chime.
    let mut tone: Option<Box<dyn PlaybackStream>> = None;
    // Which chime is playing. Bumped on every start and every stop, so an
    // ending reported by a chime that has already been replaced or cancelled
    // arrives carrying a number nothing matches any more and is dropped.
    let mut chime: u64 = 0;

    while let Ok(message) = inbox.recv() {
        match message {
            Message::Start { device, gate } => {
                // Torn down before the new one is opened, always. Two open
                // streams means two claims on the sound card, and on a device
                // that allows only one the second fails for a reason nothing
                // explains. This also runs when the new stream then fails,
                // which is deliberate: leaving the previous microphone live
                // while the screen says otherwise is worse than silence.
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
                    // A chime that was replaced or cancelled, reporting its
                    // end afterwards. Believing it would silence whatever is
                    // playing now.
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
                    // `Frames` only ever emits whole frames, so this is a
                    // backend doing something unexpected. Skipping it keeps the
                    // thread alive; `VoiceGate::process` would panic, and a
                    // panicked audio thread takes the microphone with it.
                    tracing::warn!(
                        samples = frame.len(),
                        expected = FRAME_SAMPLES,
                        "discarding a capture frame of the wrong length"
                    );
                    continue;
                }

                let decision = state.gate.process(&frame, &mut state.gated);

                // Before the meter, because this is the frame's reason for
                // existing and the meter is a picture of it. A send failure on
                // the event channel below ends the loop, and ending it after
                // the audio has gone out costs a caller nothing.
                if let Some(sink) = publishing.as_mut() {
                    sink(&state.gated, decision.open);
                }

                // Metered on the captured frame, not on the gate's output. A
                // bar that reads zero whenever the gate is shut cannot tell
                // "the microphone is dead" from "the microphone is fine and the
                // model is not scoring this as speech", which is the single
                // most useful thing this screen can show.
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
