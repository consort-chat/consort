// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading and writing one room's messages.
//!
//! ## Which rooms this is for
//!
//! All of them, including the voice ones. A voice channel is an ordinary
//! Matrix room whose `m.room.create` carries a call type, and `rooms::facts`
//! reads exactly that one field to tell them apart. Everything else about it
//! is a room: the same timeline, the same `m.room.message`, the same
//! encryption. So there is one implementation here and the only difference is
//! where the shell draws it.
//!
//! ## Built on the base SDK
//!
//! `matrix-sdk-ui` has a timeline that does far more than this: gap-aware
//! storage, edits folded into the events they replace, reactions grouped,
//! local echo, read receipts. It is not a dependency, and adding one would
//! mean pinning a second crate to the same git revision as the SDK, which
//! [`docs/DEPENDENCIES.md`] describes the cost of. What is here instead is the
//! two things the base SDK already gives: the events a sync delivered, and a
//! page of history on request.
//!
//! What that costs is written down rather than hidden. A sync that arrives
//! `limited`, which is what a client that has been offline for a while gets,
//! has a gap in front of it that this appends across without saying so. The
//! messages drawn are all real and all in order; some in the middle may be
//! missing until the room is reopened. Fixing it properly is a gap-aware
//! store, which is the thing `matrix-sdk-ui` exists to be.
//!
//! There is also no local echo. A message goes to the homeserver and appears
//! when the sync brings it back, which on a healthy connection is a moment and
//! on a bad one is visible. Echo means a second, provisional kind of message
//! and a rule for reconciling it, and neither is worth building before
//! somebody has typed into this at all.
//!
//! [`docs/DEPENDENCIES.md`]: https://github.com/consort-chat/consort

pub mod dto;
mod facts;
mod history;

pub use dto::{Message, MessageKind, Timeline};
pub use history::History;

use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::{Client, Room};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::error::{Error, Result};

/// How many messages to ask for at a time.
///
/// Enough to fill a tall window on the first read, so that opening a busy room
/// does not immediately need a second request to have anything to scroll.
/// Small enough that a room with ten years in it does not spend a second
/// decrypting history nobody scrolled to.
const PAGE: u32 = 30;

/// How many pages one ask may consume before giving up and reporting.
///
/// A page can hold nothing to draw. The beginning of every room is a dozen
/// state events before the first word, and a spell of membership churn is the
/// same thing in the middle, so an ask that fetched exactly one page would
/// sometimes answer a scroll with nothing and look broken.
///
/// Bounded because a room can contain more of that than anybody wants to page
/// through on one press, and because each page is a request and a round of
/// decryption.
const PAGES_PER_ASK: usize = 3;

/// A room being watched, and a way to ask it for more.
///
/// Aborts on drop, so replacing one is how a room change is done: there is no
/// path that leaves two watchers publishing to one channel.
pub struct Watch {
    room_id: String,
    task: JoinHandle<()>,
    asking: UnboundedSender<()>,
}

impl Watch {
    /// Which room this is watching.
    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    /// Ask for one more page of history.
    ///
    /// Answered on the watcher's own task, in order with everything else it is
    /// doing, so two presses cannot have their pages interleaved. Silently
    /// ignored once the watcher has ended, which is what a scroll landing at
    /// the same moment as a room change is.
    pub fn earlier(&self) {
        let _ = self.asking.send(());
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Watch one room's messages, reporting all of them whenever they change.
///
/// The whole timeline every time, on the same terms as the room list: a value
/// that is always complete is a value a late subscriber can be handed as-is,
/// and the alternative is a frontend patching its own copy from a stream of
/// deltas it has to receive in order.
///
/// The first report arrives without waiting for a sync, because it is a
/// backfill request rather than a wait. An empty room reports as empty rather
/// than not reporting.
///
/// # Lifetime
///
/// Unlike [`crate::rooms::watch`], this one is per room and is meant to be
/// replaced. Dropping the [`Watch`] ends it.
pub fn watch<F>(client: Client, room_id: &str, on_change: F) -> Watch
where
    F: Fn(Timeline) + Send + Sync + 'static,
{
    let (asking, mut asked) = unbounded_channel();
    let room_id = room_id.to_owned();
    let watching = room_id.clone();

    let task = tokio::spawn(async move {
        // Subscribed before the first page is read, so a message sent between
        // the two is not lost between them.
        let mut updates = client.subscribe_to_all_room_updates();

        let Ok(parsed) = RoomId::parse(&watching) else {
            tracing::warn!(room_id = %watching, "asked to watch something that is not a room");
            on_change(Timeline {
                room_id: watching,
                ..Timeline::default()
            });
            return;
        };
        let Some(room) = client.get_room(&parsed) else {
            // Left from another session between the room list and the click.
            // Reported as an empty room rather than as an error: the shell has
            // a room list arriving that will take the channel away anyway.
            tracing::info!(room_id = %watching, "asked to watch a room this account is not in");
            on_change(Timeline {
                room_id: watching,
                ..Timeline::default()
            });
            return;
        };

        let mut loaded = Loaded::new(watching.clone());
        loaded.publish(&on_change);
        loaded.page(&room, &on_change).await;

        loop {
            tokio::select! {
                asked = asked.recv() => {
                    if asked.is_none() {
                        break;
                    }
                    loaded.page(&room, &on_change).await;
                }
                update = updates.recv() => match update {
                    Ok(update) => {
                        let Some(joined) = update.joined.get(&parsed) else {
                            // The ordinary case. A sync delivers one update
                            // whether or not this room was in it.
                            continue;
                        };
                        let arrived: Vec<Message> =
                            joined.timeline.events.iter().filter_map(facts::message).collect();
                        if loaded.history.arrived(arrived) {
                            loaded.publish(&on_change);
                        }
                    }
                    // Too many syncs while this task was busy decrypting a
                    // page. What was missed is history, and scrolling back is
                    // how it is asked for, so there is nothing to do but carry
                    // on from the next one.
                    Err(RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, room_id = %watching, "fell behind the sync updates");
                    }
                    Err(RecvError::Closed) => break,
                },
            }
        }
    });

    Watch {
        room_id,
        task,
        asking,
    }
}

/// What one watcher is holding.
///
/// Its own type so that `watch` above reads as the loop it is, rather than as
/// six variables threaded through two arms.
struct Loaded {
    room_id: String,
    history: History,
    /// Where the next backwards page starts, or `None` before the first one.
    from: Option<String>,
    /// Whether the homeserver still has older messages.
    more_before: bool,
    /// Whether a page is being fetched right now.
    loading: bool,
}

impl Loaded {
    fn new(room_id: String) -> Self {
        Self {
            room_id,
            history: History::new(),
            from: None,
            // Assumed until the homeserver says otherwise, because the first
            // page has not been asked for yet and "no more history" is a
            // stronger claim than an empty list supports.
            more_before: true,
            loading: true,
        }
    }

