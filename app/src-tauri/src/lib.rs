// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Consort's Tauri shell.
//!
//! Everything here is wiring. The Matrix work lives in `consort-matrix`, which
//! has no dependency on Tauri and can be exercised from a test or a plain
//! binary. Keeping that line sharp is what makes the interesting half testable
//! without driving a webview.

mod audio;
mod call;
mod commands;
mod ears;
mod events;
mod settings;
mod sound;
mod state;
#[cfg(test)]
mod testing;

use std::path::PathBuf;

use consort_matrix::SessionStore;
use tauri::{Manager, WindowEvent};

use crate::state::AppState;

/// Start the application.
pub fn run() {
    init_tracing();

    // Before any TLS. See `consort_matrix::install_crypto_provider`.
    if !consort_matrix::install_crypto_provider() {
        tracing::debug!("a rustls crypto provider was already installed");
    }

    // Registered before anything touches the data directory.
    //
    // Two copies of Consort would share one SQLite crypto store and one
    // session file. The store has a cross-process lock, so the second copy
    // fails to open it, and both processes then disagree about who is signed
    // in. It gets worse rather than better once verification lands: two
    // processes racing on one crypto store is how device keys get dropped.
    //
    // Rather than make the storage layer safe for concurrent processes, which
    // is a large amount of work for a case nobody wants, there is simply one
    // Consort. A second launch hands its arguments to the first and exits, and
    // the first raises its window, which is also what a user clicking the
    // launcher icon twice actually expects.
    //
    // A profile is the deliberate exception. It moves the data directory, so
    // the thing this protects is no longer shared, and running two accounts
    // against each other is the whole point of having one. See `profile`.
    let mut builder = tauri::Builder::default();
    if profile().is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tracing::info!("a second instance was launched; focusing the existing window");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }

    builder
        .setup(|app| {
            let data_dir = resolve_data_dir(app.handle())?;
            let store = SessionStore::new(&data_dir);

            tracing::info!(
                path = %data_dir.display(),
                token_store = ?store.backend_kind(),
                "using application data directory"
            );

            // The handle is the event sink. Cloning it is the documented way
            // to keep one past `setup`, and it is what every background task
            // emits through.
            app.manage(AppState::new(
                store,
                crate::settings::SettingsStore::at(&data_dir),
                std::sync::Arc::new(app.handle().clone()),
            ));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                tracing::info!(label = window.label(), "window closed");
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_status,
            commands::login,
            commands::logout,
            commands::token_storage,
            commands::resend_state,
            commands::room_avatar,
            commands::member_avatar,
            commands::member_profile,
            commands::audio_devices,
            commands::audio_settings,
            commands::set_audio_settings,
            commands::set_person_volume,
            commands::audio_test_start,
            commands::audio_test_stop,
            commands::audio_tone_play,
            commands::audio_tone_stop,
            commands::call_connect,
            commands::call_disconnect,
            commands::call_set_muted,
            commands::call_set_deafened,
            commands::call_set_away,
            commands::verification_accept,
            commands::verification_start_sas,
            commands::verification_confirm,
            commands::verification_mismatch,
            commands::verification_cancel,
            commands::verification_verify_this_session,
            commands::verification_other_sessions_exist,
            commands::verification_recovery_exists,
            commands::verification_recover,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Consort");
}

/// The per-user directory holding the session file and the SDK's SQLite stores.
///
/// Created here rather than lazily on first write so that a permissions problem
/// surfaces at startup with a clear path in the message, instead of during a
/// login as a confusing failure to save the session.
fn resolve_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dir = under_profile(app.path().app_data_dir()?, profile().as_deref());
    consort_matrix::atomic::create_dir_private(&dir)?;
    Ok(dir)
}

/// A second Consort on one machine, for testing two accounts against each
/// other.
///
/// Set `CONSORT_PROFILE` to any name and that process gets its own data
/// directory and drops the single-instance guard, so it runs beside the
/// ordinary one. Unset, which is every real user, nothing about the
/// application changes.
///
/// The guard is dropped rather than made per profile because it is a bundle
/// identifier lock on Windows and macOS with nowhere to put a profile name.
/// Two processes sharing *one* profile would still fight over one SQLite
/// store, which is the thing the guard exists to prevent, so give each its own
/// name.
///
/// Not a command-line flag because Tauri owns argument parsing and a webview
/// process re-execs itself; an environment variable is inherited and a flag is
/// not.
fn profile() -> Option<String> {
    std::env::var("CONSORT_PROFILE")
        .ok()
        .filter(|name| !name.is_empty())
}

/// Where a named profile keeps its data.
///
/// Under the ordinary directory rather than beside it, so that a machine with
/// half a dozen abandoned test profiles still has one directory to delete.
///
/// The name is reduced to characters that are safe in a path component. It
/// comes from the environment, and a profile called `../..` would otherwise
/// point the crypto store somewhere nobody asked for.
fn under_profile(base: PathBuf, profile: Option<&str>) -> PathBuf {
    let Some(name) = profile else {
        return base;
    };

    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    base.join("profiles").join(safe)
}

/// Logging to stderr, filtered by `RUST_LOG`.
///
/// See [`default_log_filter`] for what is on when nothing is set, and for the
/// trap a hand-written `RUST_LOG` inherits: a filter that lists targets and
/// gives no bare level turns every crate it forgot to `OFF`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_filter()));

    // `try_init` rather than `init`. `init` panics if a subscriber is already
    // installed, and in a test binary one may well be.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// The filter used when `RUST_LOG` says nothing.
