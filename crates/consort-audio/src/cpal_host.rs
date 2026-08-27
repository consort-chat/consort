// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The real sound card, behind [`AudioDevices`].
//!
//! Everything in this file needs hardware, so none of it is covered by tests
//! and it is excluded from the coverage numbers alongside `keyring_store.rs`.
//! That exclusion is only honest while the file stays this thin: anything here
//! that starts making decisions belongs in `devices.rs` or `frames.rs`, where
//! it can be given a fixture and checked.
//!
//! The one deliberate exception is `list_the_real_devices` at the bottom, an
//! ignored test for asking this machine what it has.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig};

use crate::capture::{AudioCapture, CaptureError, CaptureStream, FrameSink};
use crate::devices::{AudioDevices, Device, Direction, catalogue};
use crate::frames::Frames;
use crate::gate::SAMPLE_RATE;

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
            Direction::Input => host.input_devices().map(collect),
            Direction::Output => host.output_devices().map(collect),
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

fn collect<D: Iterator<Item = cpal::Device>>(devices: D) -> Vec<String> {
    devices.map(|device| device.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::catalogue;

    /// Open the real default microphone and watch the gate work.
    ///
    /// The prototype's `meter` binary, as a test. This is the tuning loop:
    /// talk, type, breathe, scrape the desk, and see where speech lands against
    /// where noise lands. The level column separates two failures that look
    /// identical from the gate's side, a microphone delivering nothing against
    /// one that is fine but whose speech the model scores low.
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
        let audio = AudioThread::spawn(Box::new(CpalHost), sender);
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
