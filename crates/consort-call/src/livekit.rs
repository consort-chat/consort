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

use std::sync::Arc;

use matrix_rtc_livekit::{Call, CallError, CallOptions};
use matrix_rtc_media::{AudioFrame, AudioSourceConfig, LocalTrackHandle, PublishOptions};
use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};

use crate::dialect::{self, Dialect};
use crate::failure::{CallFailure, classify};
use crate::publish::PublishedAudio;
use crate::transport::{CallSession, CallTransport};

/// A MatrixRTC call over LiveKit.
pub struct LiveKitTransport {
    client: Client,
    /// Which generation to speak in a room that offers no evidence.
    ///
    /// Only ever a fallback: [`dialect::detect`] runs per join and can override
    /// it. See that function for what is and is not detectable.
    fallback_dialect: Dialect,
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
    pub fn new(
        client: Client,
        fallback_dialect: Dialect,
        service_url_fallback: Option<String>,
    ) -> Self {
        tracing::info!(
            ?fallback_dialect,
            has_fallback = service_url_fallback.is_some(),
            "call transport ready"
        );
        Self {
            client,
            fallback_dialect,
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

        // In memory, against the clock, with no request behind it. The same
        // read `consort_matrix` already does once per voice channel per sync.
        let occupied = room.active_room_call_participants().len();
        let dialect = dialect::detect(occupied, self.fallback_dialect);

        // At `info`, permanently. A call in the wrong generation succeeds at
        // every step and is heard by nobody, so which one was chosen and what
        // chose it is the first question worth being able to answer.
        tracing::info!(
            %room_id,
            ?dialect,
            ?self.fallback_dialect,
            occupied,
            "joining the call"
        );

        Call::join(
            &room,
            CallOptions {
                element_call_compat: dialect.into(),
                livekit_service_url_fallback: self.service_url_fallback.clone(),
                ..CallOptions::default()
            },
        )
        .await
        .map_err(|error| classify(&error))
    }
}

impl CallSession for Call {
    type Track = Arc<dyn LocalTrackHandle>;

    async fn publish_microphone(&self) -> Result<Self::Track, CallFailure> {
        Call::publish(self, PublishOptions::microphone())
            .await
            .map_err(|error| classify(&error))
    }

    async fn leave(self) -> Result<(), CallFailure> {
        Call::leave(self).await.map_err(|error| classify(&error))
    }
}

impl PublishedAudio for Arc<dyn LocalTrackHandle> {
    fn set_muted(&self, muted: bool) -> Result<(), CallFailure> {
        self.as_ref()
            .set_muted(muted)
            .map_err(|error| classify(&CallError::Media(error)))
    }

    async fn send(&self, samples: Vec<i16>) -> Result<(), CallFailure> {
        // Taken from the publication's own defaults rather than restated, so
        // this cannot drift from what `PublishOptions::microphone` asked for.
        // That it also matches what `consort-audio` produces is asserted in
        // `tests/matches_the_capture_format.rs`, because nothing in the type
        // system connects the two.
        let config = AudioSourceConfig::default();
        let channels = config.num_channels.max(1);

        let frame = AudioFrame {
            samples_per_channel: samples.len() as u32 / channels,
            data: samples,
            sample_rate: config.sample_rate,
            num_channels: config.num_channels,
        };

        self.as_ref()
            .capture_audio(frame)
            .await
            .map_err(|error| classify(&CallError::Media(error)))
    }
}
