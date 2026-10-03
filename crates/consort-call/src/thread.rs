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

use std::future::Future;
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::arrivals::Arrivals;
use crate::camera::{Camera, PictureSize};
use crate::event::{CallEvent, ScreenShare, SelfAudio, SelfScreen, SelfVideo};
use crate::failure::CallFailure;
use crate::hearing::{Attached, Cue, Ears};
use crate::microphone::Microphone;
use crate::publish::pump;
use crate::showing::{self, PublishedVideo};
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

/// How long to wait before asking again for a track that has not arrived.
///
/// Short enough that nobody notices the gap in a conversation. The gap between
/// an announce and a subscription on a healthy SFU is well under a second, so
/// most of the time one of these is all that is spent.
pub const ATTACH_RETRY: Duration = Duration::from_millis(500);

/// How many times to ask. Past this, the track is not coming.
///
/// Ten seconds of chasing, at [`ATTACH_RETRY`] apart. A judgement rather than a
/// measurement, and the bound exists so that somebody publishing a stream this
/// client can never subscribe to does not leave a timer running for the whole
/// of the call.
pub const ATTACH_ATTEMPTS: u32 = 20;

/// What the thread accepts.
enum Message {
    Connect {
        room_id: String,
    },
    Disconnect,
    /// Mute or unmute this session's microphone.
    SetMuted(bool),
    /// Stop or resume receiving everybody else's audio.
    SetDeafened(bool),
    SetAway(bool),
    /// Publish this session's screen, or retract it.
    ///
    /// Carries the title as well as the size, because the indicator has to
    /// name what is going out from the moment it starts rather than a frame
    /// later. See [`ScreenShare`].
    SetScreen(Option<ScreenShare>),
    /// Publish this session's camera at this size, or retract it.
    ///
    /// The size rather than a bare switch, because the device is opened before
    /// anything is published and the transport needs the negotiated size to
    /// set its encoder up. See [`PictureSize`].
    SetCamera(Option<PictureSize>),
    Shutdown,
}

// The loop holds a [`SelfAudio`] rather than [`Joined`] doing it, because it
// outlives a call. Somebody who muted themselves and then clicked a different
// voice channel has not asked to be heard again, and a mute button that
// silently releases itself when you move is a mute button nobody can trust.

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
    /// `microphone` is where captured audio arrives from, `camera` and
    /// `screen` are where captured frames do, and `ears` is where everybody
    /// else's audio goes. All of them are taken here rather than at each
    /// connect because the threads filling them have to be able to hold the
    /// other end whether or not a call is up: they are started once, and what
    /// is between them outlives any one call.
    ///
    /// `camera` and `screen` are two queues rather than one because both can
    /// be publishing at the same time.
    pub fn spawn<T: CallTransport>(
        transport: T,
        events: UnboundedSender<CallEvent>,
        microphone: Microphone,
        camera: Camera,
        screen: Camera,
        ears: Ears,
    ) -> Self {
        let (commands, inbox) = unbounded_channel::<Message>();

        let join = std::thread::Builder::new()
            .name("consort-call".to_owned())
            .spawn(move || run(transport, inbox, events, microphone, camera, screen, ears))
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

    /// Mute or unmute this session's microphone.
    ///
    /// Remembered across calls, so this can be pressed before joining one and
    /// is still true after switching channels.
    pub fn set_muted(&self, muted: bool) {
        self.send(Message::SetMuted(muted));
    }

    /// Stop or resume receiving the audio of everybody else in the call.
    ///
    /// Mutes on the way, and unmuting is not implied on the way back: somebody
    /// who was muted before they deafened stays muted after, which is what the
    /// button they did not press means.
    pub fn set_deafened(&self, deafened: bool) {
        self.send(Message::SetDeafened(deafened));
    }

    /// Say that nobody is at this computer.
    ///
    /// Mutes, like deafening does, and unlike deafening leaves everybody else
    /// audible: walking away and still hearing your name is the reason to
    /// press this rather than to leave the channel.
    ///
    /// Remembered across calls like the other two, which is what makes it
    /// useful: somebody who marked themselves away and then had the call
    /// switch channels underneath them is still away.
    pub fn set_away(&self, away: bool) {
        self.send(Message::SetAway(away));
    }

    /// Publish this session's camera at `size`, or retract it with `None`.
    ///
    /// Not remembered across calls, unlike mute and deafen. A camera belongs to
    /// one call: see [`crate::SelfVideo`] for why moving channels starts with
    /// it off.
    pub fn set_camera(&self, size: Option<PictureSize>) {
        self.send(Message::SetCamera(size));
    }

    /// Publish this session's screen, or retract it with `None`.
    ///
    /// Not remembered across calls, for the reason
    /// [`set_camera`](Self::set_camera) is not: a share belongs to the call it
    /// was started in. See [`crate::SelfScreen`].
    pub fn set_screen(&self, share: Option<ScreenShare>) {
        self.send(Message::SetScreen(share));
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
    camera: Camera,
    screen: Camera,
    ears: Ears,
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
    runtime.block_on(local.run_until(serve(
        transport, inbox, events, microphone, camera, screen, ears,
    )));
}

/// A call this session is in, and the task carrying its microphone.
struct Joined<S: CallSession> {
    room_id: String,
    session: S,
    /// Ends when this is dropped. See [`AbortOnDrop`].
    publishing: AbortOnDrop,
    /// The task reporting who is in the call. Ends the same way.
    watching: AbortOnDrop,
    /// The camera, while one is up. `None` for the whole of a call nobody
    /// switched a camera on in, which is most of them.
    showing: Option<Showing<S::Video>>,
    /// The screen share, while one is up. A second slot rather than a wider
    /// one, because both can be up at the same time.
    sharing: Option<Showing<S::Video>>,
    /// The retry looking for tracks that have not arrived yet. Dies with the
    /// call it belongs to, like the two tasks above.
    chasing: Chase,
}

/// The task waiting to ask again for a track that has not arrived, and how
/// many times it already has.
///
/// The count lives here rather than in the task because the task is replaced on
/// every attempt, and a count inside it would bound nothing.
#[derive(Default)]
struct Chase {
    attempts: u32,
    /// Ends when this is dropped. See [`AbortOnDrop`].
    #[expect(
        dead_code,
        reason = "held for its Drop, which is what stops the retry; reading it \
                  would be reading a JoinHandle nobody awaits"
    )]
    task: Option<AbortOnDrop>,
}

impl<S: CallSession> Joined<S> {
    /// Arm the retry, or stand it down.
    ///
    /// Stood down either because everything is attached or because the bound is
    /// spent, and only the second of those is worth saying out loud.
    fn chase(&mut self, attached: Attached, chase: &UnboundedSender<()>) {
        let Some(next) = chasing(attached, self.chasing.attempts, ATTACH_ATTEMPTS) else {
            if attached.pending > 0 {
                tracing::warn!(
                    room_id = %self.room_id,
                    pending = attached.pending,
                    "gave up on audio whose track never arrived"
                );
            }
            self.chasing = Chase::default();
            return;
        };

        let chase = chase.clone();
        self.chasing = Chase {
            attempts: next,
            // Replacing the handle aborts whatever was already waiting, so
            // arming this twice over is the same as arming it once.
            task: Some(AbortOnDrop(tokio::task::spawn_local(async move {
                tokio::time::sleep(ATTACH_RETRY).await;
                let _ = chase.send(());
            }))),
        };
    }
}

/// A camera publication and the task feeding it.
///
/// The track is kept beside the task because retracting a publication and
/// stopping the frames going into it are two different acts: dropping the
/// handle ends the pump, and peers only drop the stream when something awaits
/// [`PublishedVideo::unpublish`].
struct Showing<V> {
    track: V,
    /// Ends when this is dropped. See [`AbortOnDrop`].
    #[expect(
        dead_code,
        reason = "held for its Drop, which is what stops the frames; reading it                   would be reading a JoinHandle nobody awaits"
    )]
    pump: AbortOnDrop,
}

