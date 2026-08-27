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
use crate::transport::{CallSession, CallTransport};

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
/// Shorter than a join, and for a different reason. Leaving is best effort
/// already: the membership carries a dead man's switch, so the worst a
/// half-finished leave costs is a ghost that clears itself. What it must not
/// cost is the application refusing to close, and it would, because dropping
/// the handle waits for this thread and this thread would be waiting on a
/// homeserver that is not answering.
pub const LEAVE_TIMEOUT: Duration = Duration::from_secs(5);

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
                if leave(current.take()).await {
                    emit(&events, CallEvent::Disconnected);
                }
            }
            Message::Shutdown => {
                // No `Disconnected` on the way out. Whatever asked for this is
                // already gone, and an event nobody is left to receive is not
                // worth the risk of the receiver having been dropped first.
                leave(current.take()).await;
                return;
            }
        }
    }

    // The handle was dropped without a `Shutdown`, which `Drop` does not do
    // but a panicking caller can. Unwind anyway.
    leave(current.take()).await;
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
    if current
        .as_ref()
        .is_some_and(|joined| joined.room_id == room_id)
    {
        emit(events, CallEvent::Connected { room_id });
        return current;
    }

    // Leave first, and say so. Two channels' worth of membership at once is a
    // person appearing to sit in two calls, which is worse than a moment of
    // showing neither.
    if leave(current).await {
        emit(events, CallEvent::Disconnected);
    }

    emit(
        events,
        CallEvent::Connecting {
            room_id: room_id.clone(),
        },
    );

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
            leave_session(&room_id, session).await;
            fail(events, room_id, failure);
            return None;
        }
    };

    // Its own task, so a frame waiting on the SFU does not sit in front of the
    // click that disconnects. `spawn_local` rather than `spawn` because this
    // whole thread is one `LocalSet` and the publication came out of a session
    // that cannot leave it.
    let publishing = AbortOnDrop(tokio::task::spawn_local(pump(track, microphone.clone())));

    emit(
        events,
        CallEvent::Connected {
            room_id: room_id.clone(),
        },
    );
    Some(Joined {
        room_id,
        session,
        publishing,
    })
}

/// Leave `current`, if there is one. Says whether there was.
///
/// A leave that fails is logged and otherwise treated as a leave. There is
/// nothing a person can do about it and nothing useful to retry: the
/// membership carries a dead man's switch, so the worst case is a ghost that
/// clears itself.
async fn leave<S: CallSession>(current: Option<Joined<S>>) -> bool {
    let Some(Joined {
        room_id,
        session,
        publishing,
    }) = current
    else {
        return false;
    };

    // Stopped before the leave rather than after it, so no frame is still
    // being pushed into a publication that is being torn down underneath it.
    drop(publishing);

    leave_session(&room_id, session).await;
    true
}

/// Leave one call. Separate from [`leave`] because a join that got as far as
/// the membership and no further has a session to unwind and no task to stop.
async fn leave_session<S: CallSession>(room_id: &str, session: S) {
    match tokio::time::timeout(LEAVE_TIMEOUT, session.leave()).await {
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
        fn set_muted(&self, _muted: bool) -> Result<(), CallFailure> {
            Ok(())
        }

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
    }

    impl CallSession for FakeSession {
        type Track = FakeTrack;

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
                CallEvent::Disconnected,
                connecting(MUSIC),
                connected(MUSIC),
            ]
        );
        assert_eq!(log.joined(), vec![GENERAL, MUSIC]);
        // Twice: the move out of general, then the shutdown out of music.
        assert_eq!(log.left(), vec![GENERAL, MUSIC]);
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

                leave(joined).await;
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
}
