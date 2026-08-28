// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The thread that owns the call.
//!
//! `Call::join` spawns its heartbeat, its dead man's switch and its media key
//! distribution with `spawn_local`, because the command sender underneath is
//! `?Send`. Called outside a `tokio::task::LocalSet` it panics. So the call
//! gets a thread of its own, with a current-thread runtime and a `LocalSet` on
//! it, and everything else reaches it through a channel.
//!
//! That is the shape [`consort_audio::AudioThread`] already has, for a
//! different reason with the same consequence: a `cpal::Stream` is `!Send`, a
//! `Call` is pinned to its `LocalSet`, and neither can be held in shared
//! application state or across an await inside a Tauri command.
//!
//! [`consort_audio::AudioThread`]: https://docs.rs/consort-audio
//!
//! ## Where it differs from the audio thread
//!
//! Async channels both ways, where the audio thread uses `std::sync::mpsc`.
//! The audio thread has to: its frames are posted by a realtime callback that
//! must not await, and its loop is an ordinary blocking `recv`. This loop is
//! async from top to bottom, and a blocking `recv` inside it would starve the
//! very `spawn_local` tasks keeping the call alive. A missed heartbeat is a
//! membership that expires, so that is not a small mistake.
//!
//! ## One thing at a time
//!
//! The loop does not read another command while a join is in flight. Somebody
//! clicking a second channel during a slow connect is queued behind the first,
//! not raced against it. That is a deliberate simplification: cancelling a
//! half-finished join means unwinding a membership that may or may not have
//! published, and the queue costs at most [`JOIN_TIMEOUT`] of waiting.

use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::event::CallEvent;
use crate::failure::CallFailure;
use crate::microphone::Microphone;
use crate::publish::pump;
use crate::transport::{CallSession, CallTransport, Roster};

/// How long a join may take before it is abandoned.
///
/// Every step of one waits on something remote: the homeserver for the room
/// and the membership, the authorisation service for a token, the SFU for the
/// connection. Without a bound, one of them hanging leaves a voice channel
/// showing "Connecting" for as long as the application runs, with no way back
/// short of restarting it.
///
/// Thirty seconds is long enough that a slow but working connect is not cut
/// off, and short enough that somebody has not yet given up on the software.
pub const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a leave may take before it is abandoned.
///
/// Longer than one attempt at the request it is waiting for, which is the
/// whole of the constraint. The bridge gives a membership send fifteen seconds
/// before it calls the attempt dead, so a budget under that does not bound a
/// hang: it abandons requests that were going to succeed, on any homeserver
/// slower than the budget. What that costs is not abstract. The leave is
/// cancelled in flight, the membership is never retracted, and the person is
/// still sitting in the voice channel to everybody else in the space.
///
/// Waiting this long is free, because nobody is waiting on it. `Disconnected`
/// goes out before the leave is awaited, so the interface has already closed
/// the call by the time any of this runs.
///
/// Still bounded, and bounded below the dead man's switch that backs it up: a
/// leave that has not landed within the membership's own keep-alive window has
/// been overtaken by it.
pub const LEAVE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a leave may take when the application is closing.
///
/// The one case where the membership really is the expendable half. Dropping
/// the handle waits for this thread, so a leave with no short bound on it is
/// an application that will not close, and a person who cannot quit is a worse
/// outcome than a ghost in a channel that the dead man's switch will clear.
pub const SHUTDOWN_LEAVE_TIMEOUT: Duration = Duration::from_secs(5);

/// What the thread accepts.
enum Message {
    Connect { room_id: String },
    Disconnect,
    Shutdown,
}

/// A handle on the call thread.
///
/// Dropping it leaves any call in progress and ends the thread.
pub struct CallThread {
    commands: UnboundedSender<Message>,
    /// `Option` only so [`Drop`] can take it. Always `Some` otherwise.
    join: Option<JoinHandle<()>>,
}

impl CallThread {
    /// Start the thread. It idles until told to [`connect`](Self::connect).
    ///
    /// `microphone` is where captured audio arrives from. It is taken here
    /// rather than at each connect because the audio thread has to be able to
    /// hold the other end of it whether or not a call is up: the two threads
    /// are started once, and the queue between them outlives any one call.
    pub fn spawn<T: CallTransport>(
        transport: T,
        events: UnboundedSender<CallEvent>,
        microphone: Microphone,
    ) -> Self {
        let (commands, inbox) = unbounded_channel::<Message>();

        let join = std::thread::Builder::new()
            .name("consort-call".to_owned())
            .spawn(move || run(transport, inbox, events, microphone))
            .expect("the operating system refused a thread");

        Self {
            commands,
            join: Some(join),
        }
    }

    /// Join the call in `room_id`, leaving whatever call is current first.
    pub fn connect(&self, room_id: String) {
        self.send(Message::Connect { room_id });
    }

    /// Leave the current call, if there is one.
    pub fn disconnect(&self) {
        self.send(Message::Disconnect);
    }

    /// Post a command, ignoring a thread that has already gone.
    ///
    /// Nothing useful is done about it. The thread only ends when this handle
    /// is dropped or the runtime could not be built, and in both cases the
    /// caller is on its way out too.
    fn send(&self, message: Message) {
        if self.commands.send(message).is_err() {
            tracing::warn!("the call thread is gone; command dropped");
        }
    }
}

impl Drop for CallThread {
    fn drop(&mut self) {
        // Best effort, in order: ask the loop to leave the call and return,
        // then wait for it. Waiting is the point. The loop unwinds the
        // membership on its way out, and a process that exits without that
        // leaves a ghost sitting in the channel until the membership expires.
        let _ = self.commands.send(Message::Shutdown);

        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            tracing::error!("the call thread panicked");
        }
    }
}

