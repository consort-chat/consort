// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Where a call's audio goes: the mixer, behind the call layer's trait.
//!
//! Two crates that must not know about each other meet here. `consort-audio`
//! owns sound cards and knows nothing about MatrixRTC; `consort-call` owns
//! calls and must never link a sound backend, or `cargo test -p consort-call`
//! would need one. So the call layer declares [`Heard`] and this is the one
//! place that says which thing implements it.
//!
//! A newtype rather than an implementation on [`Voices`] directly, because
//! neither the trait nor the type belongs to this crate and Rust will not allow
//! the impl anywhere else. That constraint happens to be the right shape
//! anyway: it leaves exactly one seam to look at when a call is silent.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use consort_audio::{Sound, Voices};
use consort_call::hearing::{Chime, Ears, Heard};

/// The mixer, as somewhere a call can be played.
struct Speakers {
    voices: Voices,
    chiming: Wanted,
}

/// Whether the join and leave sounds are switched on.
///
/// An atomic rather than a read of the settings file, because this is asked on
/// the call thread while it is servicing the SFU and the answer changes from a
/// different thread entirely, whenever somebody saves the settings. A file read
/// per arrival would be a disk touch in the middle of a call for a boolean.
pub type Wanted = Arc<AtomicBool>;

/// Point a call at `voices`.
///
/// A free function rather than `Speakers::new`, because what a caller wants is
/// the trait object and a `new` that does not return `Self` is a surprise
/// worth avoiding. Nothing outside this module needs the type itself.
pub fn speakers(voices: Voices, chiming: Wanted) -> Ears {
    Arc::new(Speakers { voices, chiming })
}

impl Heard for Speakers {
    fn hear(&self, who: &str, samples: &[i16]) {
        self.voices.hear(who, samples);
    }

    fn forget(&self, who: &str) {
        self.voices.forget(who);
    }

    fn silence(&self) {
        self.voices.silence();
    }

    fn chime(&self, chime: Chime) {
        // Checked here rather than at the diff, so that turning the sounds off
        // stops the noise and nothing else: who is in the call is still
        // tracked, and turning them back on mid-call starts working
        // immediately rather than at the next join.
        if !self.chiming.load(Ordering::Relaxed) {
            return;
        }

        // Into the call's own mixer rather than through `AudioPlayback::play`.
        // That would open a second stream on the same device, which is fine
        // once for the settings-screen test tone and is not fine several times
        // an evening under a live call: opening a device costs tens of
        // milliseconds and this is exactly the moment not to spend them.
        self.voices.play(match chime {
            Chime::Arrived => Sound::Joined.samples(),
            Chime::Departed => Sound::Left.samples(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sounds switched on, which is what somebody who never chose has.
    fn on() -> Wanted {
        Arc::new(AtomicBool::new(true))
    }

    /// One buffer's worth of what the mixer would play.
    fn played(voices: &Voices, frames: usize) -> Vec<i16> {
        let mut mixing = consort_audio::Mixing::new(voices.clone(), 1);
        let mut out = vec![0i16; frames];
        mixing.fill_i16(&mut out);
        out
    }

    #[test]
    fn what_a_call_says_reaches_the_mixer() {
        // The whole point of the seam. Every layer under this reported success
        // while the audio went nowhere, so the one thing worth pinning is that
        // a frame handed to the trait comes back out of the sound card's end.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on());

        ears.hear("alice", &[1, 2, 3]);

        assert_eq!(played(&voices, 3), vec![1, 2, 3]);
    }

    #[test]
    fn forgetting_somebody_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on());
        ears.hear("alice", &[9, 9]);

        ears.forget("alice");

        assert!(voices.everyone().is_empty());
    }

    #[test]
    fn an_arrival_reaches_the_mixer() {
        // The seam the whole sound feature hangs off. `consort-call` knows
        // somebody arrived and must never link an audio backend;
        // `consort-audio` owns the sound and knows nothing about calls. If
        // this does not connect them nothing does, and the symptom is a
        // channel that is merely quiet.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on());

        ears.chime(Chime::Arrived);

        assert!(voices.sound_waiting() > 0, "nothing was queued to play");
    }

    #[test]
    fn arriving_and_leaving_queue_different_sounds() {
        let arriving = Voices::new();
        speakers(arriving.clone(), on()).chime(Chime::Arrived);
        let leaving = Voices::new();
        speakers(leaving.clone(), on()).chime(Chime::Departed);

        assert_eq!(
            arriving.sound_waiting(),
            leaving.sound_waiting(),
            "the two shipped sounds are the same length, so this comparison \
             is about which samples were queued rather than how many"
        );
        assert_ne!(
            played(&arriving, 4096),
            played(&leaving, 4096),
            "arriving and leaving queued the same audio"
        );
    }

    #[test]
    fn a_chime_does_not_disturb_whoever_is_talking() {
        // It mixes with them rather than replacing them. Somebody arriving
        // mid-word must not cut the word.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on());
        ears.hear("alice", &[100, 100, 100, 100]);

        ears.chime(Chime::Arrived);

        let played = played(&voices, 4);
        assert!(
            played.iter().all(|sample| *sample != 0),
            "the chime silenced the person talking: {played:?}"
        );
    }

    #[test]
    fn turning_the_sounds_off_stops_them() {
        let voices = Voices::new();
        let ears = speakers(voices.clone(), Arc::new(AtomicBool::new(false)));

        ears.chime(Chime::Arrived);

        assert_eq!(
            voices.sound_waiting(),
            0,
            "a chime played while switched off"
        );
    }

    #[test]
    fn turning_them_back_on_works_without_rejoining() {
        // Read at each chime rather than captured when the call started, so
        // somebody who switches them on mid-call does not have to leave and
        // come back to find out whether it took.
        let voices = Voices::new();
        let wanted = Arc::new(AtomicBool::new(false));
        let ears = speakers(voices.clone(), wanted.clone());
        ears.chime(Chime::Arrived);

        wanted.store(true, Ordering::Relaxed);
        ears.chime(Chime::Arrived);

        assert!(voices.sound_waiting() > 0);
    }

    #[test]
    fn the_sounds_being_off_does_not_silence_the_call() {
        // Two different things. Somebody who turned the chimes off still wants
        // to hear the people.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), Arc::new(AtomicBool::new(false)));

        ears.hear("alice", &[1, 2, 3]);

        assert_eq!(played(&voices, 3), vec![1, 2, 3]);
    }

    #[test]
    fn silencing_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on());
        ears.hear("alice", &[9, 9]);
        ears.hear("bob", &[9, 9]);

        ears.silence();

        assert_eq!(played(&voices, 2), vec![0, 0]);
    }
}
