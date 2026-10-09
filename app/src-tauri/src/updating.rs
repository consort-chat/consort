// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Whether this build updates itself, and whether now is the moment.
//!
//! See [docs/PLAN-self-update.md](../../../docs/PLAN-self-update.md).

use serde::Serialize;

/// Whether an updater was compiled into this build.
pub const UPDATES_ITSELF: bool = cfg!(feature = "self-update");

/// Why installing must not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A call is up, and installing restarts Consort.
    InCall,
}

impl Refusal {
    /// What the interface says.
    pub fn message(self) -> &'static str {
        match self {
            Self::InCall => {
                "Consort will restart to finish updating, which would drop you out \
                 of the call. Leave the call first."
            }
        }
    }
}

/// Whether an install may start now.
///
/// Asked twice, and the second answer is the one somebody's voice depends on:
/// docs/PLAN-self-update.md#a-call-in-progress.
pub fn may_install(in_a_call: bool) -> Result<(), Refusal> {
    if in_a_call {
        return Err(Refusal::InCall);
    }
    Ok(())
}

/// What went wrong, in the only four kinds a person can act on differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// Nothing answered.
    Unreachable,
    /// Something answered and it was not a manifest this build can read.
    Manifest,
    /// The bytes arrived and are not the ones this key signed.
    Signature,
    /// They were signed, and putting them in place failed.
    Install,
}

impl Trouble {
    /// What the interface says.
    pub fn message(self) -> &'static str {
        match self {
            Self::Unreachable => "Consort could not reach the update server.",
            Self::Manifest => "The update server answered with something Consort could not read.",
            Self::Signature => "That update was not signed by Consort and was not installed.",
            Self::Install => "That update downloaded but could not be installed.",
        }
    }
}

/// What the `update` channel carries.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Update {
    /// Checked, and this is the newest release.
    UpToDate,
    /// A newer release exists and is waiting for a press.
    Ready {
        version: String,
        /// Whatever the manifest's `notes` said, which is a link to the release.
        notes: String,
    },
    /// Bytes are moving. `total` is absent when the server sent no length.
    Downloading { received: u64, total: Option<u64> },
    /// Verified, and being put in place.
    Installing,
    /// A check, a download or an install did not work.
    Failed { reason: &'static str },
}

/// How often to look, once the first look is out of the way.
///
/// Six hours rather than minutes: a release here is a weekly event.
#[cfg(feature = "self-update")]
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// How long to wait before the first look.
///
/// Long enough that it never competes with the first sync for the network.
#[cfg(feature = "self-update")]
pub const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(feature = "self-update")]
impl Trouble {
    /// Which kind of trouble one of the plugin's errors is.
    ///
    /// The enum is `#[non_exhaustive]`, so the fallthrough is it growing a
    /// variant rather than a case being forgotten.
    pub fn of(error: &tauri_plugin_updater::Error) -> Self {
        use tauri_plugin_updater::Error as E;
        match error {
            // `reqwest` carries both halves: the plugin parses the body with
            // `res.json()`, so a manifest that is not JSON arrives here as a
            // decode error rather than as a serde one.
            E::Reqwest(error) if error.is_decode() => Self::Manifest,
            E::Reqwest(_) | E::Network(_) => Self::Unreachable,
            E::Serialization(_)
            | E::Semver(_)
            | E::ReleaseNotFound
            | E::TargetNotFound(_)
            | E::TargetsNotFound(_)
            | E::UrlParse(_)
            | E::EmptyEndpoints => Self::Manifest,
            E::Minisign(_)
            | E::Base64(_)
            | E::SignatureUtf8(_)
            | E::SignedVersionMismatch { .. } => Self::Signature,
            _ => Self::Install,
        }
    }
}