/// The thread body: a current-thread runtime with a `LocalSet` on it.
fn run<T: CallTransport>(
    transport: T,
    inbox: UnboundedReceiver<Message>,
    events: UnboundedSender<CallEvent>,
    microphone: Microphone,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            // Nothing to fall back to, and nothing to report it on that the
            // caller would see: the thread is not up, so no event can be
            // emitted from it. Said in the log and then the thread ends, which
            // is what every later command will notice.
            tracing::error!(%error, "could not build the call runtime");
            return;
        }
    };

    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(serve(transport, inbox, events, microphone)));
}

/// A call this session is in, and the task carrying its microphone.
struct Joined<S> {
    room_id: String,
    session: S,
    /// Ends when this is dropped. See [`AbortOnDrop`].
    publishing: AbortOnDrop,
    /// The task reporting who is in the call. Ends the same way.
    watching: AbortOnDrop,
}

/// A task that ends when this handle is dropped.
///
/// The task it holds waits on a queue, so nothing else would ever end it: a
/// microphone that has been switched off stops filling the queue rather than
/// closing it. Tying it to a handle means no path out of a call can forget.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The loop. Returns when told to shut down, or when the handle is dropped.
async fn serve<T: CallTransport>(
    transport: T,
    mut inbox: UnboundedReceiver<Message>,
    events: UnboundedSender<CallEvent>,
    microphone: Microphone,
) {
    let mut current: Option<Joined<T::Session>> = None;

    while let Some(message) = inbox.recv().await {
        match message {
            Message::Connect { room_id } => {
                current = connect(&transport, current, room_id, &events, &microphone).await;
            }
            Message::Disconnect => {
                if let Some(joined) = current.take() {
                    // Said before the leave rather than after it. The decision
                    // to leave is this client's own and is already final by the
                    // time it gets here; what follows is telling the homeserver
                    // about it, and there is no answer it could give that would
                    // put anybody back in the call. Waiting for one would be a
                    // disconnect button that does nothing for several seconds,
                    // which is exactly how long the homeserver feels like
                    // taking.
                    emit(&events, CallEvent::Disconnected);
                    leave(Some(joined), LEAVE_TIMEOUT).await;
                }
            }
            Message::Shutdown => {
                // No `Disconnected` on the way out. Whatever asked for this is
                // already gone, and an event nobody is left to receive is not
                // worth the risk of the receiver having been dropped first.
                leave(current.take(), SHUTDOWN_LEAVE_TIMEOUT).await;
                return;
            }
        }
    }

    // The handle was dropped without a `Shutdown`, which `Drop` does not do
    // but a panicking caller can. Unwind anyway, and on the closing budget:
    // this is the same teardown, reached the untidy way.
    leave(current.take(), SHUTDOWN_LEAVE_TIMEOUT).await;
}

/// Join `room_id`, having first left whatever call was current.
///
/// Returns the call to hold on to, or `None` when there is none: both a join
/// that failed and one that was asked for while already in that same call.
async fn connect<T: CallTransport>(
    transport: &T,
    current: Option<Joined<T::Session>>,
    room_id: String,
    events: &UnboundedSender<CallEvent>,
    microphone: &Microphone,
) -> Option<Joined<T::Session>> {
    // Already there. Re-announced rather than ignored, because the interface
    // may be asking precisely because it has lost track of where it is, and a
    // second `Connected` costs nothing.
    if let Some(joined) = current.as_ref()
        && joined.room_id == room_id
    {
        // With the roster read again rather than remembered. The interface
        // asking where it is deserves the current answer, and the alternative
        // is holding a copy here that the watcher below is already keeping.
        let roster = joined.session.roster();
        emit(events, connected(&room_id, &roster).await);
        return current;
    }

    // Said before the leave rather than after it, and the leave is not
    // announced at all.
    //
    // The leave still happens first, for the reason it always did: two
    // channels' worth of membership at once is one person sitting in two voice
    // channels to everybody else in the space. What changed is that nobody is
    // told about it. `Disconnected` means the call is over, and everything
    // downstream acts on that: the panel closes, and the microphone the call
    // opened is given back. Emitting one here, immediately superseded by this
    // `Connecting`, would close and reopen both between two calls that are
    // meant to be continuous.
    emit(
        events,
        CallEvent::Connecting {
            room_id: room_id.clone(),
        },
    );

    // On the full budget, not a short one. An abandoned leave here is a
    // membership still published in the channel just left, which is the same
    // person in two voice channels at once to everybody else in the space.
    leave(current, LEAVE_TIMEOUT).await;

    let session = match tokio::time::timeout(JOIN_TIMEOUT, transport.join(&room_id)).await {
        Ok(Ok(session)) => session,
        Ok(Err(failure)) => {
            fail(events, room_id, failure);
            return None;
        }
        Err(_) => {
            fail(events, room_id, CallFailure::TimedOut(JOIN_TIMEOUT));
            return None;
        }
    };

    // Publishing is part of joining, not something that happens afterwards. A
    // call this session cannot be heard in is not a call somebody wanted, and
    // saying `Connected` for one would be a lie the interface then has to be
    // corrected out of.
    let track = match session.publish_microphone().await {
        Ok(track) => track,
        Err(failure) => {
            // Unwound rather than kept. A membership left published for a call
            // this session cannot speak in is a name sitting in the channel
            // that nobody can reach.
            leave_session(&room_id, session, LEAVE_TIMEOUT).await;
            fail(events, room_id, failure);
            return None;
        }
    };

    // Its own task, so a frame waiting on the SFU does not sit in front of the
    // click that disconnects. `spawn_local` rather than `spawn` because this
    // whole thread is one `LocalSet` and the publication came out of a session
    // that cannot leave it.
    let publishing = AbortOnDrop(tokio::task::spawn_local(pump(track, microphone.clone())));

    // Two views of the same roster: one read now, for the `Connected` that
    // says the call is up, and one handed to the task that reports every
    // change after it. Reading it before the task starts is what stops the
    // interface drawing an empty channel until the next person moves.
    let roster = session.roster();
    emit(events, connected(&room_id, &roster).await);

    // Ended by its handle, like the publication and for the same reason: it
    // waits on something that goes quiet rather than closing when the call is
    // left.
    let watching = AbortOnDrop(tokio::task::spawn_local(watch_roster(
        room_id.clone(),
        roster,
        events.clone(),
    )));

    Some(Joined {
        room_id,
        session,
        publishing,
        watching,
    })
}

