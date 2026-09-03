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
mod media;
mod reactions;
mod thread;

pub use dto::{Media, Message, MessageKind, Reaction, Thread, ThreadSummary, Timeline};
pub use history::History;
pub use media::{Attachment, bytes, media};
pub use reactions::Reactions;
pub use thread::thread;

use std::collections::HashMap;

use futures_util::StreamExt;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::Direction;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::events::reaction::ReactionEventContent;
use matrix_sdk::ruma::events::relation::Annotation;
use matrix_sdk::ruma::events::relation::Thread as ThreadRelation;
use matrix_sdk::ruma::events::room::message::{Relation, RoomMessageEventContent};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{EventId, OwnedEventId, RoomId};
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
    /// Open the thread hanging from this message, or close whatever is open.
    Thread(Option<String>),
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

    /// Open the thread hanging from `root_id`, or close whatever is open.
    ///
    /// Answered on the watcher's own task, like everything else, so a thread
    /// opened and closed quickly cannot report the two out of order. Silently
    /// ignored once the watcher has ended, which is what pressing a thread at
    /// the same moment as a room change is.
    pub fn open_thread(&self, root_id: Option<String>) {
        let _ = self.asking.send(Ask::Thread(root_id));
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
pub fn watch<F, G>(client: Client, room_id: &str, on_change: F, on_thread: G) -> Watch
where
    F: Fn(Timeline) + Send + Sync + 'static,
    G: Fn(Option<Thread>) + Send + Sync + 'static,
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
        loaded.publish(&on_change);
        loaded.publish_thread(&on_thread);
        loaded.page(&room, &on_change).await;

        loop {
            tokio::select! {
                asked = asked.recv() => {
                    match asked {
                        None => break,
                        Some(Ask::Earlier) => loaded.page(&room, &on_change).await,
                        Some(Ask::Thread(root_id)) => {
                            // Boxed because the compiler otherwise gives up
                            // computing the layout of this task: the arm holds
                            // a `/relations` request and an event fetch, both
                            // of them deep, inside a `select!` inside a spawn.
                            Box::pin(loaded.open(&client, root_id)).await;
                            loaded.publish_thread(&on_thread);
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
                        let arrived = loaded.read(&joined.timeline.events);
                        let counted = loaded.count_replies(&joined.timeline.events);
                        let annotated = loaded.annotations(&joined.timeline.events);
                        if loaded.history.arrived(arrived) | counted | annotated {
                            loaded.publish(&on_change);
                        }
                        // A reaction in the room may be on the thread's root or
                        // on one of its replies, both of which the panel draws.
                        if annotated && loaded.open.is_some() {
                            loaded.publish_thread(&on_thread);
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
    /// The events this session could not read, by event ID.
    ///
    /// Held as the JSON they arrived as, which is what `decrypt_event` takes,
    /// and dropped as each one opens. An encrypted room that has been quiet
    /// holds nothing here at all; one this session arrived late to holds a
    /// screenful, which is the case this exists for.
    waiting: HashMap<String, Raw<AnySyncTimelineEvent>>,
    /// Where the next backwards page starts, or `None` before the first one.
    from: Option<String>,
    /// Whether the homeserver still has older messages.
    more_before: bool,
    /// Whether a page is being fetched right now.
    loading: bool,
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
            waiting: HashMap::new(),
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
        let page: Vec<TimelineEvent> = page.chunk.into_iter().rev().collect();
        let older = self.read(&page);
        // A page carries every kind of event, reactions among them, which is
        // how a message scrolled back to arrives with what is already on it.
        // The thread panel has no equivalent: `/relations` is asked for thread
        // replies only, so a reply's reactions appear when one arrives live
        // rather than when the panel opens.
        self.annotations(&page);
        let drawable = !older.is_empty();
        self.history.backfilled(older);

        !drawable && self.more_before
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

    /// Take note of the reactions and the redactions in one batch.
    ///
    /// Reports whether anything drawn changed. Separate from [`Self::read`]
    /// because these are not messages and never become any: an annotation is
    /// something *on* a message, and a redaction can remove either.
    fn annotations(&mut self, events: &[TimelineEvent]) -> bool {
        let mut changed = false;
        for event in events {
            if let Some(one) = facts::annotation(event) {
                changed |= self
                    .reactions
                    .added(&one.event_id, &one.target, &one.key, &one.sender);
                continue;
            }
            if let Some(gone) = facts::redaction(event) {
                // Whichever it was. A redacted message stops being a message
                // and is dropped from the history; a redacted annotation is
                // somebody taking a reaction back.
                changed |= self.reactions.redacted(&gone);
                changed |= self.history.forget(&gone);
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

    /// The messages, each carrying what people have reacted to it with.
    ///
    /// Merged here rather than held on the message, because the two change for
    /// different reasons: a message is replaced when a room key opens it, and
    /// what is on it changes when somebody presses a pill. Keeping the
    /// reactions on the message would mean every re-read had to carry them
    /// forward by hand, and the one that forgot would silently clear them.
    fn drawn(&self, messages: &[Message]) -> Vec<Message> {
        let me = self.me.as_deref();
        messages
            .iter()
            .map(|message| {
                let reactions = self.reactions.on(&message.id, me);
                if reactions.is_empty() {
                    return message.clone();
                }
                Message {
                    reactions,
                    ..message.clone()
                }
            })
            .collect()
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
        on_change(Timeline {
            room_id: self.room_id.clone(),
            messages: self.drawn(self.history.messages()),
            more_before: self.more_before,
            loading: self.loading,
        });
    }
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

/// Say something in a thread.
///
/// `latest_id` is the last thing said in the thread as far as the caller
/// knows, and it goes on the reply fallback rather than on the relation
/// itself. A client that understands threads reads `event_id` and puts this in
/// the right conversation; one that does not sees an ordinary reply pointing
/// at whatever was being answered, which is the whole reason the fallback is
/// there. Stale is harmless: nothing about which thread this belongs to
/// depends on it.
pub async fn send_in_thread(
    client: &Client,
    room_id: &str,
    root_id: &str,
    latest_id: &str,
    body: &str,
) -> Result<()> {
    let mut content = written(body)?;
    let root = event_id_of(root_id)?;
    let latest = event_id_of(latest_id)?;
    content.relates_to = Some(Relation::Thread(ThreadRelation::plain(root, latest)));

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
