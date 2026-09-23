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
//! three things the base SDK already gives: the events a sync delivered, a
//! page of history on request, and the window around one event.
//!
//! What that costs is written down rather than hidden. A sync that arrives
//! `limited`, which is what a client that has been offline for a while gets,
//! has a gap in front of it that this appends across without saying so. The
//! messages drawn are all real and all in order; some in the middle may be
//! missing until the room is reopened. Fixing it properly is a gap-aware
//! store, which is the thing `matrix-sdk-ui` exists to be.
//!
//! The window is the same absence answered honestly rather than papered over.
//! With nowhere to put a piece of history that is not next to what is loaded,
//! going to a message somebody linked or answered means drawing that part of
//! the room instead of this one, and saying so. A store that knew where its
//! gaps were could hold both at once and would not have to choose.
//!
//! There is also no local echo. A message goes to the homeserver and appears
//! when the sync brings it back, which on a healthy connection is a moment and
//! on a bad one is visible. Echo means a second, provisional kind of message
//! and a rule for reconciling it, and neither is worth building before
//! somebody has typed into this at all.
//!
//! [`docs/DEPENDENCIES.md`]: https://github.com/consort-chat/consort

mod answering;
mod around;
pub mod dto;
mod edits;
pub(crate) mod facts;
mod history;
mod media;
mod permalink;
mod reactions;
mod sending;
mod thread;

pub use dto::{Media, Message, MessageKind, Reaction, Thread, ThreadSummary, Timeline, Typing};
pub use edits::Edits;
pub use history::History;
pub use media::{Attachment, MAX_BYTES, bytes, media};
pub use permalink::permalink;
pub use reactions::Reactions;
pub use sending::{Attaching, send_attachment};
pub use thread::thread;

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::room::edit::{EditError, EditedContent};
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::relation::Annotation;
use matrix_sdk::ruma::events::relation::Thread as ThreadRelation;
use matrix_sdk::ruma::events::room::message::{
    AddMentions, ForwardThread, Relation, ReplyMetadata, ReplyWithinThread, RoomMessageEventContent,
};
use matrix_sdk::ruma::events::{AnySyncEphemeralRoomEvent, AnySyncTimelineEvent};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{EventId, OwnedEventId, RoomId, UserId};
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

/// What a watcher can be asked to do.
///
/// One channel rather than two, so that opening a thread and scrolling back
/// cannot be answered in the other order from the one they were asked in.
enum Ask {
    /// One more page of the room's own history.
    Earlier,
    /// One more page of what came after what is loaded.
    ///
    /// Only ever answerable inside a window somebody jumped into. The room as
    /// it is normally drawn ends at the live end, where later is a sync away
    /// rather than a request.
    Later,
    /// Draw the history around this message instead of the present.
    Around(String),
    /// Go back to the live end of the room.
    Present,
    /// Open the thread hanging from this message, or close whatever is open.
    Thread(Option<String>),
    /// Say that everything up to this message has been read.
    ///
    /// The boolean is whether the receipt that rides along with the marker is
    /// the public one. Carried per ask rather than held by the watcher because
    /// it is a setting somebody can change with a room already open, and a
    /// watcher holding the answer from when the room was opened would keep
    /// publishing receipts under the choice that has just been revoked.
    Read(String, bool),
}