/// Report who is in the call, every time that changes.
///
/// One `Connected` per change rather than an event of its own, because being
/// in a call and who is in it are one state: a reader that keeps only the
/// latest thing said on this channel then has both, and a reader that missed
/// one has not also lost track of whether it is in a call.
///
/// Ends when the roster says it will never change again, which is a call that
/// went away underneath this task. Nothing is emitted for that: whatever ended
/// the call is what says so.
async fn watch_roster<R: Roster>(
    room_id: String,
    mut roster: R,
    events: UnboundedSender<CallEvent>,
) {
    while roster.changed().await {
        let said = connected(&room_id, &roster).await;
        emit(&events, said);
    }
}

/// What being in this call currently means, all of it.
///
/// Built in one place rather than at each of the three call sites, because the
/// roster and the trouble have to be read together: two readers a moment apart
/// can report a call whose people and whose fault came from different
/// instants, and the interface would draw the pair as though they were one.
async fn connected<R: Roster>(room_id: &str, roster: &R) -> CallEvent {
    CallEvent::Connected {
        room_id: room_id.to_owned(),
        participants: roster.now().await,
        trouble: roster.trouble(),
    }
}

/// Leave `current`, if there is one. Says whether there was.
///
/// A leave that fails is logged and otherwise treated as a leave. There is
/// nothing a person can do about it and nothing useful to retry: the
/// membership carries a dead man's switch, so the worst case is a ghost that
/// clears itself.
async fn leave<S: CallSession>(current: Option<Joined<S>>, budget: Duration) -> bool {
    let Some(Joined {
        room_id,
        session,
        publishing,
        watching,
    }) = current
    else {
        return false;
    };

    // Stopped before the leave rather than after it, so no frame is still
    // being pushed into a publication that is being torn down underneath it,
    // and no roster is reported for a call that is on its way out.
    drop(publishing);
    drop(watching);

    leave_session(&room_id, session, budget).await;
    true
}

/// Leave one call. Separate from [`leave`] because a join that got as far as
/// the membership and no further has a session to unwind and no task to stop.
async fn leave_session<S: CallSession>(room_id: &str, session: S, budget: Duration) {
    match tokio::time::timeout(budget, session.leave()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%room_id, %error, "leaving the call failed; treating it as left anyway")
        }
        Err(_) => {
            tracing::warn!(%room_id, "leaving the call did not finish in time; giving up on it")
        }
    }
}

/// Report a failure on the event channel and in the log.
///
/// Both, because they are read by different people at different times. The
/// event is what somebody sees now; the log is what explains it later.
fn fail(events: &UnboundedSender<CallEvent>, room_id: String, failure: CallFailure) {
    tracing::warn!(%room_id, %failure, "could not join the call");
    emit(
        events,
        CallEvent::Failed {
            room_id,
            error: failure.to_string(),
        },
    );
}

