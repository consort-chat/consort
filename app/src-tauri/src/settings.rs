// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Settings, and the file they live in.
//!
//! Separate from `consort-audio`, which owns the audio types and deliberately
//! knows nothing about files or about where this application keeps them. This
//! is the other half: one JSON file beside `session.json`, written the same way
//! the session is.
//!
//! Loading never fails. A settings file is not load-bearing the way a session
//! is: if it cannot be read, the right answer is to start with the defaults and
//! say so in the log, not to refuse to open the window. What loading must never
//! do is destroy what it could not read, because that file is the only copy of
//! choices somebody made by hand.

use std::path::{Path, PathBuf};

use consort_audio::AudioSettings;
use consort_call::Dialect;
use consort_matrix::atomic;
use serde::{Deserialize, Serialize};

use crate::notify::NotificationSettings;
use crate::recent::RecentRooms;

/// The name of the file inside the application data directory.
const FILE: &str = "settings.json";

/// Distinguishes this writer's temporary file from any other in the directory.
const UNIQUE: &str = "settings";

/// Everything the application remembers between runs that is not a session.
///
/// A struct rather than `AudioSettings` directly, so that appearance,
/// notifications and keybinds can arrive later without changing the shape of a
/// file that already exists on disk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub calls: CallSettings,
    pub privacy: PrivacySettings,
    pub notifications: NotificationSettings,
    /// The rooms this account has opened, most recently first.
    ///
    /// Here rather than in a file of its own because it is the same kind of
    /// thing as the per-person volumes above: a by-product of using the
    /// application, not worth a second writer, and written through the one
    /// that already fsyncs before it renames.
    pub recent: RecentRooms,
}

/// What this account tells other people about itself.
///
/// One field so far, and a section of its own rather than a loose flag, because
/// everything that belongs here is the same kind of question: what leaves this
/// machine that nobody asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    /// Whether a read receipt is the public one.
    ///
    /// True sends `m.read`, which everybody in the room can see and which
    /// cannot be taken back: it is what makes the read markers in Element and
    /// every other client correct about this account, and it is what tells the
    /// room when this account was looking. False sends `m.read.private`, which
    /// resets the unread counts here and tells nobody.
    ///
    /// True by default. The alternative is a client that silently makes every
    /// other person in the room wrong about whether their message was seen,
    /// which is not a courtesy, and the switch is there for anybody who would
    /// rather not be observed.
    ///
    /// The marker behind the "new messages" line is unaffected either way. It
    /// is room account data rather than a receipt and is private in both
    /// directions; see `consort_matrix::receipts`.
    pub public_read_receipts: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            public_read_receipts: true,
        }
    }
}

/// The two things about voice calls that a deployment can get wrong.
///
/// No interface, deliberately. Both of these are properties of a homeserver
/// and its voice deployment rather than preferences, the right value is the
/// same for everybody on that server, and a picker offering somebody a choice
/// between three MatrixRTC generations would be a picker nobody can answer.
/// They are here so that a deployment this build cannot work out for itself
/// can be told, by hand, in one file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CallSettings {
    /// Which MatrixRTC generation to speak in a channel that offers no
    /// evidence either way.
    ///
    /// Only ever a fallback. `consort_call::detect` looks at the channel first
    /// and overrides this when somebody is already sitting in it through
    /// pre-MSC4354 room state; see that function for what it can and cannot
    /// tell apart.
    pub fallback_dialect: Dialect,
    /// Where to ask for an SFU token, overruling whatever was discovered.
    ///
    /// Normally empty, and normally it should stay that way. A call looks for
    /// an SFU twice before reading this: at the MSC4143 transports endpoint,
    /// and then at `org.matrix.msc4143.rtc_foci` in the server's own
    /// `.well-known/matrix/client`, which is where Element Call has always
    /// looked. Between them those cover both generations of deployment.
    ///
    /// What is left is the deployment whose discovery is wrong rather than
    /// missing, and which needs to be able to say so without waiting for a
    /// release. A value here wins over both.
    pub service_url_fallback: Option<String>,
}

