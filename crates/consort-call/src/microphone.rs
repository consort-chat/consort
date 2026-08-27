// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The queue between the microphone and the call.
//!
//! Two threads meet here and they are paced by different clocks. The audio
//! thread produces one frame every 10 ms, driven by a sound card that will not
//! wait. The call thread consumes them through `capture_audio`, which is
//! `async` and applies backpressure: it resolves when the frame has been
//! accepted, and what accepts it is on the other side of a network.
//!
//! So the producer must never wait on the consumer. The audio thread is also
//! servicing a capture callback and running the denoiser, and a capture loop
//! stalled on an SFU is a glitching microphone. [`Microphone::offer`] therefore
//! takes a lock, pushes, and returns, and when the queue is full it drops the
//! **oldest** frame rather than refusing the newest.
//!
//! Oldest, because this is a conversation. A frame that has been waiting is a
//! frame the listener would hear late, and late audio in a call is worse than
//! missing audio: it puts every frame behind it further behind. Dropping the
//! newest would keep the backlog and grow the delay instead.
//!
//! That also decides the bound. With both ends running at 100 frames a second,
//! a backlog never drains on its own, so whatever the queue is holding is
//! permanent added latency until something drops. The bound is therefore the
//! cap on how far behind live this can persistently sit.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

/// How many frames may be waiting before the oldest is dropped.
///
/// Eight frames is 80 ms. Large enough to absorb the scheduling jitter of a
/// desktop that is also drawing an interface, small enough that recovering
/// from a hiccup costs less delay than a person notices in a conversation.
pub const QUEUE_FRAMES: usize = 8;

/// How many drops between complaints, so a call that cannot keep up says so
/// about once a second rather than a hundred times.
const REPORT_EVERY: u64 = 100;

/// One frame on its way out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutgoingFrame {
    /// Mono 48 kHz PCM, one [`consort_audio::FRAME_SAMPLES`] frame.
    ///
    /// [`consort_audio::FRAME_SAMPLES`]: https://docs.rs/consort-audio
    pub samples: Vec<i16>,
    /// The gate's verdict for this frame.
    ///
    /// `false` means the samples are the silence the gate substituted rather
    /// than anything anybody said. Carried per frame rather than signalled as
    /// an edge so that muting a publication can be lined up with the exact
    /// frame it applies to, even when some of the frames before it are still
    /// queued.
    pub open: bool,
}

/// Frames waiting to be published, newest wins.
///
/// Cheap to clone: every clone is the same queue. One goes to the audio
/// thread, which only ever [`offer`](Self::offer)s, and one to the call
/// thread, which only ever takes [`next`](Self::next).
///
/// Nothing drains it while no call is up, so the audio thread should only be
/// pointed at one for as long as there is a call to carry the audio. Doing it
/// anyway is harmless, because the bound holds and nothing blocks, but it is
/// work done for nobody and the log will say so.
#[derive(Clone, Default)]
pub struct Microphone(Arc<Shared>);

#[derive(Default)]
struct Shared {
    queue: Mutex<VecDeque<OutgoingFrame>>,
    /// Frames dropped for want of room, for the life of this queue.
    dropped: AtomicU64,
    /// Woken on every offer. `Notify` keeps a permit when nobody is waiting,
    /// so a frame that arrives between a failed pop and the await that follows
    /// it is not a lost wake-up.
    ready: Notify,
}

impl Microphone {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer one gated frame, dropping the oldest if there is no room.
    ///
    /// Never blocks and never awaits. Called from the audio thread.
    pub fn offer(&self, samples: &[i16], open: bool) {
        let frame = OutgoingFrame {
            samples: samples.to_vec(),
            open,
        };

        let dropped = {
            let mut queue = self.queue();
            let dropped = queue.len() >= QUEUE_FRAMES && queue.pop_front().is_some();
            queue.push_back(frame);
            dropped
        };

        if dropped {
            self.report_drop();
        }
        self.0.ready.notify_one();
    }

    /// The next frame, waiting for one if the queue is empty.
    ///
    /// Waits forever on a microphone that is not capturing, which is why the
    /// task doing this is one the call thread aborts rather than one that is
    /// expected to return.
    pub async fn next(&self) -> OutgoingFrame {
        loop {
            let popped = self.queue().pop_front();
            if let Some(frame) = popped {
                return frame;
            }
            self.0.ready.notified().await;
        }
    }