/// Look for an update now and then, for as long as the application runs.
///
/// Started in `setup` rather than with a session: a newer Consort is worth
/// knowing about on the sign-in screen too.
#[cfg(feature = "self-update")]
pub fn poll<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE).await;
        loop {
            let found = look(&app).await;
            app.state::<crate::state::AppState>().announce(found);
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Hand verified bytes to the installer, unless a call started meanwhile.
///
/// A closure rather than the bytes, or the guard is untestable: getting here
/// needs a signed artifact and the key that signed it.
#[cfg(feature = "self-update")]
pub fn put_in_place(
    in_a_call: bool,
    install: impl FnOnce() -> Result<(), tauri_plugin_updater::Error>,
) -> Result<(), crate::commands::CommandError> {
    may_install(in_a_call)?;
    install().map_err(|error| {
        let trouble = Trouble::of(&error);
        tracing::error!(%error, "installing the update did not work");
        trouble.into()
    })
}

/// The newer release on offer, or `None` when this is the newest.
#[cfg(feature = "self-update")]
pub async fn waiting<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<tauri_plugin_updater::Update>, Trouble> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|error| {
        tracing::warn!(%error, "the updater could not be built");
        Trouble::of(&error)
    })?;
    updater.check().await.map_err(|error| {
        tracing::info!(%error, "looking for an update did not work");
        Trouble::of(&error)
    })
}

/// What to publish about one look.
#[cfg(feature = "self-update")]
pub async fn look<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Update {
    match waiting(app).await {
        Ok(Some(found)) => Update::Ready {
            version: found.version.clone(),
            notes: found.body.clone().unwrap_or_default(),
        },
        Ok(None) => Update::UpToDate,
        Err(trouble) => Update::Failed {
            reason: trouble.message(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last moment a call can be noticed. See [`put_in_place`].
    #[cfg(feature = "self-update")]
    mod the_last_moment {
        use super::*;
        use std::cell::Cell;

        #[test]
        fn bytes_are_handed_over_when_no_call_is_up() {
            let handed = Cell::new(false);

            let outcome = put_in_place(false, || {
                handed.set(true);
                Ok(())
            });

            assert!(outcome.is_ok());
            assert!(handed.get(), "the installer was never reached");
        }

        /// The installer exits Consort as soon as it is launched, so reaching
        /// it here would cut somebody off mid-word.
        #[test]
        fn a_call_that_started_during_the_download_stops_the_install() {
            let handed = Cell::new(false);

            let outcome = put_in_place(true, || {
                handed.set(true);
                Ok(())
            });

            let refused = outcome.expect_err("a call was up");
            assert!(refused.message().contains("call"), "{refused:?}");
            assert!(
                !handed.get(),
                "the installer ran during a call, which is the whole bug"
            );
        }
    }

    /// The whole mechanism keeping an updater out of a .deb and an Arch
    /// package. CI runs this suite both ways, so this pins both directions.
    #[test]
    fn only_a_build_that_asked_for_an_updater_has_one() {
        assert_eq!(UPDATES_ITSELF, cfg!(feature = "self-update"));
    }

    #[test]
    fn an_install_outside_a_call_is_allowed() {
        assert_eq!(may_install(false), Ok(()));
    }

    #[test]
    fn an_install_during_a_call_is_refused() {
        assert_eq!(may_install(true), Err(Refusal::InCall));
    }

    #[test]
    fn the_refusal_says_why_and_what_to_do_about_it() {
        let said = Refusal::InCall.message();
        assert!(said.contains("restart"), "{said}");
        assert!(said.contains("call"), "{said}");
    }

    #[test]
    fn every_trouble_has_a_sentence_of_its_own() {
        let all = [
            Trouble::Unreachable,
            Trouble::Manifest,
            Trouble::Signature,
            Trouble::Install,
        ];
        let said: Vec<&str> = all.iter().map(|t| t.message()).collect();

        for sentence in &said {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(!sentence.is_empty());
        }
        let mut unique = said.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            all.len(),
            "two kinds share a sentence: {said:?}"
        );
    }

    #[test]
    fn a_signature_failure_never_blames_the_person_reading_it() {
        // It is the one failure that might be an attack, so it says the update
        // was refused rather than suggesting anything be retried.
        let said = Trouble::Signature.message();
        assert!(!said.contains("again"), "{said}");
        assert!(said.contains("not installed"), "{said}");
    }

    #[test]
    fn the_states_serialise_under_a_tag_the_frontend_switches_on() {
        let ready = serde_json::to_value(Update::Ready {
            version: "0.12.0".to_owned(),
            notes: "https://example.invalid".to_owned(),
        })
        .unwrap();
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["version"], "0.12.0");

        let moving = serde_json::to_value(Update::Downloading {
            received: 12,
            total: None,
        })
        .unwrap();
        assert_eq!(moving["state"], "downloading");
        assert!(moving["total"].is_null());

        assert_eq!(
            serde_json::to_value(Update::UpToDate).unwrap()["state"],
            "upToDate"
        );
    }
}
