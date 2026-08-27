// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Putting the gate's output into the call.
//!
//! ## Silence is published, not withheld
//!
//! When the gate shuts it fills the frame with zeroes and the frame still goes
//! out. A sender that simply stops sending is indistinguishable from a client
//! that has wedged, and that is what a peer's interface will show. Opus
//! collapses silence on the wire, so the frames cost close to nothing.
//!
//! ## And the publication is muted as well
//!
//! Publishing silence says "still here". It does not say "deliberately quiet",
//! and a peer cannot tell the two apart from the audio alone.
//! `LocalTrackHandle::set_muted` is the signal for that, and it is what lets a
//! peer's interface show somebody as not talking rather than as broken.
//!
//! Both, then: mute on the way into silence, and keep the frames flowing
//! underneath it. The mute is set immediately before the first frame it
//! applies to, so a peer never receives voice while believing the publication
//! muted, nor the reverse.
//!
//! This is where the voice activity switch in `GateConfig` stops being a
//! demonstration and becomes policy, and it needs nothing here to do it: with
//! it off the gate reports every frame open, so there is never a closing edge
//! and this never mutes anything.

use crate::failure::CallFailure;
use crate::microphone::{Microphone, OutgoingFrame};

/// A live microphone publication in a call.
///
/// The seam that keeps everything above it testable, the same way
/// [`crate::CallTransport`] does for joining. The real one is an
/// `Arc<dyn LocalTrackHandle>` and lives in [`crate::livekit`].
#[allow(
    async_fn_in_trait,
    reason = "the returned future is awaited only on the call thread, which is \
              single-threaded by construction, so a Send bound would be a \
              promise nothing needs"
)]
pub trait PublishedAudio: 'static {
    /// Mute or unmute this publication at the transport.
    ///
    /// Distinct from sending silence: this is what a peer's interface reads.
    fn set_muted(&self, muted: bool) -> Result<(), CallFailure>;

    /// Push one frame of mono 48 kHz PCM.
    ///
    /// Paced by the transport, which is the only pacing in the whole chain:
    /// it resolves once the frame has been accepted.
    async fn send(&self, samples: Vec<i16>) -> Result<(), CallFailure>;
}

/// A publication, and what its peers currently believe about it.
struct Publication<P> {
    track: P,
    /// What was last asked of the transport. A publication starts unmuted, so
    /// that is where this starts, and the first shut frame is what mutes it.
    muted: bool,
}

impl<P: PublishedAudio> Publication<P> {
    fn new(track: P) -> Self {
        Self {
            track,
            muted: false,
        }
    }

    /// Send one frame, changing the mute state first if this frame needs it.
    ///
    /// First, so the transport is never carrying voice under a mute it has
    /// been told about, and never carrying silence that a peer reads as a
    /// stalled sender.
    async fn send(&mut self, frame: OutgoingFrame) -> Result<(), CallFailure> {
        let wanted = !frame.open;
        if wanted != self.muted {
            if let Err(error) = self.track.set_muted(wanted) {
                // Not fatal, and not retried on the next frame either. A
                // transport that cannot mute leaves a peer's indicator wrong,
                // which is a much smaller thing than dropping the audio over
                // it, and asking again a hundred times a second would only
                // fill the log. The next edge asks again.
                tracing::warn!(%error, wanted, "the publication could not be muted");
            }
            self.muted = wanted;
        }

        self.track.send(frame.samples).await
    }
}

