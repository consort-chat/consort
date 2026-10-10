// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What a room says its widgets are.
//!
//! A widget is a web page a room asks a client to embed, and the room says so
//! in state: one event per widget keyed by its ID, plus one layout event
//! saying where they go. Nothing here embeds anything, because the webview
//! refuses a remote origin in a frame and widening that is a decision nobody
//! has taken. What a widget may load, and where every event type and field
//! below was read from, is in
//! `docs/adr/0016-what-a-widget-is-allowed-to-load.md`.
//!
//! Three parts, split the way [`crate::rooms`] is split. `definition` decides
//! whether one event describes something drawable and holds every refusal.
//! `layout` arranges what survived. This module is the only part needing a
//! live client, and holds no rules of its own.

mod definition;
mod dto;
mod layout;
mod template;

pub use dto::{Container, Viewer, Widget};

use std::collections::BTreeMap;

use matrix_sdk::Client;
use matrix_sdk::Room;
use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::events::StateEventType;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};

/// What Element writes, and the only thing it reads.
const WIDGET_STATE_ELEMENT: &str = "im.vector.modular.widgets";

/// What MSC2764 calls the same event.
const WIDGET_STATE_SPEC: &str = "m.widget";

/// Where the widgets go. Element's own event, with an empty state key.
const WIDGET_LAYOUT_STATE: &str = "io.element.widgets.layout";

/// The two fields of a state event this needs, whichever type it is.
///
/// Both a joined room's state and an invited room's stripped state carry them,
/// so one shape reads either.
#[derive(Deserialize)]
struct StateEvent {
    state_key: String,
    content: Value,
}

/// The widgets a room has, arranged the way the room asks for.
pub async fn widgets(client: &Client, room_id: &str) -> Result<Vec<Widget>> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;
    let room = client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;

    let viewer = viewer_of(client, &room).await?;
    let found = definitions_of(&room, &viewer).await;

    Ok(layout::arrange(found, layout_of(&room).await))
}

/// Every drawable widget the room's state describes.
async fn definitions_of(room: &Room, viewer: &Viewer) -> Vec<Widget> {
    let mut found: BTreeMap<String, Widget> = BTreeMap::new();

    // Element's name first, so that the spec's wins a room carrying both for
    // one widget, which is the direction a migration runs.
    for event_type in [WIDGET_STATE_ELEMENT, WIDGET_STATE_SPEC] {
        let events = match room
            .get_state_events(StateEventType::from(event_type))
            .await
        {
            Ok(events) => events,
            // A room whose widgets could not be read renders as a room with no
            // widgets, which is what it looked like before this existed.
            Err(error) => {
                tracing::warn!(%error, room_id = %room.room_id(), event_type, "could not read a room's widgets");
                continue;
            }
        };

        for raw in events {
            let Some(event) = fields_of(&raw) else {
                continue;
            };
            if let Some(widget) = definition::of(&event.state_key, event.content, viewer) {
                found.insert(widget.id.clone(), widget);
            }
        }
    }

    found.into_values().collect()
}

/// The room's layout event, if it has one worth reading.
async fn layout_of(room: &Room) -> Option<Value> {
    let raw = room
        .get_state_event(StateEventType::from(WIDGET_LAYOUT_STATE), "")
        .await
        .inspect_err(|error| {
            tracing::warn!(%error, room_id = %room.room_id(), "could not read a room's widget layout");
        })
        .ok()??;

    fields_of(&raw).map(|event| event.content)
}

/// The two fields this needs, out of either shape of state event.
fn fields_of(raw: &RawAnySyncOrStrippedState) -> Option<StateEvent> {
    match raw {
        RawAnySyncOrStrippedState::Sync(raw) => raw.deserialize_as_unchecked().ok(),
        RawAnySyncOrStrippedState::Stripped(raw) => raw.deserialize_as_unchecked().ok(),
    }
}

/// What the default template variables stand for, for this account in this
/// room.
async fn viewer_of(client: &Client, room: &Room) -> Result<Viewer> {
    let user_id = client.user_id().ok_or(Error::NotLoggedIn)?;

    // The spec's fallback, not a convenience: a display name nobody set is the
    // user ID.
    let display_name = room
        .get_member_no_sync(user_id)
        .await
        .ok()
        .flatten()
        .and_then(|member| member.display_name().map(ToOwned::to_owned))
        .unwrap_or_else(|| user_id.to_string());

    Ok(Viewer {
        user_id: user_id.to_string(),
        room_id: room.room_id().to_string(),
        display_name,
    })
}