/// The settings file.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    /// The store for the application data directory `dir`.
    pub fn at(dir: &Path) -> Self {
        Self {
            path: dir.join(FILE),
        }
    }

    /// Where the file is.
    ///
    /// Test-only. The application never needs to know: `load` and `save` say
    /// the path themselves when something goes wrong with it.
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the settings, falling back to the defaults for anything unreadable.
    ///
    /// Deliberately infallible, and deliberately non-destructive: a file that
    /// fails to parse is left exactly as it is. Somebody hand-editing their
    /// thresholds and getting a comma wrong should find their file still there
    /// afterwards, not replaced with defaults.
    pub fn load(&self) -> Settings {
        let raw = match std::fs::read(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // First run. Not worth a warning.
                return Settings::default();
            }
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error,
                    "could not read the settings file; starting from the defaults");
                return Settings::default();
            }
        };

        match serde_json::from_slice(&raw) {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error,
                    "the settings file is not readable JSON; starting from the \
                     defaults and leaving the file alone");
                Settings::default()
            }
        }
    }

    /// Write the settings, atomically.
    ///
    /// Reuses the session's writer, which sets the mode at creation, fsyncs
    /// before the rename and fsyncs the directory after it. Settings are not
    /// secret and do not need the `0600`, but a second, worse writer alongside
    /// a correct one would be the wrong kind of thrift.
    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        let json = serde_json::to_vec_pretty(settings).map_err(SettingsError::Serialise)?;
        atomic::write_private(&self.path, &json, UNIQUE).map_err(SettingsError::Write)
    }
}