/// A room being watched, and a way to ask it for more.
///
/// Aborts on drop, so replacing one is how a room change is done: there is no
/// path that leaves two watchers publishing to one channel.
pub struct Watch {
    room_id: String,
    task: JoinHandle<()>,
    asking: UnboundedSender<Ask>,
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
        let _ = self.asking.send(Ask::Earlier);
    }

    /// Ask for one more page of what came after what is loaded.
    ///
    /// Answered with nothing at the live end, which is where the room normally
    /// is: there is no page after the present, and what comes next arrives on
    /// its own.
    pub fn later(&self) {
        let _ = self.asking.send(Ask::Later);
    }

    /// Draw the history around `event_id` instead of the present.
    ///
    /// What following a reply row or a link to a message older than what is
    /// loaded does. The window replaces what was loaded rather than joining
    /// it, and [`present`](Self::present) is the way back.
    ///
    /// Answered on the watcher's own task, like everything else here, so a
    /// jump and a scroll cannot be answered in the other order from the one
    /// they were asked in.
    pub fn go_to(&self, event_id: String) {
        let _ = self.asking.send(Ask::Around(event_id));
    }

    /// Go back to the live end of the room.
    ///
    /// The first page again, freshly read, which is also how whatever was said
    /// while somebody was reading last March arrives. What had been scrolled
    /// back through is not kept: coming back to the present is a request to be
    /// at the bottom of the room, and restoring somebody's old scroll position
    /// underneath them would be answering a different one.
    pub fn present(&self) {
        let _ = self.asking.send(Ask::Present);
    }

    /// Open the thread hanging from `root_id`, or close whatever is open.
    ///
    /// Answered on the watcher's own task, like everything else, so a thread
    /// opened and closed quickly cannot report the two out of order. Silently
    /// ignored once the watcher has ended, which is what pressing a thread at
    /// the same moment as a room change is.
    pub fn open_thread(&self, root_id: Option<String>) {
        let _ = self.asking.send(Ask::Thread(root_id));
    }

    /// Say that everything up to and including `event_id` has been read.
    ///
    /// Safe to call as often as a scroll does. The watcher drops an ask naming
    /// the message it last sent, so a reader sitting still at the bottom of a
    /// quiet room sends one receipt and then nothing, however many times this
    /// is called.
    ///
    /// This is also where leaving a room cancels the receipt. The ask travels
    /// on the channel a room change drops, so one that arrives just after
    /// somebody clicked away is answered by nobody rather than by the new
    /// room's watcher, and no receipt is sent for a room nobody is reading.
    pub fn mark_read(&self, event_id: String, public: bool) {
        let _ = self.asking.send(Ask::Read(event_id, public));
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
pub fn watch<F, G, H>(
    client: Client,
    room_id: &str,
    on_change: F,
    on_thread: G,
    on_typing: H,
) -> Watch
where
    F: Fn(Timeline) + Send + Sync + 'static,
    G: Fn(Option<Thread>) + Send + Sync + 'static,
    H: Fn(Typing) + Send + Sync + 'static,
{
    let (asking, mut asked) = unbounded_channel();
    let room_id = room_id.to_owned();
    let watching = room_id.clone();

    let task = tokio::spawn(async move {
        // Subscribed before the first page is read, so a message sent between
        // the two is not lost between them.
        let mut updates = client.subscribe_to_all_room_updates();
        // The same reasoning, for keys. A key that lands while the first page
        // is decrypting would otherwise be missed, and missing one leaves a
        // message waiting for a key this session already holds.
        //
        // `None` before the crypto machine exists, which for a signed-in
        // client it always does. Nothing waits on a stream that is not there:
        // an unreadable message stays unreadable, which is what happened
        // before any of this.
        let mut rekeyed = client.encryption().room_keys_received_stream().await;

        let Ok(parsed) = RoomId::parse(&watching) else {
            tracing::warn!(room_id = %watching, "asked to watch something that is not a room");
            on_change(Timeline {
                room_id: watching,
                ..Timeline::default()
            });
            on_thread(None);
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
            on_thread(None);
            return;
        };

        let mut loaded = Loaded::new(watching.clone(), client.user_id().map(ToString::to_string));
        // Before the first page, and before anything this watcher does can
        // move it. Reading the room is what moves the marker, so a value read
        // any later would already be the answer to "what have you just read".
        loaded.read_up_to = room.fully_read_event_id().map(|id| id.to_string());
        loaded.publish(&on_change);
        loaded.publish_thread(&on_thread);
        // Said once at the start, so a reader that has just changed room is
        // not left holding the last room's answer until somebody here types.
        // In a quiet room that is never.
        on_typing(Typing {
            room_id: watching.clone(),
            users: Vec::new(),
        });
        loaded.page(&room, &on_change, Direction::Backward).await;

        loop {
            tokio::select! {
                asked = asked.recv() => {
                    match asked {
                        None => break,
                        Some(Ask::Earlier) => {
                            loaded.page(&room, &on_change, Direction::Backward).await;
                        }
                        Some(Ask::Later) => {
                            loaded.page(&room, &on_change, Direction::Forward).await;
                        }
                        // Boxed for the reason the thread arm below is: the
                        // compiler otherwise gives up computing the layout of
                        // this task.
                        Some(Ask::Around(event_id)) => {
                            Box::pin(loaded.go_to(&room, &on_change, event_id)).await;
                        }
                        Some(Ask::Present) => {
                            Box::pin(loaded.present(&room, &on_change)).await;
                        }
                        Some(Ask::Thread(root_id)) => {
                            // Boxed because the compiler otherwise gives up
                            // computing the layout of this task: the arm holds
                            // a `/relations` request and an event fetch, both
                            // of them deep, inside a `select!` inside a spawn.
                            Box::pin(loaded.open(&client, root_id)).await;
                            loaded.publish_thread(&on_thread);
                        }
                        // Boxed for the reason the two above are.
                        Some(Ask::Read(event_id, public)) => {
                            Box::pin(loaded.mark_read(&room, event_id, public)).await;
                        }
                    }
                }
                update = updates.recv() => match update {
                    Ok(update) => {
                        let Some(joined) = update.joined.get(&parsed) else {
                            // The ordinary case. A sync delivers one update
                            // whether or not this room was in it.
                            continue;
                        };
                        // The thread first, because counting a reply against
                        // the message it hangs from writes into the same
                        // history the room is about to be published from.
                        if loaded.replied(&joined.timeline.events) {
                            loaded.publish_thread(&on_thread);
                        }
                        // Dropped while a window somebody jumped into is
                        // being drawn. What was just said does not belong
                        // after a message from last March, and appending it
                        // there would draw the two as one conversation. It is
                        // read afresh when they come back to the present.
                        let arrived = match loaded.focus {
                            Some(_) => Vec::new(),
                            None => loaded.read(&joined.timeline.events),
                        };
                        let counted = loaded.count_replies(&joined.timeline.events);
                        let added = loaded.history.arrived(arrived);
                        // After the history and not before it. Somebody who
                        // deletes what they just said sends the message and
                        // the redaction inside one sync, and a sweep that ran
                        // first would find nothing to mark and then watch the
                        // line below draw the words it was sent to remove.
                        let annotated = loaded.annotations(&joined.timeline.events);
                        if added {
                            // A message can arrive answering something older
                            // than what is loaded, and the row above it has
                            // the same nothing to draw as any other reply.
                            loaded.resolve(&room).await;
                        }
                        if added | counted | annotated {
                            loaded.publish(&on_change);
                        }
                        // A reaction in the room may be on the thread's root or
                        // on one of its replies, both of which the panel draws.
                        if annotated && loaded.open.is_some() {
                            loaded.publish_thread(&on_thread);
                        }
                        if let Some(typing) = loaded.typing(&joined.ephemeral) {
                            on_typing(typing);
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
                // `Pin::as_mut` on an `Option` is not a thing, so the arm is
                // guarded instead: with no stream there is nothing to poll and
                // the other two arms carry on.
                keys = async { rekeyed.as_mut().expect("guarded").next().await },
                    if rekeyed.is_some() =>
                {
                    match keys {
                        // The room is checked here rather than in the loop
                        // body because a key for somewhere else is the
                        // ordinary case: every room on the account shares this
                        // one stream.
                        Some(Ok(keys)) => {
                            if keys.iter().any(|key| key.room_id == parsed)
                                && loaded.reread(&room).await
                            {
                                loaded.publish(&on_change);
                            }
                        }
                        // Too many keys at once, which a session catching up
                        // after a long absence produces. Nothing is retried
                        // for the batch that was dropped, so a message may
                        // stay waiting until the room is reopened, which is
                        // where this started.
                        Some(Err(missed)) => {
                            tracing::debug!(%missed, room_id = %watching, "fell behind the arriving keys");
                        }
                        None => rekeyed = None,
                    }
                }
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
    /// Who is signed in, so a reply this session sent counts as one this
    /// session took part in. `None` only for a client with no session, which
    /// is not one that reaches here.
    me: Option<String>,
    /// The thread somebody has open, if any.
    open: Option<OpenThread>,
    history: History,
    /// What people have reacted with, for every message annotated in anything
    /// this watcher has seen.
    ///
    /// Beside the history rather than inside it, because an annotation arrives
    /// for a message that may not be loaded, may be loaded later by a page, or
    /// may never be. Merged onto the messages when a timeline is published.
    reactions: Reactions,
    /// The corrections people have made, for every message replaced in
    /// anything this watcher has seen.
    ///
    /// Beside the history for the reason the reactions are, and for one more:
    /// an edit is a different event from the message it corrects, so writing
    /// the new text into the history would be undone the moment a room key
    /// re-read the original. Merged onto the messages when a timeline is
    /// published.
    edits: Edits,
    /// The events this session could not read, by event ID.
    ///
    /// Held as the JSON they arrived as, which is what `decrypt_event` takes,
    /// and dropped as each one opens. An encrypted room that has been quiet
    /// holds nothing here at all; one this session arrived late to holds a
    /// screenful, which is the case this exists for.
    waiting: HashMap<String, Raw<AnySyncTimelineEvent>>,
    /// The message each reply names, for the ones not in the history.
    ///
    /// Beside the history rather than inside it, on the same terms as the
    /// reactions: a reply names a message that may not be loaded, may be
    /// loaded later by a page, or may never be. Merged onto the timeline when
    /// one is published.
    ///
    /// `None` is a lookup that came back with nothing to draw, kept so that a
    /// redacted message is asked about once rather than on every publish for
    /// as long as the reply to it is on screen.
    answered: HashMap<String, Option<Message>>,
    /// Where the next backwards page starts, or `None` before the first one.
    from: Option<String>,
    /// Where the next forwards page starts.
    ///
    /// `None` at the live end, which is where the room normally is and where
    /// there is nothing after the present to ask for.
    forward: Option<String>,
    /// The message the loaded window was opened around, when it is not the
    /// present.
    focus: Option<String>,
    /// Whether the homeserver still has older messages.
    more_before: bool,
    /// Whether the homeserver has messages after the loaded window.
    more_after: bool,
    /// Whether a page of older messages is being fetched right now.
    ///
    /// Also what a jump raises. A window around a message from last March is
    /// earlier messages by any reading, and it is drawn where they would be.
    loading: bool,
    /// Whether a page of newer messages is being fetched right now.
    loading_after: bool,
    /// Who was last reported as typing, so an unchanged list is not
    /// republished on every sync for as long as somebody keeps typing.
    typing: Vec<String>,
    /// Where this account had stopped reading when the room was opened.
    ///
    /// Read once and then left alone. See [`Timeline::read_up_to`] for why it
    /// must not follow the marker it came from.
    read_up_to: Option<String>,
    /// The message this watcher last sent a receipt for.
    ///
    /// The whole of the throttling. A reader sitting at the bottom of a room
    /// asks for the same message to be marked read on every scroll and every
    /// arriving sync, and without this each of those would be a request.
    marked: Option<String>,
}

/// One thread being watched alongside the room.
///
/// Its replies are not in the room's timeline, so this holds its own history
/// rather than filtering the room's. What it shares with the room is the
/// arriving sync: the events are already in hand, so keeping a thread current
/// costs a second read of a batch rather than a second subscription.
struct OpenThread {
    root_id: String,
    root: Option<Message>,
    history: History,
    more_before: bool,
}

impl Loaded {
    fn new(room_id: String, me: Option<String>) -> Self {
        Self {
            room_id,
            me,
            open: None,
            history: History::new(),
            reactions: Reactions::new(),
            edits: Edits::new(),
            waiting: HashMap::new(),
            answered: HashMap::new(),
            from: None,
            forward: None,
            focus: None,
            // Assumed until the homeserver says otherwise, because the first
            // page has not been asked for yet and "no more history" is a
            // stronger claim than an empty list supports.
            more_before: true,
            more_after: false,
            loading: true,
            loading_after: false,
            typing: Vec::new(),
            read_up_to: None,
            marked: None,
        }
    }

    /// Answer one ask for a page at either end, and report the result.
    ///
    /// Reports twice, once to put the spinner up and once to take it down.
    /// Both are cheap, and the first is the only thing that makes a slow
    /// homeserver distinguishable from a button that did nothing.
    async fn page<F>(&mut self, room: &Room, on_change: &F, towards: Direction)
    where
        F: Fn(Timeline),
    {
        if !self.has_more(towards) {
            return;
        }

        self.loading(towards, true);
        self.publish(on_change);

        for _ in 0..PAGES_PER_ASK {
            if !self.fetch(room, towards).await {
                break;
            }
        }

        self.resolve(room).await;
        self.loading(towards, false);
        self.publish(on_change);
    }

    /// Whether the homeserver has anything left in that direction.
    fn has_more(&self, towards: Direction) -> bool {
        match towards {
            Direction::Backward => self.more_before,
            Direction::Forward => self.more_after,
        }
    }

    /// Say that a page is on its way, or has stopped being.
    ///
    /// Which end is part of it, because the interface draws the notice at that
    /// end of the list. One flag for both would put "loading earlier messages"
    /// at the top of a box whose reader is at the bottom waiting for the
    /// opposite page.
    fn loading(&mut self, towards: Direction, loading: bool) {
        match towards {
            Direction::Backward => self.loading = loading,
            Direction::Forward => self.loading_after = loading,
        }
    }

    /// Fetch one page at either end.
    ///
    /// `true` only when the page held nothing to draw and the homeserver has
    /// more, which is the one reason to go round again. A failure answers
    /// `false`: the room is still drawn and still live, the scroll can be
    /// tried again, and three requests in a row to a homeserver that just
    /// refused one is not a way to be told anything new.
    async fn fetch(&mut self, room: &Room, towards: Direction) -> bool {
        let mut options = MessagesOptions::new(towards);
        options.from = match towards {
            Direction::Backward => self.from.clone(),
            Direction::Forward => self.forward.clone(),
        };
        options.limit = PAGE.into();

        let page = match room.messages(options).await {
            Ok(page) => page,
            Err(error) => {
                // Logged rather than raised. A dialog about a page of history
                // would be worse than the absence of it.
                tracing::warn!(%error, room_id = %self.room_id, "could not read a page of messages");
                return false;
            }
        };

        // An empty chunk is the end of what the homeserver will give, and so
        // is a missing `end`. Both are checked because homeservers differ
        // about which one they say it with.
        let more = page.end.is_some() && !page.chunk.is_empty();
        let chunk: Vec<TimelineEvent> = match towards {
            // Backwards, so the homeserver answers newest first and the page
            // has to be turned round. Getting this wrong reverses every page
            // while leaving the pages themselves in order, which reads as a
            // conversation that almost makes sense.
            Direction::Backward => {
                self.more_before = more;
                self.from = page.end;
                page.chunk.into_iter().rev().collect()
            }
            Direction::Forward => {
                self.more_after = more;
                self.forward = page.end;
                page.chunk
            }
        };

        let arrived = self.read(&chunk);
        let drawable = !arrived.is_empty();
        match towards {
            Direction::Backward => self.history.backfilled(arrived),
            Direction::Forward => self.history.arrived(arrived),
        };
        // A page carries every kind of event, reactions among them, which is
        // how a message scrolled back to arrives with what is already on it.
        // The thread panel has no equivalent: `/relations` is asked for thread
        // replies only, so a reply's reactions appear when one arrives live
        // rather than when the panel opens.
        //
        // After the history, for the reason the sync has it in that order: a
        // page regularly holds a message and the redaction that emptied it.
        self.annotations(&chunk);

        !drawable && self.has_more(towards)
    }

    /// Draw the history around `event_id` instead of the present.
    ///
    /// The window replaces what is loaded rather than joining it, for the
    /// reason [`around`] gives: two pieces of a room that are not next to each
    /// other, drawn as though they were, is a year passing with nothing to say
    /// so.
    ///
    /// A window that will not load leaves the room where it was. There is
    /// nothing better to do with a message the homeserver will not hand over,
    /// and emptying the room to say so would take away the conversation
    /// somebody was reading as well as the one they asked for.
    async fn go_to<F>(&mut self, room: &Room, on_change: &F, event_id: String)
    where
        F: Fn(Timeline),
    {
        self.loading = true;
        self.publish(on_change);

        match around::around(room, &event_id).await {
            Ok(window) => {
                self.history = History::new();
                self.history.backfilled(window.messages);
                self.more_before = window.back.is_some();
                self.more_after = window.forward.is_some();
                self.from = window.back;
                self.forward = window.forward;
                self.focus = Some(event_id);
                self.resolve(room).await;
            }
            Err(error) => {
                tracing::warn!(%error, room_id = %self.room_id, %event_id, "could not go to the message");
            }
        }

        self.loading = false;
        self.publish(on_change);
    }

    /// Go back to the live end of the room.
    ///
    /// The first page again rather than whatever was loaded before the jump.
    /// A sync arriving while somebody reads last March is deliberately not
    /// appended to the window they are reading, so what is held is out of date
    /// by however long they were away, and reading it fresh is both the
    /// correction and how the messages they missed arrive.
    async fn present<F>(&mut self, room: &Room, on_change: &F)
    where
        F: Fn(Timeline),
    {
        if self.focus.is_none() {
            return;
        }

        self.history = History::new();
        self.focus = None;
        self.from = None;
        self.forward = None;
        self.more_before = true;
        self.more_after = false;
        self.page(room, on_change, Direction::Backward).await;
    }

    /// Look up whatever the loaded replies name and this does not hold.
    ///
    /// One request each at worst, and for a message the SDK has already stored
    /// none at all. Bounded by what is loaded rather than by the room: a reply
    /// is only looked up while it is on screen, and the answer is kept so
    /// scrolling past it twice is not asking twice.
    async fn resolve(&mut self, room: &Room) {
        let held: HashSet<&str> = self
            .history
            .messages()
            .iter()
            .map(|message| message.id.as_str())
            .collect();
        let wanted: HashSet<String> = self
            .history
            .messages()
            .iter()
            .filter_map(|message| message.reply_to.as_deref())
            .filter(|id| !held.contains(id) && !self.answered.contains_key(*id))
            .map(ToOwned::to_owned)
            .collect();

        for id in wanted {
            // Boxed here rather than at the three call sites, which is where
            // the compiler would otherwise report it: an event fetch is a deep
            // future, this is awaited from inside a `select!` inside a spawn,
            // and holding it inline overflows the layout depth limit.
            let found = Box::pin(answering::answered(room, &id)).await;
            self.answered.insert(id, found);
        }
    }

    /// One batch of events as messages, remembering the ones with no key.
    ///
    /// The remembering is the whole reason this is not a `filter_map` at the
    /// two call sites. An event that arrives unreadable is drawn as a wait,
    /// and the wait can only be redeemed by something holding the ciphertext
    /// until the key turns up.
    fn read(&mut self, events: &[TimelineEvent]) -> Vec<Message> {
        events
            .iter()
            .filter_map(|event| {
                let message = facts::message(event)?;
                if event.kind.is_utd() {
                    self.waiting.insert(message.id.clone(), event.raw().clone());
                }
                Some(message)
            })
            .collect()
    }

    /// Who this batch says is typing, when it says anything about it.
    ///
    /// `None` when the batch carried no `m.typing` at all, which is almost
    /// every sync, and when it says the same thing as the last one. A room
    /// where somebody is typing sends one of these on every sync until they
    /// stop, and republishing an unchanged list would wake the webview several
    /// times a minute to hand it what it has.
    fn typing(&mut self, ephemeral: &[Raw<AnySyncEphemeralRoomEvent>]) -> Option<Typing> {
        let said = ephemeral.iter().find_map(facts::typing)?;

        // Ours taken out here rather than in the interface, because it is the
        // same answer for every reader and there is exactly one of us.
        let users: Vec<String> = said
            .into_iter()
            .filter(|user| Some(user) != self.me.as_ref())
            .collect();

        if users == self.typing {
            return None;
        }
        self.typing.clone_from(&users);
        Some(Typing {
            room_id: self.room_id.clone(),
            users,
        })
    }

    /// Take note of everything in one batch that is not a message.
    ///
    /// The reactions, the corrections and the redactions. Reports whether
    /// anything drawn changed. Separate from [`Self::read`] because none of
    /// these is a message and none of them ever becomes one: a reaction and an
    /// edit are both something *about* a message, and a redaction can remove
    /// any of the three.
    fn annotations(&mut self, events: &[TimelineEvent]) -> bool {
        let mut changed = false;
        for event in events {
            if let Some(one) = facts::annotation(event) {
                changed |= self
                    .reactions
                    .added(&one.event_id, &one.target, &one.key, &one.sender);
                continue;
            }
            if let Some(one) = facts::replacement(event) {
                // Taken whoever sent it. Whether they wrote the message being
                // replaced is a comparison against the original, which is
                // regularly not loaded here, and `Edits::latest_on` is where
                // it can be made.
                changed |= self.edits.added(
                    &one.event_id,
                    &one.target,
                    &one.sender,
                    one.at,
                    one.body,
                    one.html,
                );
                continue;
            }
            if let Some(gone) = facts::redaction(event) {
                // Whichever it was. A redacted annotation is somebody taking a
                // reaction back and a redacted edit is somebody taking a
                // correction back, both of which leave nothing behind. A
                // redacted message is emptied where it stands instead of being
                // dropped, so that the reply underneath is not left answering
                // a gap.
                let by = Some(gone.sender.as_str());
                changed |= self.reactions.redacted(&gone.event_id);
                changed |= self.edits.redacted(&gone.event_id);
                changed |= self.history.redacted(&gone.event_id, by);
                // The panel draws out of its own history and its own root,
                // neither of which is the room's. Without this, deleting a
                // message while its thread is open empties it in the room and
                // leaves the words in the panel, on one screen at once.
                if let Some(open) = self.open.as_mut() {
                    changed |= open.history.redacted(&gone.event_id, by);
                    if let Some(root) = open.root.as_mut().filter(|root| root.id == gone.event_id) {
                        changed |= history::redact(root, by);
                    }
                }
            }
        }
        changed
    }

    /// Try every message this session had no key for again.
    ///
    /// Answered on the watcher's own task, in order with the pages and the
    /// syncs, so a retry cannot interleave with a backfill writing into the
    /// same history.
    ///
    /// Reports whether anything on screen changed. A key usually opens nothing
    /// here: it is one stream for the whole account, and most keys are for
    /// rooms nobody is looking at.
    async fn reread(&mut self, room: &Room) -> bool {
        let held: Vec<(String, Raw<AnySyncTimelineEvent>)> = self
            .waiting
            .iter()
            .map(|(id, raw)| (id.clone(), raw.clone()))
            .collect();

        let mut changed = false;
        for (id, raw) in held {
            // Cast unchecked because it is the same JSON either way: this is
            // the raw event as the homeserver sent it, and it reached here
            // only by having been an `m.room.encrypted` nothing could open.
            let Ok(event) = room.decrypt_event(raw.cast_ref_unchecked(), None).await else {
                continue;
            };
            if event.kind.is_utd() {
                // This key was for a different session. Kept, because the one
                // that opens it may still arrive.
                continue;
            }

            self.waiting.remove(&id);
            changed |= match facts::message(&event) {
                Some(message) => self.history.replace(message),
                // It opened, and it is a reaction or a thread reply, which are
                // not drawn. The wait has to go: a placeholder for something
                // that was never a message would sit there forever.
                None => self.history.forget(&id),
            };
        }

        changed
    }

    /// Open the thread hanging from `root_id`, or close whatever is open.
    ///
    /// A thread that will not load closes rather than half-opening. The
    /// alternative is a panel drawn from a root with no replies under it,
    /// which reads as a thread somebody deleted rather than as a request that
    /// failed.
    async fn open(&mut self, client: &Client, root_id: Option<String>) {
        let Some(root_id) = root_id else {
            self.open = None;
            return;
        };

        match thread::thread(client, &self.room_id, &root_id).await {
            Ok(loaded) => {
                let mut history = History::new();
                history.backfilled(loaded.messages);
                self.open = Some(OpenThread {
                    root_id,
                    root: loaded.root,
                    history,
                    more_before: loaded.more_before,
                });
            }
            Err(error) => {
                // Logged rather than raised, on the same terms as a page of
                // history that would not come back.
                tracing::warn!(%error, room_id = %self.room_id, %root_id, "could not read the thread");
                self.open = None;
            }
        }
    }

    /// Say that everything up to and including `event_id` has been read.
    ///
    /// Dropped when it names the message this watcher last sent, which is what
    /// keeps a reader sitting still in a quiet room from sending a receipt per
    /// scroll event. Recorded before the request rather than after it, so a
    /// homeserver that is slow to answer does not collect a queue of asks for
    /// the same message behind it.
    ///
    /// A failure is logged rather than raised. There is nothing to say to
    /// somebody about a receipt that did not go out: the room is still drawn,
    /// the count settles on the next one, and a dialog about it would be a
    /// dialog about bookkeeping.
    async fn mark_read(&mut self, room: &Room, event_id: String, public: bool) {
        if !self.worth_sending(&event_id) {
            return;
        }

        if let Err(error) = crate::receipts::mark_read(room, &event_id, public).await {
            tracing::warn!(%error, room_id = %self.room_id, %event_id, "could not send a read receipt");
        }
    }

    /// Whether a receipt for `event_id` is worth a request, recording it if so.
    ///
    /// Recording and answering in one step on purpose. Two callers cannot race
    /// here, because every ask is answered on the watcher's own task in the
    /// order it arrived, and a check that did not record would let the second
    /// of two identical asks through while the first was still in flight.
    fn worth_sending(&mut self, event_id: &str) -> bool {
        if self.marked.as_deref() == Some(event_id) {
            return false;
        }
        self.marked = Some(event_id.to_owned());
        true
    }

    /// Add whichever of `events` are replies in the open thread.
    ///
    /// Reports whether the panel changed.
    fn replied(&mut self, events: &[TimelineEvent]) -> bool {
        let Some(open) = &mut self.open else {
            return false;
        };

        let arrived: Vec<Message> = events
            .iter()
            .filter(|event| facts::thread_root(event).as_deref() == Some(open.root_id.as_str()))
            .filter_map(facts::in_thread)
            .collect();

        open.history.arrived(arrived)
    }

    /// Count whichever of `events` are thread replies against the messages in
    /// this room they hang from.
    ///
    /// The tally on a message is the homeserver's, and it is only recounted
    /// when the message is read again. Without this a thread somebody has just
    /// replied in shows nothing until the room is reopened, which includes
    /// replying from here.
    ///
    /// Reports whether the room changed.
    fn count_replies(&mut self, events: &[TimelineEvent]) -> bool {
        let mut changed = false;
        for event in events {
            let Some(root_id) = facts::thread_root(event) else {
                continue;
            };
            let Some(existing) = self
                .history
                .messages()
                .iter()
                .find(|message| message.id == root_id)
            else {
                // A reply to something older than what is loaded. The tally
                // arrives with the message when it is scrolled back to.
                continue;
            };

            let mine =
                facts::message(event).is_some_and(|reply| Some(&reply.sender) == self.me.as_ref());
            let counted = match existing.thread {
                Some(summary) => ThreadSummary {
                    count: summary.count.saturating_add(1),
                    participated: summary.participated || mine,
                },
                None => ThreadSummary {
                    count: 1,
                    participated: mine,
                },
            };

            let mut updated = existing.clone();
            updated.thread = Some(counted);
            changed |= self.history.replace(updated);
        }
        changed
    }

    /// The messages, each carrying what people have reacted to it with and
    /// whatever its author has since corrected it to say.
    ///
    /// Merged here rather than held on the message, because these change for
    /// different reasons than the message does: a message is replaced when a
    /// room key opens it, what is on it changes when somebody presses a pill,
    /// and what it says changes when its author corrects it. Keeping any of
    /// them on the message would mean every re-read had to carry them forward
    /// by hand, and the one that forgot would silently drop them.
    fn drawn(&self, messages: &[Message]) -> Vec<Message> {
        let me = self.me.as_deref();
        messages
            .iter()
            .map(|message| {
                // Nothing folds onto a mark. A pill on a message that has
                // been emptied counts agreement with nothing, and
                // [`Self::corrected`] refuses the edits for a stronger reason
                // than that.
                if message.kind == MessageKind::Deleted {
                    return message.clone();
                }
                let reactions = self.reactions.on(&message.id, me);
                if reactions.is_empty() {
                    return self.corrected(message);
                }
                Message {
                    reactions,
                    ..self.corrected(message)
                }
            })
            .collect()
    }

    /// One message as its author has since corrected it, if they have.
    ///
    /// The sender check is [`Edits::latest_on`]'s, made here rather than when
    /// the edit arrived because here is the first place the original is in
    /// hand to compare against.
    fn corrected(&self, message: &Message) -> Message {
        // An edit outlives the message it corrects: a redaction names one
        // event, and the corrections held against it are not that event.
        // Folding one onto a mark would put back the sentence the redaction
        // was sent to remove, which is the whole of what deleting is for.
        //
        // Guarded here and not only in [`Self::drawn`], because the quoted row
        // above a reply is built in [`Self::answers`] without passing through
        // it.
        if message.kind == MessageKind::Deleted {
            return message.clone();
        }
        let Some(edit) = self.edits.latest_on(&message.id, &message.sender) else {
            return message.clone();
        };

        Message {
            // Both, always, out of the edit alone. Merging field by field is
            // how a message edited down to plain text keeps the formatting it
            // had: the new sentence would come from `body` and the old one
            // from `html`, and `FormattedBody` draws the second.
            body: edit.body.clone(),
            html: edit.html.clone(),
            edited: true,
            // `at` is deliberately untouched, and it is the line a later
            // reader will be tempted to fix. A message keeps the moment it was
            // said: the date separators are keyed off it, so a message from
            // last Tuesday corrected this morning would otherwise jump to
            // today and take its separator with it.
            ..message.clone()
        }
    }

    /// The messages these replies name that are not among them.
    ///
    /// Read out of what [`Self::resolve`] found rather than looked up here,
    /// because publishing happens on every reaction and every key and is not
    /// somewhere a request belongs. A reply whose lookup has not happened yet,
    /// or came back with nothing, contributes nothing and the row says so.
    ///
    /// Reactions are deliberately not merged onto these. The row above a reply
    /// draws a name and a line of what was said; pills belong on the message
    /// itself, wherever it is drawn.
    ///
    /// Edits are, and the two are different for a reason rather than by
    /// oversight. A pill missing from a quoted row is something not drawn; the
    /// pre-edit text in a quoted row is a wrong sentence attributed to
    /// somebody by name, which is the whole defect this fold exists to fix,
    /// one layer down.
    fn answers(&self, messages: &[Message]) -> Vec<Message> {
        let held: HashSet<&str> = messages.iter().map(|message| message.id.as_str()).collect();
        let mut answers: Vec<Message> = Vec::new();
        let mut taken: HashSet<&str> = HashSet::new();
        for message in messages {
            let Some(id) = message.reply_to.as_deref() else {
                continue;
            };
            if held.contains(id) || !taken.insert(id) {
                continue;
            }
            if let Some(Some(answered)) = self.answered.get(id) {
                answers.push(self.corrected(answered));
            }
        }
        answers
    }

    fn publish_thread<G>(&self, on_thread: &G)
    where
        G: Fn(Option<Thread>),
    {
        on_thread(self.open.as_ref().map(|open| {
            Thread {
                room_id: self.room_id.clone(),
                root_id: open.root_id.clone(),
                root: open
                    .root
                    .as_ref()
                    .map(|root| self.drawn(std::slice::from_ref(root)).remove(0)),
                messages: self.drawn(open.history.messages()),
                more_before: open.more_before,
            }
        }));
    }

    fn publish<F>(&self, on_change: &F)
    where
        F: Fn(Timeline),
    {
        let messages = self.drawn(self.history.messages());
        on_change(Timeline {
            room_id: self.room_id.clone(),
            answered: self.answers(&messages),
            messages,
            more_before: self.more_before,
            more_after: self.more_after,
            focus: self.focus.clone(),
            loading: self.loading,
            loading_after: self.loading_after,
            read_up_to: self.read_up_to.clone(),
        });
    }
}

/// Say whether this session is typing in a room.
///
/// Safe to call on every keystroke. The SDK holds the time of the last notice
/// per room and sends nothing while one is still current, so the throttling
/// that this would otherwise need is already done a layer down.
pub async fn typing(client: &Client, room_id: &str, typing: bool) -> Result<()> {
    room_of(client, room_id)?.typing_notice(typing).await?;
    Ok(())
}

/// React to a message.
///
/// Nothing is returned and nothing is echoed, on the same terms as sending a
/// message: the reaction appears when the sync brings it back. Reacting twice
/// with one key is not guarded against here, because the interface knows
/// whether this session has already used that key and the specification says
/// a duplicate is ignored anyway.
pub async fn react(client: &Client, room_id: &str, event_id: &str, key: &str) -> Result<()> {
    let room = room_of(client, room_id)?;
    let target = event_id_of(event_id)?;
    room.send(ReactionEventContent::new(Annotation::new(
        target,
        key.to_owned(),
    )))
    .await?;
    Ok(())
}

/// Take a reaction back.
///
/// `reaction_id` is the annotation's own event, not the message it is on: a
/// reaction is undone by redacting it, and the two would be indistinguishable
/// here if the wrong one were passed. `Reaction::mine` is where the interface
/// gets it.
pub async fn unreact(client: &Client, room_id: &str, reaction_id: &str) -> Result<()> {
    let room = room_of(client, room_id)?;
    // `redact` answers with the SDK's HTTP error rather than its own, which is
    // the only call in this module that does. Lifted rather than given a
    // variant of its own: a redaction that failed is an SDK call that failed,
    // and `user_message` already has words for that.
    room.redact(&event_id_of(reaction_id)?, None, None)
        .await
        .map_err(matrix_sdk::Error::from)?;
    Ok(())
}

/// Delete a message.
///
/// A redaction, which is what deleting is in Matrix and is worth being exact
/// about rather than promising more than happens. The homeserver empties the
/// event and serves the emptied version from then on; the event itself
/// survives, keeping its sender and its timestamp, which is what the mark left
/// in the room is drawn from. What federation has already handed to other
/// servers is not recalled by any of this.
///
/// No reason is sent. `Room::redact` takes an optional one and there is
/// nowhere in the interface that asks for it, so `None` is the honest
/// argument: a reason invented here would be a sentence nobody wrote, filed
/// against somebody's account.
///
/// Whose message it is stays the homeserver's to enforce. The interface offers
/// the control on this account's own messages only, which is what keeps
/// somebody from being handed a control that cannot work, and a redaction of
/// anybody else's comes back as an error from the one place the power levels
/// actually live.
///
/// Nothing is returned and nothing is echoed, on the same terms as every other
/// send here: the message empties when the sync brings the redaction back.
pub async fn delete(client: &Client, room_id: &str, event_id: &str) -> Result<()> {
    // Before the room, so that something which is not an event ID is answered
    // as that rather than as whatever the room lookup happens to say first.
    let target = event_id_of(event_id)?;
    // Lifted the way `unreact` lifts it, and for the reason written there:
    // these two are the calls in this module that answer with the SDK's HTTP
    // error rather than with one of ours.
    room_of(client, room_id)?
        .redact(&target, None, None)
        .await
        .map_err(matrix_sdk::Error::from)?;
    Ok(())
}

/// Say something in a room.
///
/// Read as markdown, which is what every client somebody is arriving from
/// does. The text is sent as the plaintext fallback either way; formatting is
/// added beside it only when there was some, so a sentence with a stray
/// asterisk in it goes out as the sentence.
///
/// Encrypted or not according to the room, because the SDK decides that from
/// the room's own state rather than from anything a caller passes.
///
/// Nothing is returned and nothing is echoed. The message appears when the
/// sync brings it back, which is the same path every other message in the room
/// takes. See the module header for why there is no local echo.
pub async fn send(client: &Client, room_id: &str, body: &str) -> Result<()> {
    let content = written(body)?;
    room_of(client, room_id)?.send(content).await?;
    Ok(())
}

/// Answer one message in the room.
///
/// A reply rather than a thread: it lands in the conversation everybody is
/// reading, and `facts::answering` is what draws the row above it saying what
/// it answers. Nothing here writes the quoted fallback the specification used
/// to ask for. It was removed from the specification because every client that
/// draws replies has to strip it again, and this one strips what arrives.
///
/// `sender` is who wrote the message being answered, and it is used for one
/// thing: the `m.mentions` a reply carries, so that the person answered is
/// notified rather than having to notice. It comes from the message the
/// interface is already drawing, on the same terms as `latest_id` below.
pub async fn send_reply(
    client: &Client,
    room_id: &str,
    reply_to: &str,
    sender: &str,
    body: &str,
) -> Result<()> {
    let answered = event_id_of(reply_to)?;
    let author = UserId::parse(sender).map_err(|_| Error::NoSuchUser {
        user_id: sender.to_owned(),
    })?;
    let content = written(body)?.make_reply_to(
        ReplyMetadata::new(&answered, &author, None),
        // The room, never the thread the answered message might be in. A
        // message in a thread is not drawn in the room at all, so nothing here
        // can be pressed to reply to one.
        ForwardThread::No,
        AddMentions::Yes,
    );

    room_of(client, room_id)?.send(content).await?;
    Ok(())
}

/// Correct a message this account sent.
///
/// A wrapper rather than an implementation. `Room::make_edit_event` builds the
/// whole event: it refuses an edit of somebody else's message, carries the
/// original's `m.mentions` forward so that correcting a typo does not silently
/// unmention whoever was named, and writes both `m.new_content` and the `* `
/// fallback body a client with no idea about edits draws.
///
/// It also reads the target event first, which is a round trip. An edit of a
/// message old enough to have fallen out of the event cache can therefore fail
/// on the fetch, before anything has been sent.
///
/// Nothing is returned and nothing is echoed, on the same terms as every other
/// send here: the correction appears when the sync brings it back.
pub async fn send_edit(client: &Client, room_id: &str, event_id: &str, body: &str) -> Result<()> {
    // Before the fetch, so an empty box costs no round trip. Emptying the
    // composer is also not how a message is deleted, and sending this would
    // leave a blank line where a sentence was.
    let content = written(body)?;
    let target = event_id_of(event_id)?;
    let room = room_of(client, room_id)?;

    // Boxed, and it is load-bearing rather than tidy. `make_edit_event` reads
    // the target event through the SDK's event cache first, and inlining that
    // future makes this one deep enough that rustc gives up computing the
    // layout of anything holding it. Under `-C instrument-coverage` it gives
    // up sooner, so the failure shows up in CI's coverage job rather than in
    // an ordinary build. One box here beats one at every call site.
    let edit = Box::pin(room.make_edit_event(&target, EditedContent::RoomMessage(content.into())))
        .await
        .map_err(|error| match error {
            // Its own variant, because it is the one failure here with
            // something to say to a person. Everything else is a homeserver
            // that would not answer, which `Error::Sdk` already has words for.
            EditError::NotAuthor => Error::NotYourMessage {
                event_id: event_id.to_owned(),
            },
            other => {
                tracing::warn!(%other, %room_id, %event_id, "could not build the edit");
                Error::NoSuchEvent {
                    event_id: event_id.to_owned(),
                }
            }
        })?;

    room.send(edit).await?;
    Ok(())
}

/// Say something in a thread, answering one reply in it or none.
///
/// `in_reply_to` is what the `m.in_reply_to` points at, and `answering` is who
/// wrote it. The two cases differ only in that pair and in one boolean, and
/// the boolean is the whole of what tells them apart on the way back in.
///
/// With no `answering`, this is just another reply and `in_reply_to` is the
/// last thing said in the thread as far as the caller knows. It is falling
/// back: a client that understands threads reads `event_id` and puts the
/// message in the right conversation, one that does not sees an ordinary reply
/// pointing at whatever was being answered, and `facts::answering` ignores it
/// so that a panel does not draw a quoted row on every line in it. Stale is
/// harmless, because nothing about which thread this belongs to depends on it.
///
/// With one, this answers that message and says so. `in_reply_to` is the
/// message being answered rather than the newest, the fallback flag comes off,
/// and the author is mentioned, on the same terms and for the same reason as a
/// reply in the room: an answer nobody is notified of is a line in a panel
/// that is not open.
pub async fn send_in_thread(
    client: &Client,
    room_id: &str,
    root_id: &str,
    in_reply_to: &str,
    answering: Option<&str>,
    body: &str,
) -> Result<()> {
    let mut content = written(body)?;
    let root = event_id_of(root_id)?;
    let answered = event_id_of(in_reply_to)?;

    let content = match answering {
        None => {
            content.relates_to = Some(Relation::Thread(ThreadRelation::plain(root, answered)));
            content
        }
        Some(sender) => {
            let author = UserId::parse(sender).map_err(|_| Error::NoSuchUser {
                user_id: sender.to_owned(),
            })?;
            // Which thread, handed in rather than read off the message being
            // answered. `make_for_thread` takes the root from this and would
            // otherwise start a new thread rooted at the answered message,
            // which is a second conversation where somebody meant a sentence.
            let thread = ThreadRelation::without_fallback(root);
            content.make_for_thread(
                ReplyMetadata::new(&answered, &author, Some(&thread)),
                ReplyWithinThread::Yes,
                AddMentions::Yes,
            )
        }
    };

    room_of(client, room_id)?.send(content).await?;
    Ok(())
}

/// What was typed, as something to send.
fn written(body: &str) -> Result<RoomMessageEventContent> {
    // Trimmed before it is judged empty, so that a stray newline from a text
    // area is not a message. Sent untrimmed is not an option either: leading
    // spaces in a pasted code block are the message.
    if body.trim().is_empty() {
        return Err(Error::EmptyMessage);
    }
    Ok(RoomMessageEventContent::text_markdown(body))
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

fn event_id_of(event_id: &str) -> Result<OwnedEventId> {
    EventId::parse(event_id).map_err(|_| Error::NoSuchEvent {
        event_id: event_id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::deserialized_responses::{UnableToDecryptInfo, UnableToDecryptReason};
    use serde_json::json;

    /// An ordinary message, as a homeserver sends it.
    fn readable(id: &str) -> TimelineEvent {
        TimelineEvent::from_plaintext(
            Raw::new(&json!({
                "type": "m.room.message",
                "event_id": id,
                "sender": "@ada:example.org",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": { "msgtype": "m.text", "body": "hello" },
            }))
            .expect("the fixture is valid JSON")
            .cast_unchecked(),
        )
    }

    /// An encrypted message this session has no key for.
    fn sealed(id: &str) -> TimelineEvent {
        TimelineEvent::from_utd(
            Raw::new(&json!({
                "type": "m.room.encrypted",
                "event_id": id,
                "sender": "@bob:example.org",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "ciphertext": "AwgAEnB...",
                    "session_id": "session",
                },
            }))
            .expect("the fixture is valid JSON")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("session".to_owned()),
                reason: UnableToDecryptReason::MissingMegolmSession {
                    withheld_code: None,
                },
            },
        )
    }

    #[test]
    fn an_unreadable_event_is_kept_so_a_key_has_something_to_open() {
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);

        loaded.read(&[sealed("$sealed:example.org")]);

        assert!(loaded.waiting.contains_key("$sealed:example.org"));
    }

    #[test]
    fn a_readable_event_is_not_held_on_to() {
        // The ciphertext is the only reason to keep one, and a message that
        // arrived readable has none. A room that has been open all day should
        // not be holding a copy of everything said in it.
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);

        loaded.read(&[readable("$said:example.org")]);

        assert!(loaded.waiting.is_empty());
    }

    #[test]
    fn the_same_message_is_only_marked_read_once() {
        // A reader sitting still at the bottom of a room asks for this on
        // every scroll and every arriving sync. Without the guard each of
        // those is a request to the homeserver saying what the last one said.
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);

        assert!(loaded.worth_sending("$said:example.org"));
        assert!(!loaded.worth_sending("$said:example.org"));
    }

    #[test]
    fn a_newer_message_is_marked_read_after_an_older_one() {
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);
        loaded.worth_sending("$first:example.org");

        assert!(loaded.worth_sending("$second:example.org"));
    }

    #[test]
    fn a_room_nobody_has_read_draws_no_line() {
        // The absence is the answer rather than a position to guess at. A
        // marker invented for a room with none would put "new messages" above
        // the whole of a conversation somebody is opening for the first time.
        let loaded = Loaded::new("!room:example.org".to_owned(), None);

        assert_eq!(loaded.read_up_to, None);
    }

    #[test]
    fn reading_a_batch_still_answers_with_every_message_in_it() {
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);

        let messages = loaded.read(&[readable("$one:example.org"), sealed("$two:example.org")]);

        assert_eq!(
            messages
                .iter()
                .map(|said| said.id.as_str())
                .collect::<Vec<_>>(),
            ["$one:example.org", "$two:example.org"]
        );
    }
}
