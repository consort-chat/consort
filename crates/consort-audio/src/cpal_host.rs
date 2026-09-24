// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The real sound card, behind [`AudioDevices`].
//!
//! Needs hardware, so it is excluded from the coverage numbers alongside
//! `keyring_store.rs`. That exclusion is only honest while the file stays this
//! thin: anything here that starts deciding belongs in `devices.rs` or
//! `frames.rs`, where a fixture can reach it.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, SampleFormat, StreamConfig};

use crate::capture::{AudioCapture, CaptureError, CaptureStream, FrameSink};
use crate::devices::{Answer, AudioDevices, Device, Direction, catalogue};
use crate::frames::Frames;
use crate::gate::SAMPLE_RATE;
use crate::mixing::{Mixing, Voices};
use crate::playback::{AudioPlayback, PlaybackError, PlaybackStream, Playing, ToneEnded};
use crate::tone::Tone;

/// The default host: ALSA on Linux, CoreAudio on macOS, WASAPI on Windows.
pub struct CpalHost;

impl AudioDevices for CpalHost {
    fn enumerate(&self, direction: Direction) -> Vec<Device> {
        let host = cpal::default_host();

        // cpal 0.18 removed `Device::name()`; `Display` is the documented way
        // to get a name now, and it is also all the identity a device has.
        let default = match direction {
            Direction::Input => host.default_input_device(),
            Direction::Output => host.default_output_device(),
        }
        .map(|device| device.to_string());

        let listed = match direction {
            Direction::Input => host
                .input_devices()
                .map(|listed| collect(listed, direction)),
            Direction::Output => host
                .output_devices()
                .map(|listed| collect(listed, direction)),
        };

        let listed = match listed {
            Ok(listed) => listed,
            Err(error) => {
                // An enumeration failure is a machine with a broken audio
                // stack, not a machine with no microphone. Reporting nothing
                // is right, and saying so is what stops it looking like the
                // latter in a bug report.
                tracing::warn!(?direction, %error, "could not enumerate audio devices");
                return Vec::new();
            }
        };

        listed
            .into_iter()
            .map(|name| Device {
                is_default: Some(&name) == default.as_ref(),
                name,
            })
            .collect()
    }
}

/// A cpal stream, kept alive by being held.
struct OpenStream {
    // Dropped last-in-first-out with the struct; the field exists to own it.
    _stream: cpal::Stream,
    device: String,
}

// A cpal `Stream` is `!Send`, which is the entire reason `AudioThread` exists.
// The thread opens it, holds it and drops it without ever handing it anywhere,
// so this promise is kept structurally rather than by convention: nothing else
// in the crate can name this type.
unsafe impl Send for OpenStream {}

impl CaptureStream for OpenStream {
    fn device_name(&self) -> &str {
        &self.device
    }
}

impl AudioCapture for CpalHost {
    fn open(
        &self,
        device: Option<&str>,
        on_frame: FrameSink,
    ) -> Result<Box<dyn CaptureStream>, CaptureError> {
        let host = cpal::default_host();
        let device = match device {
            None => host.default_input_device().ok_or(CaptureError::NoDevice)?,
            Some(wanted) => host
                .input_devices()
                .map_err(|error| CaptureError::Backend(error.to_string()))?
                .find(|candidate| candidate.to_string() == wanted)
                .ok_or_else(|| CaptureError::UnknownDevice {
                    requested: wanted.to_owned(),
                    available: catalogue(self, Direction::Input)
                        .into_iter()
                        .map(|device| device.name)
                        .collect(),
                })?,
        };
        let name = device.to_string();

        // Nothing resamples, so 48 kHz is a requirement rather than a
        // preference. f32 first: it is what ALSA and CoreAudio hand out
        // natively, so asking for it avoids a conversion inside the backend.
        let mut ranges: Vec<_> = device
            .supported_input_configs()
            .map_err(|error| CaptureError::Backend(error.to_string()))?
            .filter(|range| {
                range.min_sample_rate() <= SAMPLE_RATE && range.max_sample_rate() >= SAMPLE_RATE
            })
            .collect();
        if ranges.is_empty() {
            return Err(CaptureError::NoFortyEightKilohertz { device: name });
        }
        ranges.sort_by_key(|range| match range.sample_format() {
            SampleFormat::F32 => 0,
            SampleFormat::I16 => 1,
            _ => 2,
        });
        let chosen = ranges.remove(0);
        let channels = chosen.channels();

        let config = StreamConfig {
            channels,
            sample_rate: SAMPLE_RATE,
            buffer_size: BufferSize::Default,
        };
        let mut frames = Frames::new(channels);
        let mut on_frame = on_frame;
        let on_error = |error| tracing::error!(%error, "audio input stream error");

        let stream = match chosen.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _| frames.push_f32(data, &mut on_frame),
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _| frames.push_i16(data, &mut on_frame),
                on_error,
                None,
            ),
            other => {
                return Err(CaptureError::UnsupportedFormat {
                    device: name,
                    format: format!("{other:?}"),
                });
            }
        }
        .map_err(|error| CaptureError::Backend(error.to_string()))?;

        stream
            .play()
            .map_err(|error| CaptureError::Backend(error.to_string()))?;

        tracing::info!(device = %name, channels, format = ?chosen.sample_format(),
            "capturing audio");
        Ok(Box::new(OpenStream {
            _stream: stream,
            device: name,
        }))
    }
}

