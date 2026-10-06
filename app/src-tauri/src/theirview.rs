// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What everybody else is sending, at the size the card is drawing it.
//!
//! [`crate::selfview`]'s counterpart, and the far end of
//! `consort_call::watching`: the call thread leaves the newest frame of each
//! remote camera and shared screen here, and whatever is drawing one asks for
//! it in pixels. See
//! `docs/adr/0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md` for why
//! the size travels with the ask.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use consort_call::{Asked, IncomingPicture, Kind, Seen};
use consort_video::Picture;

use crate::selfview::encode;

/// The largest picture that will be made, on either side, in pixels.
///
/// Where the encode reaches a third of a core, and where ADR-0013's `High`
/// sits, so a caller asking for more is asking for a cost nothing drawn in
/// this window is worth.
const MAX_BOUND: u32 = 1920;

/// `bound` brought inside what a picture will ever be made at.
///
/// Clamped rather than refused. The frontend asks for the box it is drawing
/// into, and the honest answer to a box larger than the ceiling is the largest
/// picture there is. Shared with the SFU side of the same ask, so what is
/// requested and what is drawn cannot disagree: see `consort_call::detail`.
pub fn clamped(bound: u32) -> u32 {
    bound.clamp(1, MAX_BOUND)
}

/// The newest frame of everything the other people in the call are sending.
///
/// One entry per membership per kind, because that is what a stream belongs to:
/// somebody in from a laptop and a phone is two cameras. The card draws people
/// rather than devices, so a lookup takes a user ID and answers with the first
/// of their memberships by name, which is arbitrary but never flickers between
/// two of them.
#[derive(Default)]
pub struct TheirViews(Mutex<BTreeMap<(String, Kind), Held>>);

/// One remote stream's newest frame, and whose it is.
struct Held {
    user_id: String,
    picture: Picture,
}

impl TheirViews {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The newest picture `user_id` is sending of `kind`, no larger than
    /// `bound` on its long edge, as a data URL.
    ///
    /// `None` when they are sending nothing of that kind, which is also the
    /// moment between a stream appearing and its first frame.
    ///
    /// The frame is kept rather than taken, so a card asking faster than a
    /// camera sends redraws the same picture instead of blinking off and on.
    pub fn latest(&self, user_id: &str, kind: Kind, bound: u32) -> Option<String> {
        let held = self.held();
        let theirs = held
            .iter()
            .find(|((_, held_kind), held)| *held_kind == kind && held.user_id == user_id)?
            .1;

        let bound = clamped(bound);

        encode(&theirs.picture.thumbnail(bound, bound))
    }

    /// The box a still of `user_id`'s `kind` is drawn in at `bound`.
    ///
    /// What the SFU is asked for, so it is the box rather than the square the
    /// box fits inside: ADR-0015. A square is the answer before the first
    /// frame, when the shape of what is coming is not known yet.
    pub fn drawn_at(&self, user_id: &str, kind: Kind, bound: u32) -> Asked {
        let bound = clamped(bound);
        let square = Asked {
            width: bound,
            height: bound,
        };

        self.held()
            .iter()
            .find(|((_, held_kind), held)| *held_kind == kind && held.user_id == user_id)
            .map_or(square, |(_, held)| {
                let (width, height) = held.picture.thumbnail_size(bound, bound);
                Asked { width, height }
            })
    }

    fn held(&self) -> MutexGuard<'_, BTreeMap<(String, Kind), Held>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Seen for TheirViews {
    fn see(&self, member_id: &str, user_id: &str, kind: Kind, picture: IncomingPicture) {
        self.held().insert(
            (member_id.to_owned(), kind),
            Held {
                user_id: user_id.to_owned(),
                picture: Picture {
                    width: picture.width,
                    height: picture.height,
                    y: picture.y,
                    u: picture.u,
                    v: picture.v,
                    // Nothing here paces anything: this is a still, and the
                    // only clock it answers to is how often the card asks.
                    timestamp_us: 0,
                },
            },
        );
    }

    fn forget(&self, member_id: &str, kind: Kind) {
        self.held().remove(&(member_id.to_owned(), kind));
    }

    fn clear(&self) {
        self.held().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    const ALICE: &str = "@alice:example.org";
    const BOB: &str = "@bob:example.org";

    /// A flat frame of one shade, at whatever size.
    fn frame(width: u32, height: u32, y: u8) -> IncomingPicture {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));

