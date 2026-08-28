// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Getting everybody else's audio out of the call and into the speakers.
//!
//! ## The bug this exists because of
//!
//! Subscribing to a track is not the same as playing it. In a browser it very
//! nearly is, because the LiveKit SDK attaches a subscribed track to an
//! `<audio>` element and the page makes noise; every native binding leaves that
//! to the application. A subscribed track here is a decoder writing PCM into a
//! stream, and a stream nobody reads is frames decoded and dropped.
//!
//! Nothing reports it. The membership is published, the SFU is connected, the
//! keys are exchanged, the frames decrypt, the roster fills in, and the call is
//! silent. Consort shipped exactly that: the outgoing half was wired end to end
//! and the incoming half stopped at the subscription, so everybody could hear
//! us and we could hear nobody.
//!
//! ## The shape
//!
//! One task per participant, each pulling that participant's frames and
//! handing them to [`Ears`]. They are started and stopped from the roster,
//! because the roster is the one thing that already knows when somebody's
//! stream appears and when they leave, and it is already consulted on every
//! change for the sake of deafening.
//!
//! [`Ears`] is a trait rather than a channel because the thing on the other end
//! is a sound card, and this crate must not know that. It is the mirror of
//! [`crate::PublishedAudio`], which is a trait for the same reason going the
//! other way.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use matrix_rtc_media::{MediaStreamKind, Participant as MediaParticipant};

/// Somewhere to play what the other people in a call are saying.
///
/// `Send + Sync` because one is shared by every pump task at once, and
/// `'static` because those tasks outlive the call that started them by however
/// long it takes them to notice.
pub trait Heard: Send + Sync + 'static {
    /// Play `samples`, mono PCM at 48 kHz, as `who`.
    ///
    /// Must not block. It is called from the call thread, which is also
    /// servicing the SFU, and a sound card that made it wait would stall the
    /// call rather than the other way round.
    fn hear(&self, who: &str, samples: &[i16]);

    /// `who` has stopped sending; drop whatever of theirs is still queued.
    fn forget(&self, who: &str);

    /// Drop everything queued for everybody.
    ///
    /// What deafening needs. Pausing the subscriptions stops more arriving, but
    /// that travels to the SFU and back, and without this the audio already in
    /// the buffer plays out underneath somebody who has just asked for quiet.
    fn silence(&self);
}

/// A shared handle on somewhere to play a call.
pub type Ears = Arc<dyn Heard>;

/// Everybody in `participants` whose audio we should be playing.
///
/// Our own membership is excluded, and not as an optimisation: an SFU does not
/// send us our own audio, so a task waiting on it would wait forever, and if
/// one ever did arrive it would be the caller hearing themselves a third of a
/// second late.
///
/// A muted participant is included. Their frames simply stop, and being already
/// attached is what makes unmuting instant instead of costing a roster round
/// trip before the first word is heard.
pub fn audible(participants: &[MediaParticipant]) -> BTreeSet<String> {
    participants
        .iter()
        .filter(|member| !member.is_local)
        .filter(|member| {
            member
                .streams
                .iter()
                .any(|stream| stream.kind == MediaStreamKind::Microphone)
        })
        .map(|member| member.member_id.clone())
        .collect()
}

/// Who to start playing, and who to stop, to get from `attached` to `audible`.
///
/// Its own function so that the answer is a value a test can look at. Doing the
/// difference inline would leave the one rule that matters (that somebody
/// already attached is left strictly alone) as an implicit property of the loop
/// that happens to implement it.
pub fn changes(
    attached: &BTreeSet<String>,
    audible: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    let start = audible.difference(attached).cloned().collect();
    let stop = attached.difference(audible).cloned().collect();
    (start, stop)
}

