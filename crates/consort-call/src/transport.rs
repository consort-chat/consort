// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The seam between the call thread and whatever is actually carrying a call.
//!
//! One implementation talks to a real SFU through `matrix-rtc-livekit`. It
//! lives in [`crate::livekit`], is excluded from coverage, and is the only
//! part of this crate that links libwebrtc.
//!
//! The seam exists so that everything above it can be tested. A test that had
//! to stand up an SFU would not be run, and a call thread nobody tests is a
//! call thread whose reconnect logic is discovered by a person clicking a
//! channel twice.
//!
//! ## Why these are not object-safe
//!
//! [`CallTransport::join`] is an `async fn` in a trait, which cannot be put
//! behind `dyn`. That is fine here and deliberate: unlike the audio backends,
//! which are chosen at runtime because a test and the application need
//! different ones in the same binary, there is exactly one call transport per
//! build. [`crate::CallThread`] is generic over it, and the choice is made
//! once at the call site.

use consort_matrix::Participant;

use crate::event::SelfAudio;
use crate::failure::CallFailure;
use crate::hearing::Ears;
use crate::publish::PublishedAudio;

/// Something that can put this session into a call.
///
/// `Send + 'static` because the thread takes ownership of one. The session it
/// hands back is neither, on purpose: see [`CallSession`].
#[allow(
    async_fn_in_trait,
    reason = "the returned futures are used only on the call thread, which is \
              single-threaded by construction, so a Send bound would be a \
              promise nothing needs and the real implementation cannot keep"
)]
pub trait CallTransport: Send + 'static {
    /// A call this session is in.
    type Session: CallSession;

    /// Join the call in `room_id`.
    async fn join(&self, room_id: &str) -> Result<Self::Session, CallFailure>;
}

/// A call this session is currently in.
///
/// Not `Send`, and that is the whole reason the call thread exists. The real
/// one is built by `Call::join`, which uses `spawn_local` for its heartbeat
/// and its key distribution, so it can only exist inside a `LocalSet` on the
/// thread that made it. It cannot be held in shared application state and it
/// cannot be moved across an await in a Tauri command.
#[allow(
    async_fn_in_trait,
    reason = "same as CallTransport above: the session is pinned to the call \
              thread, so a Send bound is one no implementation can keep"
)]
pub trait CallSession {
    /// Who is in this call, as it changes. See [`Roster`].
    type Roster: Roster;

    /// The microphone publication this call hands back.
    ///
    /// `'static` follows from [`PublishedAudio`], and it is load bearing: the
    /// call thread moves one into a task of its own so the command loop stays
    /// answerable while a frame is in flight. That is also why this is not
    /// tied to the session's lifetime the way a borrow would be.
    type Track: PublishedAudio;

    /// Publish this session's microphone and hand back somewhere to push PCM.
    ///
    /// Separate from joining because they fail differently and because the
    /// call thread does something different with each: the session it holds,
    /// the publication it hands to a task.
    async fn publish_microphone(&self) -> Result<Self::Track, CallFailure>;

    /// Mute or unmute this session's own microphone at the transport.
    ///
    /// Not a description of the audio, and nothing on the frame path may ever
    /// call this: see the header of [`crate::publish`] for what happened the
    /// last time something did. This is a person pressing a button, which is
    /// why it arrives on the command channel with the joins and the leaves
    /// rather than beside the samples.
    ///
    /// Peers are told, by the transport, in the way every other client already
    /// understands. That is the reason to mute here rather than to stop
    /// pushing frames: a sender that goes quiet reads as a wedged client, and a
    /// muted one reads as somebody who muted.
    async fn set_muted(&self, muted: bool) -> Result<(), CallFailure>;

    /// Stop or resume receiving the audio of everybody else in the call.
    ///
    /// The other half of a mute button, and a separate concern from it: mute is
    /// about what leaves this machine, this is about what arrives. Nothing in
    /// MatrixRTC or LiveKit has a name for it, so it is built out of per-stream
    /// subscription state and it is this session's business alone.
    ///
    /// Whoever implements this owns keeping it true as people join. A person
    /// who deafened and then had somebody walk into the channel must not start
    /// hearing them.
    ///
    /// Telling anybody else is not part of this. See
    /// [`announce_self`](Self::announce_self).
    async fn set_deafened(&self, deafened: bool) -> Result<(), CallFailure>;