/// Why a save did not happen.
///
/// Its own type rather than `consort_matrix::Error`, which is about sessions
/// and secrets and constructs its file variants privately. Settings are neither.
#[derive(Debug)]
pub enum SettingsError {
    /// The settings could not be turned into JSON. Not reachable with the
    /// current fields, and kept rather than unwrapped because the day somebody
    /// adds a field that can fail is not the day to find out by panicking.
    Serialise(serde_json::Error),
    /// The file could not be written.
    Write(consort_matrix::Error),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialise(error) => write!(f, "could not serialise the settings: {error}"),
            Self::Write(error) => write!(f, "could not write the settings file: {error}"),
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Serialise(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use consort_audio::GateConfig;
    use tempfile::TempDir;

    fn store() -> (TempDir, SettingsStore) {
        let dir = TempDir::new().expect("temp dir");
        let store = SettingsStore::at(dir.path());
        (dir, store)
    }

    fn tuned() -> Settings {
        Settings {
            audio: AudioSettings {
                input: Some("Yeti Stereo Microphone".to_owned()),
                output: Some("HD-Audio Generic".to_owned()),
                gate: GateConfig {
                    open_at: 0.75,
                    ..GateConfig::default()
                },
                call_sounds: false,
                call_voices: false,
                ..AudioSettings::default()
            },
            calls: CallSettings::default(),
            privacy: PrivacySettings::default(),
            notifications: NotificationSettings::default(),
            recent: RecentRooms::default(),
        }
    }

    #[test]
    fn the_rooms_an_account_opened_survive_a_restart() {
        // The whole reason this is in the file rather than in memory. A list
        // that started empty every launch would be empty on the one screen
        // that reads it, which is the screen the application opens on.
        let (dir, store) = store();
        let mut settings = Settings::default();
        settings
            .recent
            .opened("@ada:example.org", "!lounge:example.org");
        store.save(&settings).expect("save");

        // A second store over the same directory, which is all the next launch
        // builds.
        let next_launch = SettingsStore::at(dir.path());

        assert_eq!(
            next_launch.load().recent.of("@ada:example.org"),
            ["!lounge:example.org"]
        );
    }

    #[test]
    fn a_settings_file_written_before_recent_rooms_existed_still_loads() {
        // Every settings file on disk today was written before this section,
        // and a load that failed on its absence would take somebody's audio
        // thresholds with it.
        let (_dir, store) = store();
        std::fs::write(
            store.path(),
            br#"{"audio":{"input":"Yeti","output":null,"gate":{}}}"#,
        )
        .expect("write");

        let loaded = store.load();

        assert_eq!(loaded.audio.input.as_deref(), Some("Yeti"));
        assert!(loaded.recent.of("@ada:example.org").is_empty());
    }

    #[test]
    fn a_settings_file_written_before_calls_existed_still_loads() {
        // The `default` container attribute is what makes this true, and it is
        // load bearing rather than tidiness: every settings file already on
        // disk was written before this section existed, and a hard failure
        // here would replace somebody's thresholds with the defaults.
        let (_dir, store) = store();
        std::fs::write(
            store.path(),
            br#"{"audio":{"input":"Yeti","output":null,"gate":{}}}"#,
        )
        .expect("write");

        let loaded = store.load();

        assert_eq!(loaded.audio.input.as_deref(), Some("Yeti"));
        assert_eq!(loaded.calls, CallSettings::default());
    }

    #[test]
    fn the_dialect_nothing_says_otherwise_about_is_one_that_works() {
        // Deliberately not asserted against a named variant. Which dialect is
        // usable is `consort_call`'s to decide and its to change, and a second
        // copy of the answer over here would only ever be found out of date by
        // somebody whose calls had stopped connecting.
        assert!(CallSettings::default().fallback_dialect.readable());
        assert_eq!(CallSettings::default().service_url_fallback, None);
    }

    #[test]
    fn a_hand_written_dialect_survives_a_round_trip() {
        // The only way either of these is ever set. If the names on the wire
        // drift, the symptom is a file somebody edited on purpose being
        // silently ignored.
        let (_dir, store) = store();
        let chosen = Settings {
            calls: CallSettings {
                // Not the default, so that a round trip which quietly dropped
                // the field would still fail this.
                fallback_dialect: Dialect::Sticky,
                service_url_fallback: Some("https://example.org/sfu".to_owned()),
            },
            ..Settings::default()
        };

        store.save(&chosen).expect("save");

        assert_eq!(store.load(), chosen);
        let raw = std::fs::read_to_string(store.path()).expect("read");
        assert!(raw.contains("\"fallbackDialect\": \"sticky\""), "{raw}");
    }

    #[test]
    fn a_settings_file_written_before_privacy_existed_still_loads() {
        // And loads with receipts public, which is what every build before
        // this one did. A default of false would quietly stop an existing
        // account telling its rooms anything, without anybody choosing that.
        let (_dir, store) = store();
        std::fs::write(store.path(), br#"{"audio":{"input":"Yeti"}}"#).expect("write");

        let loaded = store.load();

        assert_eq!(loaded.audio.input.as_deref(), Some("Yeti"));
        assert!(loaded.privacy.public_read_receipts);
    }

    #[test]
    fn turning_public_receipts_off_survives_a_round_trip() {
        // The one field, and the one that matters: a choice about being
        // watched that came back wrong after a restart would be the setting
        // failing at the only thing it does.
        let (_dir, store) = store();
        let chosen = Settings {
            privacy: PrivacySettings {
                public_read_receipts: false,
            },
            ..Settings::default()
        };

        store.save(&chosen).expect("save");

        assert!(!store.load().privacy.public_read_receipts);
    }

    #[test]
    fn a_settings_file_written_before_notifications_existed_still_loads() {
        // And loads with them on. Every settings file already on disk was
        // written before this section existed, and defaulting them off would
        // silently give an upgrade fewer notifications than a fresh install.
        let (_dir, store) = store();
        std::fs::write(store.path(), br#"{"audio":{"input":"Yeti"}}"#).expect("write");

        let loaded = store.load();

        assert_eq!(loaded.audio.input.as_deref(), Some("Yeti"));
        assert_eq!(loaded.notifications, NotificationSettings::default());
    }

    #[test]
    fn a_chosen_notification_setting_survives_a_round_trip() {
        let (_dir, store) = store();
        let chosen = Settings {
            notifications: NotificationSettings {
                enabled: true,
                mentions_only: true,
                sound: false,
            },
            ..Settings::default()
        };

        store.save(&chosen).expect("save");

        assert_eq!(store.load().notifications, chosen.notifications);
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        let (_dir, store) = store();

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn what_was_saved_is_what_loads_back() {
        let (_dir, store) = store();
        let settings = tuned();

        store.save(&settings).expect("save");

        assert_eq!(store.load(), settings);
    }

    #[test]
    fn saving_twice_replaces_rather_than_appends() {
        let (_dir, store) = store();
        store.save(&tuned()).expect("first save");

        store.save(&Settings::default()).expect("second save");

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn a_corrupt_file_loads_the_defaults() {
        let (_dir, store) = store();
        std::fs::write(store.path(), b"{ not json").expect("write");

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn a_corrupt_file_is_left_exactly_as_it_was() {
        // Somebody hand-editing their thresholds and getting a comma wrong
        // should find their file still there, not replaced with defaults.
        let (_dir, store) = store();
        let hand_written = b"{ \"audio\": { \"gate\": { \"openAt\": 0.8, } } }";
        std::fs::write(store.path(), hand_written).expect("write");

        store.load();

        assert_eq!(
            std::fs::read(store.path()).expect("read"),
            hand_written,
            "loading must never destroy the only copy of what it could not read"
        );
    }

    #[test]
    fn saving_creates_the_directory_it_needs() {
        let dir = TempDir::new().expect("temp dir");
        let nested = dir.path().join("not").join("there").join("yet");
        let store = SettingsStore::at(&nested);

        store.save(&tuned()).expect("save");

        assert_eq!(store.load(), tuned());
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let (dir, store) = store();

        store.save(&tuned()).expect("save");

        let left: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from(FILE)], "got {left:?}");
    }

    #[test]
    fn the_file_sits_beside_the_session() {
        let (dir, store) = store();

        assert_eq!(store.path(), dir.path().join("settings.json"));
    }

    #[test]
    fn a_file_written_by_a_newer_version_still_loads_what_it_can() {
        let (_dir, store) = store();
        std::fs::write(
            store.path(),
            br#"{"audio":{"input":"Yeti"},"appearance":{"theme":"dark"}}"#,
        )
        .expect("write");

        let settings = store.load();

        assert_eq!(settings.audio.input.as_deref(), Some("Yeti"));
        assert_eq!(settings.audio.gate, GateConfig::default());
    }

    #[test]
    fn a_failed_write_says_what_it_could_not_do() {
        let dir = TempDir::new().expect("temp dir");
        let blocked = dir.path().join("in-the-way");
        std::fs::write(&blocked, b"not a directory").expect("write");
        let store = SettingsStore::at(&blocked);

        let error = store.save(&Settings::default()).expect_err("should fail");

        assert!(
            error
                .to_string()
                .starts_with("could not write the settings file"),
            "got {error}"
        );
        assert!(
            std::error::Error::source(&error).is_some(),
            "the underlying cause has to survive, or the log says nothing useful"
        );
    }

    #[test]
    fn a_serialisation_failure_says_so_instead() {
        let error =
            SettingsError::Serialise(serde_json::from_str::<Settings>("nonsense").unwrap_err());

        assert!(
            error
                .to_string()
                .starts_with("could not serialise the settings"),
            "got {error}"
        );
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn a_write_that_cannot_happen_is_reported_rather_than_swallowed() {
        // The parent exists as a file, so creating it as a directory fails.
        let dir = TempDir::new().expect("temp dir");
        let blocked = dir.path().join("in-the-way");
        std::fs::write(&blocked, b"not a directory").expect("write");
        let store = SettingsStore::at(&blocked);

        let result = store.save(&Settings::default());

        assert!(
            result.is_err(),
            "a failed save must not look like a saved one"
        );
    }
}
