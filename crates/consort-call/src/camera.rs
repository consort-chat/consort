// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The queue between the camera and the call.
//!
//! One slot, newest wins, where [`crate::microphone`] holds eight and drops the
//! oldest. Audio is a sequence and a picture is a state, so a frame that has
//! been waiting is one the next frame has already replaced. It is also what
//! `capture_video` asks for: latest-frame-wins, with no backpressure.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

/// How many dropped frames between complaints.
///
/// Thirty, so a camera the call cannot keep up with says so about once a
/// second rather than thirty times.
const REPORT_EVERY: u64 = 30;

/// How large the frames a camera publication will carry are.
///
/// Known before anything is published, because the device is opened first: the
/// transport derives its encoder and simulcast layers from this, and guessing
/// would set those up for a size the camera never sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureSize {
    pub width: u32,
    pub height: u32,
}

/// One frame on its way out, planar I420 with tight strides.
///
/// This crate's own type rather than `consort_video`'s, so that carrying a
/// call does not depend on reading a camera. The same seam the microphone has:
/// `consort-audio` is a dev-dependency here and nothing more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingPicture {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    /// When the camera captured this, in microseconds on a monotonic clock.
    pub timestamp_us: i64,
}

/// The frame waiting to be published, if any.
///
/// Cheap to clone: every clone is the same slot. One goes to whatever is
/// reading the camera, which only ever [`offer`](Self::offer)s, and one to the
/// call thread, which only ever takes [`next`](Self::next).
#[derive(Clone, Default)]
pub struct Camera(Arc<Shared>);

#[derive(Default)]
struct Shared {
    slot: Mutex<Option<OutgoingPicture>>,
    /// Frames replaced before anybody published them, for the life of this.
    dropped: AtomicU64,
    /// Woken on every offer. `Notify` keeps a permit when nobody is waiting,
    /// so a frame that arrives between a failed take and the await after it is
    /// not a lost wake-up.
    ready: Notify,
}

impl Camera {
    /// An empty slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer one frame, replacing whatever was waiting.
    ///
    /// Never blocks and never awaits. Called from the capture thread.
    pub fn offer(&self, picture: OutgoingPicture) {
        let replaced = self.slot().replace(picture).is_some();

        if replaced
            && self
                .0
                .dropped
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(REPORT_EVERY)
        {
            tracing::debug!(
                dropped = self.dropped(),
                "the call is not keeping up with the camera"
            );
        }

        self.0.ready.notify_one();
    }

    /// Wait for a frame and take it.
    pub async fn next(&self) -> OutgoingPicture {
        loop {
            if let Some(picture) = self.slot().take() {
                return picture;
            }
            self.0.ready.notified().await;
        }
    }

    /// Throw away whatever is waiting.
    ///
    /// Called when the camera is switched off, so that switching it back on
    /// does not publish the last thing it saw before it stopped.
    pub fn clear(&self) {
        self.slot().take();
    }

    /// Frames replaced before anybody published them.
    pub fn dropped(&self) -> u64 {
        self.0.dropped.load(Ordering::Relaxed)
    }

    fn slot(&self) -> MutexGuard<'_, Option<OutgoingPicture>> {
        self.0.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(nth: u8) -> OutgoingPicture {
        OutgoingPicture {
            width: 2,
            height: 2,
            y: vec![nth; 4],
            u: vec![128],
            v: vec![128],
            timestamp_us: i64::from(nth),
        }
    }

    #[tokio::test]
    async fn a_frame_comes_back_out() {
        let camera = Camera::new();

        camera.offer(frame(1));

        assert_eq!(camera.next().await, frame(1));
    }

    #[tokio::test]
    async fn the_newest_frame_wins_rather_than_the_oldest() {
        // The whole difference from the microphone queue. A picture is a
        // state, so an older frame is not missing data, it is the wrong
        // answer: publishing it would show the call something that has already
        // stopped being true.
        let camera = Camera::new();

        camera.offer(frame(1));
        camera.offer(frame(2));
        camera.offer(frame(3));

        assert_eq!(camera.next().await, frame(3));
    }

    #[tokio::test]
    async fn offering_into_a_slot_nobody_is_draining_neither_blocks_nor_grows() {
        let camera = Camera::new();

        for nth in 0..50 {
            camera.offer(frame(nth));
        }

        assert_eq!(
            camera.dropped(),
            49,
            "every frame but the last was replaced"
        );
        assert_eq!(camera.next().await, frame(49));
    }

    #[tokio::test]
    async fn a_taken_frame_is_not_handed_out_twice() {
        let camera = Camera::new();
        camera.offer(frame(1));

        let first = camera.next().await;
        camera.offer(frame(2));
        let second = camera.next().await;

        assert_eq!((first, second), (frame(1), frame(2)));
    }

    #[tokio::test]
    async fn clearing_throws_the_waiting_frame_away() {
        // What stops switching a camera back on from publishing the last thing
        // it saw before it stopped, which for somebody who covered their
        // camera is the one frame they did not want sent.
        //
        // Checked through the drop tally rather than through what comes out,
        // because the newest frame wins either way: a `clear` that did nothing
        // would still hand back frame 2, having counted frame 1 as replaced.
        let camera = Camera::new();
        camera.offer(frame(1));

        camera.clear();
        camera.offer(frame(2));

        assert_eq!(camera.next().await, frame(2));
        assert_eq!(
            camera.dropped(),
            0,
            "frame 1 was replaced rather than cleared, so it was still waiting"
        );
    }

    #[tokio::test]
    async fn clearing_an_empty_slot_is_not_a_dropped_frame() {
        let camera = Camera::new();

        camera.clear();

        assert_eq!(camera.dropped(), 0);
    }

    #[tokio::test]
    async fn a_drain_waiting_on_an_empty_slot_wakes_when_a_frame_arrives() {
        // `Notify` keeps a permit when nobody is waiting, so the frame that
        // lands between a failed take and the await after it is not a lost
        // wake-up. Without that this hangs rather than failing.
        let camera = Camera::new();
        let offering = camera.clone();

        let waiting = tokio::spawn(async move { camera.next().await });
        offering.offer(frame(7));

        assert_eq!(waiting.await.unwrap(), frame(7));
    }
}