    /// Tell the other Consort clients in the call what this session is doing
    /// with its own audio.
    ///
    /// Separate from the two setters above because it is a different kind of
    /// act and because it does not correspond to either of them one for one.
    /// Muting is already broadcast by the SFU and needs nothing here; deafening
    /// and being away are invisible to everything in the stack and need all of
    /// it. Folding this into `set_deafened` meant that adding a second thing
    /// worth announcing had nowhere to go.
    ///
    /// Called on every roster change as well as on every button, because a
    /// person who walks in has missed everything said before they arrived.
    ///
    /// A failure is the implementation's to log. This is an icon on somebody
    /// else's screen: it is not worth ending a call over, and whatever was
    /// announced has already happened locally.
    async fn announce_self(&self, audio: SelfAudio) -> Result<(), CallFailure>;

    /// Play everybody else in this call into `ears`, and keep doing it as
    /// people come and go.
    ///
    /// Called on every roster change rather than once, so an implementation
    /// must be idempotent: somebody already being played is left strictly
    /// alone, because tearing a working audio path down and rebuilding it on
    /// every membership change is audible. [`crate::hearing::changes`] is the
    /// difference, as a value.
    ///
    /// Not `async` and not fallible. There is no answer a caller could act on:
    /// audio that cannot be played is not a reason to end a call, and a
    /// participant whose stream has not arrived yet is the ordinary case rather
    /// than a failure, because the next roster change asks again.
    fn listen(&self, ears: &Ears);

    /// Start watching who is in the call.
    ///
    /// Each call to this is an independent view of the same roster, so the
    /// thread can read one now and hand another to the task that follows it
    /// without either disturbing the other.
    fn roster(&self) -> Self::Roster;

    /// Leave the call, unwinding membership and the media session.
    ///
    /// By value, because leaving is the end of the session and anything that
    /// could use it afterwards would be using a call that is over.
    async fn leave(self) -> Result<(), CallFailure>;
}

/// Who is in a call, and a way to wait for that to change.
///
/// Two methods rather than a stream, because the two are asked at different
/// moments and one of them is not cheap: [`now`](Roster::now) resolves display
/// names, which is a read per person against the room's member store, and it
/// should happen when there is something new to report rather than on a
/// schedule.
///
/// This is the source the plan calls strictly better than reading room state,
/// and it is only better for one channel: the one this session is sitting in.
/// It comes from MatrixRTC signalling rather than from `m.room.state`, so it
/// is right in every dialect, including the ones where room state shows
/// nothing at all.
#[allow(
    async_fn_in_trait,
    reason = "same as the two traits above: awaited only on the call thread"
)]
pub trait Roster {
    /// Who is in the call right now, named for somebody to read.
    ///
    /// Per human rather than per membership: a person on a laptop and a phone
    /// is one entry. Empty is a real answer, for the moment between joining
    /// and this session's own membership coming back round.
    async fn now(&self) -> Vec<Participant>;

    /// What is wrong with this call's audio, if anything.
    ///
    /// Here rather than on a seam of its own because it changes for the same
    /// reasons the roster does and is drawn in the same place. Two streams
    /// would mean two watchers racing to describe one call, and whichever
    /// spoke last would win. See [`crate::trouble`] for what these sentences
    /// are and why there is only ever one.
    fn trouble(&self) -> Option<String>;

    /// Who is talking right now, by Matrix user ID.
    ///
    /// Reported separately from [`now`](Self::now) because it changes on a
    /// completely different timescale. People join a call a few times an hour;
    /// they start and stop talking several times a second, and answering this
    /// must not cost the member-store read that naming a roster does.
    ///
    /// Delivered by [`changed`](Self::changed) rather than polled, so this has
    /// no accessor: a value nobody asked for at the right moment is a value
    /// that is already wrong.
    async fn changed(&mut self) -> Option<Change>;
}

/// Why a roster watcher woke up.
///
/// Two kinds of change on one seam, because they come from one call and are
/// drawn in one place, but they are told apart here because they cost wildly
/// different amounts to act on. Collapsing them into a single "something
/// changed" would put a member-store read behind every syllable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Who is in the call, or what is wrong with it, is different now. Worth
    /// re-reading and re-drawing in full.
    Roster,
    /// Who is talking is different now, and nothing else is. Cheap: the user
    /// IDs are already known and nothing has to be named again.
    Speaking(Vec<String>),
}