/// An open output, kept alive by being held.
struct PlayingStream {
    // Dropped last-in-first-out with the struct; the field exists to own it.
    _stream: cpal::Stream,
    device: String,
}

// `!Send` for the same reason `OpenStream` is, and kept structurally the same
// way: the audio thread opens it, holds it and drops it without handing it
// anywhere, and nothing else in the crate can name this type.
unsafe impl Send for PlayingStream {}

impl PlaybackStream for PlayingStream {
    fn device_name(&self) -> &str {
        &self.device
    }
}

/// An output device that has agreed to a format, ready to be built on.
struct Output {
    device: cpal::Device,
    name: String,
    format: SampleFormat,
    config: StreamConfig,
}

/// Find `device`, or the host's default, and settle on a format.
///
/// Shared by the chime and the call so the two cannot disagree about which
/// device "default" means, or the test button tests the wrong thing.
fn choose_output(wanted: Option<&str>) -> Result<Output, PlaybackError> {
    let host = cpal::default_host();
    let device = match wanted {
        None => host
            .default_output_device()
            .ok_or(PlaybackError::NoDevice)?,
        Some(wanted) => host
            .output_devices()
            .map_err(|error| PlaybackError::Backend(error.to_string()))?
            .find(|candidate| candidate.to_string() == wanted)
            .ok_or_else(|| PlaybackError::UnknownDevice {
                requested: wanted.to_owned(),
                available: catalogue(&CpalHost, Direction::Output)
                    .into_iter()
                    .map(|device| device.name)
                    .collect(),
            })?,
    };
    let name = device.to_string();

    // The same bargain the capture path makes: everything this crate produces
    // is at 48 kHz and nothing resamples, so a device that cannot do 48 kHz is
    // refused rather than played to at the wrong pitch. The picker already
    // filters on this, so reaching it means the device changed underneath
    // somebody between the list being drawn and the button being pressed.
    let mut ranges: Vec<_> = device
        .supported_output_configs()
        .map_err(|error| PlaybackError::Backend(error.to_string()))?
        .filter(|range| {
            range.min_sample_rate() <= SAMPLE_RATE && range.max_sample_rate() >= SAMPLE_RATE
        })
        .collect();
    if ranges.is_empty() {
        return Err(PlaybackError::NoFortyEightKilohertz { device: name });
    }
    ranges.sort_by_key(|range| match range.sample_format() {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        _ => 2,
    });
    let chosen = ranges.remove(0);

    Ok(Output {
        device,
        name,
        format: chosen.sample_format(),
        config: StreamConfig {
            channels: chosen.channels(),
            sample_rate: SAMPLE_RATE,
            buffer_size: BufferSize::Default,
        },
    })
}

