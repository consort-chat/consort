// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning roster changes into sounds.
//!
//! A voice channel with no sound on arrival is a channel where people talk
//! over each other, because the only way to know somebody joined is to be
//! looking at the right corner of the screen at the moment they do.
//!
//! The rules are the whole of this module, and each of them is a bug if it is
//! left out. They are separated from [`crate::thread`] because a roster is
//! easy to hand to a function and hard to arrange around a call.

use std::collections::BTreeSet;

use consort_matrix::Participant;

use crate::hearing::Chime;

/// Who was in the call last time anybody looked.
///
/// Per person rather than per membership, because that is what a roster is:
/// somebody on a laptop and a phone is one person, and their second device
/// connecting is not somebody walking in.
#[derive(Debug, Default)]
pub struct Arrivals {
    /// `None` until the first roster arrives, which is not the same as an
    /// empty one and decides the opposite way. "Nobody has looked" absorbs;
    /// "somebody looked and the channel was empty" is a baseline to diff the
    /// next reading against, and without the distinction the first person to
    /// walk into an empty channel arrives in silence.
    known: Option<BTreeSet<String>>,
    /// This session's own user, left out of every comparison.
    me: Option<String>,
}

impl Arrivals {
    /// Nobody yet, which is where every call starts.
    ///
    /// `me` is this session's own Matrix user, from [`crate::transport::Roster::me`].
    pub fn new(me: Option<String>) -> Self {
        Self { known: None, me }
    }

    /// Take in the roster as it now is, and say what to play.
    ///
    /// # Joining is silent
    ///
    /// A roster taken over from nothing is absorbed rather than diffed, so
    /// clicking into a channel with four people in it plays nothing. Four
    /// chimes at once is not four pieces of news, it is one piece of news
    /// delivered as a noise.
    ///
    /// # This session is never one of them
    ///
    /// The roster includes us, because a voice channel draws everybody in it.
    /// So our own user is filtered out before anything is compared, rather
    /// than being caught by the rule above: a join reports an empty roster for
    /// the moment between publishing a membership and it coming back round, so
    /// our own arrival routinely lands as the *second* reading, which is the
    /// first one that gets diffed.
    ///
    /// Leaving needs no equivalent rule. A call that ends drops the watcher
    /// with it, so there is nothing left to notice this session's own
    /// departure, and switching channels starts a new one of these rather than
    /// diffing the old channel against the new.
    ///
    /// # At most one of each
    ///
    /// Three people arriving in one change is one `Arrived`. The sound says
    /// somebody came in; playing it three times says the same thing three
    /// times, and it says it as a chord, because they overlap.
    ///
    /// Arrivals before departures, for the case where the roster changed in
    /// both directions at once. It is the more useful half: somebody who just
    /// walked in may be about to speak.
    pub fn settle(&mut self, now: &[Participant]) -> Vec<Chime> {
        let now: BTreeSet<String> = now
            .iter()
            .map(|person| person.id.clone())
            .filter(|id| Some(id) != self.me.as_ref())
            .collect();

        let Some(known) = self.known.replace(now.clone()) else {
            return Vec::new();
        };

        let mut chimes = Vec::new();
        if now.difference(&known).next().is_some() {
            chimes.push(Chime::Arrived);
        }
        if known.difference(&now).next().is_some() {
            chimes.push(Chime::Departed);
        }
        chimes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn people(ids: &[&str]) -> Vec<Participant> {
        ids.iter()
            .map(|id| Participant::named(*id, "Somebody"))
            .collect()
    }

    #[test]
    fn joining_an_occupied_channel_is_silent() {
        // Four people already in it is not four people arriving. This is the
        // rule most easily left out and the one whose absence is loudest.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));

        let chimes = arrivals.settle(&people(&["@a:example.org", "@b:example.org"]));

        assert!(chimes.is_empty(), "{chimes:?}");
    }

    #[test]
    fn this_session_arriving_after_an_empty_roster_is_silent() {
        // A join reports an empty roster for the moment between publishing a
        // membership and it coming back round, so our own arrival lands as the
        // second reading rather than the first. Without absorbing an empty
        // baseline, every call would begin by chiming at the person who
        // started it.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&[]);

        let chimes = arrivals.settle(&people(&["@me:example.org"]));

        assert!(chimes.is_empty(), "{chimes:?}");
    }

    #[test]
    fn somebody_walking_in_afterwards_is_announced() {
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org"]));

        let chimes = arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        assert_eq!(chimes, vec![Chime::Arrived]);
    }

    #[test]
    fn somebody_leaving_is_announced() {
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        let chimes = arrivals.settle(&people(&["@me:example.org"]));

        assert_eq!(chimes, vec![Chime::Departed]);
    }

    #[test]
    fn a_roster_that_did_not_change_says_nothing() {
        // `Connected` is re-emitted for reasons that are not arrivals: the
        // trouble on it changes independently. Diffing the event rather than
        // the people would chime when a media key was refused.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        let chimes = arrivals.settle(&people(&["@ada:example.org", "@me:example.org"]));

        assert!(chimes.is_empty(), "{chimes:?}");
    }

    #[test]
    fn three_people_arriving_at_once_is_one_sound() {
        // Three copies of one sound played together is a chord, not three
        // pieces of news.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org"]));

        let chimes = arrivals.settle(&people(&[
            "@me:example.org",
            "@a:example.org",
            "@b:example.org",
            "@c:example.org",
        ]));

        assert_eq!(chimes, vec![Chime::Arrived]);
    }

    #[test]
    fn a_change_in_both_directions_plays_the_arrival_first() {
        // The more useful half: somebody who just walked in may be about to
        // speak, and somebody who left is not going to.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        let chimes = arrivals.settle(&people(&["@me:example.org", "@bob:example.org"]));

        assert_eq!(chimes, vec![Chime::Arrived, Chime::Departed]);
    }

    #[test]
    fn a_second_device_is_not_a_second_person() {
        // A roster is per person and arrives deduplicated, so this is really a
        // statement about what is compared: user ids, never membership ids.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        let chimes = arrivals.settle(&[
            Participant::named("@me:example.org", "Me"),
            Participant::named("@ada:example.org", "Ada on her laptop"),
        ]);

