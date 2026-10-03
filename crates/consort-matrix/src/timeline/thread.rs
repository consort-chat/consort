// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading one thread out of a room.
//!
//! A thread is not in the room's timeline, so the only way to read one is
//! `/relations` against the message it hangs from. The SDK decrypts what comes
//! back, so an encrypted room needs nothing special here.
//!
//! Asked for backwards, because a thread long enough to need two pages is one
//! somebody is opening to read the end of, and turned round before it is
//! handed on so the panel draws downwards.

use matrix_sdk::Client;
use matrix_sdk::room::{IncludeRelations, RelationsOptions};
use matrix_sdk::ruma::UInt;
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::relation::RelationType;

use crate::error::Result;
use crate::timeline::dto::Thread;
use crate::timeline::{event_id_of, facts, room_of};

/// How many replies one page holds: enough that almost every thread arrives
/// whole. What does not fit is reported rather than dropped, through
/// [`Thread::more_before`].
const PAGE: u32 = 50;

/// Everything currently readable in the thread hanging from `root_id`.
pub async fn thread(client: &Client, room_id: &str, root_id: &str) -> Result<Thread> {
    let room = room_of(client, room_id)?;
    let root_event_id = event_id_of(root_id)?;

    // Fetched rather than taken from the room's own timeline, because a thread
    // can be opened from a message that has since scrolled out of it.
    //
    // A failure here is not a failure of the whole thread: a redacted root and
    // one with no key both look like this, and the replies still read.
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
