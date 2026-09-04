// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! An address for one message, to give to somebody else.
//!
//! `matrix.to` rather than the `matrix:` scheme, because a link is pasted into
//! places that are not Matrix clients. A `matrix.to` address opens in a browser
//! for somebody who has no client at all, and every client that does have one
//! recognises it; `matrix:` is the better address and is nothing at all to the
//! things people paste links into.
//!
//! ## Why the servers ride along
//!
//! A room ID names a room and says nothing about where to find it. Whoever
//! receives the link may be on a homeserver that has never heard of the room,
//! and without somewhere to ask, their client can only say so. The SDK works
//! out three servers likely to know from who is in the room, which is the
//! routing the specification describes, and that is the whole of what the
//! `?via=` on the end is for.

use matrix_sdk::Client;

use crate::error::Result;
use crate::timeline::{event_id_of, room_of};

/// A `matrix.to` address for one message.
///
/// The room ID rather than its alias, deliberately, and the SDK is the one
/// insisting: an event belongs to a room, an alias can be moved to point at a
/// different one, and a link that survives a room upgrade is worth more than a
/// link that reads nicely.
///
/// Not checked against the room's own timeline. Somebody can only reach this
/// through a message Consort drew, so the event is in the room by
/// construction, and confirming it would be a homeserver round trip to learn
/// what the caller already knew.
pub async fn permalink(client: &Client, room_id: &str, event_id: &str) -> Result<String> {
    let room = room_of(client, room_id)?;
    let event = event_id_of(event_id)?;

    Ok(room.matrix_to_event_permalink(event).await?.to_string())
}
