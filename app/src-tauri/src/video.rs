// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The camera, joined to the call.
//!
//! One device, open or closed, with none of the gate and mixer machinery
//! [`crate::audio::AudioBridge`] carries. Not part of the signed-in session,
//! like the audio thread: a camera has nothing to do with a Matrix account.

use std::sync::Mutex;

use consort_call::{Camera, OutgoingPicture, PictureSize};
use consort_video::{CameraStream, CaptureError, Picture, Resolution, VideoCapture, capture};

use crate::selfview::SelfView;

/// Why a camera is not in the call.
///
/// Two cases rather than one, because only one of them is about the camera. A
/// device that will not open is worth naming; being outside a call is worth
/// saying differently, and is the one somebody can fix by clicking a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraTrouble {
    /// There is no call to publish into.
    NoCall,
    /// The device would not open.
    Device(CaptureError),
}

impl CameraTrouble {
    /// One sentence for the interface to draw.
    pub fn user_message(&self) -> String {
        match self {
            Self::NoCall => "join a voice channel before switching your camera on".to_owned(),
            Self::Device(error) => error.user_message(),
        }
    }
}

/// The camera, and whatever it is currently feeding.
pub struct VideoBridge {
    backend: Box<dyn VideoCapture>,
    /// The open device, while there is one. Dropping it releases the camera.
    open: Mutex<Option<Box<dyn CameraStream>>>,
    /// The newest frame, for the call card to draw.
    mirror: SelfView,
}

impl VideoBridge {
    /// Hold `backend` and `mirror` without opening anything.
    pub fn new(backend: Box<dyn VideoCapture>, mirror: SelfView) -> Self {
        Self {
            backend,
            open: Mutex::new(None),
            mirror,
        }
    }

    /// Open `camera` and point its frames at `queue`.
    ///
    /// Reports the size that was negotiated, which is what a publication has to
    /// be set up for. Replaces whatever was open: switching camera is one act
    /// here rather than a stop and a start that could leave both devices held.
    pub fn start(&self, camera: Option<&str>, queue: Camera) -> Result<PictureSize, CaptureError> {
        let mirror = self.mirror.clone();
        let stream = self.backend.open(
            camera,
            capture::WANTED,
            Box::new(move |picture| {
                // Sampled down before the frame is moved on, which is the only
                // moment both readers can be served from one capture.
                mirror.offer(&picture);
                queue.offer(outgoing(picture));
            }),
        )?;

        let Resolution { width, height, .. } = stream.resolution();
        // Replaced rather than dropped first. The old device is released when
        // the previous value falls out of scope at the end of this statement,
        // which is after the new one is open: a camera that will not open
        // leaves the one that is working alone.
        *self.open() = Some(stream);

        Ok(PictureSize { width, height })
    }

    /// Release the camera, if one is open, and drop the card's picture.
    ///
    /// Both here, because every way out of filming comes through this: the
    /// button, a call ending, and the channel changing under it.
    pub fn stop(&self) {
        self.open().take();
        self.mirror.clear();
    }

    /// The newest frame, for the card to draw. Test-only.
    ///
    /// The application reaches the same slot through `AppState`, which is the
    /// one the command answers from.
    #[cfg(test)]
    pub fn mirror(&self) -> &SelfView {
        &self.mirror
    }

