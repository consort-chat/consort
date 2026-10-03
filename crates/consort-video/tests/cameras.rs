// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Matching a saved camera against what is plugged in, and deciding what to
//! ask that camera for.
//!
//! No device behind any of it. The host is a trait, so the awkward cases are
//! fixtures: the camera that was unplugged between runs, the laptop whose
//! second video node is a metadata node, and the webcam that offers 720p at
//! ten frames a second.

use consort_video::capture::{Offer, Resolution, WANTED, choose_offer};
use consort_video::{
    Camera, CameraDevices, CameraList, NoCameras, PixelFormat, Selection, catalogue, choose,
};

/// A machine with exactly the cameras a test says it has.
struct Fake(Vec<Camera>);

impl CameraDevices for Fake {
    fn enumerate(&self) -> Vec<Camera> {
        self.0.clone()
    }
}

fn camera(id: &str, name: &str) -> Camera {
    Camera {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn offer(format: PixelFormat, width: u32, height: u32, fps: u32) -> Offer {
    Offer {
        format,
        width,
        height,
        fps,
    }
}

mod choosing {
    use super::*;

    #[test]
    fn a_saved_camera_that_is_plugged_in_is_the_one_chosen() {
        let available = [camera("/dev/video0", "Lid"), camera("/dev/video2", "C920")];

        let chosen = choose(&available, Some("/dev/video2"));

        assert_eq!(chosen, Selection::Saved(camera("/dev/video2", "C920")));
    }

    #[test]
    fn with_nothing_saved_the_first_camera_is_used() {
        let available = [camera("/dev/video0", "Lid"), camera("/dev/video2", "C920")];

        let chosen = choose(&available, None);

        assert_eq!(chosen, Selection::First(camera("/dev/video0", "Lid")));
    }

    #[test]
    fn a_saved_camera_that_has_been_unplugged_is_reported_not_silently_replaced() {
        // The case worth having a name for. Somebody who chose an external
        // camera and is now being filmed by a laptop lid has to be told, and
        // told before a call rather than during one.
        let available = [camera("/dev/video0", "Lid")];

        let chosen = choose(&available, Some("/dev/video2"));

        assert_eq!(
            chosen,
            Selection::Substituted {
                wanted: "/dev/video2".to_owned(),
                using: camera("/dev/video0", "Lid"),
            }
        );
    }

    #[test]
    fn a_machine_with_no_camera_chooses_nothing() {
        let chosen = choose(&[], Some("/dev/video0"));

        assert_eq!(chosen, Selection::Nothing);
    }

    #[test]
    fn a_blank_saved_id_is_treated_as_nothing_saved() {
        // A settings file somebody edited by hand, not a camera that has been
        // unplugged, so it must not be reported as missing.
        let available = [camera("/dev/video0", "Lid")];

        let chosen = choose(&available, Some("   "));

        assert_eq!(chosen, Selection::First(camera("/dev/video0", "Lid")));
    }

    #[test]
    fn a_near_miss_is_a_miss_rather_than_a_match() {
        // The saved id came from this same list, so a close one means the node
        // moved rather than that it needs guessing at.
        let available = [camera("/dev/video10", "C920")];

        let chosen = choose(&available, Some("/dev/video1"));

        assert!(matches!(chosen, Selection::Substituted { .. }));
    }
}

mod the_catalogue {
    use super::*;

    #[test]
    fn one_node_listed_twice_appears_once() {
        let host = Fake(vec![
            camera("/dev/video0", "C920"),
            camera("/dev/video0", "C920"),
        ]);

        assert_eq!(catalogue(&host), vec![camera("/dev/video0", "C920")]);
    }

    #[test]
    fn two_identical_webcams_are_both_listed_because_the_node_tells_them_apart() {
        // The difference from the audio catalogue, which cannot do this: cpal
        // offers only a display name, so identical twins collapse. A camera
        // has a device node.
        let host = Fake(vec![
            camera("/dev/video0", "C920"),
            camera("/dev/video2", "C920"),
        ]);

        assert_eq!(catalogue(&host).len(), 2);
    }

    #[test]
    fn a_node_with_no_name_is_dropped() {
        let host = Fake(vec![
            camera("/dev/video0", "  "),
            camera("/dev/video2", "C920"),
        ]);

        assert_eq!(catalogue(&host), vec![camera("/dev/video2", "C920")]);
    }

    #[test]
    fn device_order_is_kept() {
        let host = Fake(vec![
            camera("/dev/video2", "C920"),
            camera("/dev/video0", "Lid"),
        ]);

        let listed = catalogue(&host);

        assert_eq!(listed[0].id, "/dev/video2");
    }

    #[test]
    fn a_platform_with_no_backend_offers_an_empty_list_rather_than_failing() {
        assert_eq!(catalogue(&NoCameras), Vec::new());
    }
}

mod the_list_the_screen_draws {
    use super::*;

    #[test]
    fn it_names_the_camera_that_will_actually_be_opened() {
        let list = CameraList::of(vec![camera("/dev/video0", "Lid")], None);

        assert_eq!(list.selected.as_deref(), Some("/dev/video0"));
        assert_eq!(list.missing, None);
    }

    #[test]
    fn it_carries_the_missing_camera_so_the_screen_can_say_so() {
        let list = CameraList::of(vec![camera("/dev/video0", "Lid")], Some("/dev/video2"));

        assert_eq!(list.selected.as_deref(), Some("/dev/video0"));
        assert_eq!(list.missing.as_deref(), Some("/dev/video2"));
    }

    #[test]
    fn with_no_cameras_nothing_is_selected_and_nothing_is_missing() {
        // No substitution to report, only an empty machine. Reporting the
        // saved name as missing here would ask somebody to plug in a camera
        // they already have.
        let list = CameraList::of(Vec::new(), Some("/dev/video2"));

        assert_eq!(list.selected, None);
        assert_eq!(list.missing, None);
    }

    #[test]
    fn it_goes_over_the_wire_as_camel_case() {
        let list = CameraList::of(vec![camera("/dev/video0", "Lid")], None);

        let json = serde_json::to_value(&list).unwrap();

        assert_eq!(json["cameras"][0]["id"], "/dev/video0");
        assert_eq!(json["selected"], "/dev/video0");
    }
}

mod choosing_a_format {
    use super::*;

    #[test]
    fn mjpeg_wins_at_720p_because_yuyv_only_reaches_ten_frames_a_second() {
        // Measured on a C920, which is the common case rather than an edge
        // one: a webcam's uncompressed modes are USB bandwidth limited. Ten
        // frames a second is not a video call, so sharpness loses to motion.
        let offers = [
            offer(PixelFormat::Yuyv, 1280, 720, 10),
            offer(PixelFormat::Mjpeg, 1280, 720, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!(chosen.format, PixelFormat::Mjpeg);
        assert_eq!((chosen.width, chosen.height), (1280, 720));
    }

    #[test]
    fn yuyv_wins_a_tie_because_nothing_has_to_decode_it() {
        let offers = [
            offer(PixelFormat::Mjpeg, 1280, 720, 30),
            offer(PixelFormat::Yuyv, 1280, 720, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!(chosen.format, PixelFormat::Yuyv);
    }

    #[test]
    fn a_camera_that_cannot_reach_the_target_rate_degrades_rather_than_refusing() {
        let offers = [offer(PixelFormat::Yuyv, 1280, 720, 15)];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!(chosen.fps, 15, "a slower camera beats no camera");
    }

    #[test]
    fn the_nearest_size_wins_among_everything_fast_enough() {
        let offers = [
            offer(PixelFormat::Yuyv, 320, 240, 30),
            offer(PixelFormat::Yuyv, 640, 480, 30),
            offer(PixelFormat::Yuyv, 1280, 720, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!((chosen.width, chosen.height), (1280, 720));
    }

    #[test]
    fn the_nearest_size_can_be_the_one_below_the_target() {
        // 480p is 614k pixels from the target and 1080p is 1152k, so nearest
        // picks 480p. Deliberate: the rule is symmetric and therefore errs
        // toward less to encode, which on a voice-first client is the right
        // way to be wrong.
        let offers = [
            offer(PixelFormat::Yuyv, 640, 480, 30),
            offer(PixelFormat::Mjpeg, 1920, 1080, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!((chosen.width, chosen.height), (640, 480));
    }

    #[test]
    fn a_size_above_the_target_is_taken_when_it_really_is_the_nearest() {
        // The other side of the same rule, so neither direction is special
        // cased: 1280x1024 is nearer to 720p than 640x360 is.
        let offers = [
            offer(PixelFormat::Yuyv, 640, 360, 30),
            offer(PixelFormat::Yuyv, 1280, 1024, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!((chosen.width, chosen.height), (1280, 1024));
    }

    #[test]
    fn frame_rate_outranks_size() {
        // The ordering this whole function exists for. A 720p mode at ten
        // frames a second is the exact size asked for and still loses to a
        // smaller mode that moves.
        let offers = [
            offer(PixelFormat::Yuyv, 1280, 720, 10),
            offer(PixelFormat::Yuyv, 640, 480, 30),
        ];

        let chosen = choose_offer(&offers, WANTED).unwrap();

        assert_eq!((chosen.width, chosen.height), (640, 480));
    }

    #[test]
    fn a_camera_offering_nothing_readable_is_chosen_from_at_all() {
        assert_eq!(choose_offer(&[], WANTED), None);
    }

    #[test]
    fn a_lower_target_takes_the_cheap_format() {
        // Not hypothetical: the target is one constant, and this is what
        // moving it down buys.
        let offers = [
            offer(PixelFormat::Yuyv, 640, 480, 30),
            offer(PixelFormat::Mjpeg, 1280, 720, 30),
        ];

        let chosen = choose_offer(
            &offers,
            Resolution {
                width: 640,
                height: 480,
                fps: 30,
            },
        )
        .unwrap();

        assert_eq!(chosen.format, PixelFormat::Yuyv);
    }
}
