// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What Windows says about its cameras and windows, and what that is taken to
//! mean.
//!
//! Compiled and run on every platform, so the Linux CI that never builds the
//! Windows hosts still checks every decision they make. What Windows actually
//! reports is `mf_host`'s and `wgc_host`'s problem.

use consort_video::screens::shareable;
use consort_video::win32::{
    Rect, TopLevel, camera_failure, frame_rate, frame_size, media_subtype, monitor_name, seen,
};
use consort_video::{CaptureError, PixelFormat};

mod camera_formats {
    use super::*;

    #[test]
    fn the_three_formats_this_can_read_are_recognised_by_their_subtype() {
        // A Media Foundation video subtype is a GUID whose first field is a
        // FourCC, and Windows spells YUYV the old way.
        assert_eq!(media_subtype(*b"YUY2"), Some(PixelFormat::Yuyv));
        assert_eq!(media_subtype(*b"MJPG"), Some(PixelFormat::Mjpeg));
        assert_eq!(media_subtype(*b"NV12"), Some(PixelFormat::Nv12));
    }

    #[test]
    fn a_format_nothing_here_decodes_is_not_offered() {
        assert_eq!(media_subtype(*b"H264"), None);
        assert_eq!(
            media_subtype(*b"YUYV"),
            None,
            "V4L2's spelling, not Windows'"
        );
    }

    #[test]
    fn a_frame_size_is_width_over_height_in_one_u64() {
        assert_eq!(frame_size((1280 << 32) | 720), (1280, 720));
    }

    #[test]
    fn a_frame_rate_is_numerator_over_denominator_in_one_u64() {
        assert_eq!(frame_rate((30 << 32) | 1), 30);
        assert_eq!(frame_rate((15 << 32) | 2), 8, "7.5 rounds up");
    }

    #[test]
    fn ntsc_thirty_counts_as_thirty() {
        // 30000/1001 is 29.97. Truncated, it is 29, and `choose_offer` ranks
        // anything under the 30 it wants below everything that reaches it, so
        // a camera offering only NTSC rates would be ranked as too slow.
        assert_eq!(frame_rate((30_000 << 32) | 1001), 30);
    }

    #[test]
    fn a_rate_with_no_denominator_is_taken_at_thirty() {
        // What `v4l_host` does with a driver that reports no interval, for the
        // same reason: unknown is not zero, and zero would rank it last.
        assert_eq!(frame_rate(30 << 32), 30);
    }
}

mod camera_failures {
    use super::*;

    const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
    const MF_E_HW_MFT_FAILED_START_STREAMING: i32 = 0xC00D_3704_u32 as i32;
    const MF_E_VIDEO_DEVICE_LOCKED: i32 = 0xC00D_4E24_u32 as i32;
    const MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED: i32 = 0xC00D_3EA3_u32 as i32;

    #[test]
    fn access_denied_is_the_privacy_switch_and_says_where_it_is() {
        // The first thing a Windows user with a working camera is likely to
        // hit, and nothing about "access denied" says it is a setting.
        let failure = camera_failure(E_ACCESSDENIED, "HD Pro Webcam C920");

        assert_eq!(failure, CaptureError::Blocked);
        assert!(failure.to_string().contains("Privacy"), "{failure}");
        assert_eq!(failure.user_message(), failure.to_string());
    }

    #[test]
    fn a_camera_another_application_holds_is_busy() {
        for code in [
            MF_E_HW_MFT_FAILED_START_STREAMING,
            MF_E_VIDEO_DEVICE_LOCKED,
            MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED,
        ] {
            assert_eq!(
                camera_failure(code, "HD Pro Webcam C920"),
                CaptureError::Busy {
                    camera: "HD Pro Webcam C920".to_owned()
                },
                "{code:#x}"
            );
        }
    }

    #[test]
    fn anything_else_is_logged_rather_than_shown() {
        let failure = camera_failure(0x8000_4005_u32 as i32, "C920");

        assert!(matches!(failure, CaptureError::Backend(_)));
        assert!(failure.to_string().contains("0x80004005"), "{failure}");
        assert_eq!(failure.user_message(), "the camera could not be started");
    }
}

mod windows_on_the_taskbar {
    use super::*;