impl AudioPlayback for CpalHost {
    fn play(
        &self,
        device: Option<&str>,
        tone: Tone,
        on_end: ToneEnded,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError> {
        let Output {
            device,
            name,
            format,
            config,
        } = choose_output(device)?;
        let channels = config.channels;
        let chosen = format;

        let mut playing = Playing::new(tone, channels, on_end);
        let on_error = |error| tracing::error!(%error, "audio output stream error");

        let stream = match chosen {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| playing.fill_f32(data),
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _| playing.fill_i16(data),
                on_error,
                None,
            ),
            other => {
                return Err(PlaybackError::UnsupportedFormat {
                    device: name,
                    format: format!("{other:?}"),
                });
            }
        }
        .map_err(|error| PlaybackError::Backend(error.to_string()))?;

        stream
            .play()
            .map_err(|error| PlaybackError::Backend(error.to_string()))?;

        tracing::info!(device = %name, channels, format = ?chosen,
            "playing the test tone");
        Ok(Box::new(PlayingStream {
            _stream: stream,
            device: name,
        }))
    }

    fn play_call(
        &self,
        device: Option<&str>,
        voices: Voices,
    ) -> Result<Box<dyn PlaybackStream>, PlaybackError> {
        let Output {
            device,
            name,
            format,
            config,
        } = choose_output(device)?;
        let channels = config.channels;

        let mut mixing = Mixing::new(voices, channels);
        let on_error = |error| tracing::error!(%error, "call audio output stream error");

        let stream = match format {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| mixing.fill_f32(data),
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _| mixing.fill_i16(data),
                on_error,
                None,
            ),
            other => {
                return Err(PlaybackError::UnsupportedFormat {
                    device: name,
                    format: format!("{other:?}"),
                });
            }
        }
        .map_err(|error| PlaybackError::Backend(error.to_string()))?;

        stream
            .play()
            .map_err(|error| PlaybackError::Backend(error.to_string()))?;

        tracing::info!(device = %name, channels, format = ?format,
            "playing the call");
        Ok(Box::new(PlayingStream {
            _stream: stream,
            device: name,
        }))
    }
}

/// The names of the devices in `devices` that can actually be opened the way
/// this crate opens them.
///
/// Not belt and braces: a host's direction flag is a declaration, and on
/// PipeWire it is wrong often enough to offer a webcam as an output. Each
/// candidate is asked instead. See `docs/adr/0004-trust-no-device-list.md`.
fn collect<D: Iterator<Item = cpal::Device>>(devices: D, direction: Direction) -> Vec<String> {
    devices
        .filter(|device| offers_our_rate(device, direction))
        .map(|device| device.to_string())
        .collect()
}

