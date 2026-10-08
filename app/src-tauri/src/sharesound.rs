// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The sound a shared screen is making, joined to the call.
//!
//! [`crate::screen`]'s opposite number for audio, and the same shape: one
//! capture, running or not, with the backend behind a trait so nothing here
//! needs a sound card to be tested.

use std::sync::Mutex;

use consort_audio::{CaptureStream, ShareSound};
use consort_call::Microphone;

/// The loopback capture, and whatever it is currently feeding.
pub struct ShareSoundBridge {
    backend: Box<dyn ShareSound>,
    /// The running capture, while there is one. Dropping it stops the frames.
    open: Mutex<Option<Box<dyn CaptureStream>>>,
}

impl ShareSoundBridge {
    /// Hold `backend` without capturing anything.
    pub fn new(backend: Box<dyn ShareSound>) -> Self {
        Self {
            backend,
            open: Mutex::new(None),
        }
    }

    /// Whether this build can capture what the machine is playing.
    ///
    /// What the switch in the settings screen is drawn from. A switch for a
    /// capture that cannot happen is a switch that lies about what it does.
    pub fn available(&self) -> bool {
        self.backend.available()
    }

    /// Start capturing into `queue` if `wanted`, and say whether anything is
    /// going out.
    ///
    /// Three ways to answer no, and the caller treats them alike: nobody asked,
    /// this build cannot, or the capture would not open. A share goes out
    /// either way, because a picture without its sound beats no picture.
    pub fn start(&self, wanted: bool, queue: Microphone) -> bool {
        if !wanted || !self.available() {
            return false;
        }

        match self.backend.open(
            None,
            Box::new(move |samples| {
                // Open, always: the voice gate is for a microphone, and
                // gating a shared video would chop it up.
                queue.offer(samples, true);
            }),
        ) {
            Ok(stream) => {
                *self.open() = Some(stream);
                true
            }
            Err(error) => {
                tracing::warn!(%error, "sharing without sound: the capture would not open");
                false
            }
        }
    }

    /// Stop capturing, if anything is.
    pub fn stop(&self) {
        self.open().take();
    }

    /// Whether a capture is running. Test-only.
    #[cfg(test)]
    pub fn capturing(&self) -> bool {
        self.open().is_some()
    }

    fn open(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn CaptureStream>>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use consort_audio::{CaptureError, FrameSink, NoShareSound};

    use super::*;

    struct FakeStream;

    impl CaptureStream for FakeStream {
        fn device_name(&self) -> &str {
            "Speakers"
        }
    }

    #[derive(Clone, Default)]
    struct Fake {
        opens: Arc<AtomicUsize>,
        refuses: bool,
        /// What to push at the sink as soon as it is opened.
        sample: Option<i16>,
    }

    impl Fake {
        fn opens(&self) -> usize {
            self.opens.load(Ordering::Relaxed)
        }
    }

    impl ShareSound for Fake {
        fn available(&self) -> bool {
            true
        }

        fn open(
            &self,
            _device: Option<&str>,
            on_frame: FrameSink,
        ) -> Result<Box<dyn CaptureStream>, CaptureError> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            if self.refuses {
                return Err(CaptureError::NoDevice);
            }
            let mut on_frame = on_frame;
            if let Some(sample) = self.sample {
                on_frame(&[sample; 4]);
            }
            Ok(Box::new(FakeStream))
        }
    }

    fn bridge(backend: Fake) -> (ShareSoundBridge, Fake) {
        (ShareSoundBridge::new(Box::new(backend.clone())), backend)
    }

    #[test]
    fn asking_for_sound_on_a_build_that_has_it_opens_the_capture() {
        let (bridge, backend) = bridge(Fake::default());

        let going_out = bridge.start(true, Microphone::new());

        assert!(going_out);
        assert_eq!(backend.opens(), 1);
        assert!(bridge.capturing());
    }

    #[test]
    fn a_share_nobody_asked_to_send_sound_for_opens_nothing() {
        // The default, and the whole of what the switch being off means.
        let (bridge, backend) = bridge(Fake::default());

        let going_out = bridge.start(false, Microphone::new());

        assert!(!going_out);
        assert_eq!(
            backend.opens(),
            0,
            "a screen was read for its sound unasked"
        );
    }

    #[test]
    fn a_build_that_cannot_capture_is_never_asked_to() {
        // Reported rather than attempted. The refusal would be identical, but
        // every share would log one.
        let bridge = ShareSoundBridge::new(Box::new(NoShareSound));

        assert!(!bridge.available());
        assert!(!bridge.start(true, Microphone::new()));
        assert!(!bridge.capturing());
    }

    #[test]
    fn a_capture_that_will_not_open_is_a_share_without_sound() {
        let (bridge, backend) = bridge(Fake {
            refuses: true,
            ..Fake::default()
        });

        let going_out = bridge.start(true, Microphone::new());

        assert!(!going_out, "a refused capture was reported as going out");
        assert_eq!(backend.opens(), 1);
        assert!(!bridge.capturing());
    }

    #[tokio::test]
    async fn what_the_machine_is_playing_reaches_the_queue_it_was_given() {
        let (bridge, _backend) = bridge(Fake {
            sample: Some(9),
            ..Fake::default()
        });
        let queue = Microphone::new();

        bridge.start(true, queue.clone());

        let frame = queue.next().await;
        assert_eq!(frame.samples, vec![9; 4]);
        assert!(frame.open, "a share's sound was gated like a microphone");
    }

    #[test]
    fn stopping_releases_the_capture() {
        let (bridge, _backend) = bridge(Fake::default());
        bridge.start(true, Microphone::new());

        bridge.stop();

        assert!(!bridge.capturing());
    }
}
