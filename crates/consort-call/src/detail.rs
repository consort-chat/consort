// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! How much of everybody else's picture to ask the SFU for.
//!
//! Two numbers meet here. The size of the box a picture is drawn into, which
//! only the window knows, and a ceiling somebody chose for one person, which
//! only they know. What is asked for is the smaller of the two.
//!
//! The units are pixels rather than a layer name, and the ceilings a named
//! choice maps to are
//! `docs/adr/0013-ask-for-a-picture-in-pixels.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::watching::Kind;

/// How much of somebody else's picture to ask for.
///
/// A word rather than a number, because a person choosing this is choosing
/// "less" rather than 640 pixels. Which pixels each word means is
/// [`Cap::ceiling`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Cap {
    /// Whatever the box it is drawn into wants. The absence of a choice.
    #[default]
    Auto,
    High,
    Medium,
    Low,
}

impl Cap {
    /// The most pixels this choice allows on the long edge.
    ///
    /// `None` for [`Cap::Auto`], which caps nothing. The numbers are
    /// ADR-0013's table.
    pub fn ceiling(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::High => Some(1920),
            Self::Medium => Some(1280),
            Self::Low => Some(640),
        }
    }
}

/// What this session wants of the pictures the rest of the call is sending.
///
/// Held by the call thread and outliving any one call, for the reason mute and
/// deafen outlive one: somebody who turned a share down has not asked for it
/// back up by clicking a different channel.
///
/// The drawn size is per kind, because a camera square and a screen on the
/// stage are different boxes. The cap is per person, because that is what the
/// control offers: one choice about somebody rather than one per picture.
#[derive(Default)]
pub struct Wanted {
    drawn: BTreeMap<(String, Kind), u32>,
    caps: BTreeMap<String, Cap>,
}

impl Wanted {
    /// Note that `user_id`'s `kind` is drawn into a box `bound` pixels on its
    /// long edge, and say whether that is news.
    ///
    /// A box with no pixels in it is not something being drawn, and is
    /// ignored: an SFU asked for nothing has no layer to answer with.
    pub fn drawn_at(&mut self, user_id: &str, kind: Kind, bound: u32) -> bool {
        if bound == 0 {
            return false;
        }
        self.drawn.insert((user_id.to_owned(), kind), bound) != Some(bound)
    }

    /// Note the cap somebody chose for `user_id`, and say whether it is news.
    pub fn cap(&mut self, user_id: &str, cap: Cap) -> bool {
        self.caps.insert(user_id.to_owned(), cap) != Some(cap)
    }