/// Ask `device` whether it supports [`SAMPLE_RATE`] in `direction`.
///
/// A failed query is not automatically a no. `DeviceBusy` in particular is
/// usually Consort's own microphone, and is proof the device works.
/// [`Answer`] is where each reason is sorted into kept or dropped, and tested.
fn offers_our_rate(device: &cpal::Device, direction: Direction) -> bool {
    let covers_our_rate = |range: &cpal::SupportedStreamConfigRange| {
        range.min_sample_rate() <= SAMPLE_RATE && range.max_sample_rate() >= SAMPLE_RATE
    };

    let asked = match direction {
        Direction::Input => device
            .supported_input_configs()
            .map(|mut ranges| ranges.any(|range| covers_our_rate(&range))),
        Direction::Output => device
            .supported_output_configs()
            .map(|mut ranges| ranges.any(|range| covers_our_rate(&range))),
    };

    let answer = match asked {
        Ok(true) => Answer::Yes,
        Ok(false) => Answer::No,
        Err(error) => match error.kind() {
            ErrorKind::DeviceBusy => Answer::Busy,
            ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => Answer::Absent,
            ErrorKind::PermissionDenied => Answer::Forbidden,
            ErrorKind::InvalidInput | ErrorKind::UnsupportedConfig => Answer::No,
            _ => Answer::Unclear,
        },
    };

    answer.worth_listing()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::catalogue;

    /// Open the real default microphone and watch the gate work.
    ///
    /// The tuning loop: talk, type, breathe, scrape the desk, and see where
    /// speech lands against where noise lands.
    ///
    /// ```sh
    /// cargo test -p consort-audio --lib -- --ignored --nocapture watch_the_real_microphone
    /// ```
    ///
    /// `AUDIO_DEVICE` picks a device by exact name; without it the host's
    /// default is used. `list_the_real_devices` prints the names.
    #[test]
    #[ignore = "needs a real microphone and somebody to talk into it"]
    fn watch_the_real_microphone() {
        use crate::AudioEvent;
        use crate::gate::GateConfig;
        use crate::thread::AudioThread;
        use std::time::{Duration, Instant};

        let (sender, events) = std::sync::mpsc::channel();
        let audio = AudioThread::spawn(Box::new(CpalHost), Box::new(CpalHost), sender);
        let device = std::env::var("AUDIO_DEVICE").ok();
        audio.start(device, GateConfig::default());

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut readings = 0;
        println!("say something for ten seconds");
        while Instant::now() < deadline {
            let Ok(event) = events.recv_timeout(Duration::from_millis(500)) else {
                continue;
            };
            match event {
                AudioEvent::Started { device } => println!("capturing from {device}"),
                AudioEvent::Failed { error } => panic!("could not capture: {error}"),
                AudioEvent::Stopped => break,
                // Nothing here plays a tone, joins a call or listens to
                // itself, so none of these can arrive.
                AudioEvent::ToneStarted { .. }
                | AudioEvent::ToneStopped
                | AudioEvent::ToneFailed { .. }
                | AudioEvent::CallAudioStarted { .. }
                | AudioEvent::CallAudioFailed { .. }
                | AudioEvent::CallAudioStopped
                | AudioEvent::MonitorStarted { .. }
                | AudioEvent::MonitorFailed { .. }
                | AudioEvent::MonitorStopped => {}
                AudioEvent::Level(reading) => {
                    readings += 1;
                    let bar = "#".repeat((reading.level * 40.0) as usize);
                    let gate = if reading.open { "OPEN" } else { "shut" };
                    println!(
                        "[{bar:<40}] p={:.2} level={:.3} {gate}",
                        reading.probability, reading.level
                    );
                }
            }
        }

        assert!(
            readings > 0,
            "ten seconds of a real microphone produced no readings at all"
        );
    }

    /// Play the test chime out of a real output and listen to it.
    ///
    /// Nothing proves an output except hearing it.
    ///
    /// ```sh
    /// cargo test -p consort-audio --lib -- --ignored --nocapture play_the_real_tone
    /// ```
    ///
    /// `AUDIO_DEVICE` picks an output by exact name; without it the host's
    /// default is used.
    #[test]
    #[ignore = "needs real speakers and somebody to listen to them"]
    fn play_the_real_tone() {
        use crate::AudioEvent;
        use crate::thread::AudioThread;
        use std::time::Duration;

        let (sender, events) = std::sync::mpsc::channel();
        let audio = AudioThread::spawn(Box::new(CpalHost), Box::new(CpalHost), sender);
        audio.play_tone(std::env::var("AUDIO_DEVICE").ok());

        loop {
            match events.recv_timeout(Duration::from_secs(5)) {
                Ok(AudioEvent::ToneStarted { device }) => println!("playing through {device}"),
                Ok(AudioEvent::ToneStopped) => break,
                Ok(AudioEvent::ToneFailed { error }) => panic!("could not play: {error}"),
                Ok(other) => println!("· {other:?}"),
                Err(error) => panic!("the chime never ended: {error}"),
            }
        }
    }

    /// Ask this machine what it actually has.
    ///
    /// Ignored because a CI runner has no sound card and would report an empty
    /// list, which is a correct answer and a useless test. Run it by hand:
    ///
    /// ```sh
    /// cargo test -p consort-audio --  --ignored --nocapture list_the_real_devices
    /// ```
    #[test]
    #[ignore = "needs a real sound card"]
    fn list_the_real_devices() {
        for direction in [Direction::Input, Direction::Output] {
            println!("\n{direction:?}");
            let listed = catalogue(&CpalHost, direction);
            assert!(
                !listed.is_empty(),
                "this machine reported no {direction:?} devices at all"
            );
            for device in listed {
                let marker = if device.is_default { " (default)" } else { "" };
                println!("  {}{marker}", device.name);
            }
        }
    }
}
