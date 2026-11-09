// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Being in a MatrixRTC voice call.
//!
//! `consort-matrix` can already say which channels are voice channels and who
//! is sitting in each one, which it reads out of room state without joining
//! anything. This crate is the other half: actually being one of those people.
//!
//! ## Why it is a crate of its own
//!
//! `matrix-rtc-livekit` brings libwebrtc, which is around eight gigabytes of
//! build output and several minutes of compile. Putting it in `consort-matrix`
//! would put it in the way of every `cargo test -p consort-matrix`, which is
//! the loop most of this project is developed in. Here it is out of the way,
//! and only the crates that genuinely need a call pay for it.
//!
//! It also does not hold the question that gates a call. Whether this session
//! can be heard at all is a cross-signing question with no LiveKit in it, so
//! it lives in `consort_matrix::calls::readiness` where it can be asked
//! without linking any of this.
//!
//! ## The shape
//!
//! [`CallThread`] owns the call, because `Call::join` pins its work to a
//! `LocalSet` and the result cannot leave the thread that made it.
//! [`CallTransport`] is the seam that lets everything above the SFU be tested
//! without one.
//!
//! Audio arrives through [`Microphone`], a bounded queue the audio thread
//! fills and the call thread drains. It is bounded because the two ends are
//! paced by different clocks and the producer must never be the one that
//! waits: a capture loop stalled on an SFU is a glitching microphone.
//!
//! Camera frames arrive through [`Camera`], which is one slot rather than a
//! queue. See that module for why a picture is not a sequence.
//!
//! ## The rustls provider, again
//!
//! Nothing here installs one, for the same reason `consort_matrix` does not.
//! With this crate in the graph the choice stops being cosmetic: both the
//! `aws-lc-rs` and `ring` backends end up compiled in, so a binary that does
//! not choose gets a panic on its first TLS connection rather than a default.
//! See `consort_matrix::install_crypto_provider`.

pub mod arrivals;
pub mod camera;
pub mod detail;
pub mod dialect;
pub mod discovery;
pub mod event;
pub mod failure;
pub mod hearing;
pub mod livekit;
pub mod microphone;
pub mod notices;
pub mod publish;
pub mod roster;
pub mod showing;
pub mod thread;
pub mod transport;
pub mod trouble;
pub mod watching;

pub use arrivals::{Arrivals, Movement};
pub use camera::{Camera, OutgoingPicture, PictureSize};
pub use detail::{Ask, Asked, Cap, Wanted};
pub use dialect::{Dialect, detect};
pub use event::{CallEvent, ScreenShare, SelfAudio, SelfScreen, SelfVideo};
pub use failure::CallFailure;
pub use hearing::{Attached, Cue, Ears, Heard, audible, changes, mono};
pub use livekit::LiveKitTransport;
pub use microphone::{Microphone, OutgoingFrame, QUEUE_FRAMES};
pub use notices::{Announced, Flags, Notice};
pub use publish::PublishedAudio;
pub use showing::PublishedVideo;
pub use thread::{CallThread, JOIN_TIMEOUT, LEAVE_TIMEOUT, SHUTDOWN_LEAVE_TIMEOUT, Senses};
pub use transport::{CallSession, CallTransport, Roster};
pub use trouble::{Fault, Faults};
pub use watching::{Eyes, IncomingPicture, Kind, Seen};
