// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Telling somebody about a message when Consort is not the window in front.
//!
//! `consort_matrix::notifications` decides what is worth interrupting somebody
//! for, from the account's own push rules. This is the other half: whether to
//! interrupt them *now*, and how a desktop draws it.
//!
//! ## notify-rust rather than the Tauri plugin
//!
//! `tauri-plugin-notification` is the obvious dependency and it is not the one
//! used here. Its desktop path builds exactly the notify-rust call below and
//! then throws away the handle that comes back, which is the only thing that
//! can say a notification was clicked. A notification you cannot click is a
//! notification that tells you something happened and then makes you find it
//! yourself, and finding it is the whole job.
//!
//! What the plugin does that is worth keeping is one thing, and it is copied
//! here: on Windows a toast with no AppUserModelID is attributed to whatever
//! launched the process, which for an installed application is nothing at all
//! and means the toast is silently dropped. [`APP_ID`] is that identifier, and
//! it has to keep matching the shortcut the NSIS installer writes.
//!
//! ## Unverified on Windows
//!
//! Said plainly because it is the risk the issue behind this work named: none
//! of this has been run on Windows. The Linux path is over DBus to whatever
//! notification daemon the desktop runs, and that is what has been exercised.

use std::sync::Arc;

use consort_matrix::Notification;
use serde::{Deserialize, Serialize};

use crate::events::{AppEvent, EventSink};

/// The identifier a Windows toast is attributed to.
///
/// Only read on Windows, hence the attribute: the constant is here on every
/// platform so that the reason it exists is read by everybody working on this
/// file rather than only by whoever is building for Windows.
///
/// The same string as `tauri.conf.json`'s `identifier`, and it has to stay
/// that way: Windows matches it against the shortcut the installer wrote, and
/// a toast whose identifier matches no shortcut is dropped without an error.
///
/// Not read from the Tauri config, which would need an `AppHandle` here and
/// make every function below untestable for the sake of one constant.
#[cfg_attr(not(windows), allow(dead_code))]
const APP_ID: &str = "chat.consort.desktop";

/// What this is called in the desktop's own notification list.
const APP_NAME: &str = "Consort";

/// The sound to ask for, when the push rules asked for one.
///
/// A name rather than a file. Both desktops keep a themed sound for exactly
/// this and both let somebody change it, and shipping an audio file would mean
/// overriding a choice they have already made.
#[cfg(not(windows))]
const SOUND: &str = "message-new-instant";
/// The Windows spelling of the same idea. `winrt_notification::Sound` parses
/// this name; a freedesktop one would not parse and would leave the toast
/// silent.
#[cfg(windows)]
const SOUND: &str = "Default";

/// The action identifier a click on the notification body carries.
///
/// `default` is the freedesktop name for "the notification itself was
/// clicked", and notify-rust maps the Windows equivalent onto the same string.
const CLICKED: &str = "default";

/// When to interrupt somebody, and how loudly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// Whether to draw notifications at all.
    pub enabled: bool,
    /// Whether to draw one only when somebody said your name.
    ///
    /// Off, so the default is whatever the account's push rules say, which is
    /// the answer every other client gives and the one somebody has already
    /// tuned by muting rooms. This is the local override for somebody in more
    /// rooms than push rules can sensibly be written for.
    pub mentions_only: bool,
    /// Whether to ask the desktop for a sound when the push rules wanted one.
    ///
    /// Which rules want one is not decided here. The default rules ask on a
    /// mention and in a direct message and stay quiet in a busy room; this is
    /// the switch for somebody who wants the notification and not the noise.
    pub sound: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mentions_only: false,
            sound: true,
        }
    }
}

