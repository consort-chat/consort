// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Getting everybody else's audio out of the call and into the speakers.
//!
//! A subscribed track is a decoder writing PCM into a stream, and a stream
//! nobody reads is frames decoded and dropped, with nothing anywhere reporting
//! it: `docs/adr/0002-consort-mixes-the-call-itself.md`. One task per
//! participant, started and stopped from the roster, which already knows when a
//! stream appears and when somebody leaves. [`Ears`] is a trait because what is
//! on the other end is a sound card, and this crate must not know that.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use matrix_rtc_call_sdk::Participant as MediaParticipant;
use matrix_rtc_transport::MediaStreamKind;

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

    /// Make whatever sound goes with something happening in the call.
    ///
    /// A request rather than samples, because this crate must never link an
    /// audio backend: what the sound actually is belongs to `consort-audio`,
    /// and the seam that already exists for playing a call is the right place
    /// to hand this over too.
    ///
    /// Must not block, like everything else here. It is called from the call
    /// thread while it is servicing the SFU.
    fn cue(&self, cue: Cue);

    /// Say which person each membership currently in the call belongs to, as
    /// `(member_id, user_id)`.
    ///
    /// The one thing the other side of the seam cannot work out for itself:
    /// everything above is keyed by membership while a person's settings are
    /// keyed by them, and this is the only place both are known. Called on
    /// every roster change and idempotent, a statement of who is in the call
    /// rather than a delta. Must not block.
    fn attribute(&self, whose: &[(String, String)]);
}

/// Something in a call worth making a sound about.
///
/// What happened, never what to play: the two settings on the other side of the
/// seam may make a cue either, both or neither. Only what this session's own
/// membership can be sure of, and deliberately no "somebody muted", because a
/// channel that pinged for every mute is a channel people turn the sounds off
/// in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// Somebody joined the voice channel this session is in.
    Arrived,
    /// Somebody left it.
    Departed,
    /// This session stopped being away.
    ///
    /// The one cue that is about us rather than about anybody else, and the
    /// only one with no chime behind it: "welcome back" is a sentence or it is
    /// nothing. It also does not come from the roster, because nobody else's
    /// roster changed. It comes from the moment the flag is put down.
    Returned,
}

/// A shared handle on somewhere to play a call.
pub type Ears = Arc<dyn Heard>;

/// What one pass of [`crate::CallSession::listen`] or
/// [`crate::CallSession::watch`] managed.
///
/// `pending` is the roster calling a stream live while the transport has no
/// track for it yet, which is the ordinary order of events rather than a
/// failure. It is reported because nothing else will necessarily ask again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attached {
    /// Streams now being pumped.
    pub playing: usize,
    /// Streams the roster calls live that have no track yet.
    pub pending: usize,
}

impl Attached {
    /// The audio and the pictures of one pass, as the one answer the chase is
    /// armed from. Either half left short is a stream nobody is pulling.
    pub fn and(self, other: Self) -> Self {
        Self {
            playing: self.playing + other.playing,
            pending: self.pending + other.pending,
        }
    }
}

/// What separates a share's queue key from its sender's own voice.
///
/// A space, because a `member_id` is a MatrixRTC membership id and carries
/// none, so no voice key can collide with a share one.
const SHARE: &str = " share";

/// Which of the two sounds somebody in a call can be sending.
///
/// Both at once is ordinary: somebody sharing a screen talks over it.
/// [`crate::watching::Kind`] is the same shape for the two pictures.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Sound {
    /// Their microphone.
    Voice,
    /// What the machine they are sharing is playing.
    Share,
}

impl Sound {
    /// What a transport calls this stream.
    pub fn stream(self) -> MediaStreamKind {
        match self {
            Self::Voice => MediaStreamKind::Microphone,
            Self::Share => MediaStreamKind::ScreenShareAudio,
        }
    }

    /// The key `member_id`'s audio of this kind is queued and mixed under.
    ///
    /// Two keys rather than one, because a mixer queue is a jitter buffer and
    /// two streams interleaved into one play as noise.
    pub fn queue(self, member_id: &str) -> String {
        match self {
            Self::Voice => member_id.to_owned(),
            Self::Share => format!("{member_id}{SHARE}"),
        }
    }
}

