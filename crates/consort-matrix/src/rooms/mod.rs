// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the account is actually in, arranged the way the shell draws it.
//!
//! Consort has been able to sign in, verify itself and read its own keys for a
//! while, and in all that time it has not once been able to say what rooms the
//! account is in. This is that: one rail entry per joined space, a Home entry
//! for everything belonging to no joined space, and the channels underneath
//! each of them, with voice channels marked as voice channels so that the next
//! milestone has something to connect to.
//!
//! Three parts, and the split is about testability. `dto` is the wire format.
//! `facts` pulls what is needed out of a `matrix_sdk::Room`, which is the only
//! part that needs a live client. `snapshot` does the grouping, the ordering
//! and the orphan detection over plain data, which is where the rules live and
//! where all of them are tested. [`avatar`] stands apart from all three: it is
//! the one thing here that fetches, and it is asked for a room at a time
//! rather than carried in the snapshot.
//!
//! ## The one request
//!
//! Almost none of this touches the network. Everything the snapshot reads is
//! already in the local state store, which matters because [`watch`] runs on
//! every sync that changed anything, forever, and a request in there would
//! turn an idle client into a client that polls.
//!
//! [`hierarchy`] is the exception, and it is kept at arm's length for exactly
//! that reason. A room a space lists and this account has never joined has no
//! name anywhere locally, so the space has to be asked. That happens once per
//! space per distinct set of unjoined children, after the snapshot has already
//! gone out, and the answer is folded into the next one.

mod avatar;
pub mod dto;
mod facts;
mod hierarchy;
mod snapshot;

pub use avatar::avatar;
pub use dto::{Channel, ChannelKind, HOME_ID, Rooms, Space};

use matrix_sdk::Client;
use matrix_sdk::sync::RoomUpdates;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

use crate::verification::Changes;
use hierarchy::Directory;

/// The room list as it stands right now, from the local store alone.
///
/// Cheap enough to call on every sync, and safe to call while offline: it
/// describes what the account was last known to be in, which is the right
/// answer when there is nothing newer.
///
/// Children the account has never joined come back unnamed, because nothing
/// local knows what they are called. [`watch`] fills those in; a caller using
/// this directly gets the honest absence.
pub async fn snapshot(client: &Client) -> Rooms {
    let mut facts = Vec::new();

    for room in client.joined_rooms() {
        facts.push(facts::extract(&room).await);
    }

    snapshot::assemble(facts)
}

/// Watch the account's rooms, reporting the whole tree whenever it changes.
///
/// The first report arrives without waiting for a sync. By the time this is
/// spawned the first sync may already have landed, and a shell that stays
/// empty until the next one is a shell that looks broken for thirty seconds
/// after every restart.
///
/// Reports are filtered down to actual changes. The SDK sends one update per
/// sync whether or not anything happened, and a sync carrying a typing
/// notification changes no room in this list, so forwarding every update would
/// wake the webview twice a minute to hand it what it already has.
///
/// # Lifetime
///
/// Same as [`crate::sync::start`], [`crate::verification::watch`] and
/// [`crate::backup::watch`]. The task holds the `Client` and watches a channel
/// belonging to it, so it never ends on its own. The caller owns the handle
/// and aborts it when the session does.
pub fn watch<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(Rooms) + Send + 'static,
{
    tokio::spawn(async move {
        // Subscribed before the first snapshot is taken, so that a sync
        // landing in between is still reported rather than missed.
        let mut updates = client.subscribe_to_all_room_updates();
        let mut changes = Changes::new();
        let mut directory = Directory::default();

        loop {
            let mut rooms = snapshot(&client).await;
            directory.apply(&mut rooms);
            report(rooms.clone(), &mut changes, &on_change);

            // The one request in this module, and only when a space has an
            // unjoined child nobody has asked about yet. It runs after the
            // snapshot has gone out rather than before, so a slow homeserver
            // delays two channel names instead of the whole room list.
            if directory.refresh(&client, &rooms).await {
                directory.apply(&mut rooms);
                report(rooms, &mut changes, &on_change);
            }

            if !touched_a_room(&mut updates).await {
                break;
            }
        }

        // Only reachable once the client is gone, which cannot happen while
        // this task holds one. Logged rather than ignored so a future change
        // to that is not silent.
        tracing::warn!("the room watcher ended");
    })
}

/// Hand a tree over, if it says anything new.
fn report<F>(rooms: Rooms, changes: &mut Changes<Rooms>, on_change: &F)
where
    F: Fn(Rooms),
{
    let Some(rooms) = changes.accept(rooms) else {
        return;
    };

    tracing::info!(
        spaces = rooms.spaces.len().saturating_sub(1),
        channels = rooms
            .spaces
            .iter()
            .map(|space| space.channels.len())
            .sum::<usize>(),
        "the room list changed"
    );

    on_change(rooms);
}

/// Wait until a sync arrives that touched at least one room.
///
/// False once the client is gone, which is the only way out of the loop above.
async fn touched_a_room(updates: &mut Receiver<RoomUpdates>) -> bool {
    loop {
        match updates.recv().await {
            // A sync that touched no room at all: the idle case, once every
            // thirty seconds for as long as the client is running. Taking a
            // snapshot for it would read every room in the store to produce
            // the answer it produced last time.
            Ok(update) if update.is_empty() => continue,
            Ok(_) => return true,
            // The channel holds thirty-two updates and this receiver fell
            // behind them. What was missed does not matter, because the next
            // snapshot describes the account as it is now rather than as a
            // series of edits, which is the second reason the whole tree is
            // sent every time.
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "missed some room updates");
                return true;
            }
            Err(RecvError::Closed) => return false,
        }
    }
}
