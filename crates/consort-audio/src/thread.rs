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

use crate::capture::{AudioCapture, CaptureStream};
use crate::gate::{FRAME_SAMPLES, GateConfig, VoiceGate};
use crate::meter::{Meter, Reading};

/// Something the audio thread has to say.
#[derive(Clone, Debug, PartialEq)]
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
}

/// What the thread accepts.
enum Message {
    Start {
        device: Option<String>,
        gate: GateConfig,
    },
    Stop,
    /// One captured frame, posted by the backend's realtime callback.
    Frame(Vec<i16>),
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
    pub fn spawn(capture: Box<dyn AudioCapture>, events: Sender<AudioEvent>) -> Self {
        let (commands, inbox) = channel::<Message>();
        let frames = commands.clone();
        let join = std::thread::Builder::new()
            .name("consort-audio".to_owned())
            .spawn(move || run(capture, inbox, frames, events))
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
    inbox: std::sync::mpsc::Receiver<Message>,
    frames: Sender<Message>,
    events: Sender<AudioEvent>,
) {
    let mut running: Option<Running> = None;

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

    // Explicit, so the microphone is closed before the event channel does. A
    // caller watching for the channel to close then knows the device is free.
    drop(running);
}