///
/// It opens with a bare `warn`, and that is the load-bearing part. `EnvFilter`
/// gives a target matching no directive `OFF`, not the level of the most
/// general directive, so a crate left out of a list is not quieter than the
/// rest: it is silent at `error!` too. A leading bare directive is the only way
/// to say "and everything else at this level", because
/// `Builder::with_default_directive` applies only when the parsed filter came
/// out empty.
///
/// That hole cost a debugging session. A log taken to find out why a call would
/// not join contained nothing from `consort_call`, nothing from any
/// `matrix_rtc_*` crate, and nothing from livekit, which between them are every
/// line that has anything to say about joining a call. Naming crates is now the
/// belt: the bare `warn` is the braces, and a crate added to the workspace
/// tomorrow can no longer be silent by omission.
///
/// `matrix_sdk` stays named even though the bare directive already puts it at
/// `warn`. At `info` it narrates every sync response and buries our own lines
/// during exactly the debugging session where they matter, so it wants to be
/// held down explicitly rather than by whatever the general level happens to be.
fn default_log_filter() -> &'static str {
    // `concat!` rather than a line continuation: rustfmt joins the lines of a
    // continued literal without removing their indentation, and the stray
    // spaces make every directive after the first fail to parse. It says so on
    // stderr and carries on with what was left, which is a filter that silently
    // does something else.
    concat!(
        "warn,",
        "consort_app_lib=info,consort_matrix=info,consort_call=info,consort_audio=info,",
        "matrix_rtc_bridge=info,matrix_rtc_core=info,matrix_rtc_livekit=info,matrix_rtc_media=info,",
        "matrix_sdk=warn",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_log_filter_is_a_valid_env_filter() {
        use tracing_subscriber::EnvFilter;
        EnvFilter::try_new(default_log_filter()).expect("the default filter must parse");
    }

    #[test]
    fn the_default_log_filter_keeps_the_sdk_quiet() {
        assert!(default_log_filter().contains("matrix_sdk=warn"));
    }

    #[test]
    fn every_crate_that_has_something_to_say_about_a_call_is_named() {
        // An omission here is not a crate that says less, it is one that says
        // nothing above the bare level below. These four are the ones that
        // narrate a join, and a join is the thing this build is hardest to
        // diagnose.
        let filter = default_log_filter();

        for target in [
            "consort_app_lib",
            "consort_matrix",
            "consort_call",
            "consort_audio",
            "matrix_rtc_bridge",
            "matrix_rtc_core",
            "matrix_rtc_livekit",
            "matrix_rtc_media",
        ] {
            assert!(
                filter.contains(&format!("{target}=info")),
                "{target} is missing from {filter}"
            );
        }
    }

    #[test]
    fn a_crate_the_default_filter_forgot_is_not_silent() {
        // The braces, and the reason the bare `warn` is in the string rather
        // than passed to `with_default_directive`, which applies only to a
        // filter that parsed to nothing. Driven through a real subscriber
        // because the failure this guards against is not a parse error: the
        // filter is perfectly valid and simply drops the event.
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(default_log_filter()))
            .with_writer(move || writer.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(target: "a_crate_nobody_thought_to_list", "the call did not join");
            tracing::info!(target: "matrix_rtc_bridge", "the sticky bridge woke up");
            tracing::info!(target: "matrix_sdk", "a sync response arrived");
        });

        let said = captured.said();
        assert!(said.contains("the call did not join"), "{said}");
        assert!(said.contains("the sticky bridge woke up"), "{said}");
        assert!(!said.contains("a sync response arrived"), "{said}");
    }

    /// Somewhere for a subscriber under test to write.
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Captured {
        fn said(&self) -> String {
            String::from_utf8(self.0.lock().expect("not poisoned").clone()).expect("utf-8")
        }
    }

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("not poisoned").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn installing_the_crypto_provider_twice_is_reported_not_fatal() {
        // The first call in a process wins; every later one returns false.
        // `run` relies on that being a debug line rather than a panic.
        let first = consort_matrix::install_crypto_provider();
        let second = consort_matrix::install_crypto_provider();
        assert!(
            first || !first,
            "the first call may go either way in a test binary"
        );
        assert!(
            !second,
            "a second install must report that one already existed"
        );
    }

    #[test]
    fn init_tracing_can_be_called_more_than_once() {
        init_tracing();
        init_tracing();
    }

    #[test]
    fn no_profile_leaves_the_ordinary_directory_alone() {
        let base = PathBuf::from("/home/ada/.local/share/chat.consort.desktop");

        assert_eq!(under_profile(base.clone(), None), base);
    }

    #[test]
    fn a_profile_gets_its_own_directory_under_the_ordinary_one() {
        let base = PathBuf::from("/home/ada/.local/share/chat.consort.desktop");

        assert_eq!(
            under_profile(base, Some("second")),
            PathBuf::from("/home/ada/.local/share/chat.consort.desktop/profiles/second")
        );
    }

    #[test]
    fn a_profile_name_cannot_climb_out_of_the_data_directory() {
        // It comes from the environment. Anything but the characters a path
        // component is allowed is flattened, so the worst a hostile name can
        // do is collide with another profile.
        let base = PathBuf::from("/home/ada/.local/share/chat.consort.desktop");

        assert_eq!(
            under_profile(base, Some("../../etc")),
            PathBuf::from("/home/ada/.local/share/chat.consort.desktop/profiles/______etc")
        );
    }
}