    /// How many frames have been dropped for want of room.
    pub fn dropped(&self) -> u64 {
        self.0.dropped.load(Ordering::Relaxed)
    }

    /// The queue, recovering from a poisoned lock rather than spreading a
    /// panic.
    ///
    /// Nothing inside the critical section can panic, so the recovery is
    /// unreachable. It is written this way because one of the two callers is
    /// the audio thread, and a panic there takes the microphone with it.
    /// Carrying on with a frame or two of nonsense is a much smaller thing.
    fn queue(&self) -> MutexGuard<'_, VecDeque<OutgoingFrame>> {
        self.0.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn report_drop(&self) {
        let dropped = self.0.dropped.fetch_add(1, Ordering::Relaxed) + 1;
        if dropped % REPORT_EVERY == 1 {
            tracing::warn!(
                dropped,
                "the call is not keeping up with the microphone; dropped the oldest frame"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A frame whose samples say which one it is.
    fn frame(nth: i16) -> Vec<i16> {
        vec![nth; 4]
    }

    fn offer(microphone: &Microphone, nth: i16) {
        microphone.offer(&frame(nth), true);
    }

    /// Everything currently queued, in order.
    async fn drain(microphone: &Microphone) -> Vec<i16> {
        let mut drained = Vec::new();
        while !microphone.queue().is_empty() {
            drained.push(microphone.next().await.samples[0]);
        }
        drained
    }

    #[tokio::test]
    async fn frames_come_back_out_in_the_order_they_went_in() {
        let microphone = Microphone::new();

        offer(&microphone, 1);
        offer(&microphone, 2);
        offer(&microphone, 3);

        assert_eq!(drain(&microphone).await, vec![1, 2, 3]);
        assert_eq!(microphone.dropped(), 0);
    }

    #[tokio::test]
    async fn the_gate_s_verdict_travels_with_the_frame() {
        // Per frame rather than as an edge, so the mute can be lined up with
        // the frame it applies to even when frames are queued behind it.
        let microphone = Microphone::new();

        microphone.offer(&frame(1), true);
        microphone.offer(&frame(2), false);

        assert!(microphone.next().await.open);
        assert!(!microphone.next().await.open);
    }

    #[tokio::test]
    async fn a_full_queue_drops_the_oldest_rather_than_the_newest() {
        // The newest frame is the one the listener is waiting for. Keeping the
        // backlog instead would make every frame after it later still.
        let microphone = Microphone::new();

        for nth in 0..=QUEUE_FRAMES {
            offer(&microphone, nth as i16);
        }

        let drained = drain(&microphone).await;
        assert_eq!(drained.len(), QUEUE_FRAMES);
        assert_eq!(drained[0], 1, "the oldest frame survived: {drained:?}");
        assert_eq!(
            drained[QUEUE_FRAMES - 1],
            QUEUE_FRAMES as i16,
            "the newest frame was the one dropped: {drained:?}"
        );
        assert_eq!(microphone.dropped(), 1);
    }

    #[tokio::test]
    async fn offering_into_a_queue_nobody_is_draining_neither_blocks_nor_grows() {
        // A call that went away, or one that never started. The audio thread
        // must not notice either.
        let microphone = Microphone::new();
        let offered = 1_000;

        for nth in 0..offered {
            offer(&microphone, nth as i16);
        }

        assert_eq!(microphone.queue().len(), QUEUE_FRAMES);
        assert_eq!(microphone.dropped(), offered as u64 - QUEUE_FRAMES as u64);
    }

    #[tokio::test(start_paused = true)]
    async fn a_drain_waiting_on_an_empty_queue_wakes_when_a_frame_arrives() {
        // The whole point of the queue: the consumer parks instead of polling,
        // and an offer is what unparks it. Time is paused, so this test either
        // wakes or hangs; it cannot pass by accident.
        let microphone = Microphone::new();
        let offering = microphone.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            offering.offer(&frame(7), false);
        });

        let frame = microphone.next().await;

        assert_eq!(frame.samples, vec![7; 4]);
        assert!(!frame.open);
    }
}
