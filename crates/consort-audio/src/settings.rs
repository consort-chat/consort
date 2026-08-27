// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The audio choices worth remembering between runs.
//!
//! Data only. Where this is written, and in what file, is the application's
//! business, and keeping it out of here is what lets this crate stay free of
//! everything the application depends on.
//!
//! Every field defaults. A settings file outlives the build that wrote it: it
//! gets read by an older Consort after a downgrade, by a newer one after an
//! upgrade, and by whatever somebody turned it into with a text editor. None of
//! those should stop the application from starting, so an unreadable field
//! falls back rather than failing.

use serde::{Deserialize, Serialize};

use crate::gate::GateConfig;

/// Which devices to use, and how eager the voice gate should be.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// The chosen input device by name, or `None` to ask the host.
    ///
    /// `None` rather than an empty string, because an empty string is a device
    /// name that can never match anything.
    pub input: Option<String>,
    /// The chosen output device by name, or `None` to ask the host.
    pub output: Option<String>,
    /// How the voice gate is tuned. Per person and per room, so there is no
    /// useful default beyond a starting point.
    pub gate: GateConfig,
}
