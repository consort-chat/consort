// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The icon in the system tray, and getting the window back from it.
//!
//! A chat client is left running, and on a desktop that hides minimised windows
//! there is otherwise nothing left of Consort to click: the taskbar entry is
//! gone, the launcher starts a second copy that hands its arguments to the first
//! and exits, and the window stays where it was. The tray is what is still
//! there.
//!
//! ## Everything here is a menu item
//!
//! There is no click handler, and that is not an omission. On Linux the tray is
//! an `AppIndicator`, and the StatusNotifierItem protocol behind it delivers a
//! menu rather than clicks: `on_tray_icon_event` is never called, on any
//! desktop, and a tray whose only affordance was a click would do nothing at
//! all. The menu is the one interaction every platform has, so it is the only
//! one built.
//!
//! ## The library is opened by name, not linked
//!
//! `libappindicator-sys` reaches `libayatana-appindicator3.so.1` with
//! `libloading` instead of linking it, which has two consequences worth knowing.
//!
//! It does not appear in `readelf -d`, so the rule the Arch recipes follow for
//! deriving `depends` cannot see it and both recipes name it by hand.
//!
//! And a machine without it gets no link error, because there is no link. It
//! gets a panic, the first time anything touches the library, which is inside
//! the tray builder during startup. Losing the whole client to a missing tray
//! icon is not a trade worth making, so [`install`] catches it and Consort
//! comes up without a tray and says why.

use std::panic::AssertUnwindSafe;

use tauri::menu::{IsMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

use crate::notify::Front;

/// Closing Consort.
///
/// A trait for the reason [`Front`] is one, and for one more: the only
/// implementation is an `AppHandle`, and asking it to exit ends the process, so
/// a test that reached the real one would take the test run with it.
pub trait Quit: Send + Sync {
    /// Close Consort.
    fn quit(&self);
}

impl<R: Runtime> Quit for AppHandle<R> {
    fn quit(&self) {
        // As abrupt as `commands::quit` and the window's own close button. See
        // the comment there for what that costs.
        self.exit(0);
    }
}

/// What somebody asked the tray for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    /// Bring the window back.
    Raise,
    /// Close Consort.
    Quit,
}

impl Wanted {
    /// The identifier this travels under.
    ///
    /// A menu event carries the id of the item that was clicked and nothing
    /// else, so this is the whole vocabulary between the menu and the handler.
    const fn id(self) -> &'static str {
        match self {
            Self::Raise => "tray-show",
            Self::Quit => "tray-quit",
        }
    }
}

/// The menu, in the order it is drawn.
///
/// The labels say Consort rather than "Show" and "Quit" alone, because this
/// menu is drawn among every other application's and a bare "Quit" in a tray is
/// a question about which program.
const MENU: [(Wanted, &str); 2] = [
    (Wanted::Raise, "Show Consort"),
    (Wanted::Quit, "Quit Consort"),
];

/// What a menu item id asks for.
///
/// `None` for anything else. The handler registered below is global rather than
/// per-menu: Tauri delivers every menu event in the process to it, so an id from
/// somewhere else is an ordinary thing to be handed and not an error.
fn wanted(id: &str) -> Option<Wanted> {
    MENU.iter()
        .map(|(wanted, _)| *wanted)
        .find(|wanted| wanted.id() == id)
}

/// Put Consort in the system tray.
///
/// Never fatal. A desktop with nowhere to put a tray icon is a desktop Consort
/// still runs on, so every way this can fail is a log line.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    // `build` runs the tray builder on the main thread, and during `setup` that
    // is this thread, so the panic described at the top of this file unwinds
    // through here and can be caught. Called from anywhere else it would be
    // raised on the event loop instead, where nothing could catch it.
    match std::panic::catch_unwind(AssertUnwindSafe(|| build(app))) {
        Ok(Ok(())) => tracing::info!("Consort is in the system tray"),
        Ok(Err(error)) => {
            tracing::warn!(%error, "this desktop would not take a tray icon");
        }
        Err(_) => {
            tracing::warn!(
                "no libayatana-appindicator3 on this machine, so there is no tray icon. \
                 Install it to get one."
            );
        }
    }
}

