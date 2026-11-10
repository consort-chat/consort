// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Being in a MatrixRTC voice call.
//!
//! The other half of `consort-matrix`, which says which channels are voice
//! channels and who is in them without joining anything. A crate of its own
//! because `matrix-rtc-livekit` brings libwebrtc, around eight gigabytes of
//! build output, which would otherwise sit in front of every
//! `cargo test -p consort-matrix`.

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
