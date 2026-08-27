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

use crate::failure::CallFailure;
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

    /// Wait for either of the two above to change.
    ///
    /// `false` once neither ever will again, which is a call that has ended
    /// underneath its watcher. Returning `true` forever in that case would be
    /// a task spinning on a call nobody is in.
    async fn changed(&mut self) -> bool;
}
