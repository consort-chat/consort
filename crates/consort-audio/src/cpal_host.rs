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

use cpal::traits::HostTrait;

use crate::devices::{AudioDevices, Device, Direction};

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

fn collect<D: Iterator<Item = cpal::Device>>(devices: D) -> Vec<String> {
    devices.map(|device| device.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::catalogue;

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