/// Whether this one is worth drawing right now.
///
/// Pure, and the whole of the "not now" rule. Three reasons not to, and the
/// third is the one that matters most for a client somebody leaves open: a
/// notification about the room already on screen, in a window they are looking
/// at, is telling somebody about a message they are watching arrive.
///
/// `focused` alone is not enough, and neither is the open room alone. Consort
/// in the background with a room open is a Consort nobody can see, and Consort
/// in front on a different room is a room they are not reading.
pub fn worth_drawing(
    one: &Notification,
    settings: NotificationSettings,
    focused: bool,
    open_room: Option<&str>,
) -> bool {
    if !settings.enabled {
        return false;
    }
    if settings.mentions_only && !one.mention {
        return false;
    }
    if focused && open_room == Some(one.room_id.as_str()) {
        return false;
    }
    true
}

/// The heading and the line under it.
///
/// The room and the person, then what they said. Both in the heading when they
/// differ, because "Ada" alone does not say where to look and "#general" alone
/// does not say who; a direct message is the case where they are the same
/// thing said twice, and there the room's name is dropped.
///
/// Split out so that it can be read without a notification daemon, which is
/// the only part of drawing one that is worth checking.
pub fn wording(one: &Notification) -> (String, String) {
    let heading = if one.room_name == one.sender_name {
        one.sender_name.clone()
    } else {
        format!("{} in {}", one.sender_name, one.room_name)
    };
    (heading, one.body.clone())
}

/// Escape what a notification daemon's markup parser would read as markup.
///
/// `&` first, or the replacements escape one another.
#[cfg(all(unix, not(target_os = "macos")))]
fn for_the_daemon(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Only the XDG backend parses markup. A Windows toast and a macOS
/// notification take none, and would draw the entities instead.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn for_the_daemon(text: &str) -> String {
    text.to_owned()
}

/// The notification handed to the desktop, built but not yet shown.
///
/// Separate from [`Notifier::draw`] so that what reaches the daemon can be
/// read by a test. Showing one needs a desktop; deciding what it says does
/// not.
fn compose(one: &Notification, sound: bool) -> notify_rust::Notification {
    let (heading, body) = wording(one);
    // Both halves are written by other people: the body is the sender's, and
    // the heading is their display name beside the room's.
    let heading = for_the_daemon(&heading);
    let body = for_the_daemon(&body);

    let mut notification = notify_rust::Notification::new();
    notification
        .appname(APP_NAME)
        .summary(&heading)
        .body(&body)
        // Without this a click closes the notification and nothing
        // else. It is what makes the body itself clickable.
        .action(CLICKED, "Open");
    if sound {
        notification.sound_name(SOUND);
    }
    #[cfg(windows)]
    notification.app_id(APP_ID);

    notification
}

/// Bringing the window to the front.
///
/// A trait for the reason [`EventSink`] is one: the only implementation needs
/// an `AppHandle`, which exists only inside a running application, and a state
/// holding one directly would be state no test could build.
pub trait Front: Send + Sync + 'static {
    /// Show the window and give it focus.
    fn raise(&self);
}