/// Everybody in `participants` whose `sound` we should be playing.
///
/// Our own membership is excluded, and not as an optimisation: an SFU does not
/// send us our own audio, so a task waiting on it would wait forever, and if
/// one ever did arrive it would be the caller hearing themselves a third of a
/// second late. For a share that is also the feedback loop, since the sound it
/// carries came out of these speakers.
///
/// A muted participant is included. Their frames simply stop, and being already
/// attached is what makes unmuting instant instead of costing a roster round
/// trip before the first word is heard.
pub fn audible(participants: &[MediaParticipant], sound: Sound) -> BTreeSet<String> {
    let wanted = sound.stream();
    participants
        .iter()
        .filter(|member| !member.is_local)
        .filter(|member| member.streams.iter().any(|stream| stream.kind == wanted))
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
    use matrix_rtc_call_sdk::ConnectionState;
    use matrix_rtc_call_sdk::StreamState;

    fn member(member_id: &str, is_local: bool, kinds: &[MediaStreamKind]) -> MediaParticipant {
        MediaParticipant {
            member_id: member_id.to_owned(),
            user_id: format!("@{member_id}:example.org"),
            device_id: None,
            is_local,
            connection: ConnectionState::Connected,
            unable_to_decrypt: None,
            hand_raised_position: None,
            hand_raised_at_ms: None,
            joined_at_ms: None,
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
        assert_eq!(audible(&[speaking("alice")], Sound::Voice), set(&["alice"]));
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

        assert_eq!(audible(&people, Sound::Voice), set(&["alice"]));
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

        assert!(audible(&people, Sound::Voice).is_empty());
    }

    #[test]
    fn a_muted_participant_stays_attached() {
        // Their frames stop on their own. Detaching would make unmuting cost a
        // roster round trip before the first word of it could be heard.
        let mut muted = speaking("alice");
        muted.streams[0].muted = true;

        assert_eq!(audible(&[muted], Sound::Voice), set(&["alice"]));
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
    fn somebody_sharing_a_screen_with_sound_is_audible_as_a_share() {
        let people = vec![member(
            "alice",
            false,
            &[
                MediaStreamKind::Microphone,
                MediaStreamKind::ScreenShareAudio,
            ],
        )];

        assert_eq!(audible(&people, Sound::Share), set(&["alice"]));
    }

    #[test]
    fn a_share_without_sound_is_not_waited_on() {
        // A screen share publishes its audio as a track of its own, so a
        // picture arriving is no promise that any sound will.
        let people = vec![member(
            "alice",
            false,
            &[MediaStreamKind::Microphone, MediaStreamKind::ScreenShare],
        )];

        assert!(audible(&people, Sound::Share).is_empty());
    }

    #[test]
    fn a_share_with_sound_but_no_microphone_is_still_heard() {
        // Sharing a video with the microphone off. The two streams are
        // independent and either may be the only one.
        let people = vec![member("alice", false, &[MediaStreamKind::ScreenShareAudio])];

        assert!(audible(&people, Sound::Voice).is_empty());
        assert_eq!(audible(&people, Sound::Share), set(&["alice"]));
    }

    #[test]
    fn our_own_share_is_never_played_back_to_us() {
        // The feedback loop this track exists to avoid: our own share audio
        // arriving back through the mixer and out of the speakers it was
        // captured from.
        let people = vec![member("me", true, &[MediaStreamKind::ScreenShareAudio])];

        assert!(audible(&people, Sound::Share).is_empty());
    }

    #[test]
    fn each_sound_names_the_stream_it_pulls() {
        assert_eq!(Sound::Voice.stream(), MediaStreamKind::Microphone);
        assert_eq!(Sound::Share.stream(), MediaStreamKind::ScreenShareAudio);
    }

    #[test]
    fn a_share_is_queued_apart_from_its_senders_voice() {
        // One queue is a jitter buffer. Two streams in it interleave into
        // noise, so a share cannot share its sender's key.
        assert_eq!(Sound::Voice.queue("alice"), "alice");
        assert_ne!(Sound::Share.queue("alice"), Sound::Voice.queue("alice"));
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
