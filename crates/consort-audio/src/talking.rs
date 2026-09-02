// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Who is audible right now.
//!
//! ## Why this is measured here rather than asked of the SFU
//!
//! LiveKit reports active speakers, and Consort used to draw its green rings
//! from that. It answers a different question. Dominant-speaker detection is
//! built to pick out the one or two people worth putting on a video tile in a
//! meeting of thirty, so it is smoothed, it is thresholded against everybody
//! else in the room, and it is deliberately slow to change: three separate
//! judgements had to agree before a ring lit, and the last of them was made on
//! a machine across a network. In a channel of three people the result was a
//! ring you could only light by leaning into the microphone, and somebody
//! else's ring you might never see at all.
//!
//! Consort does not need to know who is dominant. It needs to know who is
//! audible, which is a question about samples, and every sample is already
//! here: ours on the way out and everybody else's on the way in.
//!
//! ## The same rule for everybody
//!
//! Both directions are measured on the frames that actually travel. Ours are
//! the gate's output, so they are zeroed while it is shut, and theirs have
//! already been through the gate on the sending machine. Nothing here consults
//! the gate's verdict directly, which is what keeps the picture honest in both
//! configurations: with voice activity switched on a ring follows the gate,
//! and with it switched off a ring follows the room, because that is what the
//! other person is hearing in each case.
//!
//! ## Counted, not clocked
//!
//! Frames arrive every 10 ms by construction, so the hold below is a count and
//! there is no `Instant` anywhere in this file. Same reasoning as [`crate::meter`]:
//! it makes the tests exact and it makes them fast.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::meter::peak_of;

/// How loud a frame has to be to count as somebody talking, as a fraction of
/// full scale.
///
/// Deliberately near the bottom. The question this answers is "is there
/// anything there", not "is this speech", because the gate has already made
/// the second judgement by the time these samples exist and making it twice is
/// how the rings got slow in the first place.
///
/// Not zero, though. Opus reconstructs silence as comfort noise rather than as
/// zeroes, so an exact test would light every ring in the call for as long as
/// anybody was connected.
pub const FLOOR: f32 = 0.005;

/// How many frames somebody stays lit after their last audible one.
///
/// Twenty frames is 200 ms. The gap between two words is longer than one frame
/// and shorter than this, so a ring holds across a sentence instead of
/// stuttering once per syllable, and still goes dark within a fifth of a second
/// of somebody actually stopping.
pub const HOLD_FRAMES: u16 = 20;

/// Who has been audible lately, and whether that has changed.
///
/// Keyed by whatever the caller uses to name a person. Consort keys it by
/// Matrix user ID, which is what makes somebody talking on a laptop and a
/// phone one lit ring rather than two.
///
/// Cheap to clone: every clone is the same tally. One goes to the audio
/// thread, which reports this session's own frames, and one to the mixer's
/// side, which reports everybody else's.
#[derive(Clone, Default)]
pub struct Talking(Arc<Mutex<Tally>>);

#[derive(Default)]
struct Tally {
    /// Everybody currently audible, and how many frames they have left before
    /// they are not. Somebody quiet is absent rather than present at zero.
    lit: BTreeMap<String, u16>,
    /// What [`Talking::advance`] last handed back, so it can stay silent while
    /// nothing changes.
    reported: Vec<String>,
}

impl Talking {
    /// Nobody, yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// `who` just produced `samples`.
    ///
    /// Mono PCM at [`crate::SAMPLE_RATE`], like everything else here. A frame
    /// under [`FLOOR`] says nothing: it neither lights somebody nor puts them
    /// out early, because the hold is what decides when they go dark.
    ///
    /// Called from both the audio thread and the call thread. Never blocks on
    /// anything but the tally itself.
    pub fn heard(&self, who: &str, samples: &[i16]) {
        if peak_of(samples) < FLOOR {
            return;
        }

        let mut tally = self.tally();
        // Asked about before being inserted into rather than through `entry`,
        // which would clone the key on every frame somebody is talking. This
        // runs a hundred times a second per person.
        match tally.lit.get_mut(who) {
            Some(left) => *left = HOLD_FRAMES,
            None => {
                tally.lit.insert(who.to_owned(), HOLD_FRAMES);
            }
        }
    }

    /// One frame's worth of time has passed.
    ///
    /// Hands back everybody currently audible, by whatever key they were heard
    /// under, but only when that differs from the last set it handed back.
    /// `None` is the ordinary answer: a settled call is somebody talking for
    /// several seconds, which is several hundred frames in which nothing about
    /// the rings changes.
    ///
    /// Driven by this session's own capture, because that is the one clock in
    /// the building that ticks whether or not anybody is saying anything.
    pub fn advance(&self) -> Option<Vec<String>> {
        let mut tally = self.tally();
        tally.lit.retain(|_, left| {
            *left -= 1;
            *left > 0
        });
        tally.settle()
    }

    /// Nobody is in a call any more.
    ///
    /// Hands back the empty set when somebody was lit, on the same terms as
    /// [`advance`](Self::advance). Needed because the tick is the microphone,
    /// and leaving a call stops the microphone: without this the last people
    /// talking would stay lit until the next call.
    pub fn quiet(&self) -> Option<Vec<String>> {
        let mut tally = self.tally();
        tally.lit.clear();
        tally.settle()
    }

