// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! How much of everybody else's picture to ask the SFU for.
//!
//! Two things meet here. The box a picture is drawn into, which only the
//! window knows, and a ceiling somebody chose for one person, which only they
//! know. A camera is asked for at the box under the ceiling; a share is asked
//! for at its best until somebody caps it, because the rung below its best is
//! three frames a second:
//! `docs/adr/0016-only-a-person-may-ask-a-share-for-less.md`.
//!
//! A box and not a single number, because an SFU picks a rung by the height
//! it is asked for: see `docs/adr/0015-ask-for-the-box-not-a-square.md`.
//!
//! The units are pixels rather than a layer name, and the ceilings a named
//! choice maps to are
//! `docs/adr/0013-ask-for-a-picture-in-pixels.md`.

use std::collections::BTreeMap;

use matrix_rtc_media::{Dimensions, QualityLimit, VideoDetail};
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
    /// `None` is a choice that caps nothing, which is both [`Cap::Auto`] and
    /// [`Cap::High`]: a ceiling of ADR-0013's 1920 picks the lower rung of
    /// anything published above 1080p. The rest are ADR-0013's table.
    pub fn ceiling(self) -> Option<u32> {
        match self {
            Self::Auto | Self::High => None,
            Self::Medium => Some(1280),
            Self::Low => Some(640),
        }
    }
}

/// A box of pixels: both the box a picture is drawn in and what is asked of
/// the SFU for it.
///
/// One type for both, because a cap shrinks the box rather than changing what
/// it is a box of.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Asked {
    pub width: u32,
    pub height: u32,
}

impl Asked {
    /// The long edge, which is the edge a [`Cap`] is a ceiling on.
    pub fn bound(self) -> u32 {
        self.width.max(self.height)
    }

    /// This box with its long edge brought down to `bound`, keeping its shape.
    ///
    /// A box already inside `bound` is returned as it is, so a cap above what
    /// is drawn asks for no more than was being drawn.
    fn under(self, bound: u32) -> Self {
        if self.bound() <= bound {
            return self;
        }
        self.at(bound)
    }

    /// This box's shape with its long edge at `bound`, grown or shrunk.
    ///
    /// What a cap on a share asks for, which is a choice about the stream
    /// rather than about the card it is floating in: ADR-0016.
    fn at(self, bound: u32) -> Self {
        let long = self.bound();
        // Never zero: an SFU reads a zero as no hint at all, which is the
        // opposite of the cap somebody asked for.
        let scaled = |side: u32| {
            u32::try_from(u64::from(side) * u64::from(bound) / u64::from(long))
                .unwrap_or(u32::MAX)
                .max(1)
        };
        Self {
            width: scaled(self.width),
            height: scaled(self.height),
        }
    }
}

/// What to ask the SFU for.
///
/// Two answers rather than one box, because the best a publisher has is not a
/// number this side can name: see [`Wanted::asked`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ask {
    /// Whatever the publisher is sending at its best.
    Best,
    /// A box of pixels, answered with the smallest rung that covers it.
    Size(Asked),
}

impl Ask {
    /// What to tell the SFU.
    ///
    /// `Quality` and not `Auto` for the best there is: the transport sends
    /// nothing at all for `Auto`, so a size already sent would stand.
    pub fn detail(self) -> VideoDetail {
        match self {
            Self::Best => VideoDetail::Quality(QualityLimit::High),
            Self::Size(size) => VideoDetail::Dimensions(Dimensions {
                width: size.width,
                height: size.height,
            }),
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
    drawn: BTreeMap<(String, Kind), Asked>,
    caps: BTreeMap<String, Cap>,
}

impl Wanted {
    /// Note the box `user_id`'s `kind` is drawn into, and say whether that is
    /// news.
    ///
    /// A box with no pixels in it is not something being drawn, and is
    /// ignored: an SFU asked for nothing has no layer to answer with.
    pub fn drawn_at(&mut self, user_id: &str, kind: Kind, drawn: Asked) -> bool {
        if drawn.width == 0 || drawn.height == 0 {
            return false;
        }
        self.drawn.insert((user_id.to_owned(), kind), drawn) != Some(drawn)
    }

    /// Note the cap somebody chose for `user_id`, and say whether it is news.
    pub fn cap(&mut self, user_id: &str, cap: Cap) -> bool {
        self.caps.insert(user_id.to_owned(), cap) != Some(cap)
    }

    /// What to ask the SFU for, per [`Ask`].
    ///
    /// `None` for a picture nothing has drawn yet, which is the only honest
    /// answer: there is no shape to ask at and nothing on screen to answer.
    ///
    /// The box decides a camera and does not decide a share, because the rung
    /// below a share's best is three frames a second: ADR-0016.
    pub fn asked(&self, user_id: &str, kind: Kind) -> Option<Ask> {
        let drawn = *self.drawn.get(&(user_id.to_owned(), kind))?;
        let ceiling = self
            .caps
            .get(user_id)
            .copied()
            .unwrap_or_default()
            .ceiling();

        Some(match kind {
            Kind::Camera => Ask::Size(ceiling.map_or(drawn, |ceiling| drawn.under(ceiling))),
            Kind::Screen => ceiling.map_or(Ask::Best, |ceiling| Ask::Size(drawn.at(ceiling))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "@alice:example.org";
    const BOB: &str = "@bob:example.org";

    /// A box `bound` wide and 16 by 9, which is the shape of every screen
    /// share and of most cameras.
    fn wide(bound: u32) -> Asked {
        Asked {
            width: bound,
            height: bound * 9 / 16,
        }
    }

    #[test]
    fn nothing_is_asked_for_a_picture_nothing_draws() {
        // `Dimensions` is the size of a box. A cap chosen for somebody whose
        // camera is off has no box to go under yet, and sending one anyway
        // would be sending a zero.
        let mut wanted = Wanted::default();
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(wanted.asked(ALICE, Kind::Camera), None);
    }

    #[test]
    fn a_camera_is_asked_for_at_the_shape_it_is_drawn() {
        // A square asks the SFU for a picture as tall as the box is wide, and
        // an SFU picks a rung by height, so a square asks for a taller rung
        // than anything draws. ADR-0015.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(480));

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(480)))
        );
    }

    #[test]
    fn an_uncapped_camera_is_asked_for_at_the_size_it_is_drawn() {
        // A 720p camera pulled in full to fill a 320 pixel square is paid for
        // and thrown away, and every rung of a camera's ladder carries more
        // frames a second than the card redraws at.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(320));

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(320)))
        );
    }

