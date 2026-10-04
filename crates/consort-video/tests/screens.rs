// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Deciding what somebody may share, and in what order to offer it.
//!
//! Every case here is a window list as a value, because the decisions are the
//! part worth testing and none of them needs an X server. What a window
//! manager actually reports is `x11_host`'s problem.

use consort_video::{SeenWindow, ShareKind, shareable};

/// A window the window manager would put in a task bar.
fn window(id: u32, title: &str) -> SeenWindow {
    SeenWindow {
        id,
        title: title.to_owned(),
        app: "App".to_owned(),
        pid: Some(1000 + id),
        normal: true,
        skip_taskbar: false,
        fullscreen: false,
        width: 800,
        height: 600,
    }
}

/// The titles `shareable` offers, in the order it offers them.
fn titles(seen: Vec<SeenWindow>, ours: u32) -> Vec<String> {
    shareable(seen, ours)
        .into_iter()
        .map(|source| source.title)
        .collect()
}

#[test]
fn the_most_recently_raised_window_is_offered_first() {
    // `_NET_CLIENT_LIST_STACKING` is bottom to top, so the window somebody was
    // last looking at is the last entry. Offering the list as it arrives puts
    // the least relevant window at the top of the picker.
    let stacked = vec![
        window(1, "Buried"),
        window(2, "Middle"),
        window(3, "On top"),
    ];

    assert_eq!(titles(stacked, 9), vec!["On top", "Middle", "Buried"]);
}

#[test]
fn a_fullscreen_window_is_offered_before_anything_else() {
    // The acceptance criteria's "isolate a video game". Somebody alt-tabbing
    // out of a game to share it has left it below the window they tabbed to,
    // so stacking order alone would bury the one thing they came to share.
    let game = SeenWindow {
        fullscreen: true,
        ..window(1, "Game")
    };
    let stacked = vec![game, window(2, "Notes"), window(3, "Browser")];

    assert_eq!(titles(stacked, 9), vec!["Game", "Browser", "Notes"]);
}

#[test]
fn consorts_own_windows_are_not_offered() {
    // Sharing Consort into the call it is sharing into is a hall of mirrors,
    // and it is also the window somebody is looking at when they open the
    // picker, so without this it would be offered first.
    let ours = SeenWindow {
        pid: Some(4242),
        ..window(1, "Consort")
    };
    let stacked = vec![window(2, "Browser"), ours];

    assert_eq!(titles(stacked, 4242), vec!["Browser"]);
}

#[test]
fn panels_and_docks_are_not_offered() {
    // A desktop panel is a window and is never a thing anybody means to
    // share. Both properties are checked because a window manager may set
    // either: XFCE's panel sets skip_taskbar, and a notification popup
    // announces itself by type instead.
    let panel = SeenWindow {
        skip_taskbar: true,
        ..window(1, "xfce4-panel")
    };
    let popup = SeenWindow {
        normal: false,
        ..window(2, "Notification")
    };
    let stacked = vec![panel, popup, window(3, "Browser")];

    assert_eq!(titles(stacked, 9), vec!["Browser"]);
}

#[test]
fn a_window_with_no_pixels_in_it_is_not_offered() {
    // An unmapped or just-destroyed window reports a zero dimension, and a
    // capture of one has nothing to publish.
    let empty = SeenWindow {
        width: 0,
        ..window(1, "Gone")
    };
    let stacked = vec![empty, window(2, "Browser")];

    assert_eq!(titles(stacked, 9), vec!["Browser"]);
}

#[test]
fn a_window_with_no_title_is_named_after_its_application() {
    // A picker row with nothing in it is a row nobody can choose deliberately,
    // which for a screen share is the one thing that must not happen.
    let untitled = SeenWindow {
        title: String::new(),
        app: "Inkscape".to_owned(),
        ..window(1, "")
    };

    assert_eq!(titles(vec![untitled], 9), vec!["Inkscape"]);
}

#[test]
fn a_window_with_neither_a_title_nor_an_application_is_still_named() {
    let anonymous = SeenWindow {
        title: String::new(),
        app: String::new(),
        ..window(1, "")
    };

    let offered = shareable(vec![anonymous], 9);

    assert_eq!(offered.len(), 1);
    assert!(
        !offered[0].title.trim().is_empty(),
        "an unnamed window was offered with an empty label"
    );
}

#[test]
fn a_window_whose_pid_is_unknown_is_still_offered() {
    // `_NET_WM_PID` is a convention rather than a requirement, and a window
    // that does not set it is an ordinary window. Dropping it would hide
    // whole applications from the picker.
    let anonymous = SeenWindow {
        pid: None,
        ..window(1, "Xterm")
    };

    assert_eq!(titles(vec![anonymous], 9), vec!["Xterm"]);
}