/// Put the icon up, with its menu under it.
///
/// The built icon is dropped on the way out and that is not a leak: Tauri keeps
/// a reference-counted clone in the application's own resources, which is what
/// holds it open for the life of the process.
fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let items = MENU
        .iter()
        .map(|(wanted, label)| MenuItem::with_id(app, wanted.id(), label, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let menu = Menu::with_items(
        app,
        &items
            .iter()
            .map(|item| item as &dyn IsMenuItem<R>)
            .collect::<Vec<_>>(),
    )?;

    let mut tray = TrayIconBuilder::new()
        // Nothing on Linux draws this: an AppIndicator has no tooltip and the
        // call is a no-op there. It is what names the icon on Windows and
        // macOS, where hovering is how somebody finds out whose it is.
        .tooltip("Consort")
        .menu(&menu)
        .on_menu_event(|app, event| act(wanted(event.id().as_ref()), app, app));

    // The window's icon, which is the bundled one. A tray icon with no picture
    // in it is a gap in a row of other applications' icons, so a build without
    // one is worth saying out loud rather than putting up.
    match app.default_window_icon() {
        Some(icon) => tray = tray.icon(icon.clone()),
        None => tracing::warn!("this build bundles no icon, so the tray icon will be blank"),
    }

    tray.build(app)?;
    Ok(())
}

/// Do what was asked.
fn act(wanted: Option<Wanted>, front: &dyn Front, quit: &dyn Quit) {
    match wanted {
        Some(Wanted::Raise) => front.raise(),
        Some(Wanted::Quit) => quit.quit(),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    /// Both ends of what the tray can do, and nothing else.
    #[derive(Default)]
    struct Spy {
        raised: AtomicBool,
        quit: AtomicBool,
    }

    impl Front for Spy {
        fn raise(&self) {
            self.raised.store(true, Ordering::Relaxed);
        }
    }

    impl Quit for Spy {
        fn quit(&self) {
            self.quit.store(true, Ordering::Relaxed);
        }
    }

    impl Spy {
        fn was_raised(&self) -> bool {
            self.raised.load(Ordering::Relaxed)
        }

        fn was_quit(&self) -> bool {
            self.quit.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn show_consort_brings_the_window_back() {
        let spy = Spy::default();

        act(wanted(Wanted::Raise.id()), &spy, &spy);

        assert!(spy.was_raised());
        assert!(!spy.was_quit(), "showing the window must not close Consort");
    }

    #[test]
    fn quit_consort_closes_consort() {
        let spy = Spy::default();

        act(wanted(Wanted::Quit.id()), &spy, &spy);

        assert!(spy.was_quit());
        assert!(!spy.was_raised());
    }

    #[test]
    fn an_item_from_another_menu_does_neither() {
        let spy = Spy::default();

        act(wanted("file-save"), &spy, &spy);

        assert!(!spy.was_raised());
        assert!(!spy.was_quit());
    }

    #[test]
    fn every_item_the_menu_draws_comes_back_as_what_it_says() {
        for (item, label) in MENU {
            assert_eq!(wanted(item.id()), Some(item), "{label}");
        }
    }

    #[test]
    fn an_id_from_another_menu_asks_for_nothing() {
        // The handler is global, so this is not a hypothetical: every menu
        // event in the process arrives at it.
        assert_eq!(wanted("file-save"), None);
        assert_eq!(wanted(""), None);
    }

    #[test]
    fn the_menu_can_get_the_window_back() {
        // The reason the tray is worth having at all, and the one item that
        // cannot be dropped: on Linux a click reaches nothing, so a menu with
        // no way back to the window leaves a minimised Consort unreachable.
        assert!(MENU.iter().any(|(wanted, _)| *wanted == Wanted::Raise));
    }

    #[test]
    fn quitting_is_not_the_same_item_as_showing() {
        // Both ids are written out by hand in one `match`, which is exactly the
        // shape a copy-paste survives. Two items sharing an id would leave the
        // first one answering for both, and "Quit Consort" showing the window
        // is the harmless half of that mistake.
        assert_ne!(Wanted::Raise.id(), Wanted::Quit.id());
    }

    #[test]
    fn every_label_names_the_application() {
        // A tray menu is drawn among every other application's items, often
        // with nothing but the icon above it to say whose it is.
        for (_, label) in MENU {
            assert!(label.contains("Consort"), "{label}");
        }
    }
}