/// Feed `microphone` into `track` until the publication stops accepting audio.
///
/// Returns only when the call is over. It is otherwise ended by being aborted,
/// which is what the call thread does when it leaves: this waits on a queue
/// that a microphone which has been switched off will never fill again.
pub async fn pump<P: PublishedAudio>(track: P, microphone: Microphone) {
    let mut publication = Publication::new(track);

    loop {
        let frame = microphone.next().await;
        if let Err(error) = publication.send(frame).await {
            tracing::warn!(%error, "the microphone publication stopped accepting audio");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// What the fake publication was asked to do, in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Asked {
        Muted(bool),
        Sent(i16),
    }

    #[derive(Clone, Default)]
    struct FakeTrack {
        asked: Arc<Mutex<Vec<Asked>>>,
        /// How many frames this accepts before refusing the rest.
        accepts: Option<usize>,
        /// Whether it can be muted at all.
        mutable: bool,
    }

    impl FakeTrack {
        fn new() -> Self {
            Self {
                mutable: true,
                ..Self::default()
            }
        }

        fn accepting(frames: usize) -> Self {
            Self {
                accepts: Some(frames),
                ..Self::new()
            }
        }

        fn that_cannot_be_muted() -> Self {
            Self {
                mutable: false,
                ..Self::new()
            }
        }

        fn asked(&self) -> Vec<Asked> {
            self.asked.lock().unwrap().clone()
        }

        fn sent(&self) -> usize {
            self.asked()
                .iter()
                .filter(|asked| matches!(asked, Asked::Sent(_)))
                .count()
        }
    }

    impl PublishedAudio for FakeTrack {
        fn set_muted(&self, muted: bool) -> Result<(), CallFailure> {
            // Recorded before the refusal, so a test can count the attempts
            // and not just the ones that worked.
            self.asked.lock().unwrap().push(Asked::Muted(muted));
            if self.mutable {
                return Ok(());
            }
            Err(CallFailure::NoTransport(
                "this publication cannot be muted".to_owned(),
            ))
        }

        async fn send(&self, samples: Vec<i16>) -> Result<(), CallFailure> {
            if self.accepts.is_some_and(|accepts| self.sent() >= accepts) {
                return Err(CallFailure::NoTransport(
                    "the publication is gone".to_owned(),
                ));
            }
            self.asked.lock().unwrap().push(Asked::Sent(samples[0]));
            Ok(())
        }
    }

    fn frame(nth: i16, open: bool) -> OutgoingFrame {
        OutgoingFrame {
            samples: vec![nth; 4],
            open,
        }
    }

    /// Push `frames` through a publication and report what the track was asked.
    async fn through(track: &FakeTrack, frames: Vec<OutgoingFrame>) -> Vec<Asked> {
        let mut publication = Publication::new(track.clone());
        for frame in frames {
            publication.send(frame).await.unwrap();
        }
        track.asked()
    }

    #[tokio::test]
    async fn every_frame_reaches_the_publication_in_order() {
        let track = FakeTrack::new();

        let asked = through(&track, vec![frame(1, true), frame(2, true), frame(3, true)]).await;

        assert_eq!(asked, vec![Asked::Sent(1), Asked::Sent(2), Asked::Sent(3)]);
    }

    #[tokio::test]
    async fn the_gate_closing_mutes_the_publication() {
        // Not merely stopping the frames. A sender that stops sending is what
        // a wedged client looks like; muting is what says it is deliberate.
        let track = FakeTrack::new();

        let asked = through(&track, vec![frame(1, true), frame(2, false)]).await;

        assert_eq!(
            asked,
            vec![Asked::Sent(1), Asked::Muted(true), Asked::Sent(2)]
        );
    }

    #[tokio::test]
    async fn the_mute_lands_before_the_frame_it_applies_to() {
        // Either way round is a moment where the transport and the peer
        // disagree: voice under a mute, or silence that reads as a stall.
        let track = FakeTrack::new();

        let asked = through(&track, vec![frame(1, false), frame(2, true)]).await;

        assert_eq!(
            asked,
            vec![
                Asked::Muted(true),
                Asked::Sent(1),
                Asked::Muted(false),
                Asked::Sent(2),
            ]
        );
    }

    #[tokio::test]
    async fn silence_is_still_published_while_muted() {
        // Opus collapses it on the wire, and the alternative is a sender that
        // has apparently stopped.
        let track = FakeTrack::new();

        let asked = through(&track, vec![frame(1, false), frame(2, false)]).await;

        assert_eq!(
            asked,
            vec![Asked::Muted(true), Asked::Sent(1), Asked::Sent(2)],
            "a shut gate must not stop the frames"
        );
    }

    #[tokio::test]
    async fn a_publication_is_muted_once_rather_than_on_every_silent_frame() {
        // A hundred frames a second, each one a signalling round trip to every
        // peer, for a person who has simply stopped talking.
        let track = FakeTrack::new();

        let asked = through(
            &track,
            (0..20).map(|nth| frame(nth, false)).collect::<Vec<_>>(),
        )
        .await;

        let mutes = asked
            .iter()
            .filter(|a| matches!(a, Asked::Muted(_)))
            .count();
        assert_eq!(mutes, 1, "{asked:?}");
    }

    #[tokio::test]
    async fn a_gate_that_never_shuts_never_mutes_anything() {
        // Voice activity turned off. The gate reports every frame open, so
        // there is no closing edge and nothing here has to know about the
        // setting at all.
        let track = FakeTrack::new();

        let asked = through(
            &track,
            (0..20).map(|nth| frame(nth, true)).collect::<Vec<_>>(),
        )
        .await;

        assert!(
            asked.iter().all(|a| matches!(a, Asked::Sent(_))),
            "{asked:?}"
        );
    }

    #[tokio::test]
    async fn a_publication_that_cannot_be_muted_still_carries_the_audio() {
        // Losing a peer's mute indicator is a much smaller thing than losing
        // the person's voice over it.
        let track = FakeTrack::that_cannot_be_muted();

        let asked = through(&track, vec![frame(1, false), frame(2, true)]).await;

        assert_eq!(
            asked,
            vec![
                Asked::Muted(true),
                Asked::Sent(1),
                Asked::Muted(false),
                Asked::Sent(2),
            ]
        );
    }

    #[tokio::test]
    async fn a_publication_that_will_not_mute_is_not_asked_again_every_frame() {
        // Asking a hundred times a second would fill the log with a failure
        // nothing is going to fix. The next edge asks again; the frames in
        // between do not.
        let track = FakeTrack::that_cannot_be_muted();

        let asked = through(
            &track,
            (0..10).map(|nth| frame(nth, false)).collect::<Vec<_>>(),
        )
        .await;

        let attempts = asked
            .iter()
            .filter(|a| matches!(a, Asked::Muted(_)))
            .count();
        assert_eq!(attempts, 1, "{asked:?}");
        assert_eq!(track.sent(), 10);
    }

    #[tokio::test]
    async fn the_pump_stops_when_the_publication_goes_away() {
        // The call ended under it. Looping on a track that will never accept
        // another frame would spin for as long as the microphone is open.
        let track = FakeTrack::accepting(2);
        let microphone = Microphone::new();
        for nth in 0..3 {
            microphone.offer(&[nth; 4], true);
        }

        pump(track.clone(), microphone).await;

        assert_eq!(track.sent(), 2, "{:?}", track.asked());
    }
}