#[test]
fn every_window_is_offered_as_a_window_rather_than_a_screen() {
    // The two tabs read this, so a window arriving under the wrong kind is a
    // window in the Screens tab.
    let offered = shareable(vec![window(1, "Browser")], 9);

    assert_eq!(offered[0].kind, ShareKind::Window);
}

#[test]
fn what_is_offered_carries_the_id_a_capture_will_be_asked_for() {
    // The picker hands this back to start a share, so an id that does not
    // survive the trip is a share of the wrong window.
    let offered = shareable(vec![window(7, "Browser")], 9);

    assert_eq!(offered[0].id, "window:7");
}

mod what_goes_wrong {
    use consort_video::ShareError;

    #[test]
    fn a_build_with_no_backend_refuses_to_list_rather_than_offering_nothing() {
        // #163. An empty list draws "there is nothing here to share", which on
        // a desktop full of windows is false. Refusing draws the reason.
        use consort_video::{NoScreens, ScreenCapture};

        assert_eq!(NoScreens.sources(), Err(ShareError::Unsupported));
    }

    #[test]
    fn a_build_with_no_backend_says_so_rather_than_blaming_the_display() {
        // Any platform without a host. Somebody there should be told the
        // feature is not built rather than sent to debug their display server.
        let said = ShareError::Unsupported.user_message();

        assert!(
            said.contains("this build"),
            "{said:?} does not say the build is what is missing"
        );
    }

    #[test]
    fn wayland_is_named_as_the_reason_rather_than_reported_as_a_failure() {
        // The finding in #70: there is no X11 to read under Wayland, and the
        // portal route is not built. Somebody on Wayland needs to know that
        // this is a missing path and not a broken one.
        let said = ShareError::NoX11.user_message();

        assert!(
            said.to_lowercase().contains("wayland"),
            "{said:?} does not mention Wayland"
        );
    }

    #[test]
    fn a_window_that_closed_while_the_picker_was_open_is_its_own_sentence() {
        // The ordinary race, not an edge case: a picker is on screen for
        // seconds and windows close in that time. The fix is to pick again,
        // which the sentence should say.
        let said = ShareError::Gone {
            source: "window:7".to_owned(),
        }
        .user_message();

        assert!(
            said.to_lowercase().contains("no longer"),
            "{said:?} does not say the window has gone"
        );
    }

    #[test]
    fn a_display_servers_own_words_do_not_reach_the_interface() {
        // The same rule `CaptureError` keeps: "BadWindow (invalid Window
        // parameter)" is a fact about a protocol request and not something
        // anybody can act on.
        let raw = "BadWindow (invalid Window parameter)";
        let failure = ShareError::Backend(raw.to_owned());

        assert!(
            !failure.user_message().contains(raw),
            "the X error reached the interface"
        );
        assert!(
            failure.to_string().contains(raw),
            "the X error is missing from the log line"
        );
    }
}

/// What only a real display can answer.
///
/// `#[ignore]` on the same terms as the keyring and homeserver tests: CI has
/// no X server, so these are run by hand. They are the coverage for
/// `x11_host`, which is excluded from measurement for exactly that reason.
///
/// Run with `cargo test -p consort-video --test screens -- --ignored`.
#[cfg(target_os = "linux")]
mod against_a_real_display {
    use consort_video::{ScreenCapture, Screens, ShareKind};
    use std::sync::{Arc, Mutex};

    #[test]
    #[ignore = "needs an X11 display"]
    fn this_machine_offers_at_least_one_screen() {
        let offered = Screens::default().sources().expect("no X11 display");

        let screens: Vec<_> = offered
            .iter()
            .filter(|source| source.kind == ShareKind::Screen)
            .collect();

        assert!(!screens.is_empty(), "RandR reported no monitors");
        for found in screens {
            assert!(found.width > 0 && found.height > 0, "{found:?} has no size");
            assert!(found.id.starts_with("screen:"), "{found:?}");
        }
    }

    #[test]
    #[ignore = "needs an X11 display"]
    fn every_window_offered_can_be_named_and_measured() {
        let offered = Screens::default().sources().expect("no X11 display");

        for found in &offered {
            assert!(!found.title.trim().is_empty(), "{found:?} has no label");
            assert!(found.width > 0 && found.height > 0, "{found:?} has no size");
        }
    }

