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
//! What is left is very nearly one call. `Call::join` publishes the
//! membership, runs the dead man's switch and the heartbeat, distributes media
//! keys over Olm to-device, discovers the LiveKit transport, and connects to
//! the SFU with frame encryption on. There is nothing to reimplement.
//!
//! The exception is finding the SFU in the first place. Upstream discovery
//! knows one of the two mechanisms MSC4143 left behind, and the one it does
//! not know is the one Element Call uses and therefore the one most existing
//! deployments have. So the other half is read here, and parsed in
//! [`crate::discovery`] where a test can reach it.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use consort_matrix::{Participant, rooms};
use matrix_rtc_livekit::{Call, CallError, CallOptions};
use matrix_rtc_media::{
    AudioFrame, AudioSourceConfig, LocalTrackHandle, MediaConstraints, MediaStreamKind,
    Participant as MediaParticipant, PublishOptions,
};
use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};

use livekit::DataPacket;

use futures_util::StreamExt;

use crate::dialect::{self, Dialect};
use crate::event::SelfAudio;
use crate::discovery;
use crate::failure::{CallFailure, classify};
use crate::hearing::{self, Ears};
use crate::notices::{self, Notice};
use crate::publish::PublishedAudio;
use crate::roster;
use crate::thread::AbortOnDrop;
use crate::transport::{CallSession, CallTransport, Change, Roster};
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
    /// Where to get an SFU token, when an operator has said so by hand.
    ///
    /// Outranks discovery rather than backing it up. Everything found
    /// automatically is a guess about a deployment, and this is the place to
    /// overrule a wrong one without waiting for a release, so a value here is
    /// the answer and nothing is asked.
    service_url_fallback: Option<String>,
    /// What the server's own discovery document said, once anything has asked.
    ///
    /// A `.well-known` is a small static file that changes about as often as
    /// the deployment does, so reading it once per session is plenty.
    ///
    /// `Some(None)` is a real answer and is why this is not an
    /// `OnceLock<String>`: a document that was read and named no SFU has
    /// settled the question, and a current homeserver that answers the
    /// transports endpoint properly should not be re-asked on every single
    /// join for the rest of the session. A fetch that *failed* settles
    /// nothing, so that is not recorded and the next join tries again.
    discovered: OnceLock<Option<String>>,
}

