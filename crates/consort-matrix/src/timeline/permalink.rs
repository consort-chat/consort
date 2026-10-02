// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! An address for one message, to give to somebody else.
//!
//! `matrix.to` rather than the `matrix:` scheme, because a link gets pasted
//! into places that are not Matrix clients and `matrix:` means nothing to
//! them.
//!
//! The `?via=` on the end is routing: a room ID says nothing about where to
//! find the room, so the SDK names three servers likely to know.

use matrix_sdk::Client;

use crate::error::Result;
use crate::timeline::{event_id_of, room_of};

/// A `matrix.to` address for one message.
///
/// The room ID rather than its alias, which the SDK insists on: an alias can
/// be moved to point at a different room.
///
/// Not checked against the room's own timeline, because somebody can only
/// reach this through a message Consort drew.
pub async fn permalink(client: &Client, room_id: &str, event_id: &str) -> Result<String> {
    let room = room_of(client, room_id)?;
    let event = event_id_of(event_id)?;

    Ok(room.matrix_to_event_permalink(event).await?.to_string())
}
