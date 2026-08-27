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

use consort_matrix::{Participant, rooms};
use matrix_rtc_livekit::{Call, CallError, CallOptions};
use matrix_rtc_media::{
    AudioFrame, AudioSourceConfig, LocalTrackHandle, Participant as MediaParticipant,
    PublishOptions,
};
use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};

use crate::dialect::{self, Dialect};
use crate::failure::{CallFailure, classify};
use crate::publish::PublishedAudio;
use crate::transport::{CallSession, CallTransport, Roster};
use crate::trouble::{Faults, what_it_says};
use tokio::sync::{broadcast, watch};

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

/// A call this session is in, and what it takes to describe it.
///
/// `Call` alone cannot: its roster is memberships and user IDs, and turning
/// those into names somebody recognises is a per-room question that only the
/// room's member store can answer. So the client and the room travel with the
/// call rather than being looked up again.
pub struct LiveKitSession {
    call: Call,
    client: Client,
    room_id: String,
}

impl CallTransport for LiveKitTransport {
    type Session = LiveKitSession;

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

        let call = Call::join(
            &room,
            CallOptions {
                element_call_compat: dialect.into(),
                livekit_service_url_fallback: self.service_url_fallback.clone(),
                ..CallOptions::default()
            },
        )
        .await
        .map_err(|error| classify(&error))?;

        Ok(LiveKitSession {
            call,
            client: self.client.clone(),
            room_id: room_id.to_owned(),
        })
    }
}

impl CallSession for LiveKitSession {
    type Track = Arc<dyn LocalTrackHandle>;
    type Roster = LiveKitRoster;

    async fn publish_microphone(&self) -> Result<Self::Track, CallFailure> {
        self.call
            .publish(PublishOptions::microphone())
            .await
            .map_err(|error| classify(&error))
    }

    fn roster(&self) -> Self::Roster {
        LiveKitRoster {
            memberships: self.call.subscribe_participants(),
            reports: self.call.subscribe_call_events(),
            faults: Faults::default(),
            client: self.client.clone(),
            room_id: self.room_id.clone(),
        }
    }

    async fn leave(self) -> Result<(), CallFailure> {
        self.call.leave().await.map_err(|error| classify(&error))
    }
}

/// One view of a call: who is in it, and what is wrong with it.
///
/// Two upstream streams behind one seam, because they describe one thing and
/// are drawn in one place. Splitting them would mean two watchers racing to
/// say what a call currently is, and whichever spoke last would win.
pub struct LiveKitRoster {
    /// Derived from MatrixRTC signalling and enriched with live media state,
    /// and one entry per membership rather than per person.
    memberships: watch::Receiver<Vec<MediaParticipant>>,
    /// Everything else the call has to say. Only the encryption reports are
    /// read here; the rest is phase 4's.
    reports: broadcast::Receiver<matrix_rtc_media::CallEvent>,
    /// What those reports currently add up to. See [`crate::trouble`].
    faults: Faults,
    client: Client,
    room_id: String,
}

impl Roster for LiveKitRoster {
    fn trouble(&self) -> Option<String> {
        self.faults.sentence()
    }

    async fn now(&self) -> Vec<Participant> {
        // Read out and released before the await. The borrow guards the
        // watch's own lock, and holding one across a member-store read would
        // block the call's signalling on this room's SQLite.
        let user_ids: Vec<String> = self
            .memberships
            .borrow()
            .iter()
            .map(|member| member.user_id.clone())
            .collect();

        // Deduplication happens there, not here: a roster is per membership,
        // so somebody on a laptop and a phone arrives twice, and the
        // room-state path has exactly the same problem for the same reason.
        rooms::name_participants(&self.client, &self.room_id, &user_ids).await
    }

    async fn changed(&mut self) -> bool {
        // Destructured so the two futures below borrow different fields.
        // `select!` over `self.memberships` and `self.reports` directly would
        // be two mutable borrows of one `self`.
        let Self {
            memberships,
            reports,
            faults,
            ..
        } = self;

        loop {
            tokio::select! {
                changed = memberships.changed() => return changed.is_ok(),
                report = reports.recv() => match report {
                    // Only a report that changes the answer is worth waking
                    // anybody for. The cryptor reports its state per frame run
                    // rather than only on a transition, so most of these say
                    // what the last one said.
                    Ok(event) => match what_it_says(&event) {
                        Some((member_id, fault)) => {
                            if faults.note(member_id, fault) {
                                return true;
                            }
                            continue;
                        }
                        None => continue,
                    },
                    // Too many events while this task was busy. Nothing to do
                    // about it: the next report re-states the state, because
                    // they are per frame run rather than per transition.
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, "fell behind the call's own events");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => return false,
                },
            }
        }
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
