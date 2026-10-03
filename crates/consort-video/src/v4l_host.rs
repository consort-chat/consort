// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The camera backend that talks to a real device.
//!
//! Excluded from coverage, for the same reason `consort_audio`'s `cpal_host`
//! is: CI has no camera, so every line in here is a line no test can reach.
//! It is kept correspondingly thin. Everything that decides anything, which is
//! which format to ask for and how to read the bytes back, sits in
//! [`crate::capture`] and [`crate::pixels`] and is tested without a device.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use moq_v4l::buffer::Type;
use moq_v4l::io::traits::CaptureStream as _;
use moq_v4l::prelude::*;
use moq_v4l::video::Capture;
use moq_v4l::{Format, FourCC, context, format::description::Flags};

use crate::capture::{CameraStream, CaptureError, FrameSink, Offer, Resolution, VideoCapture};
use crate::devices::{Camera, CameraDevices};
use crate::pixels::{Picture, PixelFormat, decode};

/// How many mmap buffers to ask the driver for.
///
/// Four is the usual choice: enough that the driver always has somewhere to
/// put the next frame while this thread is converting the last one, few enough
/// that a stall shows up as a dropped frame rather than as half a second of
/// stale video arriving at once.
const BUFFERS: u32 = 4;

/// How long to wait for a frame before looking at the stop flag again.
const POLL_MS: i32 = 200;

/// `POLLIN`, which `moq-v4l`'s handle takes as a bare `i16`.
const POLLIN: i16 = 0x001;

/// Cameras and frames, through Video4Linux 2.
#[derive(Clone, Copy, Debug, Default)]
pub struct V4lHost;

impl CameraDevices for V4lHost {
    fn enumerate(&self) -> Vec<Camera> {
        let mut found = Vec::new();

        for node in context::enum_devices() {
            let path = node.path().to_string_lossy().into_owned();
            let Ok(device) = Device::with_path(&path) else {
                continue;
            };
            let Ok(caps) = device.query_caps() else {
                continue;
            };
            // A laptop's /dev/video1 is routinely a metadata node, and an
            // enumeration that lists it offers a camera that hands over no
            // pictures.
            if !caps
                .capabilities
                .contains(moq_v4l::capability::Flags::VIDEO_CAPTURE)
            {
                continue;
            }
            if readable_offers(&device).is_empty() {
                continue;
            }

            let name = node.name().unwrap_or_else(|| caps.card.clone());
            found.push(Camera { id: path, name });
        }

        found
    }
}

impl VideoCapture for V4lHost {
    fn open(
        &self,
        camera: Option<&str>,
        want: Resolution,
        on_frame: FrameSink,
    ) -> Result<Box<dyn CameraStream>, CaptureError> {
        let available = crate::devices::catalogue(self);
        let chosen = match camera {
            Some(id) => available
                .iter()
                .find(|camera| camera.id == id)
                .ok_or_else(|| CaptureError::UnknownCamera {
                    requested: id.to_owned(),
                    available: available.iter().map(|one| one.id.clone()).collect(),
                })?,
            None => available.first().ok_or(CaptureError::NoCamera)?,
        };

        let device =
            Device::with_path(&chosen.id).map_err(|error| open_failed(&chosen.id, error))?;
        let offers = readable_offers(&device);
        let offer = crate::capture::choose_offer(&offers, want).ok_or_else(|| {
            CaptureError::NoUsableFormat {
                camera: chosen.name.clone(),
            }
        })?;

        let agreed = device
            .set_format(&Format::new(
                offer.width,
                offer.height,
                FourCC::new(&offer.format.fourcc()),
            ))
            .map_err(|error| CaptureError::Backend(error.to_string()))?;
        // Asked for, not insisted on. A driver that will not run at this rate
        // still produces frames, and a slower camera beats no camera.
        let _ = device.set_params(&moq_v4l::video::capture::Parameters::with_fps(offer.fps));

        let format = PixelFormat::from_fourcc(&agreed.fourcc.repr).ok_or_else(|| {
            CaptureError::NoUsableFormat {
                camera: chosen.name.clone(),
            }
        })?;
        let negotiated = Resolution {
            width: agreed.width,
            height: agreed.height,
            fps: offer.fps,
        };

        let running = Arc::new(AtomicBool::new(true));
        let pump = spawn_pump(device, format, negotiated, Arc::clone(&running), on_frame)?;

        Ok(Box::new(V4lStream {
            camera_id: chosen.id.clone(),
            resolution: negotiated,
            running,
            pump: Some(pump),
        }))
    }
}

