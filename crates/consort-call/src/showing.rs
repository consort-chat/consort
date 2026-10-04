// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Feeding a camera into a call.
//!
//! Switching the camera off retracts the publication, where [`crate::publish`]
//! keeps pushing substituted silence. Not an inconsistency: a silent sender
//! reads as a wedged client, and a retracted camera is what `camera_live` in
//! [`crate::roster`] already draws an avatar for.

use crate::camera::{Camera, OutgoingPicture};
use crate::failure::CallFailure;

/// A live camera publication in a call.
///
/// The seam that keeps the call thread testable without an SFU, like
/// [`crate::PublishedAudio`]. Frames only: taking it down is
/// [`crate::CallSession::retract_camera`], because retracting the handle
/// alone left the camera on this session's own roster entry.
pub trait PublishedVideo: 'static {
    /// Push one I420 frame.
    ///
    /// Not `async`, unlike the audio counterpart, because the transport's
    /// video capture is latest-frame-wins and applies no backpressure. There
    /// is nothing to await: it either hands the frame to the encoder or says
    /// the publication is gone.
    fn send(&self, picture: OutgoingPicture) -> Result<(), CallFailure>;
}

/// Feed `camera` into `track` until the publication stops accepting frames.
///
/// Returns only when the publication is gone. It is otherwise ended by being
/// aborted, which is what the call thread does when the camera is switched off
/// or the call ends: this waits on a slot that a camera which has been closed
/// will never fill again.
pub async fn pump<P: PublishedVideo>(track: P, camera: Camera) {
    loop {
        let picture = camera.next().await;

        if let Err(error) = track.send(picture) {
            tracing::warn!(%error, "the camera publication stopped accepting frames");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A publication that takes a fixed number of frames and then reports
    /// itself gone.
    #[derive(Clone)]
    struct FakeTrack {
        taken: Arc<AtomicUsize>,
        accepts: usize,
    }

    impl FakeTrack {
        fn accepting(accepts: usize) -> Self {
            Self {
                taken: Arc::new(AtomicUsize::new(0)),
                accepts,
            }
        }

        fn taken(&self) -> usize {
            self.taken.load(Ordering::Relaxed)
        }
    }

    impl PublishedVideo for FakeTrack {
        fn send(&self, _picture: OutgoingPicture) -> Result<(), CallFailure> {
            if self.taken() >= self.accepts {
                return Err(CallFailure::NoTransport(
                    "the publication is gone".to_owned(),
                ));
            }
            self.taken.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

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
    async fn the_pump_stops_when_the_publication_stops_accepting_frames() {
        // One frame and a publication that takes none, so the refusal is the
        // first thing that happens. Queueing more would not help: the slot
        // holds one, so a pump that is still running simply waits, and a test
        // that waited with it would hang rather than fail.
        let track = FakeTrack::accepting(0);
        let camera = Camera::new();
        camera.offer(frame(1));

        pump(track.clone(), camera).await;

        assert_eq!(track.taken(), 0);
    }

    #[tokio::test]
    async fn the_pump_keeps_taking_frames_until_one_is_refused() {
        // Offered one at a time with a turn in between, because the slot holds
        // one: the pump has to be let run for each frame to reach it.
        let track = FakeTrack::accepting(2);
        let camera = Camera::new();
        let offering = camera.clone();

        let pumping = tokio::spawn(pump(track.clone(), camera));
        for nth in 0..20 {
            if pumping.is_finished() {
                break;
            }
            offering.offer(frame(nth));
            tokio::task::yield_now().await;
        }
        pumping.await.unwrap();

        assert_eq!(track.taken(), 2);
    }
}