    #[test]
    fn an_uncapped_share_is_asked_for_at_its_best() {
        // #195. The box a share floats in is smaller than half of what is
        // published, so a box asking for itself picks the rung below the best
        // and that rung is three frames a second. ADR-0016.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(960));

        assert_eq!(wanted.asked(ALICE, Kind::Screen), Some(Ask::Best));
    }

    #[test]
    fn a_share_capped_high_is_asked_for_at_its_best() {
        // The most a person can choose is the most there is, whatever it is.
        // A ceiling of 1920 would pick the lower rung of a 4K share.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(480));
        wanted.cap(ALICE, Cap::High);

        assert_eq!(wanted.asked(ALICE, Kind::Screen), Some(Ask::Best));
    }

    #[test]
    fn a_capped_share_is_asked_for_at_the_cap_and_not_at_the_box() {
        // A cap is a choice about the stream, so it asks for the same thing
        // wherever the card is floating.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(480));
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(
            wanted.asked(ALICE, Kind::Screen),
            Some(Ask::Size(Asked {
                width: 640,
                height: 360
            }))
        );
    }

    #[test]
    fn a_cap_keeps_the_shape_of_the_box() {
        // The whole of the bug report on #182. Low over a full-window share
        // asked for 640 by 640, and the only layer of a 1080p publication
        // that is 640 tall is the one it was already sending.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(1920));
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(
            wanted.asked(ALICE, Kind::Screen),
            Some(Ask::Size(Asked {
                width: 640,
                height: 360
            }))
        );
    }