    /// Whether a camera is open right now.
    ///
    /// Test-only. The interface learns this from the camera channel instead,
    /// which is the only account of it that includes the publication.
    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.open().is_some()
    }

    fn open(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn CameraStream>>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Repack a captured frame as the one the call crate carries.
///
/// A move of three vectors rather than a copy of the pixels. The two types are
/// separate so that carrying a call does not depend on reading a camera: see
/// `consort_call::camera`.
fn outgoing(picture: Picture) -> OutgoingPicture {
    OutgoingPicture {
        width: picture.width,
        height: picture.height,
        y: picture.y,
        u: picture.u,
        v: picture.v,
        timestamp_us: picture.timestamp_us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use consort_video::FrameSink;

    /// A camera backend a test can drive.
    #[derive(Clone, Default)]
    struct Fake {
        opens: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        /// What `open` should do.
        refusing: bool,
        /// The size the fake device negotiates, which is deliberately not the
        /// one that was asked for.
        negotiated: Option<Resolution>,
        /// Somewhere to keep the sink, so a test can push a frame through it.
        sink: Arc<Mutex<Option<FrameSink>>>,
    }

    impl Fake {
        fn refusing() -> Self {
            Self {
                refusing: true,
                ..Self::default()
            }
        }

        fn negotiating(width: u32, height: u32) -> Self {
            Self {
                negotiated: Some(Resolution {
                    width,
                    height,
                    fps: 30,
                }),
                ..Self::default()
            }
        }

        fn opens(&self) -> usize {
            self.opens.load(Ordering::Relaxed)
        }

        fn closes(&self) -> usize {
            self.closes.load(Ordering::Relaxed)
        }

        /// Push one frame through whatever sink is currently installed.
        fn capture(&self, nth: u8) {
            if let Some(sink) = self.sink.lock().unwrap().as_mut() {
                sink(picture(nth));
            }
        }
    }

    struct FakeStream {
        id: String,
        resolution: Resolution,
        closes: Arc<AtomicUsize>,
    }

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.closes.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl CameraStream for FakeStream {
        fn camera_id(&self) -> &str {
            &self.id
        }

        fn resolution(&self) -> Resolution {
            self.resolution
        }
    }

    impl VideoCapture for Fake {
        fn open(
            &self,
            camera: Option<&str>,
            want: Resolution,
            on_frame: FrameSink,
        ) -> Result<Box<dyn CameraStream>, CaptureError> {
            self.opens.fetch_add(1, Ordering::Relaxed);

            if self.refusing {
                return Err(CaptureError::Busy {
                    camera: camera.unwrap_or("/dev/video0").to_owned(),
                });
            }

            *self.sink.lock().unwrap() = Some(on_frame);
            Ok(Box::new(FakeStream {
                id: camera.unwrap_or("/dev/video0").to_owned(),
                resolution: self.negotiated.unwrap_or(want),
                closes: Arc::clone(&self.closes),
            }))
        }
    }

    /// One captured frame, at the smallest size a thumbnail keeps.
    fn picture(nth: u8) -> Picture {
        Picture {
            width: 2,
            height: 2,
            y: vec![nth; 4],
            u: vec![128],
            v: vec![128],
            timestamp_us: i64::from(nth),
        }
    }

    fn bridge(backend: Fake) -> (VideoBridge, Fake) {
        (
            VideoBridge::new(Box::new(backend.clone()), SelfView::new()),
            backend,
        )
    }

    #[test]
    fn starting_opens_the_camera_and_reports_what_it_negotiated() {
        // The negotiated size, not the wanted one. A publication configured for
        // 720p that is fed 480p frames has its encoder set up for a picture
        // that never arrives.
        let (bridge, backend) = bridge(Fake::negotiating(640, 480));

        let size = bridge.start(Some("/dev/video2"), Camera::new()).unwrap();

        assert_eq!(
            size,
            PictureSize {
                width: 640,
                height: 480
            }
        );
        assert_eq!(backend.opens(), 1);
        assert!(bridge.running());
    }

    #[test]
    fn frames_reach_the_queue() {
        let (bridge, backend) = bridge(Fake::default());
        let queue = Camera::new();

        bridge.start(None, queue.clone()).unwrap();
        backend.capture(7);

        let frame = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(queue.next());
        assert_eq!(frame.y, vec![7; 4]);
        assert_eq!(frame.timestamp_us, 7, "the capture time did not survive");
    }

    #[test]
    fn stopping_releases_the_device() {
        let (bridge, backend) = bridge(Fake::default());
        bridge.start(None, Camera::new()).unwrap();

        bridge.stop();

        assert_eq!(backend.closes(), 1);
        assert!(!bridge.running());
    }

    #[test]
    fn frames_reach_the_card_as_well_as_the_call() {
        // Both, from the one capture. The card draws what is being published
        // rather than opening a second device, which on V4L2 it could not do.
        let (bridge, backend) = bridge(Fake::default());

        bridge.start(None, Camera::new()).unwrap();
        backend.capture(7);

        assert!(bridge.mirror().latest(320).is_some());
    }

    #[test]
    fn stopping_takes_the_card_picture_down_with_the_device() {
        // Every path out of filming goes through `stop`, which is why the
        // picture is dropped here rather than beside each caller. A card still
        // showing a camera that is off is the one picture somebody who just
        // covered theirs did not want left on screen.
        let (bridge, _) = bridge(Fake::default());
        bridge.start(None, Camera::new()).unwrap();
        bridge.mirror().offer(&picture(7));

        bridge.stop();

        assert_eq!(bridge.mirror().latest(320), None);
    }

    #[test]
    fn a_camera_that_will_not_open_leaves_nothing_running() {
        // The ordinary failure, not an edge one: another application has the
        // camera, which on Linux means exactly one process can.
        let (bridge, backend) = bridge(Fake::refusing());

        let refused = bridge.start(None, Camera::new());

        assert_eq!(
            refused,
            Err(CaptureError::Busy {
                camera: "/dev/video0".to_owned()
            })
        );
        assert!(!bridge.running());
        assert_eq!(backend.closes(), 0);
    }

    #[test]
    fn a_camera_that_will_not_open_leaves_the_one_already_running_alone() {
        // Somebody switching camera in the settings screen while a call is up.
        // Dropping the working device first would turn their video off to find
        // out that the other camera is busy.
        let working = Fake::default();
        let bridge = VideoBridge::new(
            Box::new(Switching {
                good: working.clone(),
                bad: Fake::refusing(),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            SelfView::new(),
        );

        bridge.start(None, Camera::new()).unwrap();
        let refused = bridge.start(Some("/dev/video9"), Camera::new());

        assert!(refused.is_err());
        assert!(bridge.running(), "the working camera was released");
        assert_eq!(working.closes(), 0);
    }

    /// Opens once and refuses after that.
    struct Switching {
        good: Fake,
        bad: Fake,
        calls: Arc<AtomicUsize>,
    }

    impl VideoCapture for Switching {
        fn open(
            &self,
            camera: Option<&str>,
            want: Resolution,
            on_frame: FrameSink,
        ) -> Result<Box<dyn CameraStream>, CaptureError> {
            match self.calls.fetch_add(1, Ordering::Relaxed) {
                0 => self.good.open(camera, want, on_frame),
                _ => self.bad.open(camera, want, on_frame),
            }
        }
    }

    #[test]
    fn starting_twice_releases_the_first_camera() {
        // Switching camera is one act. Two devices held at once is a webcam
        // light on beside a camera nobody is publishing.
        let (bridge, backend) = bridge(Fake::default());

        bridge.start(Some("/dev/video0"), Camera::new()).unwrap();
        bridge.start(Some("/dev/video2"), Camera::new()).unwrap();

        assert_eq!(backend.opens(), 2);
        assert_eq!(backend.closes(), 1, "the first camera is still held");
    }

    #[test]
    fn stopping_a_bridge_that_never_started_is_not_a_failure() {
        // How every call ends: the interface puts the camera away whether or
        // not one was ever open.
        let (bridge, backend) = bridge(Fake::default());

        bridge.stop();

        assert_eq!(backend.closes(), 0);
        assert!(!bridge.running());
    }
}