/// Read frames until the flag clears.
///
/// Its own thread because a V4L2 capture is a blocking dequeue, so there is no
/// callback to be handed the way cpal hands one.
fn spawn_pump(
    device: Device,
    format: PixelFormat,
    negotiated: Resolution,
    running: Arc<AtomicBool>,
    mut on_frame: FrameSink,
) -> Result<JoinHandle<()>, CaptureError> {
    std::thread::Builder::new()
        .name("consort-camera".to_owned())
        .spawn(move || {
            let handle = device.handle();
            let mut stream = match MmapStream::with_buffers(&device, Type::VideoCapture, BUFFERS) {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(%error, "the camera would not start streaming");
                    return;
                }
            };

            while running.load(Ordering::Relaxed) {
                // Polled rather than blocked on, so switching the camera off
                // is not waiting for one more frame from a device that has
                // stopped producing them.
                match handle.poll(POLLIN, POLL_MS) {
                    Ok(0) => continue,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "the camera stopped answering");
                        return;
                    }
                }

                let (buffer, meta) = match stream.next() {
                    Ok(frame) => frame,
                    Err(error) => {
                        tracing::warn!(%error, "the camera stopped handing over frames");
                        return;
                    }
                };
                if meta.flags.contains(moq_v4l::buffer::Flags::ERROR) {
                    continue;
                }

                let bytes = &buffer[..(meta.bytesused as usize).min(buffer.len())];
                // Through `Duration` rather than by hand, so this is right on a
                // target whose `time_t` is not 64 bits wide.
                let captured_us = Duration::from(meta.timestamp).as_micros() as i64;
                match decode(format, negotiated.width, negotiated.height, bytes) {
                    Ok(picture) => on_frame(Picture {
                        timestamp_us: captured_us,
                        ..picture
                    }),
                    // Per frame rather than fatal: a USB camera drops and
                    // corrupts the occasional frame, and the next one is
                    // usually fine.
                    Err(error) => tracing::debug!(%error, "dropped an unreadable camera frame"),
                }
            }
        })
        .map_err(|error| CaptureError::Backend(error.to_string()))
}

/// Every format and size this device offers that [`crate::pixels`] can read.
fn readable_offers(device: &Device) -> Vec<Offer> {
    let Ok(descriptions) = device.enum_formats() else {
        return Vec::new();
    };

    let mut offers = Vec::new();
    for description in descriptions {
        // An emulated format is libv4l's software conversion, which this build
        // does not link, so the kernel will not actually produce one.
        if description.flags.contains(Flags::EMULATED) {
            continue;
        }
        let Some(format) = PixelFormat::from_fourcc(&description.fourcc.repr) else {
            continue;
        };
        let Ok(sizes) = device.enum_framesizes(description.fourcc) else {
            continue;
        };

        for size in sizes {
            for discrete in size.size.to_discrete() {
                let fps = best_fps(device, description.fourcc, discrete.width, discrete.height);
                offers.push(Offer {
                    format,
                    width: discrete.width,
                    height: discrete.height,
                    fps,
                });
            }
        }
    }

    offers
}

