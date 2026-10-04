// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The camera backend that talks to a real device on Windows, through Media
//! Foundation.
//!
//! Excluded from coverage for the reason `v4l_host` is: CI has no camera.
//! Everything that interprets what Media Foundation says is in
//! [`crate::win32`], and what to ask a camera for is [`crate::capture`]'s,
//! both tested without a device. Why Media Foundation and not a crate over it:
//! `docs/adr/0011-capture-on-windows.md`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSourceReader,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MF_VERSION, MFCreateAttributes, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFSTARTUP_LITE, MFShutdown, MFStartup,
};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::core::{GUID, PWSTR};

use crate::capture::{CameraStream, CaptureError, FrameSink, Offer, Resolution, VideoCapture};
use crate::devices::{Camera, CameraDevices};
use crate::pixels::{Picture, PixelFormat, decode};
use crate::win32::{camera_failure, frame_rate, frame_size, media_subtype};

/// The stream index every call here addresses: the first video stream, which
/// on a webcam is the only one.
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Cameras and frames, through Media Foundation.
#[derive(Clone, Copy, Debug, Default)]
pub struct MfHost;

impl CameraDevices for MfHost {
    /// Every video capture device Windows lists, without opening any of them.
    ///
    /// Unlike V4L2 there are no metadata nodes to weed out, and an IR sensor
    /// is in a category of its own that this does not ask for. Opening each
    /// one to read its formats would cost a device activation per camera and
    /// drop a camera that is merely busy from the list, where a refusal that
    /// names it is the better answer.
    fn enumerate(&self) -> Vec<Camera> {
        let Ok(_session) = Session::start() else {
            return Vec::new();
        };
        let Ok(devices) = devices() else {
            return Vec::new();
        };

        devices
            .iter()
            .filter_map(|device| {
                Some(Camera {
                    id: text(
                        device,
                        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    )?,
                    name: text(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)?,
                })
            })
            .collect()
    }
}

impl VideoCapture for MfHost {
    /// Open the camera on a thread of its own and wait for its first frame.
    ///
    /// The thread owns every Media Foundation object, so nothing crosses a
    /// COM apartment. The first frame is waited for because on Windows a busy
    /// camera and the privacy switch both fail when streaming starts rather
    /// than when the device opens, and a refusal is only worth anything if it
    /// arrives here rather than as a camera that silently stays dark.
    fn open(
        &self,
        camera: Option<&str>,
        want: Resolution,
        mut on_frame: FrameSink,
    ) -> Result<Box<dyn CameraStream>, CaptureError> {
        let requested = camera.map(str::to_owned);
        let running = Arc::new(AtomicBool::new(true));
        let still_running = Arc::clone(&running);
        let (told, answer) = sync_channel(1);

        let pump = std::thread::Builder::new()
            .name("consort-camera".to_owned())
            .spawn(move || {
                let _session = match Session::start() {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = told.send(Err(error));
                        return;
                    }
                };
                let opened = match start(requested.as_deref(), want) {
                    Ok(opened) => opened,
                    Err(error) => {
                        let _ = told.send(Err(error));
                        return;
                    }
                };
                let first = match read(&opened) {
                    Ok(first) => first,
                    Err(error) => {
                        let _ = unsafe { opened.source.Shutdown() };
                        let _ = told.send(Err(error));
                        return;
                    }
                };
                let _ = told.send(Ok((opened.camera.id.clone(), opened.resolution)));
                if let Some(picture) = first {
                    on_frame(picture);
                }

                while still_running.load(Ordering::Relaxed) {
                    match read(&opened) {
                        Ok(Some(picture)) => on_frame(picture),
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error, "the camera stopped handing over frames");
                            break;
                        }
                    }
                }
                // Released here rather than left to the drop, so the device is
                // free by the time the join in `MfStream::drop` returns and the
                // next thing somebody does, opening another camera, works.
                let _ = unsafe { opened.source.Shutdown() };
            })
            .map_err(|error| CaptureError::Backend(error.to_string()))?;

        let (camera_id, resolution) = match answer.recv() {
            Ok(Ok(opened)) => opened,
            Ok(Err(error)) => {
                let _ = pump.join();
                return Err(error);
            }
            Err(_) => {
                let _ = pump.join();
                return Err(CaptureError::Backend(
                    "the camera thread ended before it said anything".to_owned(),
                ));
            }
        };

        Ok(Box::new(MfStream {
            camera_id,
            resolution,
            running,
            pump: Some(pump),
        }))
    }
}