        assert!(chimes.is_empty(), "{chimes:?}");
    }

    #[test]
    fn the_channel_emptying_is_announced_once() {
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&people(&[
            "@me:example.org",
            "@a:example.org",
            "@b:example.org",
        ]));

        let chimes = arrivals.settle(&people(&["@me:example.org"]));

        assert_eq!(chimes, vec![Chime::Departed]);
    }

    #[test]
    fn this_session_appearing_in_its_own_roster_is_not_an_arrival() {
        // A join reports an empty roster for the moment between publishing a
        // membership and it coming back round, so our own entry routinely
        // lands as the second reading, which is the first one that is diffed.
        // Without filtering it out, every call would begin by chiming at the
        // person who started it.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&[]);

        let chimes = arrivals.settle(&people(&["@me:example.org"]));

        assert!(chimes.is_empty(), "{chimes:?}");
    }

    #[test]
    fn the_first_person_into_an_empty_channel_is_still_heard() {
        // The case an "absorb anything empty" rule silently swallows, and the
        // reason "nobody has looked" and "the channel was empty" have to be
        // different states. Somebody sitting alone in a voice channel is
        // exactly the person who most needs telling that company arrived.
        let mut arrivals = Arrivals::new(Some("@me:example.org".to_owned()));
        arrivals.settle(&[]);
        arrivals.settle(&people(&["@me:example.org"]));

        let chimes = arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        assert_eq!(chimes, vec![Chime::Arrived]);
    }

    #[test]
    fn a_roster_that_cannot_say_who_we_are_still_works_for_everybody_else() {
        // `me` is `None` from an implementation that does not know. The cost
        // is one chime at the start of a call, and everything after it is
        // right, which is the correct way round to be wrong.
        let mut arrivals = Arrivals::new(None);
        arrivals.settle(&people(&["@me:example.org"]));

        let chimes = arrivals.settle(&people(&["@me:example.org", "@ada:example.org"]));

        assert_eq!(chimes, vec![Chime::Arrived]);
    }

    #[test]
    fn a_channel_switch_does_not_diff_one_channel_against_another() {
        // Expressed here as the thing that makes it true: a fresh `Arrivals`
        // per call. The people in the channel just left are not people who
        // left, they are people this session stopped being with, and a
        // switch that chimed for all of them would be the loudest event in
        // the application.
        let mut lounge = Arrivals::new(Some("@me:example.org".to_owned()));
        lounge.settle(&people(&["@a:example.org", "@b:example.org"]));

        let mut general = Arrivals::new(Some("@me:example.org".to_owned()));
        let chimes = general.settle(&people(&["@c:example.org"]));

        assert!(chimes.is_empty(), "{chimes:?}");
    }
}
