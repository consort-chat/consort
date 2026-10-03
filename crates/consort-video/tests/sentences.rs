// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a camera failure says, and to whom.
//!
//! Every one of these reaches somebody: the interface draws `user_message` in
//! the voice strip, and `Display` goes in the log. A camera that will not start
//! is the most common thing to go wrong here and the hardest to guess at from
//! the outside, so the wording is pinned rather than left to drift.

use consort_video::capture::{Resolution, WANTED};
use consort_video::{
    Camera, CameraDevices, CaptureError, FrameError, NoCameras, PixelFormat, VideoCapture,
};

fn said(error: &CaptureError) -> String {
    error.to_string()
}

mod what_a_camera_failure_says {
    use super::*;

    #[test]
    fn a_machine_with_no_camera_says_so_without_naming_one() {
        assert_eq!(
            said(&CaptureError::NoCamera),
            "there is no camera on this machine"
        );
    }

    #[test]
    fn an_unknown_camera_lists_what_there_is_instead() {
        // The useful half. "No camera at /dev/video9" on its own leaves
        // somebody guessing at what to pick.
        let error = CaptureError::UnknownCamera {
            requested: "/dev/video9".to_owned(),
            available: vec!["/dev/video0".to_owned(), "/dev/video2".to_owned()],
        };

        assert_eq!(
            said(&error),
            "no camera at \"/dev/video9\"; this machine offers /dev/video0, /dev/video2"
        );
    }

    #[test]
    fn an_unknown_camera_on_a_machine_with_none_does_not_offer_an_empty_list() {
        let error = CaptureError::UnknownCamera {
            requested: "/dev/video9".to_owned(),
            available: Vec::new(),
        };

        assert_eq!(said(&error), "no camera at \"/dev/video9\"");
    }

    #[test]
    fn a_camera_offering_nothing_readable_names_what_would_work() {
        let error = CaptureError::NoUsableFormat {
            camera: "Infrared Camera".to_owned(),
        };

        assert!(said(&error).contains("YUYV or MJPEG"), "{}", said(&error));
        assert!(said(&error).contains("Infrared Camera"));
    }

    #[test]
    fn a_busy_camera_says_the_fix_is_elsewhere() {
        // On Linux exactly one process may hold a video node, so this is the
        // failure people actually hit, and the fix is in another application.
        let error = CaptureError::Busy {
            camera: "/dev/video0".to_owned(),
        };

        assert_eq!(
            said(&error),
            "\"/dev/video0\" is already in use by another application"
        );
    }

    #[test]
    fn a_driver_failure_carries_the_driver_s_words_into_the_log() {
        let error = CaptureError::Backend("No such file or directory (os error 2)".to_owned());

        assert_eq!(
            said(&error),
            "the camera failed: No such file or directory (os error 2)"
        );
    }

    #[test]
    fn a_driver_failure_does_not_carry_them_into_the_interface() {
        // The whole reason `user_message` exists separately. "os error 2" is a
        // fact about an ioctl and not something anybody can act on.
        let error = CaptureError::Backend("No such file or directory (os error 2)".to_owned());

        let shown = error.user_message();

        assert_eq!(shown, "the camera could not be started");
        assert!(!shown.contains("os error"));
    }

    #[test]
    fn every_other_failure_reads_the_same_in_both_places() {
        // Only the one carrying a driver's words differs. Writing a second
        // sentence for the rest would be two wordings to keep in step.
        let errors = [
            CaptureError::NoCamera,
            CaptureError::UnknownCamera {
                requested: "/dev/video9".to_owned(),
                available: Vec::new(),
            },
            CaptureError::NoUsableFormat {
                camera: "Lid".to_owned(),
            },
            CaptureError::Busy {
                camera: "Lid".to_owned(),
            },
        ];

        for error in errors {
            assert_eq!(error.user_message(), error.to_string(), "{error:?}");
        }
    }
}

mod what_an_unreadable_frame_says {
    use super::*;

    #[test]
    fn an_empty_format_says_there_are_no_pixels() {
        assert_eq!(
            FrameError::Empty.to_string(),
            "the camera negotiated a frame with no pixels in it"
        );
    }

    #[test]
    fn an_odd_width_names_the_width() {
        assert_eq!(
            FrameError::OddWidth { width: 3 }.to_string(),
            "a YUYV frame cannot be 3 pixels wide"
        );
    }

    #[test]
    fn a_short_frame_names_both_numbers_and_the_format() {
        // Both, because the interesting case is a driver reporting a size it
        // does not fill, and one number cannot show that.
        let error = FrameError::Short {
            wanted: 1_843_200,
            got: 4,
            format: PixelFormat::Yuyv,
        };

        assert_eq!(
            error.to_string(),
            "a YUYV frame wants 1843200 bytes and got 4"
        );
    }

    #[test]
    fn a_refused_jpeg_carries_the_decoder_s_reason() {
        let error = FrameError::Jpeg("unsupported marker".to_owned());

        assert_eq!(
            error.to_string(),
            "the frame was not a usable JPEG: unsupported marker"
        );
    }
}

mod naming_a_pixel_format {
    use super::*;

    #[test]
    fn each_format_reads_as_the_name_people_use_for_it() {
        assert_eq!(PixelFormat::Yuyv.to_string(), "YUYV");
        assert_eq!(PixelFormat::Mjpeg.to_string(), "MJPEG");
    }

    #[test]
    fn the_fourcc_is_the_one_v4l2_reports() {
        // Not the display name. MJPEG's fourcc is MJPG, four characters, and a
        // mismatch here is a camera whose only usable format is never offered.
        assert_eq!(&PixelFormat::Yuyv.fourcc(), b"YUYV");
        assert_eq!(&PixelFormat::Mjpeg.fourcc(), b"MJPG");
    }

    #[test]
    fn a_fourcc_comes_back_as_the_format_it_names() {
        for format in [PixelFormat::Yuyv, PixelFormat::Mjpeg] {
            assert_eq!(PixelFormat::from_fourcc(&format.fourcc()), Some(format));
        }
    }

    #[test]
    fn a_format_this_cannot_read_is_not_claimed() {
        // H.264 is the third thing a C920 offers and nothing here decodes it.
        // Claiming it would mean negotiating a format that produces frames
        // every one of which is dropped.
        assert_eq!(PixelFormat::from_fourcc(b"H264"), None);
        assert_eq!(PixelFormat::from_fourcc(b"NV12"), None);
    }
}

mod a_platform_with_no_backend {
    use super::*;

    #[test]
    fn offers_no_cameras() {
        assert_eq!(NoCameras.enumerate(), Vec::<Camera>::new());
    }

    #[test]
    fn refuses_to_open_one_rather_than_pretending() {
        // Exists because the Windows release build has to compile. A settings
        // screen there should draw an empty picker, and a press should say
        // there is no camera rather than appear to work.
        let refused = NoCameras.open(None, WANTED, Box::new(|_| {}));

        assert!(matches!(refused, Err(CaptureError::NoCamera)));
    }

    #[test]
    fn refuses_a_named_camera_too() {
        let refused = NoCameras.open(
            Some("/dev/video0"),
            Resolution {
                width: 640,
                height: 480,
                fps: 30,
            },
            Box::new(|_| {}),
        );

        assert!(matches!(refused, Err(CaptureError::NoCamera)));
    }
}
