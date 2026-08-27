// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The transport that talks to a real SFU.
//!
//! Excluded from coverage, for the same reason `consort_audio`'s `cpal_host`
//! is: CI has no SFU, so every line in here is a line no test can reach. It is
//! kept correspondingly thin. Everything that decides anything, including
//! which generation to speak and what a failure means, sits behind
//! [`crate::CallTransport`] and is tested without it.
//!
//! What is left here is one call. `Call::join` publishes the membership, runs
//! the dead man's switch and the heartbeat, distributes media keys over Olm
//! to-device, discovers the LiveKit transport, and connects to the SFU with
//! frame encryption on. There is nothing to reimplement.

use matrix_rtc_livekit::{Call, CallOptions};
use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};

use crate::dialect::Dialect;
use crate::failure::{CallFailure, classify};
use crate::transport::{CallSession, CallTransport};

/// A MatrixRTC call over LiveKit.
pub struct LiveKitTransport {
    client: Client,
    dialect: Dialect,
    /// Where to get an SFU token when the homeserver advertises no transport.
    ///
    /// MSC4143 discovery is tried first and this is the fallback, so a
    /// homeserver that does advertise one wins. A deployment whose homeserver
    /// does not, and that sets nothing here, cannot join at all: there is
    /// nowhere to ask.
    service_url_fallback: Option<String>,
}

impl LiveKitTransport {
    /// Build the transport for a signed-in session.
    pub fn new(client: Client, dialect: Dialect, service_url_fallback: Option<String>) -> Self {
        tracing::info!(
            ?dialect,
            has_fallback = service_url_fallback.is_some(),
            "call transport ready"
        );
        Self {
            client,
            dialect,
            service_url_fallback,
        }
    }

    /// The room, if this account is in it and sync has delivered it.
    fn room(&self, room_id: &str) -> Result<matrix_sdk::Room, CallFailure> {
        let unknown = || CallFailure::UnknownRoom {
            room_id: room_id.to_owned(),
        };

        let parsed: OwnedRoomId = RoomId::parse(room_id).map_err(|_| unknown())?;
        self.client.get_room(&parsed).ok_or_else(unknown)
    }
}

impl CallTransport for LiveKitTransport {
    type Session = Call;

    async fn join(&self, room_id: &str) -> Result<Self::Session, CallFailure> {
        let room = self.room(room_id)?;

        Call::join(
            &room,
            CallOptions {
                element_call_compat: self.dialect.into(),
                livekit_service_url_fallback: self.service_url_fallback.clone(),
                ..CallOptions::default()
            },
        )
        .await
        .map_err(|error| classify(&error))
    }
}

impl CallSession for Call {
    async fn leave(self) -> Result<(), CallFailure> {
        Call::leave(self).await.map_err(|error| classify(&error))
    }
}
