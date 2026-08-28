// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Being in a voice call.
//!
//! [`crate::rooms`] can already say which channels are voice channels and who
//! is sitting in each one, which it reads out of room state without joining
//! anything. This module is the other half: actually being one of those
//! people.
//!
//! It starts with the question that has to be answered before the join rather
//! than after it. See [`readiness`].

pub mod gate;
pub mod readiness;

pub use gate::{JoinVerdict, can_join};
pub use readiness::{CallReadiness, readiness, watch as watch_readiness};