    #[test]
    fn the_long_edge_is_what_a_cap_is_a_ceiling_on() {
        // A portrait camera. Capping its width to 640 would leave it 1138
        // tall, which is more picture than the uncapped box asked for.
        let mut wanted = Wanted::default();
        wanted.drawn_at(
            ALICE,
            Kind::Camera,
            Asked {
                width: 1080,
                height: 1920,
            },
        );
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(Asked {
                width: 360,
                height: 640
            }))
        );
    }

    #[test]
    fn a_cap_above_the_box_changes_nothing() {
        // Capping a tile that is already small costs no round trip and must
        // not ask for a bigger picture than the one being drawn.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(320));
        wanted.cap(ALICE, Cap::High);

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(320)))
        );
    }

    #[test]
    fn the_cap_covers_both_of_somebody_s_pictures() {
        // One choice about a person, which is what the control offers.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(960));
        wanted.drawn_at(ALICE, Kind::Screen, wide(1920));
        wanted.cap(ALICE, Cap::Medium);

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(960)))
        );
        assert_eq!(
            wanted.asked(ALICE, Kind::Screen),
            Some(Ask::Size(Asked {
                width: 1280,
                height: 720
            }))
        );
    }

    #[test]
    fn a_cap_is_about_one_person_and_nobody_else() {
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(1920));
        wanted.drawn_at(BOB, Kind::Screen, wide(1920));
        wanted.cap(ALICE, Cap::Low);

        assert_eq!(wanted.asked(BOB, Kind::Screen), Some(Ask::Best));
    }

    #[test]
    fn the_two_kinds_are_drawn_at_different_sizes() {
        // A camera square and a screen on the stage are different boxes, so a
        // store keyed by person alone would ask for one at the other's size.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(320));
        wanted.drawn_at(ALICE, Kind::Screen, wide(1920));
        wanted.cap(ALICE, Cap::Medium);

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(320)))
        );
        assert_eq!(
            wanted.asked(ALICE, Kind::Screen),
            Some(Ask::Size(Asked {
                width: 1280,
                height: 720
            }))
        );
    }

    #[test]
    fn the_same_box_twice_is_not_news() {
        // Asked twelve times a second per picture. Without this the call
        // thread's command channel carries a message per drawn frame.
        let mut wanted = Wanted::default();

        assert!(wanted.drawn_at(ALICE, Kind::Camera, wide(320)));
        assert!(!wanted.drawn_at(ALICE, Kind::Camera, wide(320)));
        assert!(wanted.drawn_at(ALICE, Kind::Camera, wide(480)));
    }

    #[test]
    fn a_box_reshaped_at_the_same_bound_is_news() {
        // The shape arrives with the first frame, so the box before one and
        // the box after it share a long edge and are not the same ask.
        let mut wanted = Wanted::default();

        assert!(wanted.drawn_at(
            ALICE,
            Kind::Screen,
            Asked {
                width: 320,
                height: 320
            }
        ));
        assert!(wanted.drawn_at(ALICE, Kind::Screen, wide(320)));
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

        assert!(!wanted.drawn_at(
            ALICE,
            Kind::Camera,
            Asked {
                width: 320,
                height: 0
            }
        ));
        assert!(!wanted.drawn_at(
            ALICE,
            Kind::Camera,
            Asked {
                width: 0,
                height: 320
            }
        ));
        assert_eq!(wanted.asked(ALICE, Kind::Camera), None);
    }

    #[test]
    fn a_capped_sliver_still_has_pixels_in_it() {
        // A zero is how the protocol says "no hint", so rounding a very wide
        // box down to one would turn a cap into its own absence.
        let mut wanted = Wanted::default();
        wanted.drawn_at(
            ALICE,
            Kind::Screen,
            Asked {
                width: 1920,
                height: 2,
            },
        );
        wanted.cap(ALICE, Cap::Low);

        let Some(Ask::Size(asked)) = wanted.asked(ALICE, Kind::Screen) else {
            unreachable!("a capped share is asked for at a size")
        };
        assert_eq!(asked.width, 640);
        assert!(asked.height >= 1, "asked for {asked:?}");
    }

    #[test]
    fn clearing_a_cap_goes_back_to_the_best_there_is() {
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Screen, wide(1920));
        wanted.cap(ALICE, Cap::Low);
        wanted.cap(ALICE, Cap::Auto);

        assert_eq!(wanted.asked(ALICE, Kind::Screen), Some(Ask::Best));
    }

    #[test]
    fn each_word_is_the_ceiling_adr_0013_chose() {
        // The numbers the control promises. Changing one is changing what
        // somebody who picked "low" gets, which is the whole of the feature.
        assert_eq!(Cap::Auto.ceiling(), None);
        assert_eq!(Cap::High.ceiling(), None);
        assert_eq!(Cap::Medium.ceiling(), Some(1280));
        assert_eq!(Cap::Low.ceiling(), Some(640));
    }

    #[test]
    fn the_best_there_is_is_told_to_the_sfu_as_a_quality_and_not_as_a_size() {
        // `Auto` is the honest word for it and the LiveKit transport sends
        // nothing at all for it, which would leave a size already sent
        // standing. ADR-0016.
        assert_eq!(Ask::Best.detail(), VideoDetail::Quality(QualityLimit::High));
    }

    #[test]
    fn a_size_is_told_to_the_sfu_as_the_box_it_is() {
        assert_eq!(
            Ask::Size(Asked {
                width: 640,
                height: 360
            })
            .detail(),
            VideoDetail::Dimensions(Dimensions {
                width: 640,
                height: 360
            })
        );
    }

    #[test]
    fn the_long_edge_of_a_box_is_its_larger_side() {
        assert_eq!(wide(1920).bound(), 1920);
        assert_eq!(
            Asked {
                width: 1080,
                height: 1920
            }
            .bound(),
            1920
        );
    }

    #[test]
    fn nobody_chosen_for_is_auto() {
        // What every person starts at, and the reason a fresh store sends the
        // drawn size rather than nothing.
        let mut wanted = Wanted::default();
        wanted.drawn_at(ALICE, Kind::Camera, wide(320));

        assert_eq!(
            wanted.asked(ALICE, Kind::Camera),
            Some(Ask::Size(wide(320)))
        );
        assert_eq!(Cap::default(), Cap::Auto);
    }
}
