// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Feeding a microphone into a call.
//!
//! ## Silence is published, not withheld
//!
//! When the gate shuts it fills the frame with zeroes and the frame still goes
//! out. A sender that simply stops sending is indistinguishable from a client
//! that has wedged, and that is what a peer's interface will show. Opus
//! collapses silence on the wire, so the frames cost close to nothing.
//!
//! ## And the publication is never muted to say so
//!
//! It used to be. The reasoning was that publishing silence says "still here"
//! without saying "deliberately quiet", and that `set_muted` was the signal
//! for the difference. That reasoning was wrong twice over, and the bug it
//! produced was bad enough to be worth writing down.
//!
//! A transport mute is not a description of the audio. It is the "I clicked
//! the mute button" state, it is broadcast to every peer, and it is sticky.
//! Driving it from voice activity meant that everybody else watched the mute
//! icon switch on and off in time with the speaker's own pauses, which is not
//! a thing any other client does and not a thing anybody wants to look at.
//!
//! Worse, it is not only an indicator. Muting a LiveKit track stops its RTP,
//! and a subscriber tears the audio path down and builds it back up around
//! that. Doing it on every pause, several times in ten seconds, produced
//! exactly the gaps and artefacts it looks like it would.
//!
//! So the gate's verdict does not reach the transport at all. It stays what it
//! always was underneath: which samples are somebody talking and which are
//! substituted silence. Who is currently speaking is a question the SFU
//! already answers, from the audio, for every participant at once, and that is
//! where an interface should read it from.
//!
//! This is also why there is no mute here to be turned off when voice activity
//! is switched off in `GateConfig`. With it off the gate reports every frame
//! open, and nothing downstream was ever looking.

use crate::failure::CallFailure;
use crate::microphone::{Microphone, OutgoingFrame};

/// A live microphone publication in a call.
///
/// The seam that keeps everything above it testable, the same way
/// [`crate::CallTransport`] does for joining. The real one is an
/// `Arc<dyn LocalTrackHandle>` and lives in [`crate::livekit`].
///
/// One method, and deliberately only one. A publication carries audio. The
/// transport can also be told to mute, and the header above is the record of
/// what happened when this seam offered that and the pump reached for it.
/// A mute button, when there is one, is a person's intent arriving from the
/// interface, and it does not belong on the path a hundred frames a second
/// travel down.
#[allow(
    async_fn_in_trait,
    reason = "the returned future is awaited only on the call thread, which is \
              single-threaded by construction, so a Send bound would be a \
              promise nothing needs"
)]
pub trait PublishedAudio: 'static {
    /// Push one frame of mono 48 kHz PCM.
    ///
    /// Paced by the transport, which is the only pacing in the whole chain:
    /// it resolves once the frame has been accepted.
    async fn send(&self, samples: Vec<i16>) -> Result<(), CallFailure>;
}

/// Feed `microphone` into `track` until the publication stops accepting audio.
///
/// Returns only when the call is over. It is otherwise ended by being aborted,
/// which is what the call thread does when it leaves: this waits on a queue
/// that a microphone which has been switched off will never fill again.
pub async fn pump<P: PublishedAudio>(track: P, microphone: Microphone) {
    loop {
        let OutgoingFrame { samples, .. } = microphone.next().await;

        if let Err(error) = track.send(samples).await {
            tracing::warn!(%error, "the microphone publication stopped accepting audio");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeTrack {
        /// The first sample of every frame the track was handed, in order.
        sent: Arc<Mutex<Vec<i16>>>,
        /// How many frames this accepts before refusing the rest.
        accepts: Option<usize>,
    }

    impl FakeTrack {
        fn new() -> Self {
            Self::default()
        }

        fn accepting(frames: usize) -> Self {
            Self {
                accepts: Some(frames),
                ..Self::new()
            }
        }

        fn sent(&self) -> Vec<i16> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl PublishedAudio for FakeTrack {
        async fn send(&self, samples: Vec<i16>) -> Result<(), CallFailure> {
            if self
                .accepts
                .is_some_and(|accepts| self.sent().len() >= accepts)
            {
                return Err(CallFailure::NoTransport(
                    "the publication is gone".to_owned(),
                ));
            }
            self.sent.lock().unwrap().push(samples[0]);
            Ok(())
        }
    }

    fn frame(nth: i16, open: bool) -> OutgoingFrame {
        OutgoingFrame {
            samples: vec![nth; 4],
            open,
        }
    }

    /// Push `frames` at a track and report the frames it was handed.
    async fn through(track: &FakeTrack, frames: Vec<OutgoingFrame>) -> Vec<i16> {
        for frame in frames {
            let OutgoingFrame { samples, .. } = frame;
            track.send(samples).await.unwrap();
        }
        track.sent()
    }

    #[tokio::test]
    async fn every_frame_reaches_the_publication_in_order() {
        let track = FakeTrack::new();

        let sent = through(&track, vec![frame(1, true), frame(2, true), frame(3, true)]).await;

        assert_eq!(sent, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn a_shut_gate_does_not_stop_the_frames() {
        // Opus collapses the silence on the wire, and the alternative is a
        // sender that has apparently stopped.
        let track = FakeTrack::new();

        let sent = through(&track, vec![frame(1, false), frame(2, false)]).await;

        assert_eq!(sent, vec![1, 2]);
    }

    #[tokio::test]
    async fn the_gate_opening_and_shutting_changes_nothing_about_the_publication() {
        // The regression this exists for. Every one of these edges used to be
        // a transport mute broadcast to every peer, so a person talking
        // normally flipped everybody else's mute indicator several times in
        // ten seconds, and tore down the audio path on the way past.
        let track = FakeTrack::new();

        let sent = through(
            &track,
            (0..20)
                .map(|nth| frame(nth, nth % 2 == 0))
                .collect::<Vec<_>>(),
        )
        .await;

        assert_eq!(sent, (0..20).collect::<Vec<i16>>());
    }

    #[tokio::test]
    async fn the_pump_stops_when_the_publication_stops_accepting_audio() {
        let track = FakeTrack::accepting(2);
        let microphone = Microphone::new();

        // Three, not ten. The queue holds `QUEUE_FRAMES` and drops the oldest
        // to make room, so offering more than it holds tests the queue rather
        // than the pump.
        for nth in 0..3 {
            microphone.offer(&[nth; 4], true);
        }

        pump(track.clone(), microphone).await;

        assert_eq!(
            track.sent(),
            vec![0, 1],
            "the pump kept pushing after a refusal"
        );
    }

    #[tokio::test]
    async fn the_pump_carries_shut_frames_as_readily_as_open_ones() {
        // `pump` returns only when a send is refused, so the fourth frame is
        // the brake. Without it this waits on an empty queue forever, which is
        // correct behaviour and a hanging test.
        let track = FakeTrack::accepting(3);
        let microphone = Microphone::new();

        microphone.offer(&[1; 4], true);
        microphone.offer(&[2; 4], false);
        microphone.offer(&[3; 4], true);
        microphone.offer(&[4; 4], true);

        pump(track.clone(), microphone).await;

        assert_eq!(
            track.sent(),
            vec![1, 2, 3],
            "the gate's verdict changed which frames were carried"
        );
    }
}
