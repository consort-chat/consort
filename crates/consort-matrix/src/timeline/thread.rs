// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading one thread out of a room.
//!
//! A thread is not in the room's timeline. Its replies carry an `m.thread`
//! relation and `timeline::facts` keeps them out, so the only way to read one
//! is to ask the homeserver for the events related to the message it hangs
//! from. That is `/relations`, and the SDK decrypts what comes back, so an
//! encrypted room needs nothing special here.
//!
//! ## Why the recent end
//!
//! The page is asked for backwards. A thread long enough to need two pages is
//! one somebody is opening to read the end of, and answering with its first
//! fifty replies would put the panel at the part of the conversation nobody
//! asked about. What comes back is turned round before it is handed on, so the
//! panel draws downwards like every other conversation.

use matrix_sdk::room::{IncludeRelations, RelationsOptions};
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::relation::RelationType;
use matrix_sdk::ruma::{EventId, RoomId, UInt};
use matrix_sdk::{Client, Room};

use crate::error::{Error, Result};
use crate::timeline::dto::Thread;
use crate::timeline::facts;

/// How many replies one page holds.
///
/// Enough that almost every thread arrives whole, and few enough that opening
/// one is a single small request. What does not fit is reported rather than
/// dropped: see [`Thread::more_before`].
const PAGE: u32 = 50;

/// Everything currently readable in the thread hanging from `root_id`.
pub async fn thread(client: &Client, room_id: &str, root_id: &str) -> Result<Thread> {
    let room = room_of(client, room_id)?;
    let root_event_id = EventId::parse(root_id).map_err(|_| Error::NoSuchEvent {
        event_id: root_id.to_owned(),
    })?;

    // Fetched rather than taken from the room's own timeline, because a thread
    // can be opened from a message that has since scrolled out of what is
    // loaded, and because the panel has to stand on its own.
    //
    // A failure here is not a failure of the whole thread. A redacted root and
    // one this session has no key for both look like this, and the replies are
    // what somebody opened the panel to read.
    let root = room
        .load_or_fetch_event(&root_event_id, None)
        .await
        .ok()
        .and_then(|event| facts::message(&event));

    let relations = room
        .relations(
            root_event_id,
            RelationsOptions {
                dir: Direction::Backward,
                limit: Some(UInt::from(PAGE)),
                include_relations: IncludeRelations::RelationsOfType(RelationType::Thread),
                ..RelationsOptions::default()
            },
        )
        .await?;

    let mut messages: Vec<_> = relations
        .chunk
        .iter()
        .filter_map(facts::in_thread)
        .collect();
    messages.reverse();

    Ok(Thread {
        room_id: room_id.to_owned(),
        root_id: root_id.to_owned(),
        root,
        messages,
        more_before: relations.prev_batch_token.is_some(),
    })
}

/// The room this account is in, by ID.
fn room_of(client: &Client, room_id: &str) -> Result<Room> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;
    client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })
}
