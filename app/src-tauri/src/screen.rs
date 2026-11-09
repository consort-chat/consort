// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The screen, joined to the call.
//!
//! [`crate::video`]'s opposite number, and deliberately the same shape: one
//! capture, running or not, with the backend behind a trait so nothing here
//! needs a display to be tested.

use std::sync::Mutex;

use consort_call::{Camera, OutgoingPicture, PictureSize, ScreenShare};
use consort_video::{Picture, ScreenCapture, ShareError, ShareSource, ShareStream};

use crate::selfview::SelfView;

/// Why a screen is not in the call.
///
/// The same two cases [`crate::video::CameraTrouble`] has, and for the same
/// reason: only one of them is about the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareTrouble {
    /// There is no call to share into.
    NoCall,
    /// The screen or window would not open.
    Display(ShareError),
}

impl ShareTrouble {
    /// One sentence for the interface to draw.
    pub fn user_message(&self) -> String {
        match self {
            Self::NoCall => "join a voice channel before sharing your screen".to_owned(),
            Self::Display(error) => error.user_message(),
        }
    }
}

/// The screen capture, and whatever it is currently feeding.
pub struct ScreenBridge {
    backend: Box<dyn ScreenCapture>,
    /// The running capture, while there is one. Dropping it stops the frames
    /// and waits until it has: see `consort_video::x11_host`.
    open: Mutex<Option<Box<dyn ShareStream>>>,
    /// The newest frame, for the call card to draw.
    mirror: SelfView,
}

impl ScreenBridge {
    /// Hold `backend` and `mirror` without capturing anything.
    pub fn new(backend: Box<dyn ScreenCapture>, mirror: SelfView) -> Self {
        Self {
            backend,
            open: Mutex::new(None),
            mirror,
        }
    }

    /// Everything shareable right now.
    pub fn sources(&self) -> Result<Vec<ShareSource>, ShareError> {
        self.backend.sources()
    }

    /// Start capturing `id` and point its frames at `queue`.
    ///
    /// Reports what a publication has to be set up for: the size, and the
    /// title the indicator will show. Both come from the capture rather than
    /// from the picker's click, because a window can be resized or renamed
    /// between being listed and being chosen.
    pub fn start(&self, id: &str, queue: Camera) -> Result<ScreenShare, ShareError> {
        let mirror = self.mirror.clone();
        let stream = self.backend.open(
            id,
            Box::new(move |picture| {
                // Sampled down before the frame is moved on, which is the only
                // moment both readers can be served from one capture.
                mirror.offer(&picture);
                queue.offer(outgoing(picture));
            }),
        )?;

        let share = ScreenShare {
            size: PictureSize {
                width: stream.resolution().width,
                height: stream.resolution().height,
            },
            title: stream.source().title.clone(),
        };
        // Replaced rather than dropped first, on the same terms as
        // `VideoBridge::start`: a source that will not open leaves the one
        // that is working alone.
        *self.open() = Some(stream);

        Ok(share)
    }

    /// Stop capturing, if anything is, and drop the card's picture.
    ///
    /// Both here, because every way out of sharing comes through this: the
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

    /// What is being captured right now, if anything.
    ///
    /// Read by the indicator's own test rather than by the interface, which
    /// learns this from the screen channel: that is the only account of it
    /// that includes the publication.
    #[cfg(test)]
    pub fn sharing(&self) -> Option<String> {
        self.open()
            .as_ref()
            .map(|stream| stream.source().title.clone())
    }

