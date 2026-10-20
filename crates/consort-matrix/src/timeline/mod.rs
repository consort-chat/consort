// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading and writing one room's messages.
//!
//! All rooms, the voice ones included: a voice channel is a Matrix room whose
//! `m.room.create` carries a call type, and nothing else about it differs.
//!
//! Built on the base SDK rather than on `matrix-sdk-ui`, which costs a gap
//! across a `limited` sync, a window instead of a gap-aware store, and no
//! local echo: docs/adr/0010-a-timeline-on-the-base-sdk.md.

mod answering;
mod around;
pub mod dto;
mod edits;
pub(crate) mod facts;
mod history;
mod media;
mod permalink;
mod reactions;
mod read_by;
mod sending;
mod thread;

pub use dto::{
    Media, Message, MessageKind, Reaction, ReadOn, Readers, SystemChange, SystemMessage, Thread,
    ThreadReaders, ThreadSummary, Timeline, Typing,
};
pub use edits::Edits;
pub use history::{History, SystemHistory};
pub use media::{Attachment, MAX_BYTES, bytes, media};
pub use permalink::permalink;
pub use reactions::Reactions;
pub use read_by::{About, ReadBy, SHOWN};
pub use sending::{Answering, Attaching, send_attachment};
pub use thread::thread;

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::room::edit::{EditError, EditedContent};
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::receipt::{ReceiptThread, ReceiptType};
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

/// How many messages to ask for at a time: enough to fill a tall window on the
/// first read, few enough that a long room does not decrypt history nobody
/// scrolled to.
const PAGE: u32 = 30;

/// How many pages one ask may consume before giving up and reporting.
///
/// A page can hold nothing to draw, since a spell of membership churn is state
/// events and no words, so one page per ask would sometimes answer a scroll
/// with nothing. Bounded because each page is a request and a decryption.
const PAGES_PER_ASK: usize = 3;

/// What a watcher can be asked to do. One channel rather than two, so two asks
/// cannot be answered in the other order from the one they arrived in.
enum Ask {
    /// One more page of the room's own history.
    Earlier,
    /// One more page of what came after what is loaded. Only answerable inside
    /// a window somebody jumped into; at the live end, later is a sync away.
    Later,
    /// Draw the history around this message instead of the present.
    Around(String),
    /// Go back to the live end of the room.
    Present,
    /// Open the thread hanging from this message, or close whatever is open.
    Thread(Option<String>),
    /// Say that everything up to this message has been read, the boolean being
    /// whether the receipt is public.
    ///
    /// Carried per ask rather than held by the watcher, because somebody can
    /// revoke that choice with the room already open and a held answer would
    /// go on publishing receipts under it.
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
    /// Answered on the watcher's own task, in order with everything else, so
    /// two presses cannot interleave their pages. Silently ignored once the
    /// watcher has ended, which is a scroll landing on a room change.
    pub fn earlier(&self) {
        let _ = self.asking.send(Ask::Earlier);
    }

    /// Ask for one more page of what came after what is loaded. Answered with
    /// nothing at the live end, where what comes next arrives on its own.
    pub fn later(&self) {
        let _ = self.asking.send(Ask::Later);
    }

    /// Draw the history around `event_id` instead of the present, which is
    /// what following a reply row or an old link does. The window replaces
    /// what was loaded, and [`present`](Self::present) is the way back.
    pub fn go_to(&self, event_id: String) {
        let _ = self.asking.send(Ask::Around(event_id));
    }

    /// Go back to the live end of the room: the first page again, freshly
    /// read, which is also how what was said while they were away arrives.
    /// What had been scrolled back through is deliberately not kept.
    pub fn present(&self) {
        let _ = self.asking.send(Ask::Present);
    }

    /// Open the thread hanging from `root_id`, or close whatever is open.
    /// Answered on the watcher's own task, so a thread opened and closed
    /// quickly cannot report the two out of order.
    pub fn open_thread(&self, root_id: Option<String>) {
        let _ = self.asking.send(Ask::Thread(root_id));
    }