/// The highest frame rate the device offers at this format and size.
///
/// One, not the list, because the only thing anything upstream does with it is
/// compare it against the target. A device that reports nothing is taken at 30
/// rather than 0, so a driver with no interval enumeration is not ranked below
/// every device that has one.
fn best_fps(device: &Device, fourcc: FourCC, width: u32, height: u32) -> u32 {
    use moq_v4l::frameinterval::FrameIntervalEnum;

    let Ok(intervals) = device.enum_frameintervals(fourcc, width, height) else {
        return 30;
    };

    intervals
        .into_iter()
        .filter_map(|interval| match interval.interval {
            FrameIntervalEnum::Discrete(fraction) if fraction.numerator > 0 => {
                Some(fraction.denominator / fraction.numerator)
            }
            // A stepwise range's fastest frame is its shortest interval.
            FrameIntervalEnum::Stepwise(stepwise) if stepwise.min.numerator > 0 => {
                Some(stepwise.min.denominator / stepwise.min.numerator)
            }
            FrameIntervalEnum::Discrete(_) | FrameIntervalEnum::Stepwise(_) => None,
        })
        .max()
        .unwrap_or(30)
}

/// Name an open failure the way somebody can act on.
fn open_failed(camera: &str, error: std::io::Error) -> CaptureError {
    match error.kind() {
        std::io::ErrorKind::ResourceBusy => CaptureError::Busy {
            camera: camera.to_owned(),
        },
        _ => CaptureError::Backend(error.to_string()),
    }
}

/// A running camera. Dropping it stops the thread and releases the device.
struct V4lStream {
    camera_id: String,
    resolution: Resolution,
    running: Arc<AtomicBool>,
    /// `Option` only so [`Drop`] can take the handle to join it.
    pump: Option<JoinHandle<()>>,
}

impl CameraStream for V4lStream {
    fn camera_id(&self) -> &str {
        &self.camera_id
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

impl Drop for V4lStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(pump) = self.pump.take() {
            // Joined rather than detached, because the thread holds the device
            // and the next thing somebody does after switching cameras is open
            // the other one.
            let _ = pump.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Print what this machine actually has, and what each camera offers.
    ///
    /// The one thing no fixture can check: that the enumeration drops the
    /// metadata nodes and keeps the webcams.
    ///
    /// ```sh
    /// cargo test -p consort-video --lib -- --ignored --nocapture list_the_real_cameras
    /// ```
    #[test]
    #[ignore = "needs a real camera"]
    fn list_the_real_cameras() {
        for camera in crate::devices::catalogue(&V4lHost) {
            println!("{} -> {}", camera.id, camera.name);
            let Ok(device) = Device::with_path(&camera.id) else {
                continue;
            };
            let offers = readable_offers(&device);
            println!(
                "  chosen: {:?}",
                crate::capture::choose_offer(&offers, crate::capture::WANTED)
            );
        }
    }

    /// Open the real camera and report the frames it produces.
    ///
    /// ```sh
    /// cargo test -p consort-video --lib -- --ignored --nocapture read_the_real_camera
    /// ```
    ///
    /// `CAMERA` picks one by device node; without it the first is used.
    #[test]
    #[ignore = "needs a real camera"]
    fn read_the_real_camera() {
        use std::sync::mpsc::channel;
        use std::time::Duration;

        let (frames, inbox) = channel();
        let wanted = std::env::var("CAMERA").ok();
        let stream = V4lHost
            .open(
                wanted.as_deref(),
                crate::capture::WANTED,
                Box::new(move |picture| {
                    let _ = frames.send((picture.width, picture.height, picture.y.len()));
                }),
            )
            .expect("could not open a camera");

        println!("opened {} at {:?}", stream.camera_id(), stream.resolution());
        // The first frame carries the device's start-up, so the rate is
        // measured from the second one. Whether the conversion keeps up with
        // the camera is the whole question this test answers.
        let first = inbox
            .recv_timeout(Duration::from_secs(5))
            .expect("no frame arrived at all");
        println!(
            "first frame {}x{}, {} luma bytes",
            first.0, first.1, first.2
        );

        let started = std::time::Instant::now();
        let count = 60;
        for _ in 0..count {
            inbox
                .recv_timeout(Duration::from_secs(2))
                .expect("the camera stopped mid-run");
        }
        let elapsed = started.elapsed();
        println!(
            "{count} frames in {elapsed:?}, {:.1} per second",
            f64::from(count) / elapsed.as_secs_f64()
        );
    }
}
