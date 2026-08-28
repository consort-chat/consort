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

use consort_audio::Voices;
use consort_call::hearing::{Ears, Heard};

/// The mixer, as somewhere a call can be played.
struct Speakers(Voices);

/// Point a call at `voices`.
///
/// A free function rather than `Speakers::new`, because what a caller wants is
/// the trait object and a `new` that does not return `Self` is a surprise
/// worth avoiding. Nothing outside this module needs the type itself.
pub fn speakers(voices: Voices) -> Ears {
    Arc::new(Speakers(voices))
}

impl Heard for Speakers {
    fn hear(&self, who: &str, samples: &[i16]) {
        self.0.hear(who, samples);
    }

    fn forget(&self, who: &str) {
        self.0.forget(who);
    }

    fn silence(&self) {
        self.0.silence();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let ears = speakers(voices.clone());

        ears.hear("alice", &[1, 2, 3]);

        assert_eq!(played(&voices, 3), vec![1, 2, 3]);
    }

    #[test]
    fn forgetting_somebody_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = speakers(voices.clone());
        ears.hear("alice", &[9, 9]);

        ears.forget("alice");

        assert!(voices.everyone().is_empty());
    }

    #[test]
    fn silencing_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = speakers(voices.clone());
        ears.hear("alice", &[9, 9]);
        ears.hear("bob", &[9, 9]);

        ears.silence();

        assert_eq!(played(&voices, 2), vec![0, 0]);
    }
}