    /// Wait for `enough` frames, or give up.
    ///
    /// Polled rather than slept through, because one frame costs 66ms in a
    /// release build and 517ms in a debug one: a fixed sleep either wastes a
    /// second or fails on the slower profile. Measured on a 2560x1440 monitor.
    fn wait_for<T>(frames: &Arc<Mutex<Vec<T>>>, enough: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while frames.lock().unwrap().len() < enough && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// The first monitor this machine offers.
    fn a_screen() -> (Screens, consort_video::ShareSource) {
        let host = Screens::default();
        let first = host
            .sources()
            .expect("no X11 display")
            .into_iter()
            .find(|source| source.kind == ShareKind::Screen)
            .expect("no monitors");
        (host, first)
    }

    #[test]
    #[ignore = "needs an X11 display"]
    fn a_screen_capture_produces_frames_of_the_size_that_screen_was_offered_as() {
        // Measured against the source the picker listed, not against whatever
        // the stream reports about itself. Those two agreeing proves nothing:
        // capturing the whole root window instead of the chosen monitor is
        // self-consistent and shares every screen on the machine.
        //
        // That was the bug. On two monitors, choosing one published both.
        let (host, first) = a_screen();

        let frames = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&frames);
        let share = host
            .open(
                &first.id,
                Box::new(move |picture| {
                    seen.lock()
                        .unwrap()
                        .push((picture.width, picture.height, picture.y.len()));
                }),
            )
            .expect("the screen would not open");

        assert_eq!(
            (share.resolution().width, share.resolution().height),
            (first.width, first.height),
            "{} was offered at one size and is capturing at another",
            first.title
        );

        wait_for(&frames, 1);
        drop(share);

        let captured = frames.lock().unwrap().clone();
        assert!(!captured.is_empty(), "no frames arrived in five seconds");
        for (width, height, luma) in &captured {
            assert_eq!(
                (*width, *height),
                (first.width, first.height),
                "a frame arrived at a different size from the screen that was chosen"
            );
            assert_eq!(
                *luma,
                (width * height) as usize,
                "the luma plane is the wrong size"
            );
        }
    }

    #[test]
    #[ignore = "needs an X11 display"]
    fn each_screen_captures_only_itself() {
        // The multi-monitor case, and the reason the test above measures
        // against the listing. Two monitors share one root window, so a
        // capture that reads the root reads both of them.
        let host = Screens::default();
        let screens: Vec<_> = host
            .sources()
            .expect("no X11 display")
            .into_iter()
            .filter(|source| source.kind == ShareKind::Screen)
            .collect();

        for found in &screens {
            let share = host
                .open(&found.id, Box::new(|_| {}))
                .expect("a listed screen would not open");

            assert_eq!(
                (share.resolution().width, share.resolution().height),
                (found.width, found.height),
                "{} captures a different rectangle from the one it was offered as",
                found.title
            );
        }
    }

    #[test]
    #[ignore = "needs an X11 display"]
    fn dropping_a_share_stops_the_frames_before_it_returns() {
        // The security property, measured rather than assumed. Somebody
        // pressing stop has said no more of their screen may leave this
        // machine, and a frame delivered after the drop returned is one that
        // did. A grab takes long enough that a stop lands inside one, so this
        // is the ordinary case rather than a narrow race.
        let (host, first) = a_screen();

        let frames: Arc<Mutex<Vec<()>>> = Arc::new(Mutex::new(Vec::new()));
        let after_stop = Arc::new(Mutex::new(0usize));
        let stopping = Arc::new(Mutex::new(false));

        let seen = Arc::clone(&frames);
        let counted = Arc::clone(&after_stop);
        let watching = Arc::clone(&stopping);
        let share = host
            .open(
                &first.id,
                Box::new(move |_| {
                    if *watching.lock().unwrap() {
                        *counted.lock().unwrap() += 1;
                    }
                    seen.lock().unwrap().push(());
                }),
            )
            .expect("the screen would not open");

        // Waited for, so the capture is known to be running and mid-grab when
        // the stop lands. Without this the share could be dropped before the
        // thread ever reached a frame, and the test would pass having proved
        // nothing.
        wait_for(&frames, 1);
        *stopping.lock().unwrap() = true;
        drop(share);

        assert_eq!(
            *after_stop.lock().unwrap(),
            0,
            "a frame was delivered after the share was told to stop"
        );
    }

    #[test]
    #[ignore = "needs an X11 display"]
    fn a_source_that_is_not_there_is_refused_rather_than_captured() {
        let refused = Screens::default().open("window:1", Box::new(|_| {}));

        assert!(refused.is_err(), "a window id nobody owns opened");
    }
}
