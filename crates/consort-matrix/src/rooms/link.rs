// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! A room and its `matrix.to` address, in both directions.
//!
//! A `matrix.to` address names a room one of two ways. A room ID is the room
//! itself and needs nothing but parsing. An alias is a name a homeserver's
//! directory holds, it can be moved, and it may belong to a server this one has
//! only heard of, so the only way to turn one into a room is to ask.
//!
//! Writing one is the same two cases read backwards, and the SDK decides
//! between them: see [`permalink`].
//!
//! ## Why this also answers whether we are in it
//!
//! Because that is the question the interface is really asking. Pressing a link
//! in a message means "show me that", and Consort can only show a room the
//! account has joined: there is no preview and no join flow, and a room ID for a
//! room nobody here is in would leave the caller to invent a sentence about it.
//! Answering both halves here keeps the sentence in one place, which is
//! [`crate::Error::user_message`].

use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, RoomAliasId, RoomId};

use crate::error::{Error, Result};

/// A `matrix.to` address for one room, to give to somebody else.
///
/// `matrix.to` for the reason a message's address is: a link is pasted into
/// places that are not Matrix clients, and this is the form that opens in a
/// browser for somebody who has no client at all.
///
/// Which of the two forms comes back is the SDK's decision and the right one:
/// a published alias if the room has one, because that is the address the room
/// chose and the one a person can read, and otherwise the room ID with the
/// three servers most likely to know it, because a bare room ID is a name with
/// nowhere to ask.
pub async fn permalink(client: &Client, room_id: &str) -> Result<String> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;

    let room = client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;

    Ok(room.matrix_to_permalink().await?.to_string())
}

/// The joined room one address names, whether it is an ID or an alias.
///
/// The directory is only asked when there is an alias to resolve, so a link
/// carrying a room ID, which is most of them, costs nothing.
pub async fn room_at(client: &Client, address: &str) -> Result<String> {
    let room_id = match RoomId::parse(address) {
        Ok(room_id) => room_id,
        Err(_) => {
            let alias = RoomAliasId::parse(address).map_err(|_| Error::NoSuchAddress {
                address: address.to_owned(),
            })?;
            resolve(client, &alias).await?
        }
    };

    // Joined, not merely named. See the header: a room this account is not in
    // is one nothing here can draw.
    client
        .get_room(&room_id)
        .map(|room| room.room_id().to_string())
        .ok_or_else(|| Error::NoSuchRoom {
            room_id: room_id.to_string(),
        })
}

/// One alias, as the room it currently points at.
///
/// A failure here is reported as an address nothing answered to rather than as
/// a homeserver error, because from where somebody is sitting those are the
/// same thing: they pressed a link and there is nowhere to go. The distinction
/// that would matter, a server that is down against an alias that was never
/// real, is not one the directory answer makes.
async fn resolve(client: &Client, alias: &RoomAliasId) -> Result<OwnedRoomId> {
    client
        .resolve_room_alias(alias)
        .await
        .map(|answer| answer.room_id)
        .map_err(|_| Error::NoSuchAddress {
            address: alias.to_string(),
        })
}