/// COM and Media Foundation, started for as long as this is held.
///
/// Fields drop in order, so Media Foundation shuts down before COM does.
struct Session {
    _media: Media,
    _com: Com,
}

impl Session {
    fn start() -> Result<Self, CaptureError> {
        let com = Com::start()?;
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.map_err(failed("Media Foundation"))?;
        Ok(Self {
            _media: Media,
            _com: com,
        })
    }
}

/// One `MFStartup`, balanced on drop.
struct Media;

impl Drop for Media {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}

/// COM on this thread, uninitialised on drop only if this is what started it.
struct Com {
    owned: bool,
}

impl Com {
    /// Join the multithreaded apartment.
    ///
    /// A thread that is already in the single-threaded one, as a Tauri
    /// command on the main thread is, refuses with `RPC_E_CHANGED_MODE`. COM
    /// is usable there all the same, and the call that failed must not be
    /// balanced, so that is not an error.
    fn start() -> Result<Self, CaptureError> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        result.ok().map_err(failed("COM"))?;
        Ok(Self { owned: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.owned {
            unsafe { CoUninitialize() };
        }
    }
}

/// Every video capture device, as activation objects nothing has opened yet.
fn devices() -> windows::core::Result<Vec<IMFActivate>> {
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;

        let mut listed: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attributes, &mut listed, &mut count)?;
        if listed.is_null() {
            return Ok(Vec::new());
        }

        // SAFETY: Media Foundation allocated `count` initialised entries at
        // `listed`. Each is taken out, so the array freed below owns nothing.
        let found = std::slice::from_raw_parts_mut(listed, count as usize)
            .iter_mut()
            .filter_map(Option::take)
            .collect();
        CoTaskMemFree(Some(listed.cast_const().cast()));
        Ok(found)
    }
}

/// A string attribute of a device, which Media Foundation allocates.
fn text(device: &IMFActivate, key: &GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0u32;
    unsafe {
        device
            .GetAllocatedString(key, &mut value, &mut length)
            .ok()?;
        let read = value.to_string().ok();
        CoTaskMemFree(Some(value.0.cast_const().cast()));
        read
    }
}

/// A camera that has been opened and set to a format.
struct Opened {
    camera: Camera,
    source: IMFMediaSource,
    reader: IMFSourceReader,
    format: PixelFormat,
    resolution: Resolution,
}

/// Find the camera `requested` names, or the first, and set it to the best
/// format it offers for `want`.
fn start(requested: Option<&str>, want: Resolution) -> Result<Opened, CaptureError> {
    let available: Vec<(Camera, IMFActivate)> = devices()
        .map_err(failed("listing cameras"))?
        .into_iter()
        .filter_map(|device| {
            let camera = Camera {
                id: text(
                    &device,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                )?,
                name: text(&device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)?,
            };
            Some((camera, device))
        })
        .collect();

    let (camera, device) = match requested {
        Some(id) => available
            .iter()
            .find(|(camera, _)| camera.id == id)
            .ok_or_else(|| CaptureError::UnknownCamera {
                requested: id.to_owned(),
                available: available.iter().map(|(one, _)| one.id.clone()).collect(),
            })?,
        None => available.first().ok_or(CaptureError::NoCamera)?,
    };
    let refused = |error: windows::core::Error| camera_failure(error.code().0, &camera.name);

    let source: IMFMediaSource = unsafe { device.ActivateObject() }.map_err(refused)?;
    let reader = unsafe { MFCreateSourceReaderFromMediaSource(&source, None) }.map_err(refused)?;

    let offered = offers(&reader);
    let wanted = crate::capture::choose_offer(
        &offered.iter().map(|(offer, _)| *offer).collect::<Vec<_>>(),
        want,
    );
    let Some((offer, kind)) =
        wanted.and_then(|wanted| offered.into_iter().find(|(offer, _)| *offer == wanted))
    else {
        let _ = unsafe { source.Shutdown() };
        return Err(CaptureError::NoUsableFormat {
            camera: camera.name.clone(),
        });
    };

    if let Err(error) = unsafe { reader.SetCurrentMediaType(VIDEO, None, &kind) } {
        let _ = unsafe { source.Shutdown() };
        return Err(refused(error));
    }

    Ok(Opened {
        camera: camera.clone(),
        source,
        reader,
        format: offer.format,
        resolution: Resolution {
            width: offer.width,
            height: offer.height,
            fps: offer.fps,
        },
    })
}