/// Post an event, ignoring a receiver that has already gone.
fn emit(events: &UnboundedSender<CallEvent>, event: CallEvent) {
    let _ = events.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use consort_matrix::Participant;
    use tokio::sync::watch;

    use crate::publish::PublishedAudio;

    /// What the fake transport was asked to do, in order.
    ///
    /// Shared rather than returned, because the loop owns the transport and a
    /// test has to be able to look at it afterwards.
    #[derive(Clone, Default)]
    struct Log {
        joined: Arc<Mutex<Vec<String>>>,
        left: Arc<Mutex<Vec<String>>>,
        published: Arc<Mutex<Vec<String>>>,
        /// Publications not yet dropped. The task carrying the microphone owns
        /// one, so this falling back to zero is how a test sees that task end.
        live: Arc<AtomicUsize>,
    }

    impl Log {
        fn joined(&self) -> Vec<String> {
            self.joined.lock().unwrap().clone()
        }

        fn left(&self) -> Vec<String> {
            self.left.lock().unwrap().clone()
        }

        fn published(&self) -> Vec<String> {
            self.published.lock().unwrap().clone()
        }

        fn live(&self) -> usize {
            self.live.load(Ordering::Relaxed)
        }
    }

    /// A publication that does nothing but count itself alive.
    ///
    /// What is being checked here is the call thread's bookkeeping. What the
    /// frames themselves do is `publish.rs`.
    struct FakeTrack {
        log: Log,
    }

    impl Drop for FakeTrack {
        fn drop(&mut self) {
            self.log.live.fetch_sub(1, Ordering::Relaxed);
        }
    }

    impl PublishedAudio for FakeTrack {
        async fn send(&self, _samples: Vec<i16>) -> Result<(), CallFailure> {
            Ok(())
        }
    }

    /// What publishing the microphone does.
    #[derive(Clone, Copy, PartialEq)]
    enum Publishing {
        Succeeds,
        /// The call was joined and then would not carry a microphone. A
        /// deployment whose SFU accepted the connection and refused the track.
        Fails,
    }

    /// What a join does.
    #[derive(Clone)]
    enum Joining {
        Succeeds,
        Fails(CallFailure),
        /// Never returns. Stands in for any of the several remote things a
        /// real join waits on refusing to answer.
        Hangs,
    }

    struct FakeTransport {
        log: Log,
        joining: Joining,
        leaving: Leaving,
        publishing: Publishing,
        /// Held by the transport rather than made per session, so a test can
        /// reach the roster of a call the loop is holding.
        roster: watch::Sender<Standing>,
    }

    /// What a leave does.
    #[derive(Clone, Copy, PartialEq)]
    enum Leaving {
        Succeeds,
        Fails,
        /// Never returns. A homeserver that accepted the connection and then
        /// stopped answering.
        Hangs,
    }

    impl FakeTransport {
        fn new(joining: Joining) -> (Self, Log) {
            let log = Log::default();
            (
                Self {
                    log: log.clone(),
                    joining,
                    leaving: Leaving::Succeeds,
                    publishing: Publishing::Succeeds,
                    roster: watch::channel((Vec::new(), None)).0,
                },
                log,
            )
        }

        fn whose_leaves(leaving: Leaving) -> (Self, Log) {
            let (mut transport, log) = Self::new(Joining::Succeeds);
            transport.leaving = leaving;
            (transport, log)
        }

        fn whose_microphone_is_refused() -> (Self, Log) {
            let (mut transport, log) = Self::new(Joining::Succeeds);
            transport.publishing = Publishing::Fails;
            (transport, log)
        }

        /// A call somebody is already in.
        fn whose_roster_holds(people: Vec<Participant>) -> (Self, Log) {
            let (mut transport, log) = Self::new(Joining::Succeeds);
            transport.roster = watch::channel((people, None)).0;
            (transport, log)
        }
    }

    impl CallTransport for FakeTransport {
        type Session = FakeSession;

        async fn join(&self, room_id: &str) -> Result<Self::Session, CallFailure> {
            self.log.joined.lock().unwrap().push(room_id.to_owned());

            match &self.joining {
                Joining::Succeeds => Ok(FakeSession {
                    room_id: room_id.to_owned(),
                    log: self.log.clone(),
                    leaving: self.leaving,
                    publishing: self.publishing,
                    roster: self.roster.clone(),
                }),
                Joining::Fails(failure) => Err(failure.clone()),
                Joining::Hangs => std::future::pending().await,
            }
        }
    }

    struct FakeSession {
        room_id: String,
        log: Log,
        leaving: Leaving,
        publishing: Publishing,
        /// The roster every view of this call reads from. A test pushes to the
        /// sender to make somebody arrive or leave.
        roster: watch::Sender<Standing>,
    }

    /// What a fake call currently is: who is in it, and what is wrong.
    ///
    /// One channel for the pair, matching the real one, where a roster change
    /// and an encryption report both wake the same watcher.
    type Standing = (Vec<Participant>, Option<String>);

    /// One view of a fake call.
    struct FakeRoster(watch::Receiver<Standing>);

    impl Roster for FakeRoster {
        async fn now(&self) -> Vec<Participant> {
            self.0.borrow().0.clone()
        }

        fn trouble(&self) -> Option<String> {
            self.0.borrow().1.clone()
        }

        async fn changed(&mut self) -> bool {
            self.0.changed().await.is_ok()
        }
    }

    /// Somebody in a call.
    fn person(name: &str) -> Participant {
        Participant {
            id: format!("@{}:example.org", name.to_lowercase()),
            name: name.to_owned(),
        }
    }

    impl CallSession for FakeSession {
        type Track = FakeTrack;
        type Roster = FakeRoster;

        fn roster(&self) -> Self::Roster {
            FakeRoster(self.roster.subscribe())
        }

        async fn publish_microphone(&self) -> Result<Self::Track, CallFailure> {
            self.log
                .published
                .lock()
                .unwrap()
                .push(self.room_id.clone());

            match self.publishing {
                Publishing::Succeeds => {
                    self.log.live.fetch_add(1, Ordering::Relaxed);
                    Ok(FakeTrack {
                        log: self.log.clone(),
                    })
                }
                Publishing::Fails => Err(CallFailure::NoTransport(
                    "the focus refused the microphone".to_owned(),
                )),
            }
        }

        async fn leave(self) -> Result<(), CallFailure> {
            self.log.left.lock().unwrap().push(self.room_id);

            match self.leaving {
                Leaving::Succeeds => Ok(()),
                Leaving::Fails => Err(CallFailure::Signalling("the leave did not land".to_owned())),
                Leaving::Hangs => std::future::pending().await,
            }
        }
    }

    const GENERAL: &str = "!general:example.org";
    const MUSIC: &str = "!music:example.org";

    fn connecting(room_id: &str) -> CallEvent {
        CallEvent::Connecting {
            room_id: room_id.to_owned(),
        }
    }

    fn connected(room_id: &str) -> CallEvent {
        CallEvent::Connected {
            room_id: room_id.to_owned(),
            participants: Vec::new(),
            trouble: None,
        }
    }

    fn connected_with(room_id: &str, participants: Vec<Participant>) -> CallEvent {
        CallEvent::Connected {
            room_id: room_id.to_owned(),
            participants,
            trouble: None,
        }
    }

    fn troubled(room_id: &str, trouble: &str) -> CallEvent {
        CallEvent::Connected {
            room_id: room_id.to_owned(),
            participants: Vec::new(),
            trouble: Some(trouble.to_owned()),
        }
    }

    /// Run the loop over `commands`, then shut it down, and collect what it
    /// said.
    ///
    /// Deterministic by construction: every command is queued before the loop
    /// starts and the last one ends it, so there is nothing to poll for and no
    /// sleep anywhere. `serve` owns the event sender and drops it on return,
    /// which is what lets the drain below terminate.
    async fn transcript(transport: FakeTransport, commands: Vec<Message>) -> Vec<CallEvent> {
        let (to_loop, inbox) = unbounded_channel();
        for command in commands {
            to_loop.send(command).unwrap();
        }
        to_loop.send(Message::Shutdown).unwrap();
        drop(to_loop);

        let (events, mut said) = unbounded_channel();
        // A `LocalSet`, because the loop spawns the task that carries the
        // microphone into one. The real thread builds its own; here the test's
        // runtime provides it.
        tokio::task::LocalSet::new()
            .run_until(serve(transport, inbox, events, Microphone::new()))
            .await;

        let mut transcript = Vec::new();
        while let Some(event) = said.recv().await {
            transcript.push(event);
        }
        transcript
    }

    fn connect_to(room_id: &str) -> Message {
        Message::Connect {
            room_id: room_id.to_owned(),
        }
    }

    #[tokio::test]
    async fn joining_says_it_is_working_before_it_says_it_worked() {
        // Both, in that order. A join is several remote round trips, and an
        // interface with nothing to show for them is an interface where the
        // click looks like it missed.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(said, vec![connecting(GENERAL), connected(GENERAL)]);
        assert_eq!(log.joined(), vec![GENERAL]);
    }

    #[tokio::test]
    async fn a_join_that_fails_says_which_room_and_why() {
        let (transport, _log) = FakeTransport::new(Joining::Fails(CallFailure::NoTransport(
            "no focus advertised".to_owned(),
        )));

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(said[0], connecting(GENERAL));
        let CallEvent::Failed { room_id, error } = &said[1] else {
            panic!("{said:?}");
        };
        assert_eq!(room_id, GENERAL);
        assert!(error.contains("no focus advertised"), "{error}");
    }

    #[tokio::test]
    async fn leaving_a_call_that_never_started_says_nothing() {
        // Not merely harmless. `Disconnected` is what the interface uses to
        // put a channel back to idle, and emitting one for a leave that left
        // nothing would let a stale message clear a call somebody is in.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![Message::Disconnect]).await;

        assert_eq!(said, vec![]);
        assert_eq!(log.left(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn leaving_a_call_leaves_it_and_says_so() {
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL), Message::Disconnect]).await;

        assert_eq!(
            said,
            vec![
                connecting(GENERAL),
                connected(GENERAL),
                CallEvent::Disconnected
            ]
        );
        assert_eq!(log.left(), vec![GENERAL]);
    }

    #[tokio::test]
    async fn moving_to_another_channel_leaves_the_first_one_first() {
        // The ordering is the assertion. Joining before leaving would publish
        // two live memberships at once, which is one person sitting in two
        // voice channels to everybody else in the space.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        assert_eq!(
            said,
            vec![
                connecting(GENERAL),
                connected(GENERAL),
                connecting(MUSIC),
                connected(MUSIC),
            ]
        );
        assert_eq!(log.joined(), vec![GENERAL, MUSIC]);
        // Twice: the move out of general, then the shutdown out of music.
        assert_eq!(log.left(), vec![GENERAL, MUSIC]);
    }

    #[tokio::test]
    async fn moving_between_channels_never_says_the_call_is_over() {
        // The one thing a reader of this channel is entitled to assume:
        // `Disconnected` means there is no call. Anything acting on it tears
        // something down, and what gets torn down here is the microphone, which
        // is shared with the settings screen and expensive to reopen. A
        // `Disconnected` between two connects would close the sound card
        // between two calls that are meant to be continuous.
        let (transport, _log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(
            transport,
            vec![connect_to(GENERAL), connect_to(MUSIC), connect_to(GENERAL)],
        )
        .await;

        assert!(
            !said.contains(&CallEvent::Disconnected),
            "a channel change reported the call as over: {said:?}"
        );
    }

    #[tokio::test]
    async fn a_channel_change_is_announced_before_the_old_call_is_left() {
        // Leaving waits on a homeserver and is bounded at `LEAVE_TIMEOUT`.
        // Saying `Connecting` only afterwards would leave the interface showing
        // the previous channel for up to five seconds after somebody clicked
        // away from it.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        let announced = said
            .iter()
            .position(|event| event == &connecting(MUSIC))
            .expect("the second connect was never announced");
        let joined = said
            .iter()
            .position(|event| event == &connected(MUSIC))
            .expect("the second call never connected");

        assert!(announced < joined, "{said:?}");
        assert_eq!(log.left().first().map(String::as_str), Some(GENERAL));
    }

    #[tokio::test]
    async fn asking_for_the_call_already_in_progress_does_not_rejoin_it() {
        // Rejoining would tear down a working call and build it again, which
        // is a gap in the audio for everybody, in answer to a click that
        // asked for nothing to change.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(GENERAL)]).await;

        assert_eq!(log.joined(), vec![GENERAL], "the call was rebuilt");
        // Once, on the way out. Nothing was torn down in between.
        assert_eq!(log.left(), vec![GENERAL]);
        // Said again rather than ignored: something asking twice may be
        // asking because it has lost track of where it is.
        assert_eq!(
            said,
            vec![connecting(GENERAL), connected(GENERAL), connected(GENERAL)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_join_that_never_answers_gives_up_rather_than_waiting_forever() {
        // Without this the channel shows "Connecting" until the application
        // is restarted, and the thread never reads another command, so
        // nothing can even leave.
        let (transport, log) = FakeTransport::new(Joining::Hangs);

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(log.joined(), vec![GENERAL]);
        let CallEvent::Failed { room_id, error } = &said[1] else {
            panic!("{said:?}");
        };
        assert_eq!(room_id, GENERAL);
        assert!(error.contains("30s"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn the_loop_still_takes_commands_after_a_join_times_out() {
        // The other half of the timeout being worth having. Giving up on the
        // join is only useful if what comes next is served.
        let (transport, log) = FakeTransport::new(Joining::Hangs);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        assert_eq!(log.joined(), vec![GENERAL, MUSIC]);
        assert_eq!(said.len(), 4, "{said:?}");
    }

    #[tokio::test]
    async fn a_leave_that_fails_is_still_a_leave() {
        // There is nothing a person can do about it and nothing useful to
        // retry: the membership carries a dead man's switch. Refusing to
        // move on would strand somebody in a call they have already left.
        let (transport, log) = FakeTransport::whose_leaves(Leaving::Fails);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        assert_eq!(log.left(), vec![GENERAL, MUSIC]);
        assert_eq!(said.last(), Some(&connected(MUSIC)), "{said:?}");
    }

    #[tokio::test]
    async fn shutting_down_leaves_the_call() {
        // The one that keeps a ghost out of the channel. A process that exits
        // without unwinding its membership sits in the roster until the
        // membership expires, which is measured in hours.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(log.left(), vec![GENERAL]);
    }

    #[tokio::test]
    async fn shutting_down_says_nothing_on_the_way_out() {
        // Whatever asked for the shutdown is already going away. A final
        // `Disconnected` would be an event with nobody left to read it.
        let (transport, _log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(said.last(), Some(&connected(GENERAL)), "{said:?}");
    }

    #[tokio::test]
    async fn losing_the_handle_without_a_shutdown_still_leaves_the_call() {
        // `Drop` sends `Shutdown`, so this is the path a panicking caller
        // takes rather than the ordinary one. The membership has to come down
        // either way.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);
        let (to_loop, inbox) = unbounded_channel();
        to_loop.send(connect_to(GENERAL)).unwrap();
        drop(to_loop);
        let (events, _said) = unbounded_channel();

        tokio::task::LocalSet::new()
            .run_until(serve(transport, inbox, events, Microphone::new()))
            .await;

        assert_eq!(log.left(), vec![GENERAL]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_leave_that_never_answers_is_given_up_on() {
        // Dropping the handle waits for this loop, so a leave with no bound
        // on it is an application that will not close. The membership is
        // expendable here and the exit is not: the dead man's switch clears a
        // half-finished leave on its own.
        let (transport, log) = FakeTransport::whose_leaves(Leaving::Hangs);

        let said = transcript(transport, vec![connect_to(GENERAL), Message::Disconnect]).await;

        assert_eq!(log.left(), vec![GENERAL]);
        assert_eq!(said.last(), Some(&CallEvent::Disconnected), "{said:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_disconnect_is_announced_without_waiting_for_the_homeserver() {
        // The bug this exists for: a homeserver that takes six seconds to
        // write a membership, against a budget of five, produced a disconnect
        // button that sat there doing nothing and then a person still shown in
        // the channel they had left. Half of that is the budget below. This
        // half is not making anybody watch it.
        let (transport, _log) = FakeTransport::whose_leaves(Leaving::Hangs);
        let (events, mut said) = unbounded_channel();
        let (post, inbox) = unbounded_channel();

        post.send(connect_to(GENERAL)).unwrap();
        post.send(Message::Disconnect).unwrap();
        drop(post);

        let started = tokio::time::Instant::now();
        let seen_at = tokio::task::LocalSet::new()
            .run_until(async {
                // Concurrently, because that is the only way the difference is
                // visible: reading the channel after the loop has finished
                // sees the same order either way. What changed is when the
                // reader could have known, and the reader here is a webview.
                let watching = async {
                    while let Some(event) = said.recv().await {
                        if event == CallEvent::Disconnected {
                            break;
                        }
                    }
                    tokio::time::Instant::now()
                };

                let (_, seen_at) =
                    tokio::join!(serve(transport, inbox, events, Microphone::new()), watching);
                seen_at
            })
            .await;

        assert!(
            seen_at.duration_since(started) < LEAVE_TIMEOUT,
            "the disconnect waited on the leave before admitting it had happened"
        );
    }

    #[test]
    fn a_leave_outlasts_one_attempt_at_the_request_it_is_waiting_for() {
        // The bridge gives a membership send fifteen seconds per attempt. A
        // budget under that does not bound a hang, it abandons requests that
        // were going to succeed, and an abandoned leave is a person left
        // sitting in a channel they are not in.
        assert!(
            LEAVE_TIMEOUT > Duration::from_secs(15),
            "a leave cannot land if it is given less than one attempt"
        );
    }

    #[test]
    fn closing_the_application_is_not_made_to_wait_that_long() {
        // The one place the membership really is the expendable half: dropping
        // the handle waits for the loop, so this budget is how long quitting
        // can take on a homeserver that has stopped answering.
        assert!(SHUTDOWN_LEAVE_TIMEOUT < LEAVE_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_leave_does_not_stop_the_next_call_from_starting() {
        let (transport, log) = FakeTransport::whose_leaves(Leaving::Hangs);

        let said = transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        assert_eq!(log.joined(), vec![GENERAL, MUSIC]);
        assert_eq!(said.last(), Some(&connected(MUSIC)), "{said:?}");
    }

    /// The thread itself, rather than the loop inside it.
    ///
    /// One test, because there is one thing to prove: that a command posted
    /// from another thread reaches the loop, and that dropping the handle
    /// unwinds the call before the thread ends. Everything else about the
    /// behaviour is settled above, without a thread in the way.
    #[test]
    fn a_spawned_thread_serves_commands_and_leaves_the_call_when_dropped() {
        let (transport, log) = FakeTransport::new(Joining::Succeeds);
        let (events, mut said) = unbounded_channel();

        let thread = CallThread::spawn(transport, events, Microphone::new());
        thread.connect(GENERAL.to_owned());
        // Dropping is what ends the thread, and its `Drop` joins, so by the
        // time this returns the loop has finished and every event is queued.
        drop(thread);

        let mut transcript = Vec::new();
        while let Ok(event) = said.try_recv() {
            transcript.push(event);
        }
        assert_eq!(transcript, vec![connecting(GENERAL), connected(GENERAL)]);
        assert_eq!(log.joined(), vec![GENERAL]);
        assert_eq!(log.left(), vec![GENERAL], "the membership was left behind");
    }

    #[test]
    fn commands_posted_after_the_thread_is_gone_are_dropped_rather_than_panicking() {
        // Reachable in the ordinary way: a sign-out drops the thread while a
        // click is still in flight through the command layer.
        let (transport, _log) = FakeTransport::new(Joining::Succeeds);
        let (events, _said) = unbounded_channel();
        let mut thread = CallThread::spawn(transport, events, Microphone::new());

        // Stand the handle down the way `Drop` does, then keep using it.
        let _ = thread.commands.send(Message::Shutdown);
        if let Some(join) = thread.join.take() {
            join.join().unwrap();
        }

        thread.connect(GENERAL.to_owned());
        thread.disconnect();
    }

    #[tokio::test]
    async fn joining_publishes_the_microphone() {
        // Being in a call nobody can hear you in is not what the click meant.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(said, vec![connecting(GENERAL), connected(GENERAL)]);
        assert_eq!(log.published(), vec![GENERAL]);
    }

    #[tokio::test]
    async fn a_call_that_will_not_carry_a_microphone_is_a_failed_join() {
        // Not a connected call with a caveat. `Connected` is what the panel
        // draws, and drawing it for a call this session is inaudible in is a
        // lie the interface then has to be corrected out of.
        let (transport, log) = FakeTransport::whose_microphone_is_refused();

        let said = transcript(transport, vec![connect_to(GENERAL)]).await;

        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(said[0], connecting(GENERAL));
        assert!(
            matches!(said[1], CallEvent::Failed { .. }),
            "expected a failure, got {:?}",
            said[1]
        );
        assert_eq!(
            log.left(),
            vec![GENERAL],
            "the membership was left published for a call nobody could reach"
        );
    }

    #[tokio::test]
    async fn asking_for_the_call_it_is_already_in_does_not_publish_a_second_time() {
        // The re-announce path. It exists so an interface that has lost track
        // of where it is can ask again, and asking again must not open a
        // second microphone.
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        transcript(transport, vec![connect_to(GENERAL), connect_to(GENERAL)]).await;

        assert_eq!(log.published(), vec![GENERAL]);
    }

    #[tokio::test]
    async fn moving_to_another_call_publishes_into_the_new_one() {
        let (transport, log) = FakeTransport::new(Joining::Succeeds);

        transcript(transport, vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

        assert_eq!(log.published(), vec![GENERAL, MUSIC]);
    }

    /// Let the local task queue run, without ever hanging on it.
    ///
    /// Aborting a task marks it; the runtime is what drops it, and it only
    /// gets the chance when whatever asked yields. A bounded number of yields
    /// gives it that chance and fails the test rather than waiting forever.
    async fn settle(log: &Log) {
        for _ in 0..8 {
            if log.live() == 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn leaving_stops_the_task_carrying_the_microphone() {
        // Nothing else would. It waits on a queue, and a microphone that has
        // been switched off stops filling that queue rather than closing it,
        // so the task would sit there for the life of the application.
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (transport, log) = FakeTransport::new(Joining::Succeeds);
                let (events, _said) = unbounded_channel();
                let microphone = Microphone::new();

                let joined =
                    connect(&transport, None, GENERAL.to_owned(), &events, &microphone).await;
                assert_eq!(log.live(), 1, "nothing was publishing");

                leave(joined, LEAVE_TIMEOUT).await;
                settle(&log).await;

                assert_eq!(
                    log.live(),
                    0,
                    "the microphone is still being pushed into a call that is over"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn a_join_that_fails_leaves_nothing_publishing() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (transport, log) = FakeTransport::whose_microphone_is_refused();
                let (events, _said) = unbounded_channel();
                let microphone = Microphone::new();

                let joined =
                    connect(&transport, None, GENERAL.to_owned(), &events, &microphone).await;

                assert!(joined.is_none());
                settle(&log).await;
                assert_eq!(log.live(), 0);
            })
            .await;
    }

    /// Who is in the call, and how that reaches the interface.
    mod roster {
        use super::*;

        /// Run the loop with the roster reachable, so a test can change it
        /// while a call is up.
        ///
        /// [`transcript`] cannot do this. It queues every command before the
        /// loop starts, and a `watch` receiver made after a send never sees
        /// that send as a change, so a roster pushed before the join is a
        /// roster nobody reports. Everything here awaits an event rather than
        /// sleeping, so it is as deterministic as `transcript` is.
        async fn driving<F, Fut>(transport: FakeTransport, act: F) -> Vec<CallEvent>
        where
            F: FnOnce(watch::Sender<Standing>, Driver) -> Fut,
            Fut: Future<Output = Driver>,
        {
            let roster = transport.roster.clone();
            let (to_loop, inbox) = unbounded_channel();
            let (events, said) = unbounded_channel();

            tokio::task::LocalSet::new()
                .run_until(async move {
                    let serving = tokio::task::spawn_local(serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                    ));

                    let driver = act(roster, Driver { to_loop, said }).await;

                    let Driver { to_loop, mut said } = driver;
                    to_loop.send(Message::Shutdown).unwrap();
                    drop(to_loop);
                    serving.await.unwrap();

                    let mut rest = Vec::new();
                    while let Some(event) = said.recv().await {
                        rest.push(event);
                    }
                    rest
                })
                .await
        }

        /// The two ends of the loop, handed to a test to drive by hand.
        struct Driver {
            to_loop: UnboundedSender<Message>,
            said: UnboundedReceiver<CallEvent>,
        }

        impl Driver {
            fn send(&self, message: Message) {
                self.to_loop.send(message).unwrap();
            }

            /// The next thing the loop says. Never sleeps, never polls.
            async fn next(&mut self) -> CallEvent {
                self.said.recv().await.expect("the loop stopped talking")
            }
        }

        #[tokio::test]
        async fn joining_reports_who_is_already_in_the_channel() {
            // Read before the watcher starts rather than waiting for the first
            // change, because the first change may be days away: a channel
            // three people are sitting in quietly would otherwise draw empty
            // for as long as nobody moved.
            let (transport, _log) =
                FakeTransport::whose_roster_holds(vec![person("Ada"), person("Bob")]);

            let said = transcript(transport, vec![connect_to(GENERAL)]).await;

            assert_eq!(
                said,
                vec![
                    connecting(GENERAL),
                    connected_with(GENERAL, vec![person("Ada"), person("Bob")]),
                ]
            );
        }

        #[tokio::test]
        async fn asking_for_the_call_it_is_already_in_answers_with_the_current_roster() {
            // The re-announce exists because the interface may have lost track
            // of where it is. Answering it with an empty roster would replace
            // one wrong picture with another.
            let (transport, _log) = FakeTransport::whose_roster_holds(vec![person("Ada")]);

            let said = transcript(transport, vec![connect_to(GENERAL), connect_to(GENERAL)]).await;

            assert_eq!(
                said,
                vec![
                    connecting(GENERAL),
                    connected_with(GENERAL, vec![person("Ada")]),
                    connected_with(GENERAL, vec![person("Ada")]),
                ]
            );
        }

        #[tokio::test]
        async fn somebody_arriving_is_reported() {
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            driving(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                roster.send((vec![person("Ada")], None)).unwrap();

                assert_eq!(
                    driver.next().await,
                    connected_with(GENERAL, vec![person("Ada")])
                );
                driver
            })
            .await;
        }

        #[tokio::test]
        async fn somebody_leaving_is_reported() {
            let (transport, _log) = FakeTransport::whose_roster_holds(vec![person("Ada")]);

            driving(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(
                    driver.next().await,
                    connected_with(GENERAL, vec![person("Ada")])
                );

                roster.send((Vec::new(), None)).unwrap();

                assert_eq!(driver.next().await, connected(GENERAL));
                driver
            })
            .await;
        }

        #[tokio::test]
        async fn audio_that_cannot_be_decrypted_is_said_out_loud() {
            // The failure phase 0 reproduced: every membership publishes, both
            // rosters are right, RTP flows, and neither side can decrypt a
            // word. Everything an interface normally draws says that call is
            // working, so this is the one thing standing between somebody and
            // an evening of checking their microphone.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);
            let _watching = transport.roster.subscribe();

            let after = driving(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                roster
                    .send((Vec::new(), Some("nobody can hear you".to_owned())))
                    .unwrap();
                driver
            })
            .await;

            assert_eq!(after, vec![troubled(GENERAL, "nobody can hear you")]);
        }

        #[tokio::test]
        async fn what_is_wrong_travels_with_who_is_in_the_call() {
            // Read together rather than separately, because two reads a moment
            // apart describe two instants and the interface would draw the
            // pair as one.
            let (transport, _log) = FakeTransport::whose_roster_holds(vec![person("Ada")]);
            transport
                .roster
                .send_replace((vec![person("Ada")], Some("no key".to_owned())));

            let said = transcript(transport, vec![connect_to(GENERAL)]).await;

            assert_eq!(
                said,
                vec![
                    connecting(GENERAL),
                    CallEvent::Connected {
                        room_id: GENERAL.to_owned(),
                        participants: vec![person("Ada")],
                        trouble: Some("no key".to_owned()),
                    },
                ]
            );
        }

        #[tokio::test]
        async fn a_call_that_has_been_left_reports_nothing_more_about_who_is_in_it() {
            // The watcher waits on something a leave does not close, so nothing
            // but its handle would ever end it. A roster arriving after a
            // disconnect would put a channel back on screen that somebody has
            // just left.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);
            // Held so the send below succeeds whatever the loop did with its
            // own view. Without it a `watch` with no receivers refuses the
            // send, and the test would pass by not testing anything.
            let _watching = transport.roster.subscribe();

            let after = driving(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                driver.send(Message::Disconnect);
                assert_eq!(driver.next().await, CallEvent::Disconnected);

                roster.send((vec![person("Ada")], None)).unwrap();
                driver
            })
            .await;

            assert_eq!(after, Vec::<CallEvent>::new());
        }

        #[tokio::test]
        async fn moving_to_another_channel_stops_reporting_the_old_one_s_roster() {
            // One watcher at a time. Two would report two channels as
            // connected, and whichever spoke last would be the one the
            // interface drew. Both calls here share one roster, so a watcher
            // left behind shows up as a second event naming the old channel.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);
            let _watching = transport.roster.subscribe();

            let after = driving(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                driver.send(connect_to(MUSIC));
                assert_eq!(driver.next().await, connecting(MUSIC));
                assert_eq!(driver.next().await, connected(MUSIC));

                roster.send((vec![person("Ada")], None)).unwrap();
                driver
            })
            .await;

            assert_eq!(after, vec![connected_with(MUSIC, vec![person("Ada")])]);
        }
    }
}