    /// Answer one ask for older messages, and report the result.
    ///
    /// Reports twice, once to put the spinner up and once to take it down.
    /// Both are cheap, and the first is the only thing that makes a slow
    /// homeserver distinguishable from a button that did nothing.
    async fn page<F>(&mut self, room: &Room, on_change: &F)
    where
        F: Fn(Timeline),
    {
        if !self.more_before {
            return;
        }

        self.loading = true;
        self.publish(on_change);

        for _ in 0..PAGES_PER_ASK {
            if !self.fetch(room).await {
                break;
            }
        }

        self.loading = false;
        self.publish(on_change);
    }

    /// Fetch one page of older messages.
    ///
    /// `true` only when the page held nothing to draw and the homeserver has
    /// more, which is the one reason to go round again. A failure answers
    /// `false`: the room is still drawn and still live, the scroll can be
    /// tried again, and three requests in a row to a homeserver that just
    /// refused one is not a way to be told anything new.
    async fn fetch(&mut self, room: &Room) -> bool {
        let mut options = MessagesOptions::new(Direction::Backward);
        options.from.clone_from(&self.from);
        options.limit = PAGE.into();

        let page = match room.messages(options).await {
            Ok(page) => page,
            Err(error) => {
                // Logged rather than raised. A dialog about a page of history
                // would be worse than the absence of it.
                tracing::warn!(%error, room_id = %self.room_id, "could not read older messages");
                return false;
            }
        };

        // An empty chunk is the start of the room's visible history, and so is
        // a missing `end`. Both are checked because homeservers differ about
        // which one they say it with.
        self.more_before = page.end.is_some() && !page.chunk.is_empty();
        self.from = page.end;

        // Backwards, so the homeserver answers newest first and the page has
        // to be turned round. Getting this wrong reverses every page while
        // leaving the pages themselves in order, which reads as a conversation
        // that almost makes sense.
        let older: Vec<Message> = page.chunk.iter().rev().filter_map(facts::message).collect();
        let drawable = !older.is_empty();
        self.history.backfilled(older);

        !drawable && self.more_before
    }

    fn publish<F>(&self, on_change: &F)
    where
        F: Fn(Timeline),
    {
        on_change(Timeline {
            room_id: self.room_id.clone(),
            messages: self.history.messages().to_vec(),
            more_before: self.more_before,
            loading: self.loading,
        });
    }
}

/// Say something in a room.
///
/// Encrypted or not according to the room, because the SDK decides that from
/// the room's own state rather than from anything a caller passes.
///
/// Nothing is returned and nothing is echoed. The message appears when the
/// sync brings it back, which is the same path every other message in the room
/// takes. See the module header for why there is no local echo.
pub async fn send(client: &Client, room_id: &str, body: &str) -> Result<()> {
    // Trimmed before it is judged empty, so that a stray newline from a text
    // area is not a message. Sent untrimmed is not an option either: leading
    // spaces in a pasted code block are the message.
    if body.trim().is_empty() {
        return Err(Error::EmptyMessage);
    }

    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;
    let room = client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;

    room.send(RoomMessageEventContent::text_plain(body)).await?;
    Ok(())
}