/// One frame's samples as mono, downmixing if they arrived interleaved.
///
/// The transport is asked for one channel and gives one channel, so the borrow
/// is what actually happens. The fold exists because getting this wrong is not
/// a crash: treating two interleaved channels as mono plays the call at double
/// speed and drains the queue twice as fast, which sounds like a bad connection
/// rather than like a bug here.
pub fn mono(samples: &[i16], channels: u32) -> Cow<'_, [i16]> {
    if channels <= 1 {
        return Cow::Borrowed(samples);
    }

    let channels = channels as usize;
    Cow::Owned(
        samples
            .chunks(channels)
            .map(|group| {
                // Averaged in a width that cannot wrap, then divided, rather
                // than summed into an `i16` first.
                let total: i32 = group.iter().copied().map(i32::from).sum();
                (total / group.len() as i32) as i16
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_rtc_media::StreamState;

    fn member(member_id: &str, is_local: bool, kinds: &[MediaStreamKind]) -> MediaParticipant {
        MediaParticipant {
            member_id: member_id.to_owned(),
            user_id: format!("@{member_id}:example.org"),
            device_id: None,
            is_local,
            reachable: true,
            streams: kinds
                .iter()
                .map(|kind| StreamState {
                    kind: *kind,
                    muted: false,
                })
                .collect(),
        }
    }

    fn speaking(member_id: &str) -> MediaParticipant {
        member(member_id, false, &[MediaStreamKind::Microphone])
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn somebody_with_a_microphone_is_audible() {
        assert_eq!(audible(&[speaking("alice")]), set(&["alice"]));
    }

    #[test]
    fn our_own_membership_is_never_played_back_to_us() {
        // An SFU does not send us our own audio, so a task waiting on it waits
        // forever, and anything that did arrive would be us hearing ourselves
        // a third of a second late.
        let people = vec![
            member("me", true, &[MediaStreamKind::Microphone]),
            speaking("alice"),
        ];

        assert_eq!(audible(&people), set(&["alice"]));
    }

    #[test]
    fn somebody_publishing_no_microphone_is_not_waited_on() {
        // Screen sharing without a microphone, or a membership that has been
        // signalled but has not published anything yet. Attaching would open a
        // frame stream that never produces one.
        let people = vec![
            member("alice", false, &[MediaStreamKind::ScreenShare]),
            member("bob", false, &[]),
        ];

        assert!(audible(&people).is_empty());
    }

    #[test]
    fn a_muted_participant_stays_attached() {
        // Their frames stop on their own. Detaching would make unmuting cost a
        // roster round trip before the first word of it could be heard.
        let mut muted = speaking("alice");
        muted.streams[0].muted = true;

        assert_eq!(audible(&[muted]), set(&["alice"]));
    }

    #[test]
    fn somebody_new_is_started_and_nobody_else_is_disturbed() {
        let (start, stop) = changes(&set(&["alice"]), &set(&["alice", "bob"]));

        assert_eq!(start, vec!["bob".to_owned()]);
        assert!(stop.is_empty(), "alice was left alone: {stop:?}");
    }

    #[test]
    fn somebody_who_left_is_stopped() {
        let (start, stop) = changes(&set(&["alice", "bob"]), &set(&["alice"]));

        assert!(start.is_empty());
        assert_eq!(stop, vec!["bob".to_owned()]);
    }

    #[test]
    fn a_roster_that_has_not_changed_asks_for_nothing() {
        // This runs on every roster change of every kind, most of which have
        // nothing to do with audio. Tearing a working audio path down and
        // building it back up on each one would be audible.
        let (start, stop) = changes(&set(&["alice"]), &set(&["alice"]));

        assert!(start.is_empty());
        assert!(stop.is_empty());
    }

    #[test]
    fn mono_frames_are_passed_straight_through() {
        let samples = [1i16, 2, 3];

        assert_eq!(mono(&samples, 1), Cow::Borrowed(&samples[..]));
        assert!(matches!(mono(&samples, 1), Cow::Borrowed(_)));
    }

    #[test]
    fn a_frame_claiming_no_channels_is_not_divided_by_zero() {
        let samples = [1i16, 2];

        assert_eq!(mono(&samples, 0), Cow::Borrowed(&samples[..]));
    }

    #[test]
    fn interleaved_stereo_is_averaged_rather_than_played_at_double_speed() {
        // Reading it as mono would drain the queue twice as fast and pitch the
        // whole call up, which sounds like a bad connection rather than a bug.
        let samples = [10i16, 20, 30, 40];

        assert_eq!(mono(&samples, 2).as_ref(), &[15, 35]);
    }

    #[test]
    fn downmixing_loud_stereo_does_not_wrap() {
        let samples = [30_000i16, 30_000];

        assert_eq!(mono(&samples, 2).as_ref(), &[30_000]);
    }
}