        IncomingPicture {
            width,
            height,
            y: vec![y; w * h],
            u: vec![128; cw * ch],
            v: vec![128; cw * ch],
        }
    }

    /// What is actually in a data URL, decoded back to a picture.
    fn drawn(url: &str) -> image::DynamicImage {
        let base64 = url
            .strip_prefix("data:image/jpeg;base64,")
            .unwrap_or_else(|| panic!("not a JPEG data URL: {}", &url[..40.min(url.len())]));
        let jpeg = STANDARD.decode(base64).expect("the URL holds base64");

        image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
            .expect("the URL holds a JPEG")
    }

    /// A store with one 720p camera from one membership of Alice's.
    fn alices_camera() -> TheirViews {
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Camera, frame(1280, 720, 120));
        views
    }

    #[test]
    fn nothing_is_drawn_for_somebody_sending_nothing() {
        let views = TheirViews::new();

        assert_eq!(views.latest(ALICE, Kind::Camera, 320), None);
    }

    #[test]
    fn a_frame_comes_back_as_a_jpeg_data_url() {
        let views = alices_camera();

        let url = views.latest(ALICE, Kind::Camera, 320).expect("a frame");
        assert_eq!(drawn(&url).color().channel_count(), 3);
    }

    #[test]
    fn the_picture_is_made_at_the_size_it_was_asked_for() {
        // The whole decision. A tile asks for a square and the stage asks for
        // something a person can read text on, and the same frame answers
        // both: issue #165, and the lever #167 is built on.
        let views = alices_camera();

        let tile = drawn(&views.latest(ALICE, Kind::Camera, 320).unwrap());
        let stage = drawn(&views.latest(ALICE, Kind::Camera, 960).unwrap());

        assert_eq!((tile.width(), tile.height()), (320, 180));
        assert_eq!((stage.width(), stage.height()), (960, 540));
    }

    #[test]
    fn the_box_asked_of_the_sfu_is_the_one_the_still_is_drawn_in() {
        // The bug on #182. A square asks for a picture as tall as the box is
        // wide, and an SFU picks a layer by height, so every cap collapsed
        // onto the layer the publisher was already sending.
        let views = alices_camera();

        assert_eq!(
            views.drawn_at(ALICE, Kind::Camera, 320),
            Asked {
                width: 320,
                height: 180
            }
        );
    }

    #[test]
    fn the_box_matches_the_picture_that_is_made_in_it() {
        // Two answers to one question, so a drift between them is a cap that
        // does not match what it is a cap on.
        let views = alices_camera();

        for bound in [320, 480, 960, 1920, 4096] {
            let made = drawn(&views.latest(ALICE, Kind::Camera, bound).unwrap());
            let box_ = views.drawn_at(ALICE, Kind::Camera, bound);

            assert_eq!(
                (box_.width, box_.height),
                (made.width(), made.height()),
                "at {bound}"
            );
        }
    }

    #[test]
    fn a_box_before_the_first_frame_is_the_square_that_was_asked_for() {
        // Nothing has arrived, so there is no shape to go on. The square is
        // the safe over-ask, and the next poll after the first frame corrects
        // it.
        let views = TheirViews::new();

        assert_eq!(
            views.drawn_at(ALICE, Kind::Screen, 480),
            Asked {
                width: 480,
                height: 480
            }
        );
    }

    #[test]
    fn a_box_is_never_larger_than_the_ceiling() {
        // The same clamp the still is made at: what is asked of the SFU and
        // what is drawn cannot disagree.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Screen, frame(3840, 2160, 90));

        assert_eq!(views.drawn_at(ALICE, Kind::Screen, 4096).width, MAX_BOUND);
    }

    #[test]
    fn nothing_larger_than_the_ceiling_is_ever_made() {
        // A frontend cannot spend a core of somebody's machine by asking for
        // a picture at the size of a 4K monitor.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Screen, frame(3840, 2160, 90));

        let picture = drawn(&views.latest(ALICE, Kind::Screen, 4096).unwrap());

        assert_eq!(picture.width(), MAX_BOUND);
    }

    #[test]
    fn a_small_frame_is_not_stretched_to_fill_a_large_box() {
        // Upsampling in Rust costs the encode of every pixel it invented and
        // buys nothing: the `img` element scales for free. What #167 does
        // about a stream that arrives too small is ask the SFU for a bigger
        // one.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Camera, frame(320, 180, 120));

        let picture = drawn(&views.latest(ALICE, Kind::Camera, 1920).unwrap());

        assert_eq!((picture.width(), picture.height()), (320, 180));
    }

    #[test]
    fn the_newest_frame_wins() {
        // A picture is a state, not a sequence: an older frame is the wrong
        // answer rather than missing data.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Camera, frame(64, 64, 16));
        views.see("alice-laptop", ALICE, Kind::Camera, frame(64, 64, 235));

        let picture = drawn(&views.latest(ALICE, Kind::Camera, 320).unwrap()).to_luma8();

        assert!(
            picture.pixels().all(|pixel| pixel.0[0] > 200),
            "the older darker frame was drawn"
        );
    }

    #[test]
    fn asking_twice_without_a_new_frame_draws_the_same_picture() {
        // The card asks on a timer and frames arrive on the SFU's schedule, so
        // the two do not line up. Answering nothing between frames would
        // blink every picture on the card off and on.
        let views = alices_camera();

        let first = views.latest(ALICE, Kind::Camera, 320);
        let second = views.latest(ALICE, Kind::Camera, 320);

        assert!(first.is_some());
        assert_eq!(first, second);
    }

    #[test]
    fn a_camera_and_a_shared_screen_are_different_pictures() {
        // Both at once is ordinary, and they are drawn in different squares.
        // One slot for both would put somebody's desktop on their face.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Camera, frame(64, 64, 16));
        views.see("alice-laptop", ALICE, Kind::Screen, frame(64, 64, 235));

        let camera = drawn(&views.latest(ALICE, Kind::Camera, 64).unwrap()).to_luma8();
        let screen = drawn(&views.latest(ALICE, Kind::Screen, 64).unwrap()).to_luma8();

        assert!(camera.pixels().all(|pixel| pixel.0[0] < 60));
        assert!(screen.pixels().all(|pixel| pixel.0[0] > 200));
    }

    #[test]
    fn one_persons_picture_is_not_drawn_for_another() {
        let views = alices_camera();

        assert_eq!(views.latest(BOB, Kind::Camera, 320), None);
    }

    #[test]
    fn a_camera_going_off_takes_its_last_frame_with_it() {
        // Not merely undrawn. The last thing a camera saw before somebody
        // covered it is the one picture they did not want left anywhere.
        let views = alices_camera();

        views.forget("alice-laptop", Kind::Camera);

        assert_eq!(views.latest(ALICE, Kind::Camera, 320), None);
    }

    #[test]
    fn forgetting_a_camera_leaves_the_shared_screen_alone() {
        // Switching a camera off mid-presentation is ordinary, and it must not
        // take the slides down with it.
        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Camera, frame(64, 64, 120));
        views.see("alice-laptop", ALICE, Kind::Screen, frame(64, 64, 120));

        views.forget("alice-laptop", Kind::Camera);

        assert_eq!(views.latest(ALICE, Kind::Camera, 64), None);
        assert!(views.latest(ALICE, Kind::Screen, 64).is_some());
    }

    #[test]
    fn forgetting_one_membership_leaves_everybody_else_drawn() {
        let views = alices_camera();
        views.see("bob-desktop", BOB, Kind::Camera, frame(64, 64, 120));

        views.forget("alice-laptop", Kind::Camera);

        assert_eq!(views.latest(ALICE, Kind::Camera, 64), None);
        assert!(views.latest(BOB, Kind::Camera, 64).is_some());
    }

    #[test]
    fn a_call_that_ended_leaves_nothing_to_draw() {
        let views = alices_camera();
        views.see("bob-desktop", BOB, Kind::Camera, frame(64, 64, 120));

        views.clear();

        assert_eq!(views.latest(ALICE, Kind::Camera, 64), None);
        assert_eq!(views.latest(BOB, Kind::Camera, 64), None);
    }

    #[test]
    fn two_devices_of_one_person_draw_whichever_comes_first_by_name() {
        // Arbitrary and deliberately stable. Somebody with a camera on a
        // laptop and a phone is one square on the card, and alternating
        // between the two frames would be a square that flickers between two
        // rooms.
        let views = TheirViews::new();
        views.see("alice-phone", ALICE, Kind::Camera, frame(64, 64, 235));
        views.see("alice-laptop", ALICE, Kind::Camera, frame(64, 64, 16));

        let first = drawn(&views.latest(ALICE, Kind::Camera, 64).unwrap()).to_luma8();
        let again = drawn(&views.latest(ALICE, Kind::Camera, 64).unwrap()).to_luma8();

        assert!(
            first.pixels().all(|pixel| pixel.0[0] < 60),
            "the laptop sorts first and should be the one drawn"
        );
        assert_eq!(first, again);
    }

    #[test]
    fn a_box_of_no_size_still_draws_something() {
        // Reachable from a card drawn before layout has given it a size. The
        // alternative to a clamp is an encoder refusing a zero-pixel picture,
        // which would read as nobody having a camera on.
        let views = alices_camera();

        assert!(views.latest(ALICE, Kind::Camera, 0).is_some());
    }

    /// What one remote picture costs at each size the card asks for.
    ///
    /// The table in
    /// `docs/adr/0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md`
    /// comes from here, so it can be taken again rather than trusted. Pure
    /// noise, which is the worst case a JPEG can be handed and not what a
    /// desktop or a face is.
    ///
    /// ```sh
    /// cargo test --release -p consort-app --lib theirview -- --ignored --nocapture measure
    /// ```
    #[test]
    #[ignore = "a measurement, not a check"]
    fn measure_what_a_remote_picture_costs() {
        use std::time::Instant;

        let mut noise = frame(3840, 2160, 0);
        for (at, pixel) in noise.y.iter_mut().enumerate() {
            *pixel = ((at * 2654435761) >> 13) as u8;
        }

        let views = TheirViews::new();
        views.see("alice-laptop", ALICE, Kind::Screen, noise);

        for bound in [160, 320, 480, 960, 1280, 1920] {
            let started = Instant::now();
            let rounds = 10;
            let mut url = String::new();
            for _ in 0..rounds {
                url = views.latest(ALICE, Kind::Screen, bound).unwrap();
            }
            let picture = drawn(&url);
            println!(
                "bound {bound}: {}x{}, sample and encode {:?}, url {} B",
                picture.width(),
                picture.height(),
                started.elapsed() / rounds,
                url.len()
            );
        }
    }
}
