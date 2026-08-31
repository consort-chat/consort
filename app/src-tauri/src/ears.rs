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

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use consort_audio::{Phrase, Sound, Voices};
use consort_call::hearing::{Cue, Ears, Heard};

/// The mixer, as somewhere a call can be played.
struct Speakers {
    voices: Voices,
    chiming: Wanted,
    speaking: Wanted,
    levels: Arc<Levels>,
}

/// Whether one of the two kinds of call sound is switched on.
///
/// An atomic rather than a read of the settings file, because this is asked on
/// the call thread while it is servicing the SFU and the answer changes from a
/// different thread entirely, whenever somebody saves the settings. A file read
/// per arrival would be a disk touch in the middle of a call for a boolean.
pub type Wanted = Arc<AtomicBool>;

/// Point a call at `voices`.
///
/// `chiming` is the join and leave sounds, `speaking` the notifications that
/// say out loud what those only announce. Two handles rather than one, because
/// they are two settings: somebody who wants a chime and no sentence, and
/// somebody who wants the sentence and no chime before it, are both ordinary.
///
/// `levels` is how loud each person should be. It outlives any one call,
/// because what somebody chose about a person is not a fact about the call they
/// happened to choose it in.
///
/// A free function rather than `Speakers::new`, because what a caller wants is
/// the trait object and a `new` that does not return `Self` is a surprise
/// worth avoiding. Nothing outside this module needs the type itself.
pub fn speakers(voices: Voices, chiming: Wanted, speaking: Wanted, levels: Arc<Levels>) -> Ears {
    Arc::new(Speakers {
        voices,
        chiming,
        speaking,
        levels,
    })
}

/// What each person in a call should be played at.
///
/// Two facts meet here and neither is any use on its own.
///
/// The settings file says what somebody chose, by **user ID**, because "that
/// one is too loud in my headphones" is a fact about a person: it should
/// survive them rejoining, switching devices, and this application being shut
/// down for a week.
///
/// The call says who is in it, by **member ID**, because that is what a queue
/// of audio belongs to. One person on a laptop and a phone is two streams, and
/// mixing them into a single queue would interleave two conversations into
/// noise.
///
/// The mixer can only be handed the second kind of key. So both are kept here,
/// either one can change without the other, and every change republishes the
/// whole set rather than working out a difference. That is cheap in the only
/// size that occurs: a call is a handful of people, and this runs when
/// somebody moves a slider or when the roster changes.
pub struct Levels {
    voices: Voices,
    known: Mutex<Known>,
}

/// The two halves, under one lock so they cannot be republished half-updated.
#[derive(Default)]
struct Known {
    /// What was chosen, by user ID. Absent means full volume, so this holds
    /// only the people somebody has actually touched.
    chosen: BTreeMap<String, u8>,
    /// Who is in the call, as `(member_id, user_id)`.
    present: Vec<(String, String)>,
}

impl Levels {
    /// Point at `voices`, with `chosen` as what the settings file says.
    pub fn new(voices: Voices, chosen: BTreeMap<String, u8>) -> Arc<Self> {
        Arc::new(Self {
            voices,
            known: Mutex::new(Known {
                chosen,
                present: Vec::new(),
            }),
        })
    }

    /// Somebody moved a person's slider, or the settings were reloaded.
    ///
    /// Takes the whole map rather than one person, because that is what the
    /// settings file holds and a caller with one entry has to have read the
    /// rest to write it anyway.
    pub fn choose(&self, chosen: BTreeMap<String, u8>) {
        let mut known = self.known();
        known.chosen = chosen;
        self.republish(&known);
    }

    /// The call's roster changed. `whose` is `(member_id, user_id)`.
    fn present(&self, whose: &[(String, String)]) {
        let mut known = self.known();
        known.present = whose.to_vec();
        self.republish(&known);
    }

    /// Push the two halves at the mixer as the one thing it understands.
    ///
    /// Only the people who have been adjusted are sent. Everybody else is
    /// absent from the map the mixer keeps, which is what it takes as full
    /// volume, so nobody pays a multiply for a level nobody chose.
    fn republish(&self, known: &Known) {
        let levels: HashMap<String, u8> = known
            .present
            .iter()
            .filter_map(|(member_id, user_id)| {
                known
                    .chosen
                    .get(user_id)
                    .map(|percent| (member_id.clone(), *percent))
            })
            .collect();
        self.voices.set_person_levels(levels);
    }