    const MONITOR: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    /// An ordinary application window: visible, unowned, half the screen.
    fn window(handle: u32, title: &str) -> TopLevel {
        TopLevel {
            handle,
            title: title.to_owned(),
            app: "notepad".to_owned(),
            pid: 100,
            visible: true,
            cloaked: false,
            minimised: false,
            tool_window: false,
            app_window: false,
            owned: false,
            bounds: Rect {
                left: 100,
                top: 100,
                right: 1060,
                bottom: 640,
            },
            monitor: MONITOR,
        }
    }

    fn offered(top_first: Vec<TopLevel>) -> Vec<String> {
        shareable(seen(top_first), 1)
            .into_iter()
            .map(|source| source.title)
            .collect()
    }

    #[test]
    fn an_ordinary_window_is_offered_at_its_own_size() {
        let listed = shareable(seen(vec![window(7, "notes.txt")]), 1);

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "window:7");
        assert_eq!((listed[0].width, listed[0].height), (960, 540));
        assert!(!listed[0].fullscreen);
    }

    #[test]
    fn the_topmost_window_is_still_offered_first() {
        // `EnumWindows` reports topmost first, and `shareable` takes X11's
        // bottom-first order. Handed over unreversed, the window somebody is
        // looking at would be offered last.
        let listed = offered(vec![
            window(1, "front"),
            window(2, "middle"),
            window(3, "back"),
        ]);

        assert_eq!(listed, ["front", "middle", "back"]);
    }

    #[test]
    fn a_window_covering_its_monitor_is_fullscreen() {
        let game = TopLevel {
            bounds: MONITOR,
            ..window(9, "Game")
        };

        let listed = offered(vec![window(1, "browser"), game]);

        assert_eq!(listed, ["Game", "browser"], "the game is offered first");
    }

    #[test]
    fn a_window_on_a_second_monitor_is_fullscreen_against_that_monitor() {
        let right = Rect {
            left: 1920,
            top: 0,
            right: 4480,
            bottom: 1440,
        };
        let game = TopLevel {
            bounds: right,
            monitor: right,
            ..window(9, "Game")
        };

        assert!(shareable(seen(vec![game]), 1)[0].fullscreen);
    }

    #[test]
    fn hidden_cloaked_and_minimised_windows_are_not_offered() {
        // Cloaked is how Windows hides a suspended store app and a window on
        // another virtual desktop, both of which report themselves visible. A
        // minimised window produces no frames, so offering it would start a
        // share that shows nothing.
        let listed = offered(vec![
            TopLevel {
                visible: false,
                ..window(1, "hidden")
            },
            TopLevel {
                cloaked: true,
                ..window(2, "cloaked")
            },
            TopLevel {
                minimised: true,
                ..window(3, "minimised")
            },
            window(4, "shown"),
        ]);

        assert_eq!(listed, ["shown"]);
    }

    #[test]
    fn tool_windows_and_owned_popups_are_not_offered() {
        let listed = offered(vec![
            TopLevel {
                tool_window: true,
                ..window(1, "palette")
            },
            TopLevel {
                owned: true,
                ..window(2, "Find and Replace")
            },
            window(3, "editor"),
        ]);

        assert_eq!(listed, ["editor"]);
    }

    #[test]
    fn an_app_window_is_offered_whatever_else_it_says() {
        // `WS_EX_APPWINDOW` is how a window asks for a taskbar button it would
        // otherwise not get, and the taskbar gives it one.
        let listed = offered(vec![TopLevel {
            owned: true,
            tool_window: true,
            app_window: true,
            ..window(1, "Settings")
        }]);

        assert_eq!(listed, ["Settings"]);
    }

    #[test]
    fn an_untitled_window_is_named_after_its_application() {
        let listed = offered(vec![window(1, "")]);

        assert_eq!(listed, ["notepad"]);
    }

    #[test]
    fn consorts_own_windows_are_left_out_by_pid() {
        let ours = TopLevel {
            pid: 1,
            ..window(1, "Consort")
        };

        assert_eq!(offered(vec![ours, window(2, "notes")]), ["notes"]);
    }
}

mod monitors {
    use super::*;

    #[test]
    fn a_monitor_is_named_without_the_device_namespace() {
        assert_eq!(monitor_name(r"\\.\DISPLAY1"), "DISPLAY1");
    }

    #[test]
    fn a_name_without_the_namespace_is_kept_as_it_is() {
        assert_eq!(monitor_name("DISPLAY2"), "DISPLAY2");
    }
}