    fn open(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn ShareStream>>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Repack a captured frame as the one the call crate carries.
///
/// A move of three vectors rather than a copy of the pixels, for the reason
/// `crate::video::outgoing` gives.
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

    use consort_video::{FrameSink, Resolution, ShareKind};

    /// A screen capture backend a test can drive.
    #[derive(Clone, Default)]
    struct Fake {
        opens: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        /// What `open` should do.
        refusing: bool,
        /// The size the fake capture reports, which is deliberately not the
        /// size the picker listed.
        captured: Option<Resolution>,
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

        fn capturing(width: u32, height: u32) -> Self {
            Self {
                captured: Some(Resolution {
                    width,
                    height,
                    fps: 15,
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
                sink(Picture {
                    width: 2,
                    height: 2,
                    y: vec![nth; 4],
                    u: vec![128],
                    v: vec![128],
                    timestamp_us: i64::from(nth),
                });
            }
        }
    }

    struct FakeStream {
        source: ShareSource,
        resolution: Resolution,
        closes: Arc<AtomicUsize>,
    }

    impl Drop for FakeStream {
        fn drop(&mut self) {
            self.closes.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ShareStream for FakeStream {
        fn source(&self) -> &ShareSource {
            &self.source
        }

        fn resolution(&self) -> Resolution {
            self.resolution
        }
    }

    impl ScreenCapture for Fake {
        fn sources(&self) -> Result<Vec<ShareSource>, ShareError> {
            Ok(vec![consort_video::screen("DP-0", 2560, 1440)])
        }

        fn open(&self, id: &str, on_frame: FrameSink) -> Result<Box<dyn ShareStream>, ShareError> {
            self.opens.fetch_add(1, Ordering::Relaxed);

            if self.refusing {
                return Err(ShareError::Gone {
                    source: id.to_owned(),
                });
            }

            *self.sink.lock().unwrap() = Some(on_frame);
            let resolution = self.captured.unwrap_or(Resolution {
                width: 800,
                height: 600,
                fps: 15,
            });
            Ok(Box::new(FakeStream {
                source: ShareSource {
                    id: id.to_owned(),
                    title: format!("{id} as it is now"),
                    kind: ShareKind::Screen,
                    fullscreen: false,
                    width: resolution.width,
                    height: resolution.height,
                },
                resolution,
                closes: Arc::clone(&self.closes),
            }))
        }
    }

    fn bridge(backend: Fake) -> (ScreenBridge, Fake) {
        (
            ScreenBridge::new(Box::new(backend.clone()), SelfView::new()),
            backend,
        )
    }

    #[test]
    fn starting_reports_the_size_the_capture_is_actually_producing() {
        // Not the size the picker listed. A window can be resized while the
        // picker is open, and a publication configured for the old size has
        // its encoder set up for frames that never arrive.
        let (bridge, backend) = bridge(Fake::capturing(1920, 1080));

        let share = bridge.start("screen:DP-0", Camera::new()).unwrap();

        assert_eq!(
            share.size,
            PictureSize {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(backend.opens(), 1);
    }

    #[test]
    fn starting_reports_what_is_being_shared_so_the_indicator_can_name_it() {
        // The requirement from #70: somebody sharing has to be able to see
        // what is going out, and that name has to come from the capture rather
        // than from the click that started it.
        let (bridge, _backend) = bridge(Fake::default());

        let share = bridge.start("window:7", Camera::new()).unwrap();

        assert_eq!(share.title, "window:7 as it is now");
    }

    #[test]
    fn frames_reach_the_queue() {
        let (bridge, backend) = bridge(Fake::default());
        let queue = Camera::new();

        bridge.start("screen:DP-0", queue.clone()).unwrap();
        backend.capture(7);

        let frame = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(queue.next());
        assert_eq!(frame.y, vec![7; 4]);
        assert_eq!(frame.timestamp_us, 7, "the capture time did not survive");
    }

    #[test]
    fn a_captured_frame_also_reaches_the_card() {
        // Both readers off one capture. Somebody sharing has to be able to see
        // what is going out, and #70's indicator can only name it.
        let (bridge, backend) = bridge(Fake::default());
        bridge.start("screen:DP-0", Camera::new()).unwrap();

        backend.capture(7);

        assert!(
            bridge.mirror().latest(320).is_some(),
            "the card has nothing to draw"
        );
    }

    #[test]
    fn stopping_leaves_the_card_nothing_to_draw() {
        // The same reason the camera clears its own: somebody who just stopped
        // sharing their bank statement should not still be looking at it.
        let (bridge, backend) = bridge(Fake::default());
        bridge.start("screen:DP-0", Camera::new()).unwrap();
        backend.capture(7);

        bridge.stop();

        assert_eq!(bridge.mirror().latest(320), None);
    }

    #[test]
    fn stopping_releases_the_capture() {
        let (bridge, backend) = bridge(Fake::default());
        bridge.start("screen:DP-0", Camera::new()).unwrap();

        bridge.stop();

        assert_eq!(backend.closes(), 1);
        assert_eq!(bridge.sharing(), None);
    }

    #[test]
    fn a_source_that_will_not_open_leaves_nothing_running() {
        // The ordinary failure: a window that closed while the picker was on
        // screen.
        let (bridge, backend) = bridge(Fake::refusing());

        let refused = bridge.start("window:7", Camera::new());

        assert_eq!(
            refused,
            Err(ShareError::Gone {
                source: "window:7".to_owned()
            })
        );
        assert_eq!(bridge.sharing(), None);
        assert_eq!(backend.closes(), 0);
    }

    #[test]
    fn switching_source_releases_the_first_capture() {
        // One capture at a time. Two held at once is two screens going out
        // where somebody asked for one.
        let (bridge, backend) = bridge(Fake::default());

        bridge.start("screen:DP-0", Camera::new()).unwrap();
        bridge.start("screen:HDMI-0", Camera::new()).unwrap();

        assert_eq!(backend.opens(), 2);
        assert_eq!(backend.closes(), 1, "the first capture is still running");
    }

    #[test]
    fn stopping_a_bridge_that_never_started_is_not_a_failure() {
        // How every call ends: the interface puts the share away whether or
        // not one was ever running.
        let (bridge, backend) = bridge(Fake::default());

        bridge.stop();

        assert_eq!(backend.closes(), 0);
        assert_eq!(bridge.sharing(), None);
    }

    #[test]
    fn being_outside_a_call_is_a_different_sentence_from_a_display_failure() {
        // The two have different fixes: one is clicking a channel, the other
        // is choosing again. Collapsing them sends somebody to debug their
        // display server when they simply are not in a call.
        let outside = ShareTrouble::NoCall.user_message();
        let display = ShareTrouble::Display(ShareError::NoX11).user_message();

        assert!(outside.contains("voice channel"), "{outside:?}");
        assert_ne!(outside, display);
    }
}