/// Every native format and size this camera offers that [`crate::pixels`]
/// can read, with the media type that asks for it.
fn offers(reader: &IMFSourceReader) -> Vec<(Offer, IMFMediaType)> {
    (0..)
        .map_while(|index| unsafe { reader.GetNativeMediaType(VIDEO, index) }.ok())
        .filter_map(|kind| {
            let subtype = unsafe { kind.GetGUID(&MF_MT_SUBTYPE) }.ok()?;
            let format = media_subtype(subtype.data1.to_le_bytes())?;
            let (width, height) = frame_size(unsafe { kind.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?);
            let fps = unsafe { kind.GetUINT64(&MF_MT_FRAME_RATE) }.map_or(30, frame_rate);
            Some((
                Offer {
                    format,
                    width,
                    height,
                    fps,
                },
                kind,
            ))
        })
        .collect()
}

/// Wait for the next frame and convert it.
///
/// `None` is a gap in the stream rather than a frame, or a frame too damaged
/// to read: a USB camera corrupts the occasional one and the next is usually
/// fine, so neither stops the capture.
fn read(opened: &Opened) -> Result<Option<Picture>, CaptureError> {
    let mut flags = 0u32;
    let mut timestamp = 0i64;
    let mut sample = None;
    unsafe {
        opened.reader.ReadSample(
            VIDEO,
            0,
            None,
            Some(&mut flags),
            Some(&mut timestamp),
            Some(&mut sample),
        )
    }
    .map_err(|error| camera_failure(error.code().0, &opened.camera.name))?;

    let ended = (MF_SOURCE_READERF_ENDOFSTREAM.0 | MF_SOURCE_READERF_ERROR.0) as u32;
    if flags & ended != 0 {
        return Err(CaptureError::Backend(format!(
            "{} ended its stream (flags {flags:#x})",
            opened.camera.name
        )));
    }
    let Some(sample) = sample else {
        return Ok(None);
    };

    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|error| camera_failure(error.code().0, &opened.camera.name))?;
    let mut start = std::ptr::null_mut();
    let mut length = 0u32;
    unsafe { buffer.Lock(&mut start, None, Some(&mut length)) }
        .map_err(|error| camera_failure(error.code().0, &opened.camera.name))?;
    // SAFETY: a locked buffer is `length` readable bytes at `start` until
    // `Unlock`, and nothing below holds on to the slice past that.
    let bytes = unsafe { std::slice::from_raw_parts(start, length as usize) };
    let decoded = decode(
        opened.format,
        opened.resolution.width,
        opened.resolution.height,
        bytes,
    );
    let _ = unsafe { buffer.Unlock() };

    match decoded {
        Ok(picture) => Ok(Some(Picture {
            // Media Foundation counts in hundreds of nanoseconds.
            timestamp_us: timestamp / 10,
            ..picture
        })),
        Err(error) => {
            tracing::debug!(%error, "dropped an unreadable camera frame");
            Ok(None)
        }
    }
}

/// Wrap a Windows failure that has no camera to name yet.
fn failed(what: &'static str) -> impl Fn(windows::core::Error) -> CaptureError {
    move |error| CaptureError::Backend(format!("{what}: {error}"))
}

/// A running camera. Dropping it stops the thread and releases the device.
struct MfStream {
    camera_id: String,
    resolution: Resolution,
    running: Arc<AtomicBool>,
    /// `Option` only so [`Drop`] can take the handle to join it.
    pump: Option<JoinHandle<()>>,
}