/// How long to wait for a discovery document before giving up on it.
///
/// Comfortably inside [`crate::thread::JOIN_TIMEOUT`], because this runs
/// before the join proper. A server that will not answer should make a join
/// fail on its own terms, and not by eating the whole budget the join itself
/// was supposed to get.
const WELL_KNOWN_TIMEOUT: Duration = Duration::from_secs(5);

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
            discovered: OnceLock::new(),
        }
    }

    /// Where to ask for an SFU token when the homeserver advertises no
    /// transport of its own.
    ///
    /// `matrix-rtc-livekit` asks the MSC4143 transports endpoint and then
    /// falls back to whatever this returns. A homeserver old enough to answer
    /// that endpoint with a 404 is very often one running Element Call
    /// perfectly well, because Element Call never used the endpoint: it reads
    /// `org.matrix.msc4143.rtc_foci` out of the server's discovery document.
    /// So that is read here, and a deployment that has ever worked with
    /// Element Call needs no configuration to work with this.
    async fn fallback_service_url(&self) -> Option<String> {
        if let Some(configured) = &self.service_url_fallback {
            return Some(configured.clone());
        }

        if let Some(remembered) = self.discovered.get() {
            return remembered.clone();
        }

        let Some(user_id) = self.client.user_id() else {
            tracing::debug!("no session yet, so no server to ask for a transport");
            return None;
        };

        let url = discovery::well_known_url(user_id.server_name().as_str());

        let document = match self.read_document(&url).await {
            Ok(document) => document,
            Err(error) => {
                // Not a warning, and deliberately not remembered. Plenty of
                // servers publish no discovery document at all, so a 404 here
                // is an ordinary answer rather than a fault, and a server that
                // was briefly unreachable deserves to be asked again.
                tracing::info!(%url, %error, "no discovery document to read");
                return None;
            }
        };

        let focus = discovery::livekit_focus(&document);
        match &focus {
            Some(found) => {
                tracing::info!(%url, %found, "the server's discovery document names an SFU");
            }
            None => {
                tracing::info!(%url, "the server's discovery document names no LiveKit SFU");
            }
        }

        // Racing here is harmless, and cannot happen anyway: joins are
        // serialised by the call thread. The loser of a race would keep the
        // equally valid answer it just read.
        let _ = self.discovered.set(focus.clone());

        focus
    }

    /// This account's server's discovery document, as it was served.
    ///
    /// The SDK's own HTTP client rather than a new one, so the request
    /// inherits the TLS setup, the proxy and the connection pool that every
    /// other request this app makes already uses. The timeout covers reading
    /// the body as well as getting a response, which is also what keeps an
    /// implausibly large document from being read to the end.
    async fn read_document(&self, url: &str) -> Result<String, matrix_sdk::reqwest::Error> {
        self.client
            .http_client()
            .get(url)
            .timeout(WELL_KNOWN_TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
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
    /// The microphone publication, kept so it can be muted.
    ///
    /// A second handle on the one [`CallSession::publish_microphone`] hands
    /// back, which the call thread moves into the task that pumps frames at it.
    /// Muting is not something that task does and must never become something
    /// it does: see the header of [`crate::publish`]. It is a person pressing a
    /// button, arriving from somewhere else entirely, and it needs its own way
    /// to reach the track.
    ///
    /// Set once, by `publish_microphone`. Empty before that, which is the
    /// window between joining and publishing and no longer.
    microphone: OnceLock<Arc<dyn LocalTrackHandle>>,
    /// One task per participant whose audio is being played, keyed by
    /// `member_id`. Dropping one stops that person's audio; dropping the
    /// session stops everybody's, which is what ends a call cleanly.
    ///
    /// A `RefCell` rather than a `Mutex` because a session never leaves the
    /// call thread. That is not a shortcut, it is the same constraint that put
    /// the call on a thread of its own: `Call::join` drives `!Send` futures.
    playing: RefCell<HashMap<String, AbortOnDrop>>,
    /// What this session last said about its own audio.
    ///
    /// LiveKit never delivers a data message back to whoever published it, and
    /// the event says so in its type: `DataReceived` carries an
    /// `Option<RemoteParticipant>`, and there is no variant of that for
    /// ourselves. So the one announcement this session will never hear is its
    /// own, and without this the headphones would appear beside everybody in
    /// the call except the person who pressed the button.
    ///
    /// A channel rather than a flag because the reader is a task, and the same
    /// task owns the map so that the local and remote halves cannot race to
    /// say what the call currently looks like.
    saying: watch::Sender<SelfAudio>,
}

impl CallTransport for LiveKitTransport {
    type Session = LiveKitSession;

    async fn join(&self, room_id: &str) -> Result<Self::Session, CallFailure> {
        let room = self.room(room_id)?;

        // In memory, against the clock, with no request behind it. The same
        // read `consort_matrix` already does once per voice channel per sync.
        let occupied = room.active_room_call_participants().len();
        let dialect = dialect::detect(occupied, self.fallback_dialect);

        // Resolved before the join rather than inside it, because upstream
        // takes the fallback as a value and has no way to ask a question at
        // the moment it turns out to need one.
        let fallback = self.fallback_service_url().await;

        // At `info`, permanently. A call in the wrong generation succeeds at
        // every step and is heard by nobody, so which one was chosen and what
        // chose it is the first question worth being able to answer.
        tracing::info!(
            %room_id,
            ?dialect,
            ?self.fallback_dialect,
            occupied,
            has_fallback = fallback.is_some(),
            "joining the call"
        );

        let call = Call::join(
            &room,
            CallOptions {
                element_call_compat: dialect.into(),
                livekit_service_url_fallback: fallback,
                ..CallOptions::default()
            },
        )
        .await
        .map_err(|error| classify(&error))?;

        Ok(LiveKitSession {
            call,
            client: self.client.clone(),
            room_id: room_id.to_owned(),
            microphone: OnceLock::new(),
            playing: RefCell::default(),
            saying: watch::channel(SelfAudio::default()).0,
        })
    }
}

impl LiveKitSession {
    /// Tell the other Consort clients in the call what this session is doing.
    ///
    /// Failure is logged and swallowed on purpose. This is an indicator on
    /// somebody else's screen: a call is not worth ending over it, and the
    /// deafening itself has already happened locally whatever the SFU makes of
    /// this.
    ///
    /// Called on every roster change as well as on the button, because the
    /// call thread re-pushes this session's audio state each time somebody
    /// arrives. That is what tells a newcomer, who by definition missed the
    /// announcement made before they were there. See [`crate::notices`].
    ///
    /// Mute is not in here and must not be. LiveKit already broadcasts a track
    /// mute, every client including Element Call draws it, and announcing it
    /// again would be a second source for one fact, disagreeing with the first
    /// whenever a packet went missing.
    async fn announce(&self, audio: SelfAudio) {
        let notice = Notice::new(self.call.membership_id(), audio.deafened, audio.away);
        let packet = DataPacket {
            payload: notice.encode(),
            topic: Some(notices::TOPIC.to_owned()),
            // Reliable: this is a state change, not a sample. A lost one leaves
            // an icon wrong until the next roster change, which could be the
            // rest of the call.
            reliable: true,
            ..DataPacket::default()
        };

        if let Err(error) = self
            .call
            .session()
            .room()
            .local_participant()
            .publish_data(packet)
            .await
        {
            tracing::warn!(%error, ?audio, "could not tell the call about this session's audio");
        }
    }
}

/// Who this session is, to the two systems that name it differently.
///
/// Both are fixed for the life of a call, and carrying them together keeps the
/// pairing in one place: filing our own notice under the wrong identity would
/// hide it behind somebody else's.
struct Us {
    /// How the SFU names this session. The key our own notice is filed under,
    /// alongside everybody else's, so that one map answers the whole question.
    identity: String,
    /// How MatrixRTC names it. What a notice carries, and what the roster
    /// matches a person against.
    member_id: String,
}

/// What one room event does to the record, if anything.
fn heard(known: &mut notices::Announced, event: livekit::RoomEvent) -> bool {
    match event {
        livekit::RoomEvent::DataReceived {
            payload,
            topic,
            participant: Some(participant),
            ..
        } if topic.as_deref() == Some(notices::TOPIC) => match Notice::decode(&payload) {
            Some(notice) => known.note(participant.identity().as_str(), notice),
            None => false,
        },
        // Somebody who crashed or dropped their connection says nothing on the
        // way out, and leaving them deafened forever would put a headphone icon
        // beside a person who is not in the call.
        livekit::RoomEvent::ParticipantDisconnected(participant) => {
            known.gone(participant.identity().as_str())
        }
        _ => false,
    }
}

/// Keep `deafened` up to date from what the Consort clients in the call say,
/// this one included.
///
/// Its own task because the room's event stream is the only place the others
/// arrive, and nothing else in this crate reads it. `Room::subscribe` hands
/// over an independent receiver, so this competes with nothing.
///
/// This session's own state arrives on `mine` instead, because LiveKit does
/// not deliver a data message back to whoever published it. One task holds the
/// map so that the local and the remote half cannot race to say what the call
/// currently looks like.
///
/// Ends when the room does, or when the roster holding its handle is dropped.
async fn watch_notices(
    mut events: tokio::sync::mpsc::UnboundedReceiver<livekit::RoomEvent>,
    mut mine: watch::Receiver<SelfAudio>,
    me: Us,
    flags: watch::Sender<notices::Flags>,
) {
    let mut known = notices::Announced::new();

    // Seeded rather than waited for. `subscribe` marks the value current at
    // the time it was called as already seen, and this session can have
    // deafened itself before there was a roster watching, in which case
    // nothing would ever arrive below to say so.
    let ours = *mine.borrow_and_update();
    known.note(
        &me.identity,
        Notice::new(&me.member_id, ours.deafened, ours.away),
    );

    loop {
        let changed = tokio::select! {
            event = events.recv() => match event {
                Some(event) => heard(&mut known, event),
                // The room has gone.
                None => break,
            },
            ours = mine.changed() => match ours {
                Ok(()) => {
                    let ours = *mine.borrow_and_update();
                    known.note(
                        &me.identity,
                        Notice::new(&me.member_id, ours.deafened, ours.away),
                    )
                }
                // The session has gone, which means so has the call.
                Err(_) => break,
            },
        };

        // Only a genuine change is published. Everybody re-announces on every
        // roster change, so most of these repeat what the last one said, and
        // sending each would redraw the roster once per participant per
        // arrival.
        if changed && flags.send(known.flags()).is_err() {
            // Nothing is watching any more, which means the call has gone.
            break;
        }
    }
}

impl CallSession for LiveKitSession {
    type Track = Arc<dyn LocalTrackHandle>;
    type Roster = LiveKitRoster;

    async fn publish_microphone(&self) -> Result<Self::Track, CallFailure> {
        let track = self
            .call
            .publish(PublishOptions::microphone())
            .await
            .map_err(|error| classify(&error))?;

        // Ignored if it is somehow already set. A session publishes once, and
        // the alternative to ignoring is a panic on a path that is otherwise
        // recoverable.
        let _ = self.microphone.set(track.clone());
        Ok(track)
    }

    async fn set_muted(&self, muted: bool) -> Result<(), CallFailure> {
        let Some(track) = self.microphone.get() else {
            // Between joining and publishing. The call thread applies this
            // again once the join has finished, so there is nothing lost and
            // nothing to report.
            return Ok(());
        };

        track
            .set_muted(muted)
            .map_err(|error| classify(&CallError::Media(error)))
    }

    async fn set_deafened(&self, deafened: bool) -> Result<(), CallFailure> {
        // Per participant, because that is the only granularity anything in
        // this stack has. There is no "stop playing this call", so deafen is
        // built by asking for every microphone in it at once.
        //
        // Through `visible` rather than `enabled`. Both currently resolve to a
        // server-side pause, but they are documented to diverge: `enabled` is
        // meant to become a full unsubscribe, whose resume renegotiates and,
        // upstream says, currently strands the stream. A pause is what deafen
        // wants anyway. No data crosses the wire, and undeafening is immediate
        // rather than a renegotiation somebody waits through.
        let constraints = MediaConstraints {
            visible: !deafened,
            ..MediaConstraints::default()
        };

        for participant in self.call.engine().participants() {
            // Ours is not something we are listening to. Asking to pause our
            // own microphone through the subscription path would be asking the
            // SFU about a stream it does not send us.
            if participant.is_local {
                continue;
            }
            self.call.set_constraints(
                &participant.member_id,
                MediaStreamKind::Microphone,
                constraints,
            );
        }

        Ok(())
    }

    async fn announce_self(&self, audio: SelfAudio) -> Result<(), CallFailure> {
        // Said to ourselves before it is said to anybody else, because the one
        // client that will not be told by the announcement below is this one.
        // LiveKit does not deliver a data message back to whoever published
        // it, so without this the icons would appear beside everybody in the
        // call except the person who pressed the button.
        self.saying.send_replace(audio);
        self.announce(audio).await;
        Ok(())
    }

    fn listen(&self, ears: &Ears) {
        // Asked of the engine rather than tracked from events, so that this is
        // a statement of what should currently be true rather than a tally that
        // can drift. It is called on every roster change and has to be
        // idempotent anyway; see `CallSession::listen`.
        let audible = hearing::audible(&self.call.engine().participants());

        let mut playing = self.playing.borrow_mut();
        let attached: BTreeSet<String> = playing.keys().cloned().collect();
        let (start, stop) = hearing::changes(&attached, &audible);

        for who in stop {
            // Dropping the handle aborts the pump; forgetting drops whatever it
            // had already queued. Both, because a participant who left mid-word
            // would otherwise finish it several seconds later.
            playing.remove(&who);
            ears.forget(&who);
            tracing::debug!(member_id = %who, "stopped playing a participant");
        }

        for who in start {
            let Some(track) = self.call.remote_track(&who, MediaStreamKind::Microphone) else {
                // The membership is known but its track has not been subscribed
                // yet, which is the ordinary order of events rather than a
                // fault. The next roster change asks again.
                continue;
            };
            let Some(mut frames) = track.audio_frames() else {
                tracing::warn!(member_id = %who, "a microphone track with no audio to pull");
                continue;
            };

            let ears = Arc::clone(ears);
            let name = who.clone();
            let pump = tokio::task::spawn_local(async move {
                while let Some(frame) = frames.next().await {
                    ears.hear(
                        &name,
                        hearing::mono(&frame.data, frame.num_channels).as_ref(),
                    );
                }
                // The stream ended, which means the track went away rather than
                // that they stopped talking. A silent participant keeps
                // producing frames.
                ears.forget(&name);
            });

            tracing::debug!(member_id = %who, "playing a participant");
            playing.insert(who, AbortOnDrop(pump));
        }
    }

    fn roster(&self) -> Self::Roster {
        // Started here rather than at the join so that its lifetime is the
        // roster's. The roster is what the call thread aborts when a call
        // ends, so there is no path out of a call that leaves this running.
        let (announcing, announced) = watch::channel(notices::Flags::default());
        let me = Us {
            identity: self
                .call
                .session()
                .room()
                .local_participant()
                .identity()
                .to_string(),
            member_id: self.call.membership_id().to_owned(),
        };
        let watching = AbortOnDrop(tokio::task::spawn_local(watch_notices(
            self.call.session().room().subscribe(),
            self.saying.subscribe(),
            me,
            announcing,
        )));

        LiveKitRoster {
            memberships: self.call.subscribe_participants(),
            reports: self.call.subscribe_call_events(),
            faults: Faults::default(),
            client: self.client.clone(),
            room_id: self.room_id.clone(),
            announced,
            _watching: watching,
        }
    }

    async fn leave(self) -> Result<(), CallFailure> {
        self.call.leave().await.map_err(|error| classify(&error))
    }
}

/// One membership, read out of the watch under a single borrow.
///
/// Its own type rather than a tuple, and read once rather than three times, so
/// that a person leaving between two reads cannot shift the pairing by one and
/// hang somebody else's mute on their name.
struct Seen {
    member_id: String,
    user_id: String,
    muted: bool,
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
    /// Which memberships have deafened themselves, as their own clients say.
    ///
    /// A separate channel because it comes from a separate place: this is
    /// Consort clients talking to each other over the call's data channel, and
    /// nothing in MatrixRTC or LiveKit reports it. See [`crate::notices`].
    announced: watch::Receiver<notices::Flags>,
    /// The task filling it in. Ends when this roster is dropped.
    _watching: AbortOnDrop,
}

impl Roster for LiveKitRoster {
    fn trouble(&self) -> Option<String> {
        self.faults.sentence()
    }

    async fn now(&self) -> Vec<Participant> {
        // Read out and released before the await. The borrow guards the
        // watch's own lock, and holding one across a member-store read would
        // block the call's signalling on this room's SQLite.
        //
        // The mute travels with the user id rather than being looked up again
        // afterwards, because between the two reads somebody can leave and the
        // pairing would silently shift by one.
        let seen: Vec<Seen> = self
            .memberships
            .borrow()
            .iter()
            .map(|member| Seen {
                member_id: member.member_id.clone(),
                user_id: member.user_id.clone(),
                muted: roster::microphone_muted(member),
            })
            .collect();

        let flags = self.announced.borrow().clone();

        let user_ids: Vec<String> = seen.iter().map(|one| one.user_id.clone()).collect();
        let mutes: Vec<(String, bool)> = seen
            .iter()
            .map(|one| (one.user_id.clone(), one.muted))
            .collect();
        let whose: Vec<(String, String)> = seen
            .into_iter()
            .map(|one| (one.member_id, one.user_id))
            .collect();

        // Deduplication happens there, not here: a roster is per membership,
        // so somebody on a laptop and a phone arrives twice, and the
        // room-state path has exactly the same problem for the same reason.
        let named = rooms::name_participants(&self.client, &self.room_id, &user_ids).await;

        let named = roster::with_mutes(named, &mutes);
        let named = roster::with_deafened(named, &whose, &flags.deafened);
        roster::with_away(named, &whose, &flags.away)
    }

    async fn changed(&mut self) -> Option<Change> {
        // Destructured so the two futures below borrow different fields.
        // `select!` over `self.memberships` and `self.reports` directly would
        // be two mutable borrows of one `self`.
        let Self {
            memberships,
            reports,
            faults,
            announced,
            ..
        } = self;

        loop {
            tokio::select! {
                changed = memberships.changed() => return changed.is_ok().then_some(Change::Roster),
                // Rare, and worth a full redraw when it happens: it is drawn
                // beside the mute, in the roster, by name.
                changed = announced.changed() => return changed.is_ok().then_some(Change::Roster),
                report = reports.recv() => match report {
                    // Answered here rather than through `what_it_says`, which
                    // is about what is wrong with a call. This one is cheap and
                    // frequent, and the whole point of telling it apart is that
                    // it must not cost a roster read.
                    //
                    // `borrow` rather than `borrow_and_update`: this is reading
                    // the memberships to name somebody, not waiting on them,
                    // and marking them seen here would swallow a genuine roster
                    // change from the arm above.
                    Ok(matrix_rtc_media::CallEvent::ActiveSpeakers { speakers }) => {
                        let talking = roster::speaking_users(&speakers, &memberships.borrow());
                        return Some(Change::Speaking(talking));
                    }
                    // Only a report that changes the answer is worth waking
                    // anybody for. The cryptor reports its state per frame run
                    // rather than only on a transition, so most of these say
                    // what the last one said.
                    Ok(event) => match what_it_says(&event) {
                        Some((member_id, fault)) => {
                            if faults.note(member_id, fault) {
                                return Some(Change::Roster);
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
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
            }
        }
    }
}

impl PublishedAudio for Arc<dyn LocalTrackHandle> {
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