/// A task that ends when this handle is dropped.
///
/// The task it holds waits on a queue, so nothing else would ever end it: a
/// microphone that has been switched off stops filling the queue rather than
/// closing it. Tying it to a handle means no path out of a call can forget.
///
/// Shared with [`crate::livekit`], whose per-participant audio pumps have the
/// same problem in the other direction: they wait on a frame stream that a
/// participant going quiet does not close.
pub(crate) struct AbortOnDrop(pub(crate) tokio::task::JoinHandle<()>);

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
    camera: Camera,
    screen: Camera,
    ears: Ears,
) {
    let mut current: Option<Joined<T::Session>> = None;
    let mut audio = SelfAudio::default();
    let mut video = SelfVideo::default();
    let mut sharing = SelfScreen::default();

    // How the roster watcher asks for this session's own audio state to be
    // pushed at the call again.
    //
    // Its own channel rather than another `Message`, because the sender goes to
    // a task this loop owns and a clone of the command sender would keep the
    // command channel open forever. `inbox.recv()` returning `None` is how a
    // dropped handle is noticed, and a loop holding its own sender would never
    // see it.
    let (restate, mut restated) = unbounded_channel::<()>();

    // How a retry asks for the attachment half to run again, with a channel of
    // its own for the reason `restate` has one: the sender goes to a task this
    // loop owns, and a clone of the command sender would keep `inbox.recv()`
    // from ever returning `None`.
    let (chase, mut chased) = unbounded_channel::<()>();

    loop {
        let message = tokio::select! {
            message = inbox.recv() => message,
            Some(()) = restated.recv() => {
                // Somebody joined or left. Deafening is per participant all
                // the way down, so a new arrival hears nothing about a
                // decision taken before they got here: without this, the one
                // thing deafen must never do, let somebody through, is exactly
                // what happens to whoever walks in next.
                apply_and_chase(current.as_mut(), audio, &ears, &chase).await;
                continue;
            }
            Some(()) = chased.recv() => {
                // The attachment half alone, never the whole of `apply`: a
                // retry must not re-announce this session to every peer in the
                // call on each attempt. See issue #157.
                let attached = attach(current.as_ref(), &ears);
                if let Some(joined) = current.as_mut() {
                    joined.chase(attached, &chase);
                }
                continue;
            }
        };
        let Some(message) = message else { break };

        match message {
            Message::Connect { room_id } => {
                // Whatever the last channel was still saying is not something
                // to hear in the new one. `connect` leaves the old call on the
                // way past, and its queued audio goes with it.
                ears.silence();
                current = connect(
                    &transport,
                    current,
                    room_id,
                    &events,
                    &microphone,
                    &restate,
                    &ears,
                )
                .await;
                // A camera does not follow a call the way a mute does. The old
                // publication went with the old session, and the new channel
                // starts with nothing published, so what the button shows has
                // to come back to off or it is claiming a camera that is not
                // running.
                video = show(&events, video, SelfVideo::default());
                camera.clear();
                // And the screen, for the same reason and a sharper one:
                // somebody who was showing their inbox in one channel has
                // certainly not asked to show it in the next.
                sharing = shared(&events, sharing, SelfScreen::default());
                screen.clear();
                // Re-applied rather than assumed. A new session starts unmuted
                // and undeafened however this one was left, so a person who
                // muted themselves in one channel and clicked another would
                // otherwise be live in it without touching anything.
                //
                // Not announced, because nothing changed. This is one half of a
                // state whose other half is already drawn, and repeating it as
                // part of joining would make it look like something a call
                // decides.
                apply_and_chase(current.as_mut(), audio, &ears, &chase).await;
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
                    // Before the leave, for the same reason the event is. The
                    // pump tasks stop when the session is dropped, but what
                    // they already queued would otherwise be played into the
                    // silence after the call, or into the next one.
                    ears.silence();
                    leave(Some(joined), LEAVE_TIMEOUT).await;
                    video = show(&events, video, SelfVideo::default());
                    camera.clear();
                    sharing = shared(&events, sharing, SelfScreen::default());
                    screen.clear();
                }
            }
            Message::SetMuted(muted) => {
                audio = announce(&events, audio, SelfAudio { muted, ..audio });
                apply_and_chase(current.as_mut(), audio, &ears, &chase).await;
            }
            Message::SetDeafened(deafened) => {
                audio = announce(&events, audio, SelfAudio { deafened, ..audio });
                apply_and_chase(current.as_mut(), audio, &ears, &chase).await;
            }
            Message::SetAway(away) => {
                // Only on the way back, and only from having actually been
                // away. Somebody pressing the button twice, or a client that
                // restates the flag it already had, is not somebody returning,
                // and "welcome back" said to a person who never left is the
                // kind of small wrongness that makes a whole feature feel
                // broken.
                let returning = audio.away && !away;
                audio = announce(&events, audio, SelfAudio { away, ..audio });
                apply_and_chase(current.as_mut(), audio, &ears, &chase).await;
                if returning {
                    ears.cue(Cue::Returned);
                }
            }
            Message::SetCamera(size) => {
                video = set_camera(&events, video, current.as_mut(), size, &camera).await;
            }
            Message::SetScreen(share) => {
                sharing = set_screen(&events, sharing, current.as_mut(), share, &screen).await;
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

/// Move to `next`, saying so only if it is different.
///
/// The interface draws these two, so a repeat is a redraw of what is already on
/// screen. Pressing mute twice is one thing somebody did twice; pressing it
/// once and being told twice is a flicker.
fn announce(events: &UnboundedSender<CallEvent>, current: SelfAudio, next: SelfAudio) -> SelfAudio {
    if next != current {
        emit(events, CallEvent::SelfAudio(next));
    }
    next
}

/// Move to `next`, saying so only if it is different.
///
/// [`announce`] for the camera, and separate from it because the two states are
/// independent and a change to one must not redraw the other.
fn show(events: &UnboundedSender<CallEvent>, current: SelfVideo, next: SelfVideo) -> SelfVideo {
    if next != current {
        emit(events, CallEvent::SelfVideo(next.clone()));
    }
    next
}

/// Put this session's camera up, or take it down, and say which happened.
///
/// Takes `&mut` on the call because this is the one command that changes what a
/// call is publishing rather than how it is configured.
///
/// A camera needs a call to be published into. Asking for one outside a call is
/// reported rather than remembered: the alternative is a button that latches on
/// and then silently publishes the moment somebody joins a channel, which is a
/// camera turning itself on.
async fn set_camera<S: CallSession>(
    events: &UnboundedSender<CallEvent>,
    current: SelfVideo,
    joined: Option<&mut Joined<S>>,
    size: Option<PictureSize>,
    camera: &Camera,
) -> SelfVideo {
    let Some(joined) = joined else {
        // Switching off with no call is the ordinary way a call ends, so it is
        // not worth a complaint. Switching on is.
        let trouble = size.map(|_| "there is no call to put a camera in".to_owned());
        return show(
            events,
            current,
            SelfVideo {
                camera: false,
                trouble,
            },
        );
    };

    // Split first, so the publish below and the slot it fills are two borrows
    // of two fields rather than two of `joined`.
    let Joined {
        session, showing, ..
    } = joined;

    match publication(showing, camera, size, |at| session.publish_camera(at)).await {
        Ok(camera) => show(
            events,
            current,
            SelfVideo {
                camera,
                trouble: None,
            },
        ),
        Err(error) => show(
            events,
            current,
            SelfVideo {
                camera: false,
                trouble: Some(error.to_string()),
            },
        ),
    }
}

/// Start this session's screen share, or stop it, and say which happened.
///
/// The twin of [`set_camera`], and for the same reasons: a share needs a call
/// to go into, and asking outside one is reported rather than remembered,
/// because a latched share would start publishing somebody's screen the moment
/// they clicked a channel.
async fn set_screen<S: CallSession>(
    events: &UnboundedSender<CallEvent>,
    current: SelfScreen,
    joined: Option<&mut Joined<S>>,
    share: Option<ScreenShare>,
    frames: &Camera,
) -> SelfScreen {
    let Some(joined) = joined else {
        // Stopping with no call is how every call ends. Starting is the only
        // half worth complaining about.
        let trouble = share.map(|_| "there is no call to share a screen into".to_owned());
        return shared(
            events,
            current,
            SelfScreen {
                sharing: None,
                trouble,
            },
        );
    };

    let (size, title) = match share {
        Some(ScreenShare { size, title }) => (Some(size), title),
        None => (None, String::new()),
    };

    // Split first, so the publish below and the slot it fills are two borrows
    // of two fields rather than two of `joined`.
    let Joined {
        session, sharing, ..
    } = joined;

    match publication(sharing, frames, size, |at| session.publish_screen(at)).await {
        Ok(true) => shared(
            events,
            current,
            SelfScreen {
                sharing: Some(title),
                trouble: None,
            },
        ),
        Ok(false) => shared(events, current, SelfScreen::default()),
        Err(error) => shared(
            events,
            current,
            SelfScreen {
                sharing: None,
                trouble: Some(error.to_string()),
            },
        ),
    }
}

/// [`show`] for the screen share. Separate so a change to one state does not
/// redraw the other.
fn shared(
    events: &UnboundedSender<CallEvent>,
    current: SelfScreen,
    next: SelfScreen,
) -> SelfScreen {
    if next != current {
        emit(events, CallEvent::SelfScreen(next.clone()));
    }
    next
}

/// Put a video publication up, or take it down, and say whether one is up.
///
/// Shared by the camera and the screen share, which differ only in which slot
/// they fill, which queue feeds them and what they are called. The lifecycle is
/// the same and the two ways of getting it wrong are both bad: a second
/// publication for one source, or a retraction that leaves peers holding a
/// decoder for a stream that will never carry another frame.
///
/// `size` of `None` means take it down. An already-absent publication is not a
/// failure, and an already-present one is not republished: the interface may be
/// asking because it lost track of what it is doing.
async fn publication<V, F, Fut>(
    slot: &mut Option<Showing<V>>,
    frames: &Camera,
    size: Option<PictureSize>,
    publish: F,
) -> Result<bool, CallFailure>
where
    V: PublishedVideo,
    F: FnOnce(PictureSize) -> Fut,
    Fut: Future<Output = Result<V, CallFailure>>,
{
    let Some(size) = size else {
        if let Some(showing) = slot.take()
            && let Err(error) = showing.track.unpublish().await
        {
            // Logged and no more. The frames have already stopped, because
            // dropping the handle aborted the pump, so the worst case is a
            // publication peers see as stalled until the call ends.
            tracing::warn!(%error, "could not retract a video publication");
        }
        // So that switching back on does not publish the last thing the source
        // showed before it stopped.
        frames.clear();
        return Ok(false);
    };

    if slot.is_some() {
        return Ok(true);
    }

    let track = publish(size).await?;
    let pump = AbortOnDrop(tokio::task::spawn_local(showing::pump(
        track.clone(),
        frames.clone(),
    )));
    *slot = Some(Showing { track, pump });
    Ok(true)
}

/// Push this session's mute and deafen state at the call it is in.
///
/// Nothing to do when there is no call, and that is not a failure: the buttons
/// work outside one, and what they set is applied at the next join.
///
/// A failure here is logged and no more. Both of these are indicators as much
/// as they are switches, and the honest thing to show is what was asked for:
/// tearing the call down over a mute that the SFU would not accept, or silently
/// snapping the button back after somebody pressed it, are both worse than a
/// line in the log.
async fn apply<S: CallSession>(
    current: Option<&Joined<S>>,
    audio: SelfAudio,
    ears: &Ears,
) -> Attached {
    let Some(joined) = current else {
        return Attached::default();
    };

    if let Err(error) = joined.session.set_muted(audio.microphone_off()).await {
        tracing::warn!(%error, muted = audio.microphone_off(), "could not mute the microphone");
    }
    if let Err(error) = joined.session.set_deafened(audio.deafened).await {
        tracing::warn!(%error, deafened = audio.deafened, "could not change what this session hears");
    }

    // After both setters, so that what is announced is what has been done
    // rather than what is about to be. Deafening and being away are invisible
    // to everything in the stack, so this is the only way anybody else learns
    // about either; mute is not in it, because the SFU already broadcasts that
    // and a second source for one fact is a disagreement waiting to happen.
    if let Err(error) = joined.session.announce_self(audio).await {
        tracing::warn!(%error, ?audio, "could not tell the call about this session's audio");
    }

    // Attached here rather than at the join, because at the join there is
    // usually nothing to attach to: the memberships are known before their
    // tracks are subscribed. This runs again on every roster change, which is
    // exactly when a track appears, and it leaves anybody already playing
    // alone.
    let attached = attach(Some(joined), ears);

    // Last, and after `set_deafened` rather than before it. Pausing the
    // subscriptions stops more audio arriving but takes a round trip to the
    // SFU to do it, and whatever is already queued would otherwise play out
    // underneath somebody who has just asked for quiet.
    if audio.deafened {
        ears.silence();
    }

    attached
}

/// Whether to ask again for a track that has not arrived, and the attempt
/// count that follows.
///
/// `None` means stand down: either everything is attached, or the bound is
/// spent and the track is not coming.
pub fn chasing(attached: Attached, spent: u32, limit: u32) -> Option<u32> {
    (attached.pending > 0 && spent < limit).then_some(spent + 1)
}

/// Push this session's state at the call, attach what has arrived, and arrange
/// to ask again for whatever has not.
///
/// One helper rather than two calls at each of five sites, because a site that
/// applied and did not chase would be issue #157 again for that one path.
async fn apply_and_chase<S: CallSession>(
    current: Option<&mut Joined<S>>,
    audio: SelfAudio,
    ears: &Ears,
    chase: &UnboundedSender<()>,
) {
    let Some(joined) = current else {
        return;
    };

    let attached = apply(Some(&*joined), audio, ears).await;
    joined.chase(attached, chase);
}

/// Attach the audio of everybody in `current` whose track has arrived.
///
/// Separate from [`apply`] because a retry must reach only this half: going
/// through the whole of `apply` would re-announce this session to every peer in
/// the call on every attempt. `None` is a retry whose send beat its own abort.
fn attach<S: CallSession>(current: Option<&Joined<S>>, ears: &Ears) -> Attached {
    current
        .map(|joined| joined.session.listen(ears))
        .unwrap_or_default()
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
    restate: &UnboundedSender<()>,
    ears: &Ears,
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
        restate.clone(),
        ears.clone(),
    )));

    Some(Joined {
        room_id,
        session,
        publishing,
        watching,
        // A join publishes a microphone and never a camera or a screen. See
        // `Message::SetCamera` and `Message::SetScreen`.
        showing: None,
        sharing: None,
        chasing: Chase::default(),
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
    restate: UnboundedSender<()>,
    ears: Ears,
) {
    // Per call, and that is what makes a channel switch silent: the people in
    // the channel just left are not people who left, and a set carried across
    // would announce every one of them as a departure and everybody in the new
    // channel as an arrival. See `Arrivals::settle`.
    let mut arrivals = Arrivals::new(roster.me());

    while roster.changed().await.is_some() {
        // Told before the roster is drawn, because the two are answers to
        // different questions and this one is the urgent half: somebody who
        // just walked in is audible until it is answered, and unheard until
        // their audio is attached.
        let _ = restate.send(());
        let said = connected(&room_id, &roster).await;

        // Diffed against the people rather than against the event.
        // `Connected` is re-emitted for reasons that are not arrivals: it
        // carries the call's trouble, which changes on its own, and a chime
        // for a refused media key would be the wrong sound for the wrong
        // thing.
        if let CallEvent::Connected { participants, .. } = &said {
            for movement in arrivals.settle(participants) {
                tracing::debug!(cue = ?movement.cue, who = ?movement.who, "the roster moved");
                ears.cue(movement.cue);
            }
        }

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
        showing,
        sharing,
        chasing,
    }) = current
    else {
        return false;
    };

    // Stopped before the leave rather than after it, so no frame is still
    // being pushed into a publication that is being torn down underneath it,
    // and no roster is reported for a call that is on its way out.
    drop(publishing);
    drop(watching);
    drop(chasing);
    // Dropped rather than retracted. Leaving takes every publication in the
    // call down with it, so an `unpublish` here would be a second request for
    // something the leave is about to do, on the budget the leave needs.
    //
    // Both, and this is the path a quit takes: `SHUTDOWN_LEAVE_TIMEOUT` ends
    // here, so a screen share that nothing else stopped stops here. See the
    // "Quitting has to leave the call" section of CLAUDE.md.
    drop(showing);
    drop(sharing);

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
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use consort_matrix::Participant;
    use tokio::sync::watch;

    use crate::camera::OutgoingPicture;
    use crate::hearing::Heard;
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
        /// What the session was last told about its microphone, and how many
        /// times it was told anything at all.
        ///
        /// The count matters as much as the value: mute is applied on joining
        /// as well as on pressing the button, and a test that only looked at
        /// the value could not tell a channel switch that carried the state
        /// over from one that quietly dropped it.
        muted: Arc<AtomicBool>,
        mutes: Arc<AtomicUsize>,
        deafened: Arc<AtomicBool>,
        /// What the session last announced to the rest of the call, and how
        /// many times it announced anything.
        ///
        /// The count is the interesting half, for the reason the mute count
        /// is: a newcomer only learns about an away flag set before they
        /// arrived because everybody re-announces on every roster change, so
        /// a test that only looked at the value could not tell that happening
        /// from it not.
        announced: Arc<Mutex<Option<SelfAudio>>>,
        announcements: Arc<AtomicUsize>,
        /// Every camera publication asked for, by size.
        cameras: Arc<Mutex<Vec<PictureSize>>>,
        /// Every screen publication asked for, by size. Separate from
        /// `cameras` because the two are different streams to a peer, and a
        /// test has to be able to tell a share from a camera.
        screens: Arc<Mutex<Vec<PictureSize>>>,
        /// Camera publications not yet dropped, and how many were retracted.
        ///
        /// Both, because they are different acts: the pump stops when the
        /// handle is dropped, and peers only drop the stream on a retraction.
        /// A test that watched one could not tell a camera switched off from a
        /// camera whose frames merely stopped.
        live_cameras: Arc<AtomicUsize>,
        retracted: Arc<AtomicUsize>,
        /// Frames the camera publication was handed.
        frames: Arc<AtomicUsize>,
        /// How many times the session was asked to play the call.
        ///
        /// Counted rather than recorded, because what matters is that it is
        /// asked again on every roster change: the tracks of people already in
        /// a call are subscribed after their memberships are known, so asking
        /// once at the join would attach to nobody.
        listens: Arc<AtomicUsize>,
        /// What each pass of `listen` reports, in order, the last entry
        /// standing for every pass after it. So one pending value is a track
        /// that never arrives, and empty is nothing ever pending.
        attachments: Arc<Mutex<VecDeque<Attached>>>,
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

        fn muted(&self) -> bool {
            self.muted.load(Ordering::Relaxed)
        }

        fn mutes(&self) -> usize {
            self.mutes.load(Ordering::Relaxed)
        }

        fn announced(&self) -> Option<SelfAudio> {
            *self.announced.lock().unwrap()
        }

        fn announcements(&self) -> usize {
            self.announcements.load(Ordering::Relaxed)
        }

        fn deafened(&self) -> bool {
            self.deafened.load(Ordering::Relaxed)
        }

        fn listens(&self) -> usize {
            self.listens.load(Ordering::Relaxed)
        }

        /// What this pass of `listen` reports, spending the script by one.
        fn attachment(&self) -> Attached {
            let mut scripted = self.attachments.lock().unwrap();
            let reported = scripted.front().copied().unwrap_or_default();
            if scripted.len() > 1 {
                scripted.pop_front();
            }
            reported
        }

        fn cameras(&self) -> Vec<PictureSize> {
            self.cameras.lock().unwrap().clone()
        }

        fn screens(&self) -> Vec<PictureSize> {
            self.screens.lock().unwrap().clone()
        }

        fn live_cameras(&self) -> usize {
            self.live_cameras.load(Ordering::Relaxed)
        }

        fn retracted(&self) -> usize {
            self.retracted.load(Ordering::Relaxed)
        }

        fn frames(&self) -> usize {
            self.frames.load(Ordering::Relaxed)
        }
    }

    /// Somewhere for a call's audio to go that a test can look inside.
    ///
    /// Only the silences are counted. What reaches the far end is `hearing.rs`
    /// and `ears.rs`; what this is for is the call thread's own decisions about
    /// when to throw the buffer away.
    #[derive(Clone, Default)]
    struct Deaf {
        silences: Arc<AtomicUsize>,
        cues: Arc<Mutex<Vec<Cue>>>,
    }

    impl Deaf {
        fn silences(&self) -> usize {
            self.silences.load(Ordering::Relaxed)
        }

        fn cues(&self) -> Vec<Cue> {
            self.cues.lock().unwrap().clone()
        }
    }

    impl Heard for Deaf {
        fn hear(&self, _who: &str, _samples: &[i16]) {}

        fn forget(&self, _who: &str) {}

        fn silence(&self) {
            self.silences.fetch_add(1, Ordering::Relaxed);
        }

        fn cue(&self, cue: Cue) {
            self.cues.lock().unwrap().push(cue);
        }

        fn attribute(&self, _whose: &[(String, String)]) {}
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

    /// A camera publication that counts itself and the frames it takes.
    ///
    /// `Arc` inside, so a clone is a second handle on one publication, which
    /// is what [`PublishedVideo`] requires and what the real one is.
    #[derive(Clone)]
    struct FakeCamera {
        log: Log,
        #[expect(
            dead_code,
            reason = "held so the publication is counted alive for as long as \
                      any handle on it is; reading it would prove nothing"
        )]
        alive: Arc<Alive>,
        /// Whether pushing a frame reports the publication gone.
        accepts: bool,
    }

    /// Counts one publication, however many handles there are on it.
    struct Alive(Log);

    impl Drop for Alive {
        fn drop(&mut self) {
            self.0.live_cameras.fetch_sub(1, Ordering::Relaxed);
        }
    }

    impl PublishedVideo for FakeCamera {
        fn send(&self, _picture: OutgoingPicture) -> Result<(), CallFailure> {
            if !self.accepts {
                return Err(CallFailure::NoTransport(
                    "the publication is gone".to_owned(),
                ));
            }
            self.log.frames.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn unpublish(&self) -> Result<(), CallFailure> {
            self.log.retracted.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    /// What publishing a camera does.
    #[derive(Clone, Copy, PartialEq)]
    enum Filming {
        Succeeds,
        /// The SFU took the microphone and refused the camera. A deployment
        /// with video disabled, or one out of publishing slots.
        Fails,
        /// The publication comes up and then will not take a frame.
        Stalls,
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
        filming: Filming,
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
                    filming: Filming::Succeeds,
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

        fn whose_camera(filming: Filming) -> (Self, Log) {
            let (mut transport, log) = Self::new(Joining::Succeeds);
            transport.filming = filming;
            (transport, log)
        }

        /// A call whose tracks arrive on the schedule in `attachments`.
        ///
        /// See `Log::attachments` for what the last entry means.
        fn whose_attachments(attachments: Vec<Attached>) -> (Self, Log) {
            let (transport, log) = Self::new(Joining::Succeeds);
            *log.attachments.lock().unwrap() = attachments.into();
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
                    filming: self.filming,
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
        filming: Filming,
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
    struct FakeRoster {
        standing: watch::Receiver<Standing>,
    }

    impl Roster for FakeRoster {
        fn me(&self) -> Option<String> {
            // The fake roster never puts this session in its own list, so
            // there is nothing to leave out. Said explicitly rather than left
            // to a default, because a `Roster` that answered wrongly here
            // would suppress a real arrival and the test would pass by not
            // testing anything.
            None
        }

        async fn now(&self) -> Vec<Participant> {
            self.standing.borrow().0.clone()
        }

        fn trouble(&self) -> Option<String> {
            self.standing.borrow().1.clone()
        }

        async fn changed(&mut self) -> Option<()> {
            self.standing.changed().await.ok()
        }
    }

    /// Somebody in a call.
    fn person(name: &str) -> Participant {
        Participant::named(format!("@{}:example.org", name.to_lowercase()), name)
    }

    impl CallSession for FakeSession {
        type Track = FakeTrack;
        type Video = FakeCamera;
        type Roster = FakeRoster;

        fn roster(&self) -> Self::Roster {
            FakeRoster {
                standing: self.roster.subscribe(),
            }
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

        async fn publish_camera(&self, size: PictureSize) -> Result<Self::Video, CallFailure> {
            self.log.cameras.lock().unwrap().push(size);

            if self.filming == Filming::Fails {
                return Err(CallFailure::NoTransport(
                    "the focus refused the camera".to_owned(),
                ));
            }

            self.log.live_cameras.fetch_add(1, Ordering::Relaxed);
            Ok(FakeCamera {
                log: self.log.clone(),
                alive: Arc::new(Alive(self.log.clone())),
                accepts: self.filming != Filming::Stalls,
            })
        }

        async fn publish_screen(&self, size: PictureSize) -> Result<Self::Video, CallFailure> {
            self.log.screens.lock().unwrap().push(size);

            if self.filming == Filming::Fails {
                return Err(CallFailure::NoTransport(
                    "the focus refused the screen".to_owned(),
                ));
            }

            self.log.live_cameras.fetch_add(1, Ordering::Relaxed);
            Ok(FakeCamera {
                log: self.log.clone(),
                alive: Arc::new(Alive(self.log.clone())),
                accepts: self.filming != Filming::Stalls,
            })
        }

        async fn set_muted(&self, muted: bool) -> Result<(), CallFailure> {
            self.log.muted.store(muted, Ordering::Relaxed);
            self.log.mutes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn announce_self(&self, audio: SelfAudio) -> Result<(), CallFailure> {
            *self.log.announced.lock().unwrap() = Some(audio);
            self.log.announcements.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn set_deafened(&self, deafened: bool) -> Result<(), CallFailure> {
            self.log.deafened.store(deafened, Ordering::Relaxed);
            Ok(())
        }

        fn listen(&self, _ears: &Ears) -> Attached {
            self.log.listens.fetch_add(1, Ordering::Relaxed);
            self.log.attachment()
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
        filmed(transport, commands, Camera::new()).await
    }

    /// [`transcript`], with the camera queue handed in so a test can prefill it
    /// or look at it afterwards.
    async fn filmed(
        transport: FakeTransport,
        commands: Vec<Message>,
        camera: Camera,
    ) -> Vec<CallEvent> {
        through(transport, commands, camera, Camera::new()).await
    }

    /// [`transcript`], with both frame queues handed in.
    async fn through(
        transport: FakeTransport,
        commands: Vec<Message>,
        camera: Camera,
        screen: Camera,
    ) -> Vec<CallEvent> {
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
            .run_until(serve(
                transport,
                inbox,
                events,
                Microphone::new(),
                camera,
                screen,
                Arc::new(Deaf::default()),
            ))
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
            .run_until(serve(
                transport,
                inbox,
                events,
                Microphone::new(),
                Camera::new(),
                Camera::new(),
                Arc::new(Deaf::default()),
            ))
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

                let (_, seen_at) = tokio::join!(
                    serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                        Camera::new(),
                        Camera::new(),
                        Arc::new(Deaf::default()),
                    ),
                    watching
                );
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

        let thread = CallThread::spawn(
            transport,
            events,
            Microphone::new(),
            Camera::new(),
            Camera::new(),
            Arc::new(Deaf::default()),
        );
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
        let mut thread = CallThread::spawn(
            transport,
            events,
            Microphone::new(),
            Camera::new(),
            Camera::new(),
            Arc::new(Deaf::default()),
        );

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

                let (restate, _restated) = unbounded_channel();
                let joined = connect(
                    &transport,
                    None,
                    GENERAL.to_owned(),
                    &events,
                    &microphone,
                    &restate,
                    &(Arc::new(Deaf::default()) as Ears),
                )
                .await;
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

                let (restate, _restated) = unbounded_channel();
                let joined = connect(
                    &transport,
                    None,
                    GENERAL.to_owned(),
                    &events,
                    &microphone,
                    &restate,
                    &(Arc::new(Deaf::default()) as Ears),
                )
                .await;

                assert!(joined.is_none());
                settle(&log).await;
                assert_eq!(log.live(), 0);
            })
            .await;
    }

    /// Muting this session, and deafening it.
    ///
    /// Two switches over one piece of state, which is why they are tested
    /// together: the interesting cases are all about what one does to the
    /// other, and about what survives a call ending.
    mod self_audio {
        use super::*;

        fn muted(muted: bool, deafened: bool) -> CallEvent {
            CallEvent::SelfAudio(SelfAudio {
                muted,
                deafened,
                away: false,
            })
        }

        /// Run `commands` and report what the session was left holding.
        async fn ending_with(commands: Vec<Message>) -> (Log, Vec<CallEvent>) {
            let (transport, log) = FakeTransport::new(Joining::Succeeds);
            let said = transcript(transport, commands).await;
            (log, said)
        }

        #[tokio::test]
        async fn muting_reaches_the_call() {
            let (log, said) = ending_with(vec![connect_to(GENERAL), Message::SetMuted(true)]).await;

            assert!(log.muted(), "the microphone was never muted");
            assert!(
                said.contains(&muted(true, false)),
                "nothing told the interface what happened: {said:?}"
            );
        }

        fn away(muted: bool, deafened: bool) -> CallEvent {
            CallEvent::SelfAudio(SelfAudio {
                muted,
                deafened,
                away: true,
            })
        }

        #[tokio::test]
        async fn being_away_mutes_the_microphone() {
            // The half of away that is not an icon. Nobody is at the keyboard,
            // so nothing said near it was said to the call.
            let (log, said) = ending_with(vec![connect_to(GENERAL), Message::SetAway(true)]).await;

            assert!(log.muted(), "away left the microphone live");
            assert_eq!(
                said.last(),
                Some(&away(false, false)),
                "the mute away implies is not one the person pressed: {said:?}"
            );
        }

        #[tokio::test]
        async fn being_away_leaves_everybody_else_audible() {
            // The entire difference from deafen, and the reason to press this
            // rather than to leave the channel: you can hear your name from
            // the next room and come back.
            let (log, _) = ending_with(vec![connect_to(GENERAL), Message::SetAway(true)]).await;

            assert!(
                !log.deafened(),
                "away stopped this session hearing the call"
            );
        }

        #[tokio::test]
        async fn being_away_is_announced_to_the_rest_of_the_call() {
            // Nothing in MatrixRTC or LiveKit has a name for it, so this
            // channel is the only way anybody else ever finds out. An away
            // flag nobody can see is mute with extra steps.
            let (log, _) = ending_with(vec![connect_to(GENERAL), Message::SetAway(true)]).await;

            assert_eq!(
                log.announced().map(|audio| audio.away),
                Some(true),
                "the call was never told"
            );
        }

        #[tokio::test]
        async fn coming_back_is_announced_too() {
            // The direction that is easy to leave out, and the one that leaves
            // a clock beside somebody who is sitting right there.
            let (log, said) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetAway(true),
                Message::SetAway(false),
            ])
            .await;

            assert_eq!(log.announced().map(|audio| audio.away), Some(false));
            assert!(!log.muted(), "coming back left the microphone muted");
            assert_eq!(said.last(), Some(&muted(false, false)));
        }

        #[tokio::test]
        async fn away_and_muted_are_remembered_separately() {
            // `microphone_off` collapses them for the one question it answers
            // and nothing else may. Somebody who muted, went away, and came
            // back asked for one of those to survive the other.
            let (log, said) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetMuted(true),
                Message::SetAway(true),
                Message::SetAway(false),
            ])
            .await;

            assert!(
                log.muted(),
                "coming back unmuted a microphone nobody unmuted"
            );
            assert_eq!(said.last(), Some(&muted(true, false)));
        }

        #[tokio::test]
        async fn away_survives_a_channel_switch() {
            // Somebody who marked themselves away and then had the call move
            // is still away. A new session starts with nothing set, so this
            // only holds because the state is re-applied on joining.
            let (log, _) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetAway(true),
                connect_to("!lounge:example.org"),
            ])
            .await;

            assert!(log.muted(), "the new channel had a live microphone in it");
            assert_eq!(log.announced().map(|audio| audio.away), Some(true));
        }

        #[tokio::test]
        async fn joining_announces_without_anybody_pressing_anything() {
            // The mechanism a newcomer is told by. A data message reaches
            // whoever is connected when it is sent, so somebody arriving
            // afterwards has missed every announcement made before they got
            // there. What saves it is that this runs on joining and on every
            // roster change, not only on a button, so everybody re-announces
            // whenever anybody walks in.
            let (log, _) = ending_with(vec![connect_to(GENERAL)]).await;

            assert_eq!(
                log.announced(),
                Some(SelfAudio::default()),
                "joining a call said nothing about this session's audio"
            );
        }

        #[tokio::test]
        async fn every_change_is_announced_rather_than_only_the_last() {
            let (log, _) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetAway(true),
                Message::SetDeafened(true),
            ])
            .await;

            assert!(
                log.announcements() >= 3,
                "only {} announcements for a join and two buttons",
                log.announcements()
            );
        }

        #[tokio::test]
        async fn deafening_mutes_the_microphone_too() {
            // Every client with both buttons does this, and it is the only
            // honest option: carrying on talking into a room you have stopped
            // listening to is not a state anybody means to be in.
            let (log, said) =
                ending_with(vec![connect_to(GENERAL), Message::SetDeafened(true)]).await;

            assert!(log.deafened());
            assert!(log.muted(), "deafening left the microphone live");
            assert_eq!(
                said.last(),
                Some(&muted(false, true)),
                "the mute it implies is not a mute the person pressed, and \
                 saying otherwise would leave the button stuck down after they \
                 undeafen"
            );
        }

        #[tokio::test]
        async fn undeafening_gives_back_the_microphone() {
            let (log, _) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetDeafened(true),
                Message::SetDeafened(false),
            ])
            .await;

            assert!(!log.deafened());
            assert!(!log.muted());
        }

        #[tokio::test]
        async fn undeafening_does_not_unmute_somebody_who_was_already_muted() {
            // The button they did not press. Handing the microphone back to
            // somebody who muted it before they deafened is putting them on
            // air without asking.
            let (log, _) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetMuted(true),
                Message::SetDeafened(true),
                Message::SetDeafened(false),
            ])
            .await;

            assert!(!log.deafened());
            assert!(log.muted(), "undeafening unmuted somebody who had muted");
        }

        #[tokio::test]
        async fn pressing_a_switch_twice_is_only_said_once() {
            let (_, said) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetMuted(true),
                Message::SetMuted(true),
            ])
            .await;

            let announcements = said.iter().filter(|event| **event == muted(true, false));
            assert_eq!(announcements.count(), 1);
        }

        #[tokio::test]
        async fn muting_carries_across_a_channel_switch() {
            // The regression worth guarding. Each call is a new session that
            // starts unmuted, so somebody who muted themselves and clicked
            // another channel would arrive in it live.
            let (log, _) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetMuted(true),
                connect_to(MUSIC),
            ])
            .await;

            assert!(
                log.muted(),
                "the second call started with a live microphone"
            );
            assert_eq!(log.joined(), vec![GENERAL, MUSIC]);
            assert_eq!(
                log.mutes(),
                3,
                "once on joining, once on pressing it, once on joining again. \
                 Anything less means a session was left to work out its own \
                 mute state, and a new one always decides it is unmuted"
            );
        }

        #[tokio::test]
        async fn muting_before_there_is_a_call_is_remembered_for_one() {
            // The buttons are drawn whether or not a call is up, and pressing
            // one with nothing to apply it to is not an error.
            let (log, _) = ending_with(vec![Message::SetMuted(true), connect_to(GENERAL)]).await;

            assert!(log.muted());
        }

        #[tokio::test]
        async fn deafening_is_pushed_again_when_somebody_joins() {
            // Deafening is per participant all the way down, so a new arrival
            // knows nothing about a decision taken before they got here.
            // Without the re-push they are simply audible, which is the one
            // thing this must never do.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);
            let roster = transport.roster.clone();

            let (to_loop, inbox) = unbounded_channel();
            to_loop.send(connect_to(GENERAL)).unwrap();
            to_loop.send(Message::SetDeafened(true)).unwrap();

            let (events, mut said) = unbounded_channel();
            tokio::task::LocalSet::new()
                .run_until(async {
                    let serving = tokio::task::spawn_local(serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                        Camera::new(),
                        Camera::new(),
                        Arc::new(Deaf::default()),
                    ));

                    // Somebody walks in after the decision. The watcher is
                    // what notices, and it is a task, so the loop only hears
                    // about it once both have had the chance to run.
                    for _ in 0..8 {
                        tokio::task::yield_now().await;
                    }
                    log.deafened.store(false, Ordering::Relaxed);
                    roster.send((vec![person("Ada")], None)).unwrap();
                    for _ in 0..8 {
                        tokio::task::yield_now().await;
                    }

                    to_loop.send(Message::Shutdown).unwrap();
                    drop(to_loop);
                    serving.await.unwrap();
                })
                .await;

            while said.recv().await.is_some() {}
            assert!(
                log.deafened(),
                "the new arrival was audible to somebody who had deafened"
            );
        }
    }

    /// Who is in the call, and how that reaches the interface.
    /// Yield until `reached` frames have been pushed, or give up.
    ///
    /// The pump is a `spawn_local` task, so it runs only when the loop awaits.
    /// [`transcript`] queues every command up front and shuts the loop down,
    /// which never gives the task a turn, so the frame tests drive
    /// [`set_camera`] and [`set_screen`] directly instead.
    async fn pushed(log: &Log, reached: usize) {
        for _ in 0..8 {
            if log.frames() >= reached {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    /// Join a call, with the plumbing the loop would otherwise own.
    ///
    /// In the parent module because both the camera tests and the screen
    /// tests drive a publication against a call they joined themselves.
    async fn joined(transport: &FakeTransport) -> Option<Joined<FakeSession>> {
        let (events, _said) = unbounded_channel();
        let (restate, _restated) = unbounded_channel();
        connect(
            transport,
            None,
            GENERAL.to_owned(),
            &events,
            &Microphone::new(),
            &restate,
            &(Arc::new(Deaf::default()) as Ears),
        )
        .await
    }

    mod the_screen {
        use super::*;

        const SIZE: PictureSize = PictureSize {
            width: 2560,
            height: 1440,
        };

        const WHAT: &str = "DP-0 (2560x1440)";

        fn share_on() -> Message {
            Message::SetScreen(Some(ScreenShare {
                size: SIZE,
                title: WHAT.to_owned(),
            }))
        }

        fn share_off() -> Message {
            Message::SetScreen(None)
        }

        fn sharing(what: Option<&str>) -> SelfScreen {
            SelfScreen {
                sharing: what.map(str::to_owned),
                trouble: None,
            }
        }

        /// The `selfScreen` events out of a transcript, in order.
        fn screens(said: &[CallEvent]) -> Vec<SelfScreen> {
            said.iter()
                .filter_map(|event| match event {
                    CallEvent::SelfScreen(screen) => Some(screen.clone()),
                    _ => None,
                })
                .collect()
        }

        #[tokio::test]
        async fn starting_one_publishes_a_screen_at_the_size_it_was_given() {
            // The size matters for the reason it does on a camera, and more
            // so: a window is whatever size that window happens to be rather
            // than one of a device's negotiated modes.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![connect_to(GENERAL), share_on()]).await;

            assert_eq!(log.screens(), vec![SIZE]);
            assert_eq!(screens(&said), vec![sharing(Some(WHAT))]);
        }

        #[tokio::test]
        async fn a_share_is_published_as_a_screen_rather_than_as_a_camera() {
            // The whole difference peers see. A screen published as a camera
            // is drawn in a face tile at the size of a thumbnail.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let _said = transcript(transport, vec![connect_to(GENERAL), share_on()]).await;

            assert_eq!(log.screens(), vec![SIZE]);
            assert_eq!(log.cameras(), Vec::new(), "it went up as a camera");
        }

        #[tokio::test]
        async fn what_is_being_shared_is_named_rather_than_only_flagged() {
            // The requirement from #70 that an indicator exists for: somebody
            // sharing has to be able to tell which window is going out.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![connect_to(GENERAL), share_on()]).await;

            assert_eq!(
                screens(&said)
                    .first()
                    .and_then(|screen| screen.sharing.clone()),
                Some(WHAT.to_owned())
            );
        }

        #[tokio::test]
        async fn stopping_retracts_the_publication() {
            // Retracted, not muted. A muted screen share leaves every peer
            // holding a decoder for a stream that will never carry another
            // frame, and leaves their interface saying somebody is sharing.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), share_on(), share_off()],
            )
            .await;

            assert_eq!(log.retracted(), 1);
            assert_eq!(screens(&said), vec![sharing(Some(WHAT)), sharing(None)]);
        }

        #[tokio::test]
        async fn stopping_stops_the_frames() {
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let _said = transcript(
                transport,
                vec![connect_to(GENERAL), share_on(), share_off()],
            )
            .await;

            assert_eq!(log.live_cameras(), 0, "something is still pushing frames");
        }

        #[tokio::test]
        async fn asking_outside_a_call_publishes_nothing_and_says_why() {
            // Not latched, for a sharper reason than the camera's: a share
            // that remembered this would start publishing somebody's screen
            // the moment they clicked any channel.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![share_on()]).await;

            assert_eq!(log.screens(), Vec::new());
            let reported = screens(&said);
            assert_eq!(reported.len(), 1);
            assert_eq!(reported[0].sharing, None);
            assert!(
                reported[0].trouble.is_some(),
                "a share that did not start has to say so"
            );
        }

        #[tokio::test]
        async fn stopping_outside_a_call_is_not_worth_a_complaint() {
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![share_off()]).await;

            assert_eq!(
                screens(&said),
                Vec::new(),
                "nothing changed, so nothing said"
            );
        }

        #[tokio::test]
        async fn a_refused_share_reports_the_reason_and_leaves_nothing_publishing() {
            let (transport, log) = FakeTransport::whose_camera(Filming::Fails);

            let said = transcript(transport, vec![connect_to(GENERAL), share_on()]).await;

            assert_eq!(log.live_cameras(), 0);
            let reported = screens(&said);
            assert_eq!(reported.len(), 1);
            assert_eq!(reported[0].sharing, None);
            assert!(
                reported[0].trouble.is_some(),
                "a refusal has to carry a reason"
            );
        }

        #[tokio::test]
        async fn asking_twice_publishes_once() {
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said =
                transcript(transport, vec![connect_to(GENERAL), share_on(), share_on()]).await;

            assert_eq!(log.screens(), vec![SIZE]);
            assert_eq!(
                screens(&said),
                vec![sharing(Some(WHAT))],
                "and said so once"
            );
        }

        #[tokio::test]
        async fn moving_to_another_channel_stops_the_share() {
            // The one that matters most. A share that followed somebody into
            // a new channel would be showing their screen to a different set
            // of people than the ones they agreed to show it to.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), share_on(), connect_to(MUSIC)],
            )
            .await;

            assert_eq!(screens(&said), vec![sharing(Some(WHAT)), sharing(None)]);
            assert_eq!(
                log.screens(),
                vec![SIZE],
                "and nothing was published in the new channel"
            );
        }

        #[tokio::test]
        async fn leaving_the_call_stops_the_share() {
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), share_on(), Message::Disconnect],
            )
            .await;

            assert_eq!(screens(&said), vec![sharing(Some(WHAT)), sharing(None)]);
            assert_eq!(log.live_cameras(), 0);
        }

        #[tokio::test]
        async fn shutting_down_stops_the_share() {
            // The quit path. `Message::Shutdown` is what
            // `AppState::leave_call_on_quit` reaches, and a share that
            // survived it would be a screen still going out after the window
            // is gone. See the "Quitting has to leave the call" section of
            // CLAUDE.md.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let _said = transcript(transport, vec![connect_to(GENERAL), share_on()]).await;

            // `transcript` queues a `Shutdown` after every command and the
            // loop returns only once it has run, so reaching here at all means
            // the shutdown happened.
            assert_eq!(log.live_cameras(), 0, "the share outlived the call thread");
            assert_eq!(log.left(), vec![GENERAL]);
        }

        #[tokio::test]
        async fn a_camera_and_a_share_can_be_up_at_once() {
            // Two publications, not one slot. Somebody presenting usually
            // wants their face in the call as well as their slides.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![
                    connect_to(GENERAL),
                    Message::SetCamera(Some(PictureSize {
                        width: 1280,
                        height: 720,
                    })),
                    share_on(),
                ],
            )
            .await;

            assert_eq!(log.cameras().len(), 1, "the camera did not go up");
            assert_eq!(log.screens().len(), 1, "the share did not go up");
            assert_eq!(screens(&said), vec![sharing(Some(WHAT))]);
            let video: Vec<_> = said
                .iter()
                .filter_map(|event| match event {
                    CallEvent::SelfVideo(video) => Some(video.camera),
                    _ => None,
                })
                .collect();
            assert_eq!(video, vec![true], "starting a share turned the camera off");
        }

        #[tokio::test]
        async fn stopping_a_share_leaves_the_camera_alone() {
            // The other half of the pair above, and the regression a single
            // slot would produce: stopping one taking the other down with it.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![
                    connect_to(GENERAL),
                    Message::SetCamera(Some(PictureSize {
                        width: 1280,
                        height: 720,
                    })),
                    share_on(),
                    share_off(),
                ],
            )
            .await;

            assert_eq!(log.retracted(), 1, "one retraction, the share's");
            let video: Vec<_> = said
                .iter()
                .filter_map(|event| match event {
                    CallEvent::SelfVideo(video) => Some(video.camera),
                    _ => None,
                })
                .collect();
            assert_eq!(video, vec![true], "the camera was taken down too");
        }

        #[tokio::test]
        async fn stopping_throws_away_the_frame_that_was_waiting() {
            // The privacy-relevant one, and sharper for a screen than for a
            // camera: the frame waiting when somebody stopped sharing is the
            // one they most want not sent when they start again.
            //
            // Driven directly rather than through `transcript`, because the
            // frame has to arrive between the stop and the restart and a
            // queued command list gives nothing a turn in between.
            let local = tokio::task::LocalSet::new();
            local
                .run_until(async {
                    let (transport, log) = FakeTransport::new(Joining::Succeeds);
                    let screen = Camera::new();
                    let mut call = joined(&transport).await;
                    let (events, _said) = unbounded_channel();

                    let on = || {
                        Some(ScreenShare {
                            size: SIZE,
                            title: WHAT.to_owned(),
                        })
                    };

                    set_screen(&events, SelfScreen::default(), call.as_mut(), on(), &screen).await;
                    screen.offer(frame_of(1));
                    set_screen(&events, sharing(Some(WHAT)), call.as_mut(), None, &screen).await;

                    set_screen(&events, SelfScreen::default(), call.as_mut(), on(), &screen).await;
                    pushed(&log, 1).await;

                    assert_eq!(
                        log.frames(),
                        0,
                        "a frame from before the share stopped was sent"
                    );
                })
                .await;
        }

        /// One frame, for the queue tests above.
        fn frame_of(nth: u8) -> OutgoingPicture {
            OutgoingPicture {
                width: 2,
                height: 2,
                y: vec![nth; 4],
                u: vec![128],
                v: vec![128],
                timestamp_us: i64::from(nth),
            }
        }
    }

    mod the_camera {
        use super::*;

        const SIZE: PictureSize = PictureSize {
            width: 1280,
            height: 720,
        };

        fn camera_on() -> Message {
            Message::SetCamera(Some(SIZE))
        }

        fn camera_off() -> Message {
            Message::SetCamera(None)
        }

        fn showing(camera: bool) -> SelfVideo {
            SelfVideo {
                camera,
                trouble: None,
            }
        }

        /// The `selfVideo` events out of a transcript, in order.
        fn video(said: &[CallEvent]) -> Vec<SelfVideo> {
            said.iter()
                .filter_map(|event| match event {
                    CallEvent::SelfVideo(video) => Some(video.clone()),
                    _ => None,
                })
                .collect()
        }

        fn frame() -> OutgoingPicture {
            OutgoingPicture {
                width: 2,
                height: 2,
                y: vec![1; 4],
                u: vec![128],
                v: vec![128],
                timestamp_us: 1,
            }
        }

        #[tokio::test]
        async fn switching_it_on_publishes_a_camera_at_the_size_it_was_given() {
            // The size matters: the transport sets its encoder and simulcast
            // layers up from it, so a publication at the wrong size is a
            // publication configured for frames that never arrive.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![connect_to(GENERAL), camera_on()]).await;

            assert_eq!(log.cameras(), vec![SIZE]);
            assert_eq!(video(&said), vec![showing(true)]);
        }

        #[tokio::test]
        async fn switching_it_off_retracts_the_publication() {
            // Retracted, not muted. Peers drop the stream and draw an avatar,
            // which is what `roster::camera_live` already reads.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), camera_on(), camera_off()],
            )
            .await;

            assert_eq!(log.retracted(), 1);
            assert_eq!(video(&said), vec![showing(true), showing(false)]);
        }

        #[tokio::test]
        async fn asking_for_a_camera_outside_a_call_publishes_nothing_and_says_why() {
            // Not latched. A button that remembered this would publish the
            // moment somebody joined any channel, which is a camera turning
            // itself on.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![camera_on()]).await;

            assert_eq!(log.cameras(), Vec::new());
            let reported = video(&said);
            assert_eq!(reported.len(), 1);
            assert!(!reported[0].camera);
            assert!(
                reported[0].trouble.is_some(),
                "a camera that did not come on has to say so"
            );
        }

        #[tokio::test]
        async fn switching_it_off_outside_a_call_is_not_worth_a_complaint() {
            // How every call ends: the interface puts the camera away, and
            // there is nothing wrong.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(transport, vec![camera_off()]).await;

            assert_eq!(video(&said), Vec::new(), "nothing changed, so nothing said");
        }

        #[tokio::test]
        async fn a_refused_camera_reports_the_reason_and_leaves_nothing_publishing() {
            let (transport, log) = FakeTransport::whose_camera(Filming::Fails);

            let said = transcript(transport, vec![connect_to(GENERAL), camera_on()]).await;

            assert_eq!(log.live_cameras(), 0);
            let reported = video(&said);
            assert_eq!(reported.len(), 1);
            assert!(!reported[0].camera);
            assert_eq!(
                reported[0].trouble.as_deref(),
                Some("no voice server would take this call: the focus refused the camera")
            );
        }

        #[tokio::test]
        async fn asking_twice_publishes_once() {
            // The interface may ask because it lost track of what it is doing.
            // A second publication would be a second camera in the call.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), camera_on(), camera_on()],
            )
            .await;

            assert_eq!(log.cameras(), vec![SIZE]);
            assert_eq!(video(&said), vec![showing(true)], "and said so once");
        }

        #[tokio::test]
        async fn moving_to_another_channel_puts_the_camera_away() {
            // A camera belongs to one call. The old publication went with the
            // old session, so a button still showing "on" would be claiming a
            // camera that is not running.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), camera_on(), connect_to(MUSIC)],
            )
            .await;

            assert_eq!(video(&said), vec![showing(true), showing(false)]);
            assert_eq!(
                log.cameras(),
                vec![SIZE],
                "and nothing published in the new one"
            );
        }

        #[tokio::test]
        async fn leaving_the_call_puts_the_camera_away() {
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let said = transcript(
                transport,
                vec![connect_to(GENERAL), camera_on(), Message::Disconnect],
            )
            .await;

            assert_eq!(video(&said), vec![showing(true), showing(false)]);
        }

        #[tokio::test]
        async fn leaving_the_call_stops_the_frames() {
            // The publication is dropped rather than retracted on a leave,
            // because leaving takes every publication down with it. What has
            // to be true either way is that nothing is still pushing frames.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);

            let _said = transcript(
                transport,
                vec![connect_to(GENERAL), camera_on(), Message::Disconnect],
            )
            .await;

            assert_eq!(log.live_cameras(), 0);
        }

        #[tokio::test]
        async fn queued_frames_reach_the_publication() {
            let local = tokio::task::LocalSet::new();
            local
                .run_until(async {
                    let (transport, log) = FakeTransport::new(Joining::Succeeds);
                    let camera = Camera::new();
                    camera.offer(frame());
                    let mut call = joined(&transport).await;
                    let (events, _said) = unbounded_channel();

                    set_camera(
                        &events,
                        SelfVideo::default(),
                        call.as_mut(),
                        Some(SIZE),
                        &camera,
                    )
                    .await;
                    pushed(&log, 1).await;

                    assert_eq!(log.frames(), 1);
                })
                .await;
        }

        #[tokio::test]
        async fn switching_it_off_stops_the_frames() {
            // Nothing else would. The pump waits on a slot, and a camera that
            // has been closed stops filling it rather than closing it, so the
            // task would sit there for the life of the application.
            let local = tokio::task::LocalSet::new();
            local
                .run_until(async {
                    let (transport, log) = FakeTransport::new(Joining::Succeeds);
                    let camera = Camera::new();
                    let mut call = joined(&transport).await;
                    let (events, _said) = unbounded_channel();

                    set_camera(
                        &events,
                        SelfVideo::default(),
                        call.as_mut(),
                        Some(SIZE),
                        &camera,
                    )
                    .await;
                    assert_eq!(log.live_cameras(), 1, "nothing was publishing");

                    set_camera(&events, showing(true), call.as_mut(), None, &camera).await;
                    camera.offer(frame());
                    pushed(&log, 1).await;

                    assert_eq!(log.frames(), 0, "a retracted camera is still being fed");
                    assert_eq!(log.live_cameras(), 0);
                })
                .await;
        }

        #[tokio::test]
        async fn switching_it_off_and_on_again_does_not_publish_what_it_saw_before() {
            // The privacy-relevant one. Somebody covers their camera and
            // switches it off, and the frame that was waiting when they did is
            // the one frame they least want sent when they switch it back on.
            let local = tokio::task::LocalSet::new();
            local
                .run_until(async {
                    let (transport, log) = FakeTransport::new(Joining::Succeeds);
                    let camera = Camera::new();
                    let mut call = joined(&transport).await;
                    let (events, _said) = unbounded_channel();

                    // On, then a frame arrives, then off before the pump has
                    // had a turn to take it.
                    set_camera(
                        &events,
                        SelfVideo::default(),
                        call.as_mut(),
                        Some(SIZE),
                        &camera,
                    )
                    .await;
                    camera.offer(frame());
                    set_camera(&events, showing(true), call.as_mut(), None, &camera).await;

                    set_camera(
                        &events,
                        SelfVideo::default(),
                        call.as_mut(),
                        Some(SIZE),
                        &camera,
                    )
                    .await;
                    pushed(&log, 1).await;

                    assert_eq!(
                        log.frames(),
                        0,
                        "a frame from before the camera went off was sent"
                    );
                })
                .await;
        }

        #[tokio::test]
        async fn a_publication_that_stops_taking_frames_does_not_end_the_call() {
            // The camera is the least important thing in a call. A publication
            // the SFU has given up on must not take the conversation with it.
            let (transport, log) = FakeTransport::whose_camera(Filming::Stalls);
            let camera = Camera::new();
            camera.offer(frame());

            let said = filmed(
                transport,
                vec![connect_to(GENERAL), camera_on(), Message::SetMuted(true)],
                camera.clone(),
            )
            .await;

            assert_eq!(log.frames(), 0);
            assert!(
                said.contains(&connected(GENERAL)),
                "the call should still be up: {said:?}"
            );
            assert!(log.muted(), "the loop stopped taking commands");
        }
    }

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
                        Camera::new(),
                        Camera::new(),
                        Arc::new(Deaf::default()),
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

        /// Like [`driving`], and reports what was played rather than what was
        /// said.
        ///
        /// A sibling rather than a parameter on the one above, because every
        /// other test there is about the event channel and threading an unused
        /// handle through all of them to serve these three would make the
        /// interesting ones harder to read.
        async fn chiming<F, Fut>(transport: FakeTransport, act: F) -> Vec<Cue>
        where
            F: FnOnce(watch::Sender<Standing>, Driver) -> Fut,
            Fut: Future<Output = Driver>,
        {
            let roster = transport.roster.clone();
            let (to_loop, inbox) = unbounded_channel();
            let (events, said) = unbounded_channel();
            let ears = Deaf::default();

            let heard = ears.clone();
            tokio::task::LocalSet::new()
                .run_until(async move {
                    let serving = tokio::task::spawn_local(serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                        Camera::new(),
                        Camera::new(),
                        Arc::new(heard),
                    ));

                    let Driver { to_loop, said } = act(roster, Driver { to_loop, said }).await;
                    to_loop.send(Message::Shutdown).unwrap();
                    drop(to_loop);
                    serving.await.unwrap();
                    drop(said);
                })
                .await;

            ears.cues()
        }

        #[tokio::test]
        async fn somebody_walking_in_is_heard() {
            // The whole feature. Without it the only way to know somebody
            // joined is to be looking at the right corner of the screen at the
            // moment they do, and the result is two people starting a sentence
            // at once.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);
            let _watching = transport.roster.subscribe();

            let played = chiming(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                // The empty roster the join reports, absorbed as the baseline.
                roster.send((Vec::new(), None)).unwrap();
                assert_eq!(driver.next().await, connected(GENERAL));

                roster.send((vec![person("Ada")], None)).unwrap();
                driver.next().await;
                driver
            })
            .await;

            assert_eq!(played, vec![Cue::Arrived]);
        }

        #[tokio::test]
        async fn joining_a_channel_that_already_has_people_in_it_is_silent() {
            // Four people already there is not four people arriving. The rule
            // most easily left out, and the one whose absence is loudest.
            let (transport, _log) =
                FakeTransport::whose_roster_holds(vec![person("Ada"), person("Bob")]);
            let _watching = transport.roster.subscribe();

            let played = chiming(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                driver.next().await;

                roster
                    .send((vec![person("Ada"), person("Bob")], None))
                    .unwrap();
                driver.next().await;
                driver
            })
            .await;

            assert!(played.is_empty(), "{played:?}");
        }

        #[tokio::test]
        async fn a_call_whose_trouble_changed_makes_no_sound() {
            // `Connected` carries the call's trouble, which changes for
            // reasons that have nothing to do with anybody moving. Diffing the
            // event rather than the people would play an arrival for a refused
            // media key.
            let (transport, _log) = FakeTransport::whose_roster_holds(vec![person("Ada")]);
            let _watching = transport.roster.subscribe();

            let played = chiming(transport, async |roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                driver.next().await;

                roster.send((vec![person("Ada")], None)).unwrap();
                driver.next().await;

                roster
                    .send((vec![person("Ada")], Some("no key".to_owned())))
                    .unwrap();
                driver.next().await;
                driver
            })
            .await;

            assert!(played.is_empty(), "{played:?}");
        }

        #[tokio::test]
        async fn coming_back_from_away_says_so() {
            // The one cue that does not come from the roster, because nobody
            // else's roster changed: it is this session putting its own flag
            // down. Driven from the message rather than from a diff for
            // exactly that reason.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let played = chiming(transport, async |_roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                driver.send(Message::SetAway(true));
                driver.next().await;
                driver.send(Message::SetAway(false));
                driver.next().await;
                driver
            })
            .await;

            assert_eq!(played, vec![Cue::Returned]);
        }

        #[tokio::test]
        async fn going_away_says_nothing() {
            // Only the return is worth a sound. Somebody walking away from the
            // keyboard is not there to hear it, and the people who are there
            // learn it from the icon.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let played = chiming(transport, async |_roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                driver.send(Message::SetAway(true));
                driver.next().await;
                driver
            })
            .await;

            assert!(played.is_empty(), "{played:?}");
        }

        #[tokio::test]
        async fn putting_down_a_flag_that_was_never_up_is_not_a_return() {
            // A client that restates the flag it already had, or a button
            // pressed twice. "Welcome back" said to somebody who never left is
            // the kind of small wrongness that makes a whole feature feel
            // broken, and it is the natural bug: the state is already there to
            // be ignored.
            let (transport, _log) = FakeTransport::new(Joining::Succeeds);

            let played = chiming(transport, async |_roster, mut driver| {
                driver.send(connect_to(GENERAL));
                assert_eq!(driver.next().await, connecting(GENERAL));
                assert_eq!(driver.next().await, connected(GENERAL));

                driver.send(Message::SetAway(false));
                // Nothing is emitted for a flag that did not move, so there is
                // no event of its own to wait for. A mute behind it is one the
                // loop must reach second, which makes it a barrier rather than
                // a sleep.
                driver.send(Message::SetMuted(true));
                driver.next().await;
                driver
            })
            .await;

            assert!(played.is_empty(), "{played:?}");
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

    /// Hearing the other people in the call.
    ///
    /// The half that was missing entirely: everything below the call thread
    /// subscribed to the other participants' tracks and nothing ever pulled a
    /// frame out of them, so every call was one-way. These are the call
    /// thread's part of the fix, which is knowing *when* to ask.
    mod hearing_the_call {
        use super::*;

        /// Run `commands` and report what the call was asked to play, and where.
        async fn ending_with(commands: Vec<Message>) -> (Log, Deaf) {
            let (transport, log) = FakeTransport::new(Joining::Succeeds);
            let ears = Deaf::default();

            let (to_loop, inbox) = unbounded_channel();
            for command in commands {
                to_loop.send(command).unwrap();
            }
            to_loop.send(Message::Shutdown).unwrap();
            drop(to_loop);

            let (events, _said) = unbounded_channel();
            tokio::task::LocalSet::new()
                .run_until(serve(
                    transport,
                    inbox,
                    events,
                    Microphone::new(),
                    Camera::new(),
                    Camera::new(),
                    Arc::new(ears.clone()),
                ))
                .await;

            (log, ears)
        }

        #[tokio::test]
        async fn joining_a_call_starts_playing_it() {
            let (log, _) = ending_with(vec![connect_to(GENERAL)]).await;

            assert!(
                log.listens() > 0,
                "the call was joined and nothing was ever asked to play it"
            );
        }

        #[tokio::test]
        async fn nothing_is_played_before_there_is_a_call_to_play() {
            let (log, _) = ending_with(vec![Message::SetMuted(true)]).await;

            assert_eq!(log.listens(), 0);
        }

        #[tokio::test]
        async fn every_roster_change_asks_again() {
            // The one that makes this work at all. A participant's membership
            // is known before their track is subscribed, so asking once at the
            // join attaches to nobody and the call is silent exactly as it was
            // before any of this existed.
            let (transport, log) = FakeTransport::new(Joining::Succeeds);
            let _watching = transport.roster.subscribe();
            let roster = transport.roster.clone();

            let (to_loop, inbox) = unbounded_channel();
            let (events, mut said) = unbounded_channel();

            tokio::task::LocalSet::new()
                .run_until(async move {
                    let serving = tokio::task::spawn_local(serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                        Camera::new(),
                        Camera::new(),
                        Arc::new(Deaf::default()),
                    ));

                    to_loop.send(connect_to(GENERAL)).unwrap();
                    said.recv().await;
                    said.recv().await;
                    let joined = log.listens();

                    roster.send((vec![person("Ada")], None)).unwrap();
                    // Awaiting the roster event this produces is what makes the
                    // count below deterministic rather than a race with the
                    // watcher task.
                    said.recv().await;

                    to_loop.send(Message::Shutdown).unwrap();
                    drop(to_loop);
                    serving.await.unwrap();

                    assert!(
                        log.listens() > joined,
                        "somebody arrived and nothing went looking for their audio"
                    );
                })
                .await;
        }

        #[tokio::test]
        async fn deafening_drops_what_is_already_queued() {
            // Pausing the subscriptions stops more arriving, but that is a
            // round trip to the SFU. Without this, the audio already buffered
            // plays out underneath somebody who has just asked for quiet.
            let (_, ears) =
                ending_with(vec![connect_to(GENERAL), Message::SetDeafened(true)]).await;

            assert!(ears.silences() > 0, "deafening left the buffer playing");
        }

        #[tokio::test]
        async fn undeafening_does_not_throw_away_what_has_just_arrived() {
            let (_, deafening) =
                ending_with(vec![connect_to(GENERAL), Message::SetDeafened(true)]).await;
            let (_, undeafening) = ending_with(vec![
                connect_to(GENERAL),
                Message::SetDeafened(true),
                Message::SetDeafened(false),
            ])
            .await;

            assert_eq!(
                deafening.silences(),
                undeafening.silences(),
                "undeafening threw away the first audio it was given"
            );
        }

        #[tokio::test]
        async fn leaving_a_call_drops_what_it_was_still_saying() {
            // The pumps stop when the session is dropped, but whatever they
            // already queued would otherwise play into the silence afterwards.
            let (_, ears) = ending_with(vec![connect_to(GENERAL), Message::Disconnect]).await;

            assert!(ears.silences() > 0);
        }

        #[tokio::test]
        async fn moving_channels_does_not_carry_the_old_one_s_audio_over() {
            // A word from the channel somebody just left, arriving in the one
            // they just joined.
            let (_, ears) = ending_with(vec![connect_to(GENERAL), connect_to(MUSIC)]).await;

            assert!(ears.silences() > 0);
        }

        #[tokio::test]
        async fn a_call_that_would_not_start_is_not_played() {
            let (transport, log) = FakeTransport::new(Joining::Fails(CallFailure::NoTransport(
                "no SFU".to_owned(),
            )));
            let (to_loop, inbox) = unbounded_channel();
            to_loop.send(connect_to(GENERAL)).unwrap();
            to_loop.send(Message::Shutdown).unwrap();
            drop(to_loop);

            let (events, _said) = unbounded_channel();
            tokio::task::LocalSet::new()
                .run_until(serve(
                    transport,
                    inbox,
                    events,
                    Microphone::new(),
                    Camera::new(),
                    Camera::new(),
                    Arc::new(Deaf::default()),
                ))
                .await;

            assert_eq!(log.listens(), 0);
        }
    }

    /// Chasing a track that arrives after the roster has stopped changing.
    ///
    /// The bug is issue #157: upstream inserts a late subscription without
    /// publishing a roster or emitting an event, so nothing asks again.
    mod chasing_a_late_track {
        use super::*;

        fn pending(pending: usize) -> Attached {
            Attached {
                playing: 0,
                pending,
            }
        }

        fn all_playing(playing: usize) -> Attached {
            Attached {
                playing,
                pending: 0,
            }
        }

        #[test]
        fn nothing_is_chased_once_everybody_is_attached() {
            // Which is the ordinary case. A timer left running for the life of
            // every call would be a cost paid by every call that works.
            assert_eq!(chasing(all_playing(2), 0, 20), None);
        }

        #[test]
        fn a_pending_track_is_chased() {
            assert_eq!(chasing(pending(1), 0, 20), Some(1));
        }

        #[test]
        fn chasing_stops_at_the_bound() {
            // Somebody publishing a stream this client can never subscribe to
            // must not produce a timer for the rest of the call.
            assert_eq!(chasing(pending(1), 20, 20), None);
        }

        /// Run the loop with the command sender held and the roster reachable.
        ///
        /// [`transcript`] cannot serve these: it queues `Shutdown` up front, so
        /// the loop drains and exits before any retry could land. Modelled on
        /// `roster::driving`, which stays alive for the same reason.
        async fn attaching<F, Fut>(transport: FakeTransport, act: F)
        where
            F: FnOnce(watch::Sender<Standing>, UnboundedSender<Message>) -> Fut,
            Fut: Future<Output = ()>,
        {
            let roster = transport.roster.clone();
            let (to_loop, inbox) = unbounded_channel();
            let (events, mut said) = unbounded_channel();

            tokio::task::LocalSet::new()
                .run_until(async move {
                    let serving = tokio::task::spawn_local(serve(
                        transport,
                        inbox,
                        events,
                        Microphone::new(),
                        Camera::new(),
                        Camera::new(),
                        Arc::new(Deaf::default()),
                    ));

                    act(roster, to_loop.clone()).await;

                    to_loop.send(Message::Shutdown).unwrap();
                    drop(to_loop);
                    serving.await.unwrap();
                    while said.recv().await.is_some() {}
                })
                .await;
        }

        /// Let the loop reach everything it has been given, then let `count`
        /// retry intervals pass.
        ///
        /// A barrier rather than a race: a paused clock only moves when every
        /// task is idle, so the loop is always done before the first interval.
        async fn retries_land(count: u32) {
            tokio::time::sleep(ATTACH_RETRY * count + Duration::from_millis(1)).await;
        }

        #[tokio::test(start_paused = true)]
        async fn a_track_that_lands_after_the_roster_settles_is_still_played() {
            // The stayer's half of #157. The roster change that announces a
            // publication is the last one there will be, and the handle lands
            // after it, so with nothing to ask again that person is silent for
            // the rest of the call.
            let (transport, log) = FakeTransport::whose_attachments(vec![
                Attached::default(),
                pending(1),
                Attached::default(),
            ]);
            // Held so the roster send below reaches the loop's own view rather
            // than being refused for want of a receiver.
            let _watching = transport.roster.subscribe();

            attaching(transport, async |roster, to_loop| {
                to_loop.send(connect_to(GENERAL)).unwrap();
                // Nothing is pending on the join's own pass, so this arms no
                // timer and only lets the loop get as far as the join.
                retries_land(1).await;

                roster.send((vec![person("Ada")], None)).unwrap();
                retries_land(2).await;
            })
            .await;

            assert_eq!(
                log.listens(),
                3,
                "the join, the roster change, and then nothing went looking for \
                 the track that arrived after it"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn a_fresh_join_chases_its_own_attachments() {
            // The joiner's half. The pass after the join is the only one a
            // joiner gets when nothing else ever changes, so the chase has to
            // be armed from there as well as from a roster change.
            let (transport, log) =
                FakeTransport::whose_attachments(vec![pending(1), Attached::default()]);

            attaching(transport, async |_roster, to_loop| {
                to_loop.send(connect_to(GENERAL)).unwrap();
                retries_land(2).await;
            })
            .await;

            assert_eq!(
                log.listens(),
                2,
                "a join that left a track pending never asked again"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn a_track_that_never_arrives_is_given_up_on() {
            // Somebody publishing a stream this client can never subscribe to
            // must not leave a timer running for the rest of the call.
            let (transport, log) = FakeTransport::whose_attachments(vec![pending(1)]);

            attaching(transport, async |_roster, to_loop| {
                to_loop.send(connect_to(GENERAL)).unwrap();
                retries_land(ATTACH_ATTEMPTS + 5).await;
            })
            .await;

            assert_eq!(
                log.listens(),
                ATTACH_ATTEMPTS as usize + 1,
                "the join's own pass and then the bound, and nothing after it"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn chasing_does_not_outlive_the_call() {
            // A retry that survived its call would attach the audio of the
            // channel just left into the one just joined.
            let (transport, log) =
                FakeTransport::whose_attachments(vec![pending(1), Attached::default()]);

            attaching(transport, async |_roster, to_loop| {
                // Queued together, so the loop reaches the disconnect while the
                // clock is still too early for the first retry to have fired.
                to_loop.send(connect_to(GENERAL)).unwrap();
                to_loop.send(Message::Disconnect).unwrap();
                to_loop.send(connect_to(MUSIC)).unwrap();
                retries_land(4).await;
            })
            .await;

            assert_eq!(
                log.listens(),
                2,
                "one pass per join, and a retry armed in general reached music"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn a_retry_does_not_re_announce_this_session() {
            // The single most likely way to make this fix worse than the bug.
            // `announce_self` goes out to every peer over the call's data
            // channel, and twenty of them is visible to other people.
            let (transport, log) = FakeTransport::whose_attachments(vec![pending(1)]);

            attaching(transport, async |_roster, to_loop| {
                to_loop.send(connect_to(GENERAL)).unwrap();
                retries_land(ATTACH_ATTEMPTS + 5).await;
            })
            .await;

            assert_eq!(
                log.listens(),
                ATTACH_ATTEMPTS as usize + 1,
                "nothing was retried, so this proves nothing about the retries"
            );
            assert_eq!(
                log.announcements(),
                1,
                "the join announced once and a retry announced again"
            );
        }
    }
}