    /// Recovering from a poisoned lock rather than spreading a panic, on the
    /// same terms as the mixer's own: one of the callers is the call thread.
    fn known(&self) -> MutexGuard<'_, Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner)
    }
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

    fn attribute(&self, whose: &[(String, String)]) {
        self.levels.present(whose);
    }

    fn cue(&self, cue: Cue) {
        // Both switches are read here rather than at the diff, so that turning
        // either off stops that noise and nothing else: who is in the call is
        // still tracked, and turning one back on mid-call starts working
        // immediately rather than at the next join.
        //
        // Read in the order the two sounds play, and queued in that order too.
        // `Voices::play` appends, so a chime and its sentence arrive as a
        // chime and then a sentence: the first gets a person's attention and
        // the second is what they hear once they have it. Doing it the other
        // way round would talk before anybody was listening.
        if self.chiming.load(Ordering::Relaxed)
            && let Some(sound) = match cue {
                Cue::Arrived => Some(Sound::Joined),
                Cue::Departed => Some(Sound::Left),
                // No chime of its own. Coming back from away is either a
                // sentence or it is nothing: there are two chimes and they
                // already mean arriving and leaving, and giving one of them a
                // third meaning would make both of them ambiguous.
                Cue::Returned => None,
            }
        {
            // Into the call's own mixer rather than through
            // `AudioPlayback::play`. That would open a second stream on the
            // same device, which is fine once for the settings-screen test
            // tone and is not fine several times an evening under a live call:
            // opening a device costs tens of milliseconds and this is exactly
            // the moment not to spend them.
            self.voices.play(sound.samples());
        }

        if self.speaking.load(Ordering::Relaxed) {
            self.voices.play(
                match cue {
                    Cue::Arrived => Phrase::Entered,
                    Cue::Departed => Phrase::Left,
                    Cue::Returned => Phrase::WelcomeBack,
                }
                .samples(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One switch on, which is what somebody who never chose has.
    fn on() -> Wanted {
        Arc::new(AtomicBool::new(true))
    }

    /// One switch off.
    fn off() -> Wanted {
        Arc::new(AtomicBool::new(false))
    }

    /// Nobody's volume adjusted, which is what everybody starts with.
    ///
    /// What these tests are about is which sounds get queued, and a person's
    /// own level is applied at the mix rather than the queue, so it cannot
    /// change any assertion here. Kept as a named helper anyway, so that a test
    /// which does want one has somewhere obvious to say so.
    fn no_levels(voices: Voices) -> Arc<Levels> {
        Levels::new(voices, BTreeMap::new())
    }

    /// Chimes on, spoken notifications off.
    ///
    /// What the tests below are about is the chime. A phrase queued behind it
    /// would be more samples waiting in every assertion about how much is,
    /// and would leave those tests passing for the wrong reason.
    fn chimes_only(voices: Voices) -> Ears {
        let levels = no_levels(voices.clone());
        speakers(voices, on(), off(), levels)
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
        let ears = chimes_only(voices.clone());

        ears.hear("alice", &[1, 2, 3]);

        assert_eq!(played(&voices, 3), vec![1, 2, 3]);
    }

    #[test]
    fn forgetting_somebody_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = chimes_only(voices.clone());
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
        let ears = chimes_only(voices.clone());

        ears.cue(Cue::Arrived);

        assert!(voices.sound_waiting() > 0, "nothing was queued to play");
    }

    #[test]
    fn arriving_and_leaving_queue_different_sounds() {
        let arriving = Voices::new();
        chimes_only(arriving.clone()).cue(Cue::Arrived);
        let leaving = Voices::new();
        chimes_only(leaving.clone()).cue(Cue::Departed);

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
        let ears = chimes_only(voices.clone());
        ears.hear("alice", &[100, 100, 100, 100]);

        ears.cue(Cue::Arrived);

        let played = played(&voices, 4);
        assert!(
            played.iter().all(|sample| *sample != 0),
            "the chime silenced the person talking: {played:?}"
        );
    }

    #[test]
    fn turning_the_sounds_off_stops_them() {
        let voices = Voices::new();
        let ears = speakers(voices.clone(), off(), off(), no_levels(voices.clone()));

        ears.cue(Cue::Arrived);

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
        let ears = speakers(
            voices.clone(),
            wanted.clone(),
            off(),
            no_levels(voices.clone()),
        );
        ears.cue(Cue::Arrived);

        wanted.store(true, Ordering::Relaxed);
        ears.cue(Cue::Arrived);

        assert!(voices.sound_waiting() > 0);
    }

    #[test]
    fn the_sounds_being_off_does_not_silence_the_call() {
        // Two different things. Somebody who turned the chimes off still wants
        // to hear the people.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), off(), off(), no_levels(voices.clone()));

        ears.hear("alice", &[1, 2, 3]);

        assert_eq!(played(&voices, 3), vec![1, 2, 3]);
    }

    #[test]
    fn both_switched_on_queues_the_chime_and_then_the_sentence() {
        // Queued rather than mixed, because `Voices::play` appends: a chime
        // gets a person's attention and the sentence is what they hear once
        // they have it. Played together they would be one noise.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on(), on(), no_levels(voices.clone()));

        ears.cue(Cue::Arrived);

        assert_eq!(
            voices.sound_waiting(),
            Sound::Joined.samples().len() + Phrase::Entered.samples().len()
        );
    }

    #[test]
    fn a_sentence_that_says_nothing_yet_does_not_swallow_the_chime() {
        // The phrases ship silent until somebody records them, and silence
        // queued around a chime is indistinguishable from the chime not
        // playing. This is what says the switch that works today still works
        // while the switch that works tomorrow has nothing to say.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on(), on(), no_levels(voices.clone()));

        ears.cue(Cue::Arrived);

        assert_eq!(
            played(&voices, Sound::Joined.samples().len()),
            Sound::Joined.samples(),
            "the chime is not what came out first"
        );
    }

    #[test]
    fn the_two_switches_are_independent_in_both_directions() {
        // The whole reason there are two of them. Either one alone has to be a
        // state somebody can be in, or the second setting is decoration.
        let chime = Voices::new();
        speakers(chime.clone(), on(), off(), no_levels(chime.clone())).cue(Cue::Arrived);
        let sentence = Voices::new();
        speakers(sentence.clone(), off(), on(), no_levels(sentence.clone())).cue(Cue::Arrived);

        assert_eq!(chime.sound_waiting(), Sound::Joined.samples().len());
        assert_eq!(sentence.sound_waiting(), Phrase::Entered.samples().len());
    }

    #[test]
    fn arriving_and_leaving_are_different_sentences() {
        // They are the same bytes today, because both are silence. What is
        // being pinned is that the two cues reach different phrases, which is
        // the part a recording cannot fix later if it is wrong now.
        let entering = Voices::new();
        speakers(entering.clone(), off(), on(), no_levels(entering.clone())).cue(Cue::Arrived);
        let leaving = Voices::new();
        speakers(leaving.clone(), off(), on(), no_levels(leaving.clone())).cue(Cue::Departed);

        assert_eq!(entering.sound_waiting(), Phrase::Entered.samples().len());
        assert_eq!(leaving.sound_waiting(), Phrase::Left.samples().len());
    }

    #[test]
    fn coming_back_from_away_is_said_and_not_chimed() {
        // There are two chimes and they already mean arriving and leaving.
        // Giving one of them a third meaning would make both ambiguous, so
        // this cue is a sentence or it is nothing, even with both switches on.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on(), on(), no_levels(voices.clone()));

        ears.cue(Cue::Returned);

        assert_eq!(
            voices.sound_waiting(),
            Phrase::WelcomeBack.samples().len(),
            "something other than the sentence was queued"
        );
    }

    #[test]
    fn coming_back_from_away_with_the_sentences_off_is_silent() {
        // Nothing to fall back on, and that is the point: somebody who turned
        // the sentences off turned this off with them, and the chime setting
        // has no opinion about it.
        let voices = Voices::new();
        let ears = speakers(voices.clone(), on(), off(), no_levels(voices.clone()));

        ears.cue(Cue::Returned);

        assert_eq!(voices.sound_waiting(), 0);
    }

    #[test]
    fn silencing_reaches_the_mixer() {
        let voices = Voices::new();
        let ears = chimes_only(voices.clone());
        ears.hear("alice", &[9, 9]);
        ears.hear("bob", &[9, 9]);

        ears.silence();

        assert_eq!(played(&voices, 2), vec![0, 0]);
    }
}

#[cfg(test)]
mod levels {
    use super::*;

    /// One buffer of what the mixer would play, as `play` in the mixing tests
    /// does it.
    fn played(voices: &Voices, frames: usize) -> Vec<i16> {
        let mut mixing = consort_audio::Mixing::new(voices.clone(), 1);
        let mut out = vec![0i16; frames];
        mixing.fill_i16(&mut out);
        out
    }

    fn ada_at(percent: u8) -> BTreeMap<String, u8> {
        BTreeMap::from([("@ada:example.org".to_owned(), percent)])
    }

    /// Ada on one device, as the call reports her.
    fn ada_present() -> Vec<(String, String)> {
        vec![("ada-laptop".to_owned(), "@ada:example.org".to_owned())]
    }

    #[test]
    fn a_level_reaches_the_membership_it_belongs_to() {
        // The whole job of this type: a choice made about a person, applied to
        // a queue keyed by a device. Neither key is available where the other
        // one is.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));

        levels.present(&ada_present());
        voices.hear("ada-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![0]);
    }

    #[test]
    fn nobody_else_is_touched_by_it() {
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));

        levels.present(&[
            ("ada-laptop".to_owned(), "@ada:example.org".to_owned()),
            ("bob-laptop".to_owned(), "@bob:example.org".to_owned()),
        ]);
        voices.hear("bob-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![1000]);
    }

    #[test]
    fn a_choice_made_mid_call_applies_without_waiting_for_the_roster() {
        // Somebody drags a slider while the person is talking. Waiting for the
        // next roster change would mean waiting for somebody to join or leave,
        // which in a settled call is never.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), BTreeMap::new());
        levels.present(&ada_present());

        levels.choose(ada_at(0));
        voices.hear("ada-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![0]);
    }

    #[test]
    fn a_choice_made_before_the_call_applies_when_they_arrive() {
        // The other order, and the one that matters on a fresh launch: the
        // settings file is read long before anybody joins anything.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));

        levels.present(&ada_present());
        voices.hear("ada-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![0]);
    }

    #[test]
    fn a_level_follows_a_person_onto_their_next_membership() {
        // A `member_id` is fresh on every join, so somebody who leaves and
        // comes back is a different key with the same person behind it. A level
        // that did not follow them would look like the setting had been
        // forgotten, which is precisely what it is there to avoid.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));

        levels.present(&ada_present());
        levels.present(&[]);
        levels.present(&[("ada-laptop-again".to_owned(), "@ada:example.org".to_owned())]);
        voices.hear("ada-laptop-again", &[1000]);

        assert_eq!(played(&voices, 1), vec![0]);
    }

    #[test]
    fn somebody_who_left_stops_being_carried() {
        // The reason the mixer is handed the whole set rather than additions.
        // Memberships are per join, so a map that was only added to would grow
        // for as long as the application runs.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));
        levels.present(&ada_present());

        levels.present(&[]);
        voices.hear("ada-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![1000]);
    }

    #[test]
    fn the_call_reports_who_is_in_it_through_the_trait() {
        // `attribute` is the seam, and a `Speakers` that dropped it on the
        // floor would leave every level in the settings file inert with nothing
        // anywhere logging a problem.
        let voices = Voices::new();
        let levels = Levels::new(voices.clone(), ada_at(0));
        let ears = speakers(
            voices.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            levels,
        );

        ears.attribute(&ada_present());
        voices.hear("ada-laptop", &[1000]);

        assert_eq!(played(&voices, 1), vec![0]);
    }
}