impl<R: tauri::Runtime> Front for tauri::AppHandle<R> {
    fn raise(&self) {
        use tauri::Manager as _;

        let Some(window) = self.get_webview_window("main") else {
            // The window is closing, which is the only way it is missing.
            return;
        };
        // Both, and in this order. A minimised window that is only focused
        // stays minimised, and a shown window that is not focused comes up
        // behind whatever the person is looking at.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Somewhere to draw a notification.
///
/// Holds what a click needs rather than what drawing needs, because drawing is
/// a call into the desktop and clicking is the part with somewhere to go: the
/// window has to come forward and the shell has to be told which room.
pub struct Notifier {
    events: Arc<dyn EventSink>,
    front: Arc<dyn Front>,
}

impl Notifier {
    pub fn new(events: Arc<dyn EventSink>, front: Arc<dyn Front>) -> Self {
        Self { events, front }
    }

    /// Draw one, and answer a click on it.
    ///
    /// Returns as soon as the notification has been handed over. Showing one
    /// is a blocking call to the desktop and waiting for a click blocks until
    /// somebody acts on it or it expires, so both happen on a blocking thread:
    /// a sync response must not stop arriving because nobody has dismissed a
    /// notification.
    ///
    /// A failure is logged rather than raised. There is nowhere to report "the
    /// notification daemon refused this" to, and the message itself is already
    /// in the room, marked unread.
    pub fn draw(&self, one: Notification, sound: bool) {
        let events = self.events.clone();
        let front = self.front.clone();

        tokio::task::spawn_blocking(move || {
            let notification = compose(&one, sound);

            let handle = match notification.show() {
                Ok(handle) => handle,
                Err(error) => {
                    tracing::warn!(%error, "could not draw a notification");
                    return;
                }
            };

            handle.wait_for_action(|action| {
                clicked(action, one.room_id, events.as_ref(), front.as_ref());
            });
        });
    }
}

/// Answer somebody acting on a notification.
///
/// Split from the wait above because the wait needs a notification daemon and
/// this needs nothing: what happens on a click is two calls in an order that
/// matters, and it is the part worth pinning.
///
/// The window comes forward before the room is named, and that is the order
/// rather than an accident. The shell can only show a room in a window that
/// exists, and raising afterwards would put a window in front that is still
/// showing whatever was open before.
fn clicked(action: &str, room_id: String, events: &dyn EventSink, front: &dyn Front) {
    // Every other value is the notification being dismissed or expiring, which
    // is somebody deciding not to look now.
    if action != CLICKED {
        return;
    }
    front.raise();
    events.emit(AppEvent::ShowRoom(room_id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RecordingSink;

    /// A window that records being asked to come forward.
    #[derive(Default)]
    struct FakeFront {
        raised: std::sync::atomic::AtomicBool,
    }

    impl FakeFront {
        fn was_raised(&self) -> bool {
            self.raised.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl Front for FakeFront {
        fn raise(&self) {
            self.raised
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn arrived() -> Notification {
        Notification {
            room_id: "!general:example.org".to_owned(),
            room_name: "general".to_owned(),
            event_id: "$said:example.org".to_owned(),
            sender: "@ada:example.org".to_owned(),
            sender_name: "Ada".to_owned(),
            body: "are you about?".to_owned(),
            mention: false,
            sound: false,
        }
    }

    fn mentioned() -> Notification {
        Notification {
            mention: true,
            ..arrived()
        }
    }

    #[test]
    fn a_message_in_a_room_nobody_is_looking_at_is_drawn() {
        assert!(worth_drawing(
            &arrived(),
            NotificationSettings::default(),
            false,
            None
        ));
    }

    #[test]
    fn nothing_is_drawn_when_notifications_are_off() {
        let off = NotificationSettings {
            enabled: false,
            ..NotificationSettings::default()
        };

        assert!(!worth_drawing(&mentioned(), off, false, None));
    }

    #[test]
    fn mentions_only_lets_a_mention_through() {
        let picky = NotificationSettings {
            mentions_only: true,
            ..NotificationSettings::default()
        };

        assert!(worth_drawing(&mentioned(), picky, false, None));
    }

    #[test]
    fn mentions_only_keeps_everything_else_out() {
        let picky = NotificationSettings {
            mentions_only: true,
            ..NotificationSettings::default()
        };

        assert!(!worth_drawing(&arrived(), picky, false, None));
    }

    #[test]
    fn the_room_on_screen_in_the_window_in_front_says_nothing() {
        // Telling somebody about a message they are watching arrive.
        assert!(!worth_drawing(
            &mentioned(),
            NotificationSettings::default(),
            true,
            Some("!general:example.org"),
        ));
    }

    #[test]
    fn a_room_open_behind_another_window_still_says_something() {
        // Consort in the background with a room open is a Consort nobody can
        // see, whatever it happens to be showing.
        assert!(worth_drawing(
            &arrived(),
            NotificationSettings::default(),
            false,
            Some("!general:example.org"),
        ));
    }

    #[test]
    fn another_room_in_the_window_in_front_still_says_something() {
        assert!(worth_drawing(
            &arrived(),
            NotificationSettings::default(),
            true,
            Some("!elsewhere:example.org"),
        ));
    }

    #[test]
    fn the_heading_says_who_and_where() {
        let (heading, body) = wording(&arrived());

        assert_eq!(heading, "Ada in general");
        assert_eq!(body, "are you about?");
    }

    #[test]
    fn a_direct_message_is_not_a_name_said_twice() {
        // A direct message's room is calculated from its members, so it is
        // called after the person in it. "Ada in Ada" is what not checking
        // looks like.
        let direct = Notification {
            room_name: "Ada".to_owned(),
            ..arrived()
        };

        assert_eq!(wording(&direct).0, "Ada");
    }

    #[test]
    fn clicking_one_raises_the_window_and_names_the_room() {
        let events = Arc::new(RecordingSink::new());
        let front = Arc::new(FakeFront::default());

        clicked(
            CLICKED,
            "!general:example.org".to_owned(),
            events.as_ref(),
            front.as_ref(),
        );

        assert!(front.was_raised());
        assert_eq!(
            events.events(),
            vec![AppEvent::ShowRoom("!general:example.org".to_owned())]
        );
    }

    #[test]
    fn letting_one_expire_moves_nothing() {
        // Somebody deciding not to look now. A window that came forward for
        // every notification that timed out would be a window that takes over
        // the screen while nobody is at the machine.
        let events = Arc::new(RecordingSink::new());
        let front = Arc::new(FakeFront::default());

        clicked(
            "__closed",
            "!general:example.org".to_owned(),
            events.as_ref(),
            front.as_ref(),
        );

        assert!(!front.was_raised());
        assert!(events.events().is_empty());
    }

    #[test]
    fn the_defaults_interrupt_for_anything_the_push_rules_would() {
        // Not mentions-only. Somebody who wanted less than their push rules
        // give them can say so; somebody who has never opened the settings
        // should get what every other client on their account gives them.
        let settings = NotificationSettings::default();

        assert!(settings.enabled);
        assert!(!settings.mentions_only);
        assert!(settings.sound);
    }

    /// The daemon's markup parser, and what a sender can put in front of it.
    #[cfg(all(unix, not(target_os = "macos")))]
    mod markup {
        use super::*;

        /// What a sender would write to forge a notification.
        const FORGERY: &str = r#"<b>Session expired.</b> <a href="https://x.example/">Sign in</a>"#;

        #[test]
        fn the_daemon_is_handed_no_tag_a_sender_wrote() {
            let said = Notification {
                body: FORGERY.to_owned(),
                ..arrived()
            };

            let composed = compose(&said, false);

            assert!(!composed.body.contains('<'), "{}", composed.body);
            assert!(composed.body.starts_with("&lt;b&gt;"), "{}", composed.body);
        }

        #[test]
        fn the_heading_is_escaped_too_because_both_halves_are_written_by_others() {
            let said = Notification {
                sender_name: "<i>Admin</i>".to_owned(),
                room_name: "<b>ops</b>".to_owned(),
                ..arrived()
            };

            let composed = compose(&said, false);

            assert!(!composed.summary.contains('<'), "{}", composed.summary);
        }

        #[test]
        fn an_ampersand_is_escaped_once_and_not_twice() {
            let said = Notification {
                body: "Tom & Jerry <3".to_owned(),
                ..arrived()
            };

            assert_eq!(compose(&said, false).body, "Tom &amp; Jerry &lt;3");
        }

        #[test]
        fn ordinary_words_are_left_exactly_as_they_were_written() {
            assert_eq!(compose(&arrived(), false).body, "are you about?");
        }
    }
}