    /// Say that everything up to and including `event_id` has been read.
    ///
    /// Safe to call as often as a scroll does: the watcher drops an ask naming
    /// the message it last sent. Leaving a room cancels the receipt, because
    /// the ask travels on the channel a room change drops.
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
/// The whole timeline every time, so a late subscriber can be handed it as-is.
/// The first report arrives without waiting for a sync, and an empty room
/// reports as empty. Dropping the [`Watch`] ends it.
pub fn watch<F, G, H, I>(
    client: Client,
    room_id: &str,
    on_change: F,
    on_thread: G,
    on_typing: H,
    on_readers: I,
) -> Watch
where
    F: Fn(Timeline) + Send + Sync + 'static,
    G: Fn(Option<Thread>) + Send + Sync + 'static,
    H: Fn(Typing) + Send + Sync + 'static,
    I: Fn(Readers) + Send + Sync + 'static,
{
    let (asking, mut asked) = unbounded_channel();
    let room_id = room_id.to_owned();
    let watching = room_id.clone();

    let task = tokio::spawn(async move {
        // Subscribed before the first page is read, so a message sent between
        // the two is not lost between them.
        let mut updates = client.subscribe_to_all_room_updates();
        // The same reasoning, for keys: one landing while the first page
        // decrypts would leave a message waiting for a key already held.
        let mut rekeyed = client.encryption().room_keys_received_stream().await;

        let Ok(parsed) = RoomId::parse(&watching) else {
            tracing::warn!(room_id = %watching, "asked to watch something that is not a room");
            on_readers(Readers {
                room_id: watching.clone(),
                ..Readers::default()
            });
            on_change(Timeline {
                room_id: watching,
                ..Timeline::default()
            });
            on_thread(None);
            return;
        };
        let Some(room) = client.get_room(&parsed) else {
            // Left from another session between the room list and the click.
            // Empty rather than an error: a room list is already on its way.
            tracing::info!(room_id = %watching, "asked to watch a room this account is not in");
            on_readers(Readers {
                room_id: watching.clone(),
                ..Readers::default()
            });
            on_change(Timeline {
                room_id: watching,
                ..Timeline::default()
            });
            on_thread(None);
            return;
        };

        let mut loaded = Loaded::new(watching.clone(), client.user_id().map(ToString::to_string));
        // Before the first page, because reading the room moves the marker.
        loaded.read_up_to = room.fully_read_event_id().map(|id| id.to_string());
        loaded.publish(&on_change);
        loaded.publish_thread(&on_thread);
        // Said once at the start, so a reader that has just changed room is
        // not left holding the last room's answer until somebody here types.
        on_typing(Typing {
            room_id: watching.clone(),
            users: Vec::new(),
        });
        loaded.page(&room, &on_change, Direction::Backward).await;
        loaded.catch_up_on_readers(&room).await;
        loaded.publish_readers(&on_readers);

        loop {
            tokio::select! {
                asked = asked.recv() => {
                    match asked {
                        None => break,
                        Some(Ask::Earlier) => {
                            loaded.page(&room, &on_change, Direction::Backward).await;
                            loaded.catch_up_on_readers(&room).await;
                            loaded.publish_readers(&on_readers);
                        }
                        Some(Ask::Later) => {
                            loaded.page(&room, &on_change, Direction::Forward).await;
                            loaded.catch_up_on_readers(&room).await;
                            loaded.publish_readers(&on_readers);
                        }
                        // Boxed for the reason the thread arm below is.
                        Some(Ask::Around(event_id)) => {
                            Box::pin(loaded.go_to(&room, &on_change, event_id)).await;
                            loaded.catch_up_on_readers(&room).await;
                            loaded.publish_readers(&on_readers);
                        }
                        Some(Ask::Present) => {
                            Box::pin(loaded.present(&room, &on_change)).await;
                            loaded.catch_up_on_readers(&room).await;
                            loaded.publish_readers(&on_readers);
                        }
                        Some(Ask::Thread(root_id)) => {
                            // Boxed or rustc gives up on this task's layout:
                            // a `/relations` request and an event fetch, both
                            // deep, inside a `select!` inside a spawn.
                            Box::pin(loaded.open(&client, root_id)).await;
                            loaded.publish_thread(&on_thread);
                            // A thread keeps receipts of its own, so opening
                            // one asks a question this watcher has not asked.
                            loaded.catch_up_on_readers(&room).await;
                            loaded.publish_readers(&on_readers);
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
                            // The ordinary case: a sync delivers one update
                            // whether or not this room was in it.
                            continue;
                        };
                        // The thread first, because counting a reply writes
                        // into the history the room is published from.
                        if loaded.replied(&joined.timeline.events) {
                            loaded.publish_thread(&on_thread);
                        }
                        // Dropped while a window somebody jumped into is on
                        // screen: appending what was just said after a message
                        // from last March draws the two as one conversation.
                        let (arrived, system_arrived) = match loaded.focus {
                            Some(_) => (Vec::new(), Vec::new()),
                            None => (
                                loaded.read(&joined.timeline.events),
                                loaded.read_system(&joined.timeline.events),
                            ),
                        };
                        let counted = loaded.count_replies(&joined.timeline.events);
                        let added = loaded.history.arrived(arrived);
                        // After the history, not before: one sync can carry a
                        // message and the redaction that empties it, and a
                        // sweep run first would find nothing to mark.
                        let annotated = loaded.annotations(&joined.timeline.events);
                        let system_added = loaded.system.arrived(system_arrived);
                        if added {
                            // A message can arrive answering something older
                            // than what is loaded.
                            loaded.resolve(&room).await;
                        }
                        if added | counted | annotated | system_added {
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
                        // Off the batch rather than out of the store: an
                        // `m.receipt` carries everything that moved.
                        if loaded.receipts(&joined.ephemeral) {
                            loaded.publish_readers(&on_readers);
                        }
                    }
                    // Too many syncs while this task was decrypting a page.
                    // What was missed is history, asked for by scrolling back.
                    Err(RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, room_id = %watching, "fell behind the sync updates");
                    }
                    Err(RecvError::Closed) => break,
                },
                // `Pin::as_mut` on an `Option` is not a thing, so the arm is
                // guarded instead and the other two carry on without it.
                keys = async { rekeyed.as_mut().expect("guarded").next().await },
                    if rekeyed.is_some() =>
                {
                    match keys {
                        // Checked here because a key for somewhere else is the
                        // ordinary case: one stream serves every room.
                        Some(Ok(keys)) => {
                            if keys.iter().any(|key| key.room_id == parsed)
                                && loaded.reread(&room).await
                            {
                                loaded.publish(&on_change);
                            }
                        }
                        // Too many keys at once, which catching up after a
                        // long absence produces. The dropped batch is not
                        // retried, so a message may wait for a reopen.
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
struct Loaded {
    room_id: String,
    /// Who is signed in, so a reply this session sent counts as taking part.
    me: Option<String>,
    /// The thread somebody has open, if any.
    open: Option<OpenThread>,
    history: History,
    /// The membership changes loaded alongside `history`. Its own field
    /// because a join is never looked up again by ID, which is what `history`
    /// exists for.
    system: SystemHistory,
    /// What people have reacted with, for everything this watcher has seen.
    ///
    /// Beside the history because an annotation arrives for a message that may
    /// not be loaded, or may never be. Merged on when a timeline is published.
    reactions: Reactions,
    /// The corrections people have made, for everything this watcher has seen.
    ///
    /// Beside the history for the reason the reactions are, and because
    /// writing the new text in would be undone the moment a room key re-read
    /// the original.
    edits: Edits,
    /// The events this session could not read, by event ID, held as the JSON
    /// they arrived as, which is what `decrypt_event` takes.
    waiting: HashMap<String, Raw<AnySyncTimelineEvent>>,
    /// The message each reply names, for the ones not in the history.
    ///
    /// Beside the history on the same terms as the reactions. `None` is a
    /// lookup that came back with nothing, kept so a redacted message is asked
    /// about once rather than on every publish.
    answered: HashMap<String, Option<Message>>,
    /// Where the next backwards page starts, or `None` before the first one.
    from: Option<String>,
    /// Where the next forwards page starts; `None` at the live end.
    forward: Option<String>,
    /// The message the loaded window was opened around, when it is not the
    /// present.
    focus: Option<String>,
    /// Whether the homeserver still has older messages.
    more_before: bool,
    /// Whether the homeserver has messages after the loaded window.
    more_after: bool,
    /// Whether a page of older messages is being fetched, a jump included.
    loading: bool,
    /// Whether a page of newer messages is being fetched right now.
    loading_after: bool,
    /// Who was last reported as typing, so an unchanged list is not
    /// republished on every sync for as long as somebody keeps typing.
    typing: Vec<String>,
    /// Where this account had stopped reading when the room was opened. Read
    /// once and then left alone: see [`Timeline::read_up_to`] for why.
    read_up_to: Option<String>,
    /// Where everybody else in the room has read up to. Beside the history,
    /// because putting it on a message would redraw the conversation every
    /// time somebody else looked at it: see [`Readers`].
    read_by: ReadBy,
    /// What was last said about that, so an unchanged answer is not
    /// republished. Held rather than flagged, because what is drawn depends on
    /// which thread is open as well as on what has arrived.
    published: Readers,
    /// The message this watcher last sent a receipt for, which is the whole of
    /// the throttling: a reader sitting at the bottom of a room asks for the
    /// same message on every scroll and every sync.
    marked: Option<String>,
}

/// One thread being watched alongside the room.
///
/// Its replies are not in the room's timeline, so it holds its own history.
/// What it shares is the arriving sync, so keeping it current costs a second
/// read of a batch rather than a second subscription.
struct OpenThread {
    root_id: String,
    root: Option<Message>,
    history: History,
    more_before: bool,
}

impl Loaded {
    fn new(room_id: String, me: Option<String>) -> Self {
        let me_again = me.clone();
        Self {
            room_id,
            me,
            open: None,
            history: History::new(),
            system: SystemHistory::new(),
            reactions: Reactions::new(),
            edits: Edits::new(),
            waiting: HashMap::new(),
            answered: HashMap::new(),
            from: None,
            forward: None,
            focus: None,
            // Assumed until the homeserver says otherwise: "no more history"
            // is a stronger claim than an unasked question supports.
            more_before: true,
            more_after: false,
            loading: true,
            loading_after: false,
            typing: Vec::new(),
            read_up_to: None,
            read_by: ReadBy::new(me_again),
            published: Readers::default(),
            marked: None,
        }
    }

    /// Answer one ask for a page at either end, and report the result.
    ///
    /// Reports twice, to put the spinner up and to take it down. The first is
    /// what makes a slow homeserver distinguishable from a dead button.
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
        // A window read forwards until the homeserver has nothing after it is
        // not a window any more. Held on to, it goes on telling the sync arm
        // to drop every message that arrives, so the room reads as frozen.
        //
        // Nothing arriving is lost: a sync that landed while the page was
        // read is still on the broadcast receiver when the loop comes round.
        if matches!(towards, Direction::Forward) && !self.more_after {
            self.focus = None;
        }
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

    /// Say that a page is on its way, or has stopped being. Which end is part
    /// of it, because the interface draws the notice at that end of the list.
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
    /// `false`, because three requests to a homeserver that just refused one
    /// tell nobody anything new.
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
                // Logged rather than raised: a dialog about a page of history
                // is worse than the absence of it.
                tracing::warn!(%error, room_id = %self.room_id, "could not read a page of messages");
                return false;
            }
        };

        // Both checked: homeservers differ about whether the end of history is
        // an empty chunk or a missing `end`.
        let more = page.end.is_some() && !page.chunk.is_empty();
        let chunk: Vec<TimelineEvent> = match towards {
            // Backwards, so the homeserver answers newest first and the page
            // has to be turned round.
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
        let system_arrived = self.read_system(&chunk);
        // Membership changes are left out of `drawable`: a page holding
        // nothing but joins and leaves still asks for the next page.
        let drawable = !arrived.is_empty();
        match towards {
            Direction::Backward => {
                self.history.backfilled(arrived);
                self.system.backfilled(system_arrived);
            }
            Direction::Forward => {
                self.history.arrived(arrived);
                self.system.arrived(system_arrived);
            }
        };
        // A page carries reactions too, which is how a message scrolled back
        // to arrives with what is already on it. The thread panel has no
        // equivalent: `/relations` asks for thread replies only.
        //
        // After the history, for the reason the sync arm has it in that order.
        self.annotations(&chunk);

        !drawable && self.has_more(towards)
    }

    /// Draw the history around `event_id` instead of the present.
    ///
    /// The window replaces what is loaded rather than joining it: see
    /// [`around`]. A window that will not load leaves the room where it was,
    /// rather than emptying it to say so.
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
                self.system = SystemHistory::new();
                self.system.backfilled(window.system);
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
    /// A sync arriving during the jump was dropped, so what is held is out of
    /// date and reading it fresh is both the correction and the catch-up.
    async fn present<F>(&mut self, room: &Room, on_change: &F)
    where
        F: Fn(Timeline),
    {
        if self.focus.is_none() {
            return;
        }

        self.history = History::new();
        self.system = SystemHistory::new();
        self.focus = None;
        self.from = None;
        self.forward = None;
        self.more_before = true;
        self.more_after = false;
        self.page(room, on_change, Direction::Backward).await;
    }

    /// Look up whatever the loaded replies name and this does not hold.
    ///
    /// One request each at worst, none for a message the SDK has stored.
    /// Bounded by what is loaded, and the answer is kept, so scrolling past a
    /// reply twice is not asking twice.
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
            // the compiler would otherwise report the layout depth limit.
            let found = Box::pin(answering::answered(room, &id)).await;
            self.answered.insert(id, found);
        }
    }

    /// One batch of events as messages, remembering the ones with no key.
    ///
    /// The remembering is why this is not a `filter_map` at the two call
    /// sites: a wait drawn for an unreadable event is only redeemed by
    /// something holding the ciphertext until the key turns up.
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

    /// One batch of events as membership changes. No waiting list, unlike
    /// [`Self::read`]: a membership event is never encrypted.
    fn read_system(&self, events: &[TimelineEvent]) -> Vec<SystemMessage> {
        events.iter().filter_map(facts::system).collect()
    }

    /// Who this batch says is typing, when it says anything about it.
    ///
    /// `None` when the batch carried no `m.typing`, and when it says what the
    /// last one did: a room where somebody is typing sends one on every sync
    /// until they stop.
    fn typing(&mut self, ephemeral: &[Raw<AnySyncEphemeralRoomEvent>]) -> Option<Typing> {
        let said = ephemeral.iter().find_map(facts::typing)?;

        // Ours taken out here rather than in the interface: same answer for
        // every reader.
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

    /// Take note of the reactions, corrections and redactions in one batch,
    /// reporting whether anything drawn changed. Separate from [`Self::read`]
    /// because none of these is a message or ever becomes one.
    fn annotations(&mut self, events: &[TimelineEvent]) -> bool {
        let mut changed = false;
        for event in events {
            if let Some(one) = facts::annotation(event) {
                changed |= self.reactions.added(one);
                continue;
            }
            if let Some(one) = facts::replacement(event) {
                // Taken whoever sent it: the comparison against the original's
                // author needs the original, so `Edits::latest_on` makes it.
                changed |= self.edits.added(one);
                continue;
            }
            if let Some(gone) = facts::redaction(event) {
                // Whichever it was. An annotation and an edit leave nothing
                // behind; a message is emptied where it stands, so the reply
                // underneath is not left answering a gap.
                let by = Some(gone.sender.as_str());
                changed |= self.reactions.redacted(&gone.event_id);
                changed |= self.edits.redacted(&gone.event_id);
                changed |= self.history.redacted(&gone.event_id, by);
                // The panel draws out of its own history and its own root, so
                // without this a deletion empties the room copy and leaves the
                // words in the panel, both on screen at once.
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

    /// Try every message this session had no key for again, reporting whether
    /// anything on screen changed. Answered on the watcher's own task, so a
    /// retry cannot interleave with a backfill writing the same history.
    async fn reread(&mut self, room: &Room) -> bool {
        let held: Vec<(String, Raw<AnySyncTimelineEvent>)> = self
            .waiting
            .iter()
            .map(|(id, raw)| (id.clone(), raw.clone()))
            .collect();

        let mut changed = false;
        for (id, raw) in held {
            // Cast unchecked because it is the same JSON either way: nothing
            // reaches here but an `m.room.encrypted` that would not open.
            let Ok(event) = room.decrypt_event(raw.cast_ref_unchecked(), None).await else {
                continue;
            };
            if event.kind.is_utd() {
                // This key was for a different session. Kept: the one that
                // opens it may still arrive.
                continue;
            }

            self.waiting.remove(&id);
            changed |= match facts::message(&event) {
                Some(message) => self.history.replace(message),
                // It opened, and it is a reaction or a thread reply, neither
                // of which is drawn, so the wait has to go.
                None => self.history.forget(&id),
            };
        }

        changed
    }

    /// Open the thread hanging from `root_id`, or close whatever is open.
    ///
    /// A thread that will not load closes rather than half-opening: a root
    /// with no replies under it reads as a thread somebody deleted.
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
                // Logged rather than raised, like a page that would not come
                // back.
                tracing::warn!(%error, room_id = %self.room_id, %root_id, "could not read the thread");
                self.open = None;
            }
        }
    }

    /// Say that everything up to and including `event_id` has been read.
    ///
    /// Dropped when it names the message this watcher last sent, and recorded
    /// before the request so a slow homeserver does not collect a queue of
    /// asks for the same message. A failure is logged rather than raised.
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
    /// Recording and answering in one step on purpose: a check that did not
    /// record would let the second of two identical asks through while the
    /// first was still in flight.
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
    /// this room they hang from, reporting whether the room changed.
    ///
    /// The homeserver's tally is only recounted when the message is read
    /// again, so without this a thread just replied in shows nothing until the
    /// room is reopened.
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
    /// Merged here rather than held on the message, because a message is
    /// replaced wholesale when a room key opens it and every re-read would
    /// have to carry the rest forward by hand.
    fn drawn(&self, messages: &[Message]) -> Vec<Message> {
        let me = self.me.as_deref();
        messages
            .iter()
            .map(|message| {
                // Nothing folds onto a mark: a pill on an emptied message
                // counts agreement with nothing. [`Self::corrected`] refuses
                // the edits for a stronger reason than that.
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

    /// One message as its author has since corrected it, if they have. The
    /// sender check is [`Edits::latest_on`]'s, made here because this is the
    /// first place the original is in hand to compare against.
    fn corrected(&self, message: &Message) -> Message {
        // An edit outlives the message it corrects, because a redaction names
        // one event and the corrections against it are not that event, so
        // folding one on would put the removed sentence back.
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
            // Both, always, out of the edit alone. Field by field, a message
            // edited down to plain text keeps its old `html`, and that is the
            // one `FormattedBody` draws.
            body: edit.body.clone(),
            html: edit.html.clone(),
            edited: true,
            // The edit's, not the original's: the words above are the
            // edit's, so its device is the one to vouch for.
            sender_trust: edit.sender_trust,
            // `at` is deliberately untouched: the date separators are keyed
            // off it, and a message from last Tuesday corrected this morning
            // would otherwise jump to today and take its separator with it.
            ..message.clone()
        }
    }

    /// The messages these replies name that are not among them.
    ///
    /// Read out of what [`Self::resolve`] found rather than looked up here,
    /// because publishing happens on every reaction and every key.
    ///
    /// Edits are merged onto these and reactions deliberately are not. A pill
    /// missing from a quoted row is something not drawn; pre-edit text in one
    /// is a wrong sentence attributed to somebody by name.
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

    /// Read what the store already knows about who has read what.
    ///
    /// Needed because the receipts a sync carries are a delta, so what anybody
    /// read before this launch is in the store and nowhere else.
    ///
    /// Two lookups per loaded message, because a client that predates threads
    /// and a current one spell the same claim differently: `m.read` with no
    /// `thread_id`, and `m.read` with `main`. Asking for one of the two draws
    /// an empty room for half the people in it.
    ///
    /// Fifty messages cost under four milliseconds against the real encrypted
    /// store, and this runs on a page rather than on a receipt.
    async fn catch_up_on_readers(&mut self, room: &Room) {
        let wanted: Vec<(String, About)> = self
            .history
            .messages()
            .iter()
            .map(|message| (message.id.clone(), About::Room))
            .chain(self.open.iter().flat_map(|open| {
                open.history
                    .messages()
                    .iter()
                    .map(|reply| (reply.id.clone(), About::Thread(open.root_id.clone())))
            }))
            .collect();

        for (id, about) in wanted {
            let Ok(event_id) = EventId::parse(&id) else {
                continue;
            };
            let asked = match &about {
                About::Room => vec![ReceiptThread::Unthreaded, ReceiptThread::Main],
                About::Thread(root) => match EventId::parse(root) {
                    Ok(root) => vec![ReceiptThread::Thread(root)],
                    Err(_) => continue,
                },
            };
            for thread in asked {
                let found = room
                    .load_event_receipts(ReceiptType::Read, thread, &event_id)
                    .await;
                let found = match found {
                    Ok(found) => found,
                    Err(error) => {
                        // Logged rather than raised: a row of faces is not
                        // worth a dialog.
                        tracing::warn!(%error, room_id = %self.room_id, "could not read the receipts on a message");
                        continue;
                    }
                };
                for (user, _) in found {
                    self.read_by.noted(user.as_str(), &id, about.clone());
                }
            }
        }
    }

    /// Take note of the receipts one sync batch carried, reporting whether
    /// anything drawn changed. No store lookup: an `m.receipt` names everybody
    /// who has moved and where they moved to.
    fn receipts(&mut self, ephemeral: &[Raw<AnySyncEphemeralRoomEvent>]) -> bool {
        let mut changed = false;
        for event in ephemeral {
            let Some(said) = facts::receipts(event) else {
                continue;
            };
            for one in said {
                changed |= self.read_by.noted(&one.user, &one.event_id, one.about);
            }
        }
        changed
    }

    /// Say who has read what, unless it is what was said last time.
    ///
    /// The suppression is the point rather than an optimisation: a room where
    /// everybody has caught up receives an unchanged `m.receipt` on every
    /// sync, and republishing it would wake the webview for ever.
    fn publish_readers<I>(&mut self, on_readers: &I)
    where
        I: Fn(Readers),
    {
        let now = Readers {
            room_id: self.room_id.clone(),
            main: self.read_by.on(&About::Room),
            thread: self.open.as_ref().map(|open| ThreadReaders {
                root_id: open.root_id.clone(),
                on: self.read_by.on(&About::Thread(open.root_id.clone())),
            }),
        };

        if now == self.published {
            return;
        }
        self.published = now.clone();
        on_readers(now);
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
            system: self.system.messages().to_vec(),
            more_before: self.more_before,
            more_after: self.more_after,
            focus: self.focus.clone(),
            loading: self.loading,
            loading_after: self.loading_after,
            read_up_to: self.read_up_to.clone(),
        });
    }
}

/// Say whether this session is typing in a room. Safe to call on every
/// keystroke: the SDK sends nothing while the last notice is still current, so
/// the throttling is already done a layer down.
pub async fn typing(client: &Client, room_id: &str, typing: bool) -> Result<()> {
    room_of(client, room_id)?.typing_notice(typing).await?;
    Ok(())
}

/// React to a message.
///
/// Nothing is returned and nothing is echoed: the reaction appears when the
/// sync brings it back. Reacting twice with one key is not guarded here, and
/// the specification says a duplicate is ignored.
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
/// `reaction_id` is the annotation's own event, not the message it is on,
/// because a reaction is undone by redacting it and the two are
/// indistinguishable here. `Reaction::mine` is where the interface gets it.
pub async fn unreact(client: &Client, room_id: &str, reaction_id: &str) -> Result<()> {
    let room = room_of(client, room_id)?;
    // `redact` answers with the SDK's HTTP error rather than its own, so it is
    // lifted: `user_message` already has words for a failed SDK call.
    room.redact(&event_id_of(reaction_id)?, None, None)
        .await
        .map_err(matrix_sdk::Error::from)?;
    Ok(())
}

/// Delete a message.
///
/// A redaction, which is what deleting is in Matrix: the event survives with
/// its sender and timestamp, which is what the mark in the room is drawn from,
/// and what federation already handed on is not recalled.
///
/// No reason is sent, because nowhere in the interface asks for one. Whose
/// message it is stays the homeserver's to enforce, and nothing is echoed:
/// the message empties when the sync brings the redaction back.
pub async fn delete(client: &Client, room_id: &str, event_id: &str) -> Result<()> {
    // Before the room, so that something which is not an event ID is answered
    // as that rather than as whatever the room lookup says first.
    let target = event_id_of(event_id)?;
    // Lifted the way `unreact` lifts it, and for the reason written there.
    room_of(client, room_id)?
        .redact(&target, None, None)
        .await
        .map_err(matrix_sdk::Error::from)?;
    Ok(())
}

/// Say something in a room.
///
/// Read as markdown, with the text sent as the plaintext fallback either way,
/// so a sentence with a stray asterisk in it goes out as the sentence.
/// Encrypted or not according to the room, which the SDK decides.
///
/// Nothing is returned and nothing is echoed: see the module header.
pub async fn send(client: &Client, room_id: &str, body: &str) -> Result<()> {
    let content = written(body)?;
    room_of(client, room_id)?.send(content).await?;
    Ok(())
}

/// Answer one message in the room.
///
/// A reply rather than a thread, so it lands in the conversation everybody is
/// reading. Nothing here writes the quoted fallback: it was removed from the
/// specification because every client has to strip it again.
///
/// `sender` is who wrote the message being answered, used only for the
/// `m.mentions` a reply carries, so the person answered is notified.
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
        // The room, never the thread the answered message might be in: a
        // message in a thread is not drawn in the room at all.
        ForwardThread::No,
        AddMentions::Yes,
    );

    room_of(client, room_id)?.send(content).await?;
    Ok(())
}

/// Correct a message this account sent.
///
/// A wrapper. `Room::make_edit_event` refuses an edit of somebody else's
/// message, carries the original's `m.mentions` forward, and writes both
/// `m.new_content` and the `* ` fallback body.
///
/// It reads the target event first, so an edit of a message old enough to have
/// fallen out of the event cache fails on the fetch, before anything is sent.
/// Nothing is echoed: the correction appears when the sync brings it back.
pub async fn send_edit(client: &Client, room_id: &str, event_id: &str, body: &str) -> Result<()> {
    // Before the fetch, so an empty box costs no round trip. Emptying the
    // composer is also not how a message is deleted.
    let content = written(body)?;
    let target = event_id_of(event_id)?;
    let room = room_of(client, room_id)?;

    // Boxed, and load-bearing rather than tidy: inlining `make_edit_event`
    // makes this future deep enough that rustc gives up on its layout. Under
    // `-C instrument-coverage` it gives up sooner, so the failure lands in
    // CI's coverage job rather than in an ordinary build.
    let edit = Box::pin(room.make_edit_event(&target, EditedContent::RoomMessage(content.into())))
        .await
        .map_err(|error| match error {
            // Its own variant, because it is the one failure here with
            // something to say to a person.
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
/// With no `answering`, `in_reply_to` is the last thing said in the thread as
/// far as the caller knows, and it is only a fallback for clients that do not
/// understand threads. Stale is harmless: nothing about which thread this
/// belongs to depends on it.
///
/// With one, `in_reply_to` is the message being answered, the fallback flag
/// comes off, and the author is mentioned, because an answer nobody is
/// notified of is a line in a panel that is not open.
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
            // answered: `make_for_thread` takes the root from this, and would
            // otherwise start a second thread rooted at the answered message.
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
    // Trimmed before it is judged empty, so a stray newline is not a message.
    // Sent untrimmed, because leading spaces in a pasted code block count.
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
    use crate::timeline::dto::SenderTrust;
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

    /// One drawn message, as a watcher is holding it before any fold.
    fn held(id: &str, trust: Option<SenderTrust>) -> Message {
        Message {
            id: id.to_owned(),
            sender: "@ada:example.org".to_owned(),
            at: 1_700_000_000_000,
            body: "as first said".to_owned(),
            html: None,
            media: None,
            thread: None,
            reactions: Vec::new(),
            reply_to: None,
            mentions: Vec::new(),
            edited: false,
            deleted_by: None,
            sender_trust: trust,
            kind: MessageKind::Text,
        }
    }

    /// Note one edit on `$said:example.org`, carrying `trust`.
    fn correction(loaded: &mut Loaded, trust: Option<SenderTrust>) {
        loaded.edits.added(facts::Replacement {
            event_id: "$edit:example.org".to_owned(),
            target: "$said:example.org".to_owned(),
            sender: "@ada:example.org".to_owned(),
            at: 1_700_000_100_000,
            body: "as corrected".to_owned(),
            html: None,
            sender_trust: trust,
        });
    }

    #[test]
    fn a_correction_brings_the_trust_of_the_device_that_sent_it() {
        // The words on screen are the edit's, so the device that has to be
        // vouched for is the edit's. A verified message rewritten from an
        // unsigned device is the case that would otherwise draw nothing.
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);
        correction(&mut loaded, Some(SenderTrust::UnsignedDevice));

        let folded = loaded.corrected(&held("$said:example.org", None));

        assert_eq!(folded.body, "as corrected");
        assert_eq!(folded.sender_trust, Some(SenderTrust::UnsignedDevice));
    }

    #[test]
    fn a_correction_from_a_sound_device_takes_the_old_mark_off() {
        // The other direction, and it has to work too: the sentence being read
        // is the edit's, and nothing is wrong with the device that sent it.
        let mut loaded = Loaded::new("!room:example.org".to_owned(), None);
        correction(&mut loaded, None);

        let folded = loaded.corrected(&held(
            "$said:example.org",
            Some(SenderTrust::UnsignedDevice),
        ));

        assert_eq!(folded.sender_trust, None);
    }

    #[test]
    fn a_message_nobody_corrected_keeps_its_own_trust() {
        let loaded = Loaded::new("!room:example.org".to_owned(), None);

        let folded = loaded.corrected(&held(
            "$said:example.org",
            Some(SenderTrust::MismatchedSender),
        ));

        assert_eq!(folded.sender_trust, Some(SenderTrust::MismatchedSender));
    }
}
