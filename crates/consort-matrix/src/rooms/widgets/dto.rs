// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for a room's widgets.
//!
//! None of the SDK's own types appear here, for the reason given at the top of
//! [`crate::rooms::dto`]: the wire format is a contract with the frontend, and
//! pinning it to an upstream type lets an SDK bump change it silently.

use serde::{Deserialize, Serialize};

/// Where a room's layout puts a widget.
///
/// Element's two containers, and the names are its own
/// (`io.element.widgets.layout`). [`Container::Right`] is the default for a
/// widget the layout says nothing about, which is Element's behaviour and the
/// reason a widget somebody added without touching the layout does not appear
/// over the timeline by itself.
/// The order of the variants is load-bearing: `layout::arrange` sorts on it, so
/// the top widgets come out before the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Container {
    /// Across the top of the room, above the timeline.
    Top,
    /// Beside the timeline.
    #[default]
    Right,
}

/// A widget a room says it has, with everything undrawable or unsafe already
/// dropped.
///
/// [`Self::url`] is the only field anything has to trust, and it is the one
/// field here that a room member chose. See `definition` for what it
/// has already been through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    /// The state key, which is the widget's ID. Deliberately not `content.id`:
    /// that is a second, unenforced copy of the same thing, nothing makes the
    /// two agree, and the layout addresses widgets by state key.
    pub id: String,
    /// `None` rather than the widget's type standing in for a name, so the
    /// interface cannot label something `m.custom` by forgetting a check.
    pub name: Option<String>,
    /// Absolute and `https:`, with the template variables already filled in.
    pub url: String,
    pub container: Container,
    /// A percentage of the container, from the room's layout. `None` when the
    /// layout says nothing, which is the caller's cue to use its own size.
    pub width: Option<u8>,
    pub height: Option<u8>,
}

/// What the spec's default template variables stand for.
///
/// Filled by the caller rather than read in here, which is what keeps the
/// templating rules testable without a homeserver.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Viewer {
    pub user_id: String,
    pub room_id: String,
    /// The viewer's display name, or their user ID when they have not set one.
    /// The fallback is the spec's, not a convenience.
    pub display_name: String,
}