    /// The long edge in pixels to ask the SFU for.
    ///
    /// `None` for a picture nothing has drawn yet, which is the only honest
    /// answer: `Dimensions` is the size of a box, so a caller with no box has
    /// nothing to send.
    pub fn pixels(&self, user_id: &str, kind: Kind) -> Option<u32> {
        let bound = *self.drawn.get(&(user_id.to_owned(), kind))?;
        let ceiling = self
            .caps
            .get(user_id)
            .copied()
            .unwrap_or_default()
            .ceiling();

        Some(ceiling.map_or(bound, |ceiling| bound.min(ceiling)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "@alice:example.org";
    const BOB: &str = "@bob:example.org";

    #[test]
    fn nothing_is_asked_for_a_picture_nothing_draws() {
        // `Dimensions` is the size of a box. A cap chosen for somebody whose
        // camera is off has no box to go under yet, and sending one anyway
        // would be sending a zero.
        let mut wanted = Wanted::default();
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(wanted.pixels(ALICE, Kind::Camera), None);
    }

    #[test]
    fn an_uncapped_picture_is_asked_for_at_the_size_it_is_drawn() {
        // Phase 1 of the plan: a 1080p share pulled in full to fill a 320
        // pixel square is paid for and thrown away.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, 320);

        assert_eq!(wanted.pixels(ALICE, Kind::Screen), Some(320));
    }

    #[test]
    fn a_cap_wins_over_a_larger_box() {
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, 1920);
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(wanted.pixels(ALICE, Kind::Screen), Some(640));
    }

    #[test]
    fn a_cap_above_the_box_changes_nothing() {
        // Capping a tile that is already small costs no round trip and must
        // not ask for a bigger picture than the one being drawn.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, 320);
        wanted.cap(ALICE, Cap::High);

        assert_eq!(wanted.pixels(ALICE, Kind::Camera), Some(320));
    }

    #[test]
    fn the_cap_covers_both_of_somebody_s_pictures() {
        // One choice about a person, which is what the control offers.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, 960);
        wanted.drawn_at(ALICE, Kind::Screen, 1920);
        wanted.cap(ALICE, Cap::Medium);

        assert_eq!(wanted.pixels(ALICE, Kind::Camera), Some(960));
        assert_eq!(wanted.pixels(ALICE, Kind::Screen), Some(1280));
    }

    #[test]
    fn a_cap_is_about_one_person_and_nobody_else() {
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, 1920);
        wanted.drawn_at(BOB, Kind::Screen, 1920);
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(wanted.pixels(BOB, Kind::Screen), Some(1920));
    }

    #[test]
    fn the_two_kinds_are_drawn_at_different_sizes() {
        // A camera square and a screen on the stage are different boxes, so a
        // store keyed by person alone would ask for one at the other's size.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, 320);
        wanted.drawn_at(ALICE, Kind::Screen, 1920);

        assert_eq!(wanted.pixels(ALICE, Kind::Camera), Some(320));
        assert_eq!(wanted.pixels(ALICE, Kind::Screen), Some(1920));
    }

    #[test]
    fn the_same_size_twice_is_not_news() {
        // Asked twelve times a second per picture. Without this the call
        // thread's command channel carries a message per drawn frame.
        let mut wanted = Wanted::default();

        assert!(wanted.drawn_at(ALICE, Kind::Camera, 320));
        assert!(!wanted.drawn_at(ALICE, Kind::Camera, 320));
        assert!(wanted.drawn_at(ALICE, Kind::Camera, 480));
    }

    #[test]
    fn the_same_cap_twice_is_not_news() {
        let mut wanted = Wanted::default();

        assert!(wanted.cap(ALICE, Cap::Low));
        assert!(!wanted.cap(ALICE, Cap::Low));
        assert!(wanted.cap(ALICE, Cap::Auto));
    }

    #[test]
    fn a_box_with_no_pixels_in_it_is_not_a_drawn_picture() {
        // Nothing in the card asks for one, and a command is reachable from
        // anything running in the webview. A zero reaching the SFU is a
        // request for a layer with no size.
        let mut wanted = Wanted::default();

        assert!(!wanted.drawn_at(ALICE, Kind::Camera, 0));
        assert_eq!(wanted.pixels(ALICE, Kind::Camera), None);
    }

    #[test]
    fn clearing_a_cap_goes_back_to_the_drawn_size() {
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, 1920);
        wanted.cap(ALICE, Cap::Low);
        wanted.cap(ALICE, Cap::Auto);

        assert_eq!(wanted.pixels(ALICE, Kind::Screen), Some(1920));
    }

    #[test]
    fn each_word_is_the_ceiling_adr_0013_chose() {
        // The numbers the control promises. Changing one is changing what
        // somebody who picked "low" gets, which is the whole of the feature.
        assert_eq!(Cap::Auto.ceiling(), None);
        assert_eq!(Cap::High.ceiling(), Some(1920));
        assert_eq!(Cap::Medium.ceiling(), Some(1280));
        assert_eq!(Cap::Low.ceiling(), Some(640));
    }

    #[test]
    fn nobody_chosen_for_is_auto() {
        // What every person starts at, and the reason a fresh store sends the
        // drawn size rather than nothing.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, 320);

        assert_eq!(wanted.pixels(ALICE, Kind::Camera), Some(320));
        assert_eq!(Cap::default(), Cap::Auto);
    }
}
