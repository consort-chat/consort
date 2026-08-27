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

pub use avatar::{avatar, member_avatar};
pub use dto::{Channel, ChannelKind, HOME_ID, Participant, Rooms, Space};

use std::time::Duration;

use matrix_sdk::Client;
use matrix_sdk::sync::RoomUpdates;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

use crate::verification::Changes;
use hierarchy::Directory;

/// How long to wait before looking again while somebody is in a call.
///
/// Only for the one thing no event announces. Joining a call writes a state
/// event and so does leaving one, so both of those reach [`watch`] through the
/// ordinary room update and neither needs this. A membership that simply runs
/// out sends nothing at all, and without a second look the person whose laptop
/// was closed stays in the channel for the four hours their membership was
/// good for.
///
/// Thirty seconds is what matrix-rust-rtc's own bridge waits for the same
/// problem in the same dialect, and there is no reason to disagree with it.
/// The cost is a snapshot, which reads memory and the local store and touches
/// no network, and an unchanged snapshot is filtered before it reaches the
/// webview.
const OCCUPIED_POLL: Duration = Duration::from_secs(30);

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

/// Name the people a live call roster names, in the order it gives them.
///
/// The other half of what a voice channel's participant list can come from.
/// [`snapshot`] reads room state, which is what every channel this session is
/// not sitting in has to use; a channel it is sitting in has something better,
/// a roster derived from MatrixRTC signalling and enriched with real media
/// state. What that roster does not have is names, because a Matrix profile is
/// per room and only a room can answer.
///
/// Local, and no request. Somebody in the call whose `m.room.member` has not
/// arrived comes back as their user ID, which is the same answer the room-state
/// path gives for the same reason. Per human, not per device: a roster is per
/// membership, so the same person on a laptop and a phone arrives twice and
/// leaves here once.
///
/// A room this account is not in comes back empty, because there is no local
/// store to read names out of and inventing them would be worse than the
/// absence.
pub async fn name_participants(
    client: &Client,
    room_id: &str,
    user_ids: &[String],
) -> Vec<Participant> {
    let Ok(parsed) = matrix_sdk::ruma::RoomId::parse(room_id) else {
        return Vec::new();
    };
    let Some(room) = client.get_room(&parsed) else {
        return Vec::new();
    };

    facts::name_all(&room, user_ids.iter().map(String::as_str)).await
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

            // Read before the hierarchy request, which can only fill in the
            // names of rooms this account has not joined and so can never
            // change who is in a call. Reading it here means the value is
            // still to hand after `rooms` has been handed over below.
            let poll = poll_after(&rooms);

            // The one request in this module, and only when a space has an
            // unjoined child nobody has asked about yet. It runs after the
            // snapshot has gone out rather than before, so a slow homeserver
            // delays two channel names instead of the whole room list.
            if directory.refresh(&client, &rooms).await {
                directory.apply(&mut rooms);
                report(rooms, &mut changes, &on_change);
            }

            if !wait(&mut updates, poll).await {
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
        // Counted because it is the one part of this that changes on its own,
        // without anybody here doing anything. Spaces and channels move when
        // this account joins or leaves something, so a line carrying only
        // those says nothing at all about somebody else picking up a call.
        in_calls = rooms
            .spaces
            .iter()
            .flat_map(|space| &space.channels)
            .map(|channel| channel.participants.len())
            .sum::<usize>(),
        "the room list changed"
    );

    on_change(rooms);
}

/// How long the watcher should wait before looking again on its own.
///
/// `None` unless somebody is actually in a voice channel, which is the usual
/// answer. An account with every call empty arms no timer at all, so an idle
/// Consort stays idle, and the timer exists only for the window in which
/// something can go stale without saying so.
fn poll_after(rooms: &Rooms) -> Option<Duration> {
    rooms
        .spaces
        .iter()
        .flat_map(|space| &space.channels)
        .any(|channel| !channel.participants.is_empty())
        .then_some(OCCUPIED_POLL)
}