    /// Recovering from a poisoned lock rather than spreading a panic, on the
    /// same terms as the mixer's: one of the callers is a realtime audio path
    /// and the worst a stale tally can do is draw a ring wrong.
    fn tally(&self) -> MutexGuard<'_, Tally> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Tally {
    /// The current set, if it is not the one already reported.
    fn settle(&mut self) -> Option<Vec<String>> {
        // Sorted, because a `BTreeMap`'s keys are, which is what makes this
        // comparison a comparison of sets rather than of orderings.
        let lit: Vec<String> = self.lit.keys().cloned().collect();
        if lit == self.reported {
            return None;
        }

        self.reported.clone_from(&lit);
        Some(lit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame loud enough to be somebody talking.
    fn loud() -> Vec<i16> {
        vec![8_000; crate::FRAME_SAMPLES]
    }

    /// A frame at the level Opus reconstructs silence at.
    fn comfort_noise() -> Vec<i16> {
        vec![7; crate::FRAME_SAMPLES]
    }

    /// A gated-shut frame, which is what this session publishes while quiet.
    fn silence() -> Vec<i16> {
        vec![0; crate::FRAME_SAMPLES]
    }

    /// Run `frames` ticks and hand back the last answer that was not `None`.
    fn after(talking: &Talking, frames: u16) -> Option<Vec<String>> {
        let mut last = None;
        for _ in 0..frames {
            if let Some(lit) = talking.advance() {
                last = Some(lit);
            }
        }
        last
    }

    #[test]
    fn one_audible_frame_lights_somebody() {
        // The whole point of the change. One frame is 10 ms, against the
        // several hundred the SFU's detector took to make up its mind.
        let talking = Talking::new();

        talking.heard("@ada:example.org", &loud());

        assert_eq!(talking.advance(), Some(vec!["@ada:example.org".to_owned()]));
    }

    #[test]
    fn somebody_lit_stays_lit_while_nothing_changes() {
        // `None` is the ordinary answer. A caller that emitted on every frame
        // would put a hundred messages a second across the IPC boundary to say
        // what the last one said.
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        talking.advance();

        talking.heard("@ada:example.org", &loud());

        assert_eq!(talking.advance(), None);
    }

    #[test]
    fn somebody_who_stops_goes_dark_after_the_hold() {
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        talking.advance();

        let lit = after(&talking, HOLD_FRAMES);

        assert_eq!(lit, Some(Vec::new()));
    }

    #[test]
    fn the_hold_carries_across_the_gap_between_two_words() {
        // Half the hold of quiet, which is longer than the pause between
        // syllables and shorter than the pause between sentences. A ring that
        // went out here would stutter through every word somebody said.
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        talking.advance();

        assert_eq!(after(&talking, HOLD_FRAMES / 2), None);
    }

    #[test]
    fn talking_again_before_the_hold_expires_restarts_it() {
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        after(&talking, HOLD_FRAMES / 2);

        talking.heard("@ada:example.org", &loud());

        assert_eq!(
            after(&talking, HOLD_FRAMES - 1),
            None,
            "the ring went out while they were still talking"
        );
    }

    #[test]
    fn comfort_noise_lights_nobody() {
        // Opus reconstructs silence rather than transmitting it, so the frames
        // a quiet participant decodes to are near zero and not zero. Testing
        // for exact silence would light every ring in the call for as long as
        // anybody was connected.
        let talking = Talking::new();

        talking.heard("@ada:example.org", &comfort_noise());

        assert_eq!(talking.advance(), None);
    }

    #[test]
    fn a_gated_shut_frame_lights_nobody() {
        // What this session publishes while the gate is closed. Metering the
        // frames that actually travel is what makes a lit ring mean "this is
        // reaching people" without anything here having to read the gate's
        // verdict.
        let talking = Talking::new();

        talking.heard("@ada:example.org", &silence());

        assert_eq!(talking.advance(), None);
    }

    #[test]
    fn two_people_talking_are_two_rings() {
        let talking = Talking::new();

        talking.heard("@ada:example.org", &loud());
        talking.heard("@bob:example.org", &loud());

        assert_eq!(
            talking.advance(),
            Some(vec![
                "@ada:example.org".to_owned(),
                "@bob:example.org".to_owned()
            ])
        );
    }

    #[test]
    fn one_person_going_quiet_does_not_disturb_the_other() {
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        talking.heard("@bob:example.org", &loud());
        talking.advance();

        for _ in 0..HOLD_FRAMES {
            talking.heard("@bob:example.org", &loud());
            talking.advance();
        }

        assert_eq!(talking.advance(), None, "bob was reported again");
    }

    #[test]
    fn somebody_on_two_devices_is_one_ring() {
        // Keyed by person rather than by membership, which is the reason the
        // caller resolves a member ID before it gets here. A laptop and a
        // phone in the same call are one human talking.
        let talking = Talking::new();

        talking.heard("@ada:example.org", &loud());
        talking.heard("@ada:example.org", &loud());

        assert_eq!(talking.advance(), Some(vec!["@ada:example.org".to_owned()]));
    }

    #[test]
    fn leaving_a_call_puts_everybody_out() {
        // The tick is this session's own microphone, and leaving stops it. Without
        // this the last people talking stay lit until the next call.
        let talking = Talking::new();
        talking.heard("@ada:example.org", &loud());
        talking.advance();

        assert_eq!(talking.quiet(), Some(Vec::new()));
    }

    #[test]
    fn leaving_a_call_nobody_was_talking_in_says_nothing() {
        let talking = Talking::new();

        assert_eq!(talking.quiet(), None);
    }

    #[test]
    fn a_tally_with_nobody_in_it_never_reports() {
        // An idle call still ticks a hundred times a second.
        let talking = Talking::new();

        assert_eq!(after(&talking, 100), None);
    }

    #[test]
    fn every_clone_is_the_same_tally() {
        // The two writers are on different threads: this session's frames come
        // from the audio thread and everybody else's from the call thread.
        let talking = Talking::new();
        let elsewhere = talking.clone();

        elsewhere.heard("@ada:example.org", &loud());

        assert_eq!(talking.advance(), Some(vec!["@ada:example.org".to_owned()]));
    }
}
