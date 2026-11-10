// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The seam between the call thread and whatever is carrying a call.
//!
//! The one real implementation is [`crate::livekit`], excluded from coverage
//! and the only part of this crate that links libwebrtc. Why the seam exists
//! and why these traits are not object-safe: `docs/CALLS.md`.

use consort_matrix::Participant;

use crate::camera::PictureSize;
use crate::detail::Wanted;
use crate::event::SelfAudio;
use crate::failure::CallFailure;
use crate::hearing::{Attached, Ears};
use crate::publish::PublishedAudio;
use crate::showing::PublishedVideo;
use crate::watching::Eyes;

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
/// Not `Send`, which is why the call thread exists: the real one is pinned to
/// the `LocalSet` that made it, so it cannot be held in shared state or moved
/// across an await in a Tauri command.
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
    /// `'static` is load bearing: the call thread moves one into a task of its
    /// own so the command loop stays answerable while a frame is in flight.
    type Track: PublishedAudio;

    /// The camera publication this call hands back, when there is one.
    type Video: PublishedVideo;

    /// The publication a shared screen's own sound goes out on.
    ///
    /// A second audio publication rather than the microphone's: see
    /// [`publish_share_audio`](Self::publish_share_audio).
    type ShareAudio: PublishedAudio;

    /// Publish this session's microphone and hand back somewhere to push PCM.
    ///
    /// Separate from joining because they fail differently.
    async fn publish_microphone(&self) -> Result<Self::Track, CallFailure>;

    /// Publish this session's camera and hand back somewhere to push frames.
    ///
    /// Not called at join, unlike the microphone: it is reached from the
    /// command channel, and [`retract_camera`](Self::retract_camera) takes it
    /// down.
    async fn publish_camera(&self, size: PictureSize) -> Result<Self::Video, CallFailure>;

    /// Take this session's camera down, so peers drop the stream and this
    /// session's own roster entry stops saying it is filming.
    ///
    /// Here rather than on the publication: retracting LiveKit's track handle
    /// left the camera on the engine's roster, which is where this session's
    /// own icon is read from, and only the session can reach the engine.
    /// Already retracted is success.
    async fn retract_camera(&self) -> Result<(), CallFailure>;

    /// Publish this session's screen and hand back somewhere to push frames.
    ///
    /// A separate method rather than a parameter because peers read the two as
    /// different streams and a session may have both up at once. `size` is what
    /// the capture is producing, which for a window is whatever size it is.
    async fn publish_screen(&self, size: PictureSize) -> Result<Self::Video, CallFailure>;

    /// Take this session's screen share down. See
    /// [`retract_camera`](Self::retract_camera), which this mirrors.
    async fn retract_screen(&self) -> Result<(), CallFailure>;

    /// Publish what the machine a share comes from is playing, and hand back
    /// somewhere to push PCM.
    ///
    /// A track of its own, never the microphone's. Mixed in, a share's sound
    /// could not be separated again: a listener who turned the sharer down
    /// would lose the share with them, and a microphone carrying what the
    /// speakers play feeds back. Peers read it as
    /// `MediaStreamKind::ScreenShareAudio`.
    async fn publish_share_audio(&self) -> Result<Self::ShareAudio, CallFailure>;

    /// Take a shared screen's sound down. See
    /// [`retract_screen`](Self::retract_screen), which this mirrors.
    async fn retract_share_audio(&self) -> Result<(), CallFailure>;

    /// Mute or unmute this session's own microphone at the transport.
    ///
    /// Nothing on the frame path may ever call this: see [`crate::publish`].
    /// Muted at the transport rather than going quiet, because a sender that
    /// goes quiet reads as a wedged client and a muted one reads as somebody
    /// who muted.
    async fn set_muted(&self, muted: bool) -> Result<(), CallFailure>;

    /// Stop or resume receiving the audio of everybody else in the call.
    ///
    /// About what arrives rather than what leaves. Nothing in MatrixRTC or
    /// LiveKit names it, so it is built from per-stream subscription state, and
    /// the implementation owns keeping it true as people join.
    async fn set_deafened(&self, deafened: bool) -> Result<(), CallFailure>;

    /// Tell the other Consort clients in the call what this session is doing
    /// with its own audio.
    ///
    /// Called on every roster change as well as every button, because somebody
    /// who walks in missed what was said before they arrived. Mute is not in
    /// here: the SFU already broadcasts it. A failure is the implementation's
    /// to log.
    async fn announce_self(&self, audio: SelfAudio) -> Result<(), CallFailure>;

    /// Play everybody else in this call into `ears`, and keep doing it as
    /// people come and go.
    ///
    /// Called on every roster change rather than once, so it must be
    /// idempotent: rebuilding a working audio path on every membership change
    /// is audible, and [`crate::hearing::changes`] is the difference as a
    /// value. The count it answers with is not a failure.
    fn listen(&self, ears: &Ears) -> Attached;

    /// Pull everybody else's camera and shared screen into `eyes`, and keep
    /// doing it as they switch them on and off.
    ///
    /// [`listen`](Self::listen)'s sibling, idempotent on the same terms.
    /// Reported the same way for issue #185: a camera switched on mid-call
    /// publishes one roster change and its track lands after it, and upstream
    /// inserts that subscription into a membership that already carries the
    /// stream, which publishes no roster, so nothing would ask again.
    fn watch(&self, eyes: &Eyes) -> Attached;

    /// Ask for everybody else's picture at the size it is drawn, under
    /// whatever cap was chosen for them.
    ///
    /// Restated on every roster change rather than set once, because a
    /// constraint is keyed by membership and somebody who rejoins arrives
    /// without theirs. A cap rather than a demand, so the transport may deliver
    /// less.
    fn request(&self, wanted: &Wanted);

    /// Start watching who is in the call.
    ///
    /// Each call is an independent view of the same roster.
    fn roster(&self) -> Self::Roster;

    /// Leave the call, unwinding membership and the media session.
    ///
    /// By value, because anything that could use the session afterwards would
    /// be using a call that is over.
    async fn leave(self) -> Result<(), CallFailure>;
}

/// Who is in a call, and a way to wait for that to change.
///
/// Two methods rather than a stream: `docs/CALLS.md` has why.
#[allow(
    async_fn_in_trait,
    reason = "same as the two traits above: awaited only on the call thread"
)]
pub trait Roster {
    /// Who is in the call right now, named for somebody to read.
    ///
    /// Per human rather than per membership. Empty is a real answer.
    async fn now(&self) -> Vec<Participant>;

    /// Which of the people in [`now`](Self::now) is this session's own user.
    ///
    /// A `member_id` is per device and a roster is per person, so the two
    /// cannot be matched up further down: only this layer can say which entry
    /// is ours. `None` is treated as none of them.
    fn me(&self) -> Option<String>;

    /// What is wrong with this call's audio, if anything.
    ///
    /// Here rather than on a seam of its own, because it changes for the same
    /// reasons the roster does and two watchers would race to describe one
    /// call. See [`crate::trouble`].
    fn trouble(&self) -> Option<String>;

    /// Wait for anything about this call to be different.
    ///
    /// `Some` means re-read and redraw, `None` means the call is over. A bare
    /// wake-up rather than a description of what moved.
    async fn changed(&mut self) -> Option<()>;
}