/// Wait for something worth re-reading for.
///
/// A sync that touched a room, or, when `poll` says so, the timer running out.
/// False once the client is gone, which is the only way out of the loop above.
///
/// Dropping the half of the [`tokio::select`] that did not win is safe here:
/// `recv` on a broadcast receiver is cancellation safe, so a tick that
/// interrupts the wait loses no update.
async fn wait(updates: &mut Receiver<RoomUpdates>, poll: Option<Duration>) -> bool {
    let Some(poll) = poll else {
        return touched_a_room(updates).await;
    };

    tokio::select! {
        touched = touched_a_room(updates) => touched,
        () = tokio::time::sleep(poll) => true,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use dto::{Channel, ChannelKind, Participant};
    use matrix_sdk::ruma::owned_room_id;
    use matrix_sdk::sync::JoinedRoomUpdate;

    fn channel(kind: ChannelKind, participants: Vec<Participant>) -> Channel {
        Channel {
            id: "!v:example.org".to_owned(),
            name: Some("Lounge".to_owned()),
            kind,
            avatar: None,
            joined: true,
            participants,
        }
    }

    fn ada() -> Participant {
        Participant {
            id: "@ada:example.org".to_owned(),
            name: "Ada".to_owned(),
        }
    }

    fn rooms(channels: Vec<Channel>) -> Rooms {
        Rooms {
            spaces: vec![Space::home(channels)],
        }
    }

    /// A sync that touched one room, which is all `touched_a_room` looks at.
    fn touched_one_room() -> RoomUpdates {
        let mut updates = RoomUpdates::default();
        updates.joined.insert(
            owned_room_id!("!v:example.org"),
            JoinedRoomUpdate::default(),
        );
        updates
    }

    mod polling {
        use super::*;

        #[test]
        fn an_account_with_nobody_in_a_call_arms_no_timer() {
            // The usual case, and the reason this is a decision rather than a
            // fixed interval: an idle Consort has to stay idle.
            assert_eq!(poll_after(&rooms(Vec::new())), None);
            assert_eq!(
                poll_after(&rooms(vec![channel(ChannelKind::Voice, Vec::new())])),
                None
            );
        }

        #[test]
        fn an_occupied_voice_channel_arms_one() {
            assert_eq!(
                poll_after(&rooms(vec![channel(ChannelKind::Voice, vec![ada()])])),
                Some(OCCUPIED_POLL)
            );
        }

        #[test]
        fn one_occupied_channel_anywhere_is_enough() {
            // The rail can hold many spaces, and the timer is about the
            // account rather than about whichever one is on screen.
            let occupied = Rooms {
                spaces: vec![
                    Space::home(vec![channel(ChannelKind::Text, Vec::new())]),
                    Space {
                        id: "!s:example.org".to_owned(),
                        name: "Kahu HQ".to_owned(),
                        avatar: None,
                        channels: vec![channel(ChannelKind::Voice, vec![ada()])],
                    },
                ],
            };

            assert_eq!(poll_after(&occupied), Some(OCCUPIED_POLL));
        }
    }

    mod waiting {
        use super::*;

        #[tokio::test]
        async fn a_sync_that_touched_a_room_ends_the_wait() {
            let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
            sender.send(touched_one_room()).unwrap();

            assert!(wait(&mut receiver, None).await);
        }

        #[tokio::test]
        async fn a_sync_that_touched_nothing_does_not() {
            // Sync fires every thirty seconds forever whether or not anything
            // happened. Waking for those is a full re-render of the shell,
            // twice a minute, carrying what it carried last time.
            let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
            sender.send(RoomUpdates::default()).unwrap();

            let woke =
                tokio::time::timeout(Duration::from_millis(50), wait(&mut receiver, None)).await;

            assert!(woke.is_err(), "{woke:?}");
        }

        #[tokio::test]
        async fn the_timer_ends_the_wait_when_no_sync_does() {
            // The whole reason the timer exists. A membership that runs out
            // sends nothing, so without this nothing would ever look again.
            let (_sender, mut receiver) = tokio::sync::broadcast::channel(4);

            let woke = tokio::time::timeout(
                Duration::from_secs(5),
                wait(&mut receiver, Some(Duration::from_millis(10))),
            )
            .await;

            assert_eq!(woke.ok(), Some(true));
        }

        #[tokio::test]
        async fn a_sync_still_wins_while_the_timer_is_armed() {
            let (sender, mut receiver) = tokio::sync::broadcast::channel(4);
            sender.send(touched_one_room()).unwrap();

            let woke = tokio::time::timeout(
                Duration::from_millis(50),
                wait(&mut receiver, Some(Duration::from_secs(3_600))),
            )
            .await;

            assert_eq!(woke.ok(), Some(true));
        }

        #[tokio::test]
        async fn a_client_that_is_gone_ends_the_loop_rather_than_the_wait() {
            // The one case that returns false, and the only way out of the
            // watcher's loop.
            let (sender, mut receiver) = tokio::sync::broadcast::channel::<RoomUpdates>(4);
            drop(sender);

            assert!(!wait(&mut receiver, None).await);
            assert!(!wait(&mut receiver, Some(Duration::from_secs(3_600))).await);
        }

        #[tokio::test]
        async fn falling_behind_is_a_reason_to_look_rather_than_to_give_up() {
            // The channel holds a bounded number of updates. What was missed
            // does not matter: the next snapshot describes the account as it
            // is now rather than as a series of edits.
            let (sender, mut receiver) = tokio::sync::broadcast::channel(1);
            sender.send(touched_one_room()).unwrap();
            sender.send(touched_one_room()).unwrap();

            assert!(wait(&mut receiver, None).await);
        }
    }
}