impl CameraStream for MfStream {
    fn camera_id(&self) -> &str {
        &self.camera_id
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

impl Drop for MfStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(pump) = self.pump.take() {
            // Joined for the reason `V4lStream` joins: the thread holds the
            // device until it ends. Bounded by one frame interval, which is as
            // long as a `ReadSample` waits.
            let _ = pump.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_failures_win32_names_are_the_ones_windows_defines() {
        // `win32` writes these out so it compiles on Linux. This is what keeps
        // a typo in one from turning the privacy switch into "the camera could
        // not be started".
        use windows::Win32::Foundation::E_ACCESSDENIED;
        use windows::Win32::Media::MediaFoundation::{
            MF_E_HW_MFT_FAILED_START_STREAMING, MF_E_VIDEO_DEVICE_LOCKED,
            MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
        };

        assert_eq!(
            camera_failure(E_ACCESSDENIED.0, "C920"),
            CaptureError::Blocked
        );
        for busy in [
            MF_E_HW_MFT_FAILED_START_STREAMING,
            MF_E_VIDEO_DEVICE_LOCKED,
            MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
        ] {
            assert_eq!(
                camera_failure(busy.0, "C920"),
                CaptureError::Busy {
                    camera: "C920".to_owned()
                }
            );
        }
    }

    #[test]
    fn the_subtypes_win32_reads_are_the_ones_windows_defines() {
        use windows::Win32::Media::MediaFoundation::{
            MFVideoFormat_MJPG, MFVideoFormat_NV12, MFVideoFormat_YUY2,
        };

        for (subtype, format) in [
            (MFVideoFormat_YUY2, PixelFormat::Yuyv),
            (MFVideoFormat_MJPG, PixelFormat::Mjpeg),
            (MFVideoFormat_NV12, PixelFormat::Nv12),
        ] {
            assert_eq!(media_subtype(subtype.data1.to_le_bytes()), Some(format));
        }
    }

    /// Print what this machine actually has.
    ///
    /// ```sh
    /// cargo test -p consort-video --lib -- --ignored --nocapture list_the_real_cameras
    /// ```
    #[test]
    #[ignore = "needs a real camera"]
    fn list_the_real_cameras() {
        let _session = Session::start().expect("Media Foundation would not start");
        for camera in crate::devices::catalogue(&MfHost) {
            println!("{} -> {}", camera.name, camera.id);
            let opened = start(Some(&camera.id), crate::capture::WANTED);
            match opened {
                Ok(opened) => {
                    println!("  chosen: {} {:?}", opened.format, opened.resolution);
                    let _ = unsafe { opened.source.Shutdown() };
                }
                Err(error) => println!("  refused: {error}"),
            }
        }
    }

    /// Open the real camera and report the frames it produces.
    ///
    /// ```sh
    /// cargo test -p consort-video --lib -- --ignored --nocapture read_the_real_camera
    /// ```
    ///
    /// `CAMERA` picks one by symbolic link; without it the first is used.
    #[test]
    #[ignore = "needs a real camera"]
    fn read_the_real_camera() {
        use std::sync::mpsc::channel;
        use std::time::{Duration, Instant};

        let (frames, inbox) = channel();
        let wanted = std::env::var("CAMERA").ok();
        let stream = MfHost
            .open(
                wanted.as_deref(),
                crate::capture::WANTED,
                Box::new(move |picture| {
                    let _ = frames.send((picture.width, picture.height, picture.timestamp_us));
                }),
            )
            .expect("could not open a camera");

        println!("opened {} at {:?}", stream.camera_id(), stream.resolution());
        let first = inbox
            .recv_timeout(Duration::from_secs(5))
            .expect("no frame arrived at all");
        println!("first frame {}x{}", first.0, first.1);
        assert_eq!(
            (first.0, first.1),
            (stream.resolution().width, stream.resolution().height)
        );

        let started = Instant::now();
        let count = 60;
        let mut last = first.2;
        for _ in 0..count {
            let (_, _, at) = inbox
                .recv_timeout(Duration::from_secs(2))
                .expect("the camera stopped mid-run");
            assert!(at > last, "timestamps went backwards: {last} then {at}");
            last = at;
        }
        let elapsed = started.elapsed();
        println!(
            "{count} frames in {elapsed:?}, {:.1} per second",
            f64::from(count) / elapsed.as_secs_f64()
        );

        let stopping = Instant::now();
        drop(stream);
        println!("stopped in {:?}", stopping.elapsed());
    }
}
