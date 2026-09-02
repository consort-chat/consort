// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The commands the frontend can invoke.
//!
//! Each `#[tauri::command]` is a one-line delegate to a plain async function
//! taking `&AppState`. That split is not decoration: `State<'_, AppState>` can
//! only be produced by a running Tauri application, so logic written directly
//! inside a command is logic no test can reach. The delegates below are the
//! only untested lines in this file, and there is nothing in them to break.

use consort_audio::{
    AudioDeviceReport, AudioDevices, AudioSettings, CpalHost, Direction, GateConfig, catalogue,
    choose,
};
use consort_call::LiveKitTransport;
use consort_matrix::{
    BackendKind, Credentials, JoinVerdict, Profile, auth, calls, rooms, timeline, verification,
};
use serde::Serialize;
use tauri::State;

use crate::audio::Backends;
use crate::state::{AppState, CallAudio};

/// An error in the shape the frontend consumes.
///
/// Carries two strings on purpose. `message` is written for a person and is
/// what the UI renders. `detail` is the underlying error text, which goes to
/// the console for whoever is debugging and is never shown in the interface.
#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
    detail: String,
}

/// Read accessors for the two halves.
///
/// Almost everything that produces one of these hands it straight across the
/// IPC boundary, where both fields go out by serialisation. The exception is
/// the attachment scheme in `lib.rs`, which answers with a status and a
/// sentence rather than with JSON and so has to read them.
impl CommandError {
    /// What the UI will render.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What goes to the console.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<crate::settings::SettingsError> for CommandError {
    fn from(error: crate::settings::SettingsError) -> Self {
        Self {
            // Deliberately not "try again". A settings file that will not write
            // is a full disk or a permissions problem, and neither is fixed by
            // pressing the same button.
            message: "Your settings could not be saved.".to_owned(),
            detail: error.to_string(),
        }
    }
}

impl From<consort_matrix::Error> for CommandError {
    fn from(error: consort_matrix::Error) -> Self {
        Self {
            message: error.user_message(),
            detail: error.to_string(),
        }
    }
}

/// Result of asking whether anyone is signed in.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SessionStatus {
    /// Nobody is signed in. Show the login screen.
    SignedOut,
    /// Somebody is signed in, either already or just now restored from disk.
    SignedIn { profile: Profile },
}

/// Where this machine is keeping the access token.
///
/// Surfaced to the UI so that a fallback to a plain file is something the user
/// is told about rather than something they would have to read the source to
/// discover.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStorage {
    pub kind: BackendKind,
    pub description: String,
    /// False when we had to fall back, which is the UI's cue to say so.
    pub is_preferred: bool,
}

/// Decide which screen to show.
pub async fn session_status_for(state: &AppState) -> Result<SessionStatus, CommandError> {
    if let Some(client) = state.client().await {
        let profile = Profile::from_client(&client).await?;
        return Ok(SessionStatus::SignedIn { profile });
    }

    let stored = match state.store().load() {
        Ok(Some(stored)) => stored,
        Ok(None) => return Ok(SessionStatus::SignedOut),
        Err(error) => {
            // Only discard what is genuinely unusable. A session file we cannot
            // parse would fail identically on every launch, so it goes. A
            // keyring that did not answer, or a store another process has
            // locked, is a reason to try again later and emphatically not a
            // reason to delete the one credential we hold.
            if error.invalidates_session() {
                tracing::warn!(%error, "discarding an unusable stored session");
                let _ = state.store().clear();
                return Ok(SessionStatus::SignedOut);
            }

            tracing::error!(%error, "could not read the stored session; keeping it");
            return Err(error.into());
        }
    };

    match auth::restore(&stored).await {
        Ok((client, profile)) => {
            // `set_client` also starts the task that writes rotated tokens
            // back to the store. Without it the next launch restores a spent
            // refresh token. See `auth::persist_token_refreshes`.
            state.set_client(client).await;
            Ok(SessionStatus::SignedIn { profile })
        }
        Err(error) if error.invalidates_session() => {
            tracing::warn!(%error, "the homeserver rejected the stored session; signing out");
            let _ = state.store().clear();
            Ok(SessionStatus::SignedOut)
        }
        Err(error) => {
            // Offline, or the homeserver is down. The session is still good.
            tracing::warn!(%error, "could not restore the session right now; keeping it");
            Err(error.into())
        }
    }
}

/// Sign in with a password.
pub async fn login_for(
    state: &AppState,
    server: String,
    username: String,
    password: String,
) -> Result<Profile, CommandError> {
    // Held across the whole login. Two concurrent calls would otherwise
    // register two devices on the homeserver and race on the session store.
    let _gate = state.lock_auth().await;

    // Somebody else may have completed a login while this call waited.
    if let Some(client) = state.client().await {
        tracing::info!("a login completed while this one waited; reusing it");
        return Ok(Profile::from_client(&client).await?);
    }

    let credentials = Credentials {
        server,
        username,
        password,
    };
    let (client, profile) = auth::login(state.store(), &credentials).await?;
    state.set_client(client).await;
    Ok(profile)
}

/// Sign out, locally and on the server.
pub async fn logout_for(state: &AppState) -> Result<(), CommandError> {
    let _gate = state.lock_auth().await;

    if let Some(client) = state.client().await {
        auth::logout(&client, state.store()).await?;
    } else {
        // No client but possibly a session file, for instance if a restore
        // failed earlier in this run. Clearing is still the right outcome.
        state.store().clear()?;
    }
    state.clear_client().await;
    Ok(())
}

/// The signed-in client, or an error the interface can render.
///
/// Every verification action needs one, and none of them can do anything
/// useful without it. Reaching this with nobody signed in is not a bug in the
/// application: any script running in the webview can invoke a command, and
/// unwrapping here would turn that into a panic in a command thread.
async fn signed_in_client(state: &AppState) -> Result<consort_matrix::Client, CommandError> {
    state
        .client()
        .await
        .ok_or_else(|| consort_matrix::Error::NotLoggedIn.into())
}

/// Agree to a verification somebody else asked for.
pub async fn verification_accept_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::accept(&client, &user_id, &flow_id).await?)
}

/// Start the emoji comparison from this side.
pub async fn verification_start_sas_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::start_sas(&client, &user_id, &flow_id).await?)
}

/// Say the emoji matched.
pub async fn verification_confirm_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::confirm(&client, &user_id, &flow_id).await?)
}

/// Say the emoji did not match.
pub async fn verification_mismatch_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::mismatch(&client, &user_id, &flow_id).await?)
}

/// Ask this account's other sessions to verify this one.
pub async fn verification_verify_this_session_for(state: &AppState) -> Result<(), CommandError> {
    Ok(state.verify_this_session().await?)
}

/// Whether there is another signed-in session to compare emoji with.
pub async fn verification_other_sessions_exist_for(state: &AppState) -> Result<bool, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::has_devices_to_verify_against(&client).await?)
}

/// Whether this account has a recovery key worth asking for.
pub async fn verification_recovery_exists_for(state: &AppState) -> Result<bool, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::has_recovery_set_up(&client).await?)
}

/// One room's avatar, as a data URL.
///
/// A command rather than a field on the room list, because the list is re-sent
/// in full whenever anything about it changes and image bytes would make that
/// expensive enough to notice. The interface asks for the ones it is drawing.
///
/// `Ok(None)` covers a room with no avatar, a room that has gone, and an
/// avatar the homeserver would not hand over. All three end in the same place:
/// the interface draws initials. Only "not signed in" is an error, because it
/// means the caller is asking at a moment when nothing can be answered.
pub async fn room_avatar_for(
    state: &AppState,
    room_id: String,
) -> Result<Option<String>, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(rooms::avatar(&client, &room_id).await)
}

/// One person's avatar in one room, as a data URL.
///
/// Two identifiers rather than one because a Matrix profile is per room: the
/// picture to draw beside a voice channel is the one that room knows the
/// person by.
///
/// Same failure contract as [`room_avatar_for`]. `Ok(None)` for somebody with
/// no avatar, somebody the room has never heard of, a room that has gone, and
/// an image the homeserver would not hand over, because all four end in
/// initials.
pub async fn member_avatar_for(
    state: &AppState,
    room_id: String,
    user_id: String,
) -> Result<Option<String>, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(rooms::member_avatar(&client, &room_id, &user_id).await)
}

/// What can be said about one person beyond their name.
///
/// One request to the homeserver, made when somebody opens a person's card and
/// never on the way to drawing a roster. It cannot fail: presence is off on
/// most homeservers, and a dialog in front of somebody who clicked a name out
/// of curiosity would be worse than the word "unknown".
pub async fn member_profile_for(
    state: &AppState,
    user_id: String,
) -> Result<rooms::MemberProfile, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(rooms::member_profile(&client, &user_id).await)
}

/// What to call each of `user_ids` in `room_id`.
///
/// A batch rather than one at a time, because a screen of messages is a
/// handful of people saying several things each, and one read per message from
/// the same person would be arithmetic nobody asked for.
///
/// Local: it reads the member store and makes no request. Somebody the store
/// has never heard of is absent from the map, and the interface draws their
/// user ID.
pub async fn member_names_for(
    state: &AppState,
    room_id: String,
    user_ids: Vec<String>,
) -> Result<std::collections::BTreeMap<String, String>, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(rooms::member_names(&client, &room_id, &user_ids).await)
}

/// Open a room and start watching its messages.
///
/// Answers nothing. What was asked for arrives on the `timeline` channel, in
/// full, along with every later change to it, which is the same shape the room
/// list uses and for the same reason: a value that is always complete is one a
/// reader can draw without patching a copy of its own.
///
/// Not an error while signed out. The interface that would draw a room is not
/// on screen then, and a caller asking at that moment is a stale click rather
/// than something to complain about.
///
/// Both kinds of channel come here. A voice channel is an ordinary Matrix room
/// carrying an ordinary timeline, and the only thing that makes it a voice
/// channel is one field of its `m.room.create`.
pub async fn timeline_open_for(state: &AppState, room_id: String) {
    state.open_room(room_id).await;
}

/// Stop watching whatever room was open.
///
/// What deselecting a channel does. Without it the last room's messages sit on
/// a retained channel and come back the next time anything asks to be caught
/// up.
pub fn timeline_close_for(state: &AppState) {
    state.close_room();
}

/// Ask the open room for a page of older messages.
///
/// Answers nothing, like opening: the page arrives on the `timeline` channel
/// as a longer list, with `loading` true in between so a slow homeserver is
/// distinguishable from a control that did nothing.
///
/// Idempotent and infallible. Asking with no room open, or at the start of a
/// room's history, does nothing rather than complaining: both are what a
/// scroll that lands at the wrong moment looks like.
pub fn timeline_earlier_for(state: &AppState) {
    state.earlier_messages();
}

/// Say something in a room.
///
/// Nothing comes back. The message appears when the sync brings it round,
/// which is the path every other message in the room takes; see
/// `consort_matrix::timeline` for why there is no local echo yet.
///
/// The two failures worth returning are an empty message and a room this
/// account is not in, and both are answered before anything reaches the
/// network.
pub async fn timeline_send_for(
    state: &AppState,
    room_id: String,
    body: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    timeline::send(&client, &room_id, &body).await?;
    Ok(())
}

/// The room to say something to one person in, made if there is not one.
///
/// A create is a side effect, which is unusual for something a click reaches,
/// and it is the behaviour every other client has: pressing Message on
/// somebody you have never messaged has to produce a room or the button does
/// nothing for almost everybody who presses it. See
/// `consort_matrix::rooms::direct`.
pub async fn direct_room_for(state: &AppState, user_id: String) -> Result<String, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(rooms::direct(&client, &user_id).await?)
}

/// One attachment as something to draw, by the handle its message carried.
///
/// Not a command. It is what the `consortmedia` scheme in `lib.rs` answers a
/// range request with, and it lives here so that the client lookup and the
/// error mapping are the same ones every command uses.
pub async fn attachment_for(
    state: &AppState,
    handle: &str,
) -> Result<timeline::Attachment, CommandError> {
    let client = signed_in_client(state).await?;
    Ok(timeline::media(&client, handle).await?)
}

/// What devices this machine has, and which of them are in use.
///
/// Asked for whenever the settings screen opens, and after every change: a
/// device can appear or vanish while the window is open, and the only honest
/// way to draw a picker is to have just asked.
fn audio_devices_for(state: &AppState, host: &dyn AudioDevices) -> AudioDeviceReport {
    let settings = state.settings().load().audio;
    AudioDeviceReport::of(host, settings.input.as_deref(), settings.output.as_deref())
}

/// The saved audio settings, or the defaults on first run.
fn audio_settings_for(state: &AppState) -> AudioSettings {
    state.settings().load().audio
}

/// Replace the saved audio settings.
///
/// Writes the whole audio section rather than one field, because the settings
/// screen holds all of it and a partial update would need a merge that could
/// lose a concurrent change for no benefit.
fn set_audio_settings_for(
    state: &AppState,
    mut audio: AudioSettings,
) -> Result<(), crate::settings::SettingsError> {
    let gate = audio.gate;
    let call_sounds = audio.call_sounds;
    let call_voices = audio.call_voices;
    let output_volume = audio.output_volume;
    let notification_volume = audio.notification_volume;
    let mut settings = state.settings().load();
    // The one field this does not take the caller's word for. The settings
    // screen holds every other part of the audio section, so writing the whole
    // thing is honest; it does not hold the per-person levels, which are set
    // from a menu beside a person's name in a call. Taking its word for them
    // would erase every one of them the first time somebody changed a device.
    audio.person_volumes = std::mem::take(&mut settings.audio.person_volumes);
    settings.audio = audio;
    state.settings().save(&settings)?;
    // Same reasoning as the retune below, and the same ordering: after the
    // save, so a call already in progress and the file can never disagree.
    state.set_call_sounds(call_sounds);
    state.set_call_voices(call_voices);
    state.set_volumes(output_volume, notification_volume);
    // After the save rather than instead of it, so what the meter is doing and
    // what the file says can never disagree. A microphone that is open right
    // now was started with the old tuning, and nothing else would tell it.
    // Cheap and idempotent when only a device name changed.
    state.retune_gate(gate);
    Ok(())
}

/// Set how loud one person should be, as a percentage, and remember it.
///
/// Its own command rather than part of the audio section above, because it is
/// set from somewhere else entirely: a menu beside a person's name in a call,
/// which knows one user ID and nothing about devices or gates. A caller that
/// had to send the whole audio section to change one person's volume would be
/// sending back settings it never read.
///
/// A read-modify-write of the map, which is safe here for the same reason the
/// whole-section write above is: everything that touches this file goes through
/// `SettingsStore`, on the main thread, one command at a time.
fn set_person_volume_for(
    state: &AppState,
    user_id: String,
    percent: u8,
) -> Result<(), crate::settings::SettingsError> {
    // Clamped rather than trusted. The mixer clamps on the way out, so a
    // larger number would be written to the file, read back, and drawn on a
    // slider as a level nothing is actually playing at. The slider cannot send
    // one, but a command is reachable from anything running in the webview and
    // the file outlives whatever sent it.
    let percent = percent.min(consort_audio::MAX_PERSON_VOLUME);
    let mut settings = state.settings().load();
    if percent == consort_audio::FULL_VOLUME {
        // Full volume is the absence of a choice, not a choice of 100. Storing
        // it would grow the file by a line for every person somebody ever
        // nudged and put back, and the mixer would pay a multiply by one for
        // each of them.
        //
        // Equality rather than "at or above", which is what this was while a
        // hundred was the top of the slider. It no longer is: a person can be
        // boosted past it, and treating 150 as "nothing chosen" would drop the
        // choice on the way to the file and put them straight back to full.
        settings.audio.person_volumes.remove(&user_id);
    } else {
        settings.audio.person_volumes.insert(user_id, percent);
    }
    state.settings().save(&settings)?;
    // After the save, like everything else here, so a call in progress and the
    // file cannot disagree.
    state.levels().choose(settings.audio.person_volumes.clone());
    Ok(())
}

/// Which microphone to open, and how to gate it.
///
/// Shared by the settings screen's meter and by a call, deliberately. They are
/// the same device tuned the same way, and resolving it twice in two places is
/// how they would end up disagreeing: two answers means two `start`s, and a
/// second `start` for a device already open is an audible hole in what a peer
/// hears.
///
/// A saved choice that is not plugged in falls back to the host's default
/// rather than refusing to open, which is what `Selection::name_to_open` is
/// for.
fn microphone_to_open(state: &AppState, host: &dyn AudioDevices) -> (Option<String>, GateConfig) {
    let audio = state.settings().load().audio;
    let available = catalogue(host, Direction::Input);
    let device = choose(&available, audio.input.as_deref())
        .name_to_open()
        .map(str::to_owned);

    (device, audio.gate)
}

/// Which output to play a call out of, resolved the same way the chime's is.
///
/// Its own function beside [`microphone_to_open`] rather than a second return
/// value from it, because the chime resolves an output too and all three must
/// agree. A person who tested their speakers and heard the chime has tested the
/// device the call comes out of.
fn speakers_to_open(state: &AppState, host: &dyn AudioDevices) -> Option<String> {
    let audio = state.settings().load().audio;
    let available = catalogue(host, Direction::Output);

    choose(&available, audio.output.as_deref())
        .name_to_open()
        .map(str::to_owned)
}

/// Start the microphone test behind the settings screen's level meter.
///
/// The result arrives on the `audio` event channel, including the failure
/// case: opening a microphone fails often enough on a real desktop that it is a
/// state the screen draws, not an exception.
fn audio_test_start_for(
    state: &AppState,
    host: &dyn AudioDevices,
    backends: impl FnOnce() -> Backends,
) {
    let (device, gate) = microphone_to_open(state, host);

    state.start_microphone(backends, device, gate);
}

/// Stop the microphone test.
fn audio_test_stop_for(state: &AppState) {
    state.stop_microphone();
}

/// Play the test chime out of the chosen output.
///
/// The output picker's only feedback. A microphone can be checked by talking
/// at it and watching the meter; speakers cannot be checked by anything at all
/// unless something plays, so without this the output picker is a control that
/// gives no sign of having done anything.
///
/// Resolves the saved output the same way `audio_test_start_for` resolves the
/// input, and for the same reason: a device that has gone should fall back to
/// the host's default rather than refuse.
fn audio_tone_play_for(
    state: &AppState,
    host: &dyn AudioDevices,
    backends: impl FnOnce() -> Backends,
) {
    let audio = state.settings().load().audio;
    let available = catalogue(host, Direction::Output);
    let device = choose(&available, audio.output.as_deref())
        .name_to_open()
        .map(str::to_owned);

    state.play_test_tone(backends, device);
}

/// Cut the chime short.
fn audio_tone_stop_for(state: &AppState) {
    state.stop_test_tone();
}

/// The real sound card, in both directions.
fn cpal_backends() -> Backends {
    Backends {
        capture: Box::new(CpalHost),
        playback: Box::new(CpalHost),
    }
}

/// Join the voice channel in `room_id`, leaving whatever call is current.
///
/// Everything that can go wrong arrives on the `call` event channel rather
/// than as an error here. Joining is a sequence of remote steps that each take
/// their own time, so the command that starts it cannot be the thing that
/// reports how it went: the interface needs "working on it" before it needs
/// an answer.
///
/// The only failure worth returning is this one. Asking to join a call while
/// signed out is not a call that failed, it is a caller asking at a moment
/// when nothing can be answered.
async fn call_connect_for(
    state: &AppState,
    host: &dyn AudioDevices,
    backends: impl Fn() -> Backends + Send + 'static,
    room_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;

    // Asked before anything is opened, published or connected, because every
    // one of those steps succeeds in the failure this prevents. A call joined
    // by a session that cannot distribute a media key connects, fills its
    // roster and carries RTP, and is heard by nobody.
    //
    // A gate that cannot reach an answer lets the join through. The error is
    // about not being able to ask rather than about the answer, `CallReadiness`
    // has no variant for "not known" on purpose, and there is no honest
    // refusal to draw from a request that timed out. The same network that
    // stopped the question will stop the join a moment later and say so in the
    // vocabulary of the thing that actually failed.
    match calls::can_join(&client, &room_id).await {
        Ok(JoinVerdict::Allowed) => {}
        Ok(JoinVerdict::Refused(readiness)) => {
            state.refuse_call(room_id, readiness);
            return Ok(());
        }
        Err(error) => {
            tracing::warn!(%error, %room_id, "joining without knowing whether it can be heard");
        }
    }

    let settings = state.settings().load().calls;
    let (device, gate) = microphone_to_open(state, host);
    let output = speakers_to_open(state, host);

    // Read before the client is moved into the closure below. It is what the
    // audio thread draws this session's own green ring under, because the
    // frames it decides from carry no name of their own.
    let us = client
        .user_id()
        .ok_or(consort_matrix::Error::NotLoggedIn)?
        .to_string();

    // Out here rather than in the closure below, which runs at most once per
    // process. Editing `settings.json` and pressing join again reuses the
    // transport built from the old file, and nothing about that is visible;
    // with this line it is, because the dialect logged here and the one
    // `consort_call` logs a moment later stop agreeing.
    tracing::info!(
        %room_id,
        fallback_dialect = settings.fallback_dialect.name(),
        "connecting to a call"
    );

    state.connect_call(
        room_id,
        // Only called if this session has never joined a call before. The
        // dialect here is the fallback for a channel nobody is in;
        // `consort_call::detect` looks at the channel first.
        move || {
            LiveKitTransport::new(
                client,
                settings.fallback_dialect,
                settings.service_url_fallback,
            )
        },
        CallAudio {
            device,
            output,
            gate,
            backends: Box::new(backends),
            us,
        },
    );
    Ok(())
}

/// Leave the voice channel.
///
/// Infallible and idempotent, including when there is no call and when there
/// is no session. Both are the same thing from the interface's side: a
/// disconnect control that outlived the call it belonged to, which should do
/// nothing rather than complain.
fn call_disconnect_for(state: &AppState) {
    state.disconnect_call();
}

/// Mute or unmute this session's microphone.
///
/// Infallible and idempotent, like the disconnect above and for the same
/// reason: what comes back is the `call` event channel saying what the state
/// now is, and a command that also returned it would be a second answer for
/// the interface to disagree with.
fn call_set_muted_for(state: &AppState, muted: bool) {
    state.set_call_muted(muted);
}

/// Stop or resume receiving the audio of everybody else in the call.
fn call_set_deafened_for(state: &AppState, deafened: bool) {
    state.set_call_deafened(deafened);
}

/// Say that nobody is at this computer.
///
/// Not a third way of muting, though it mutes. The microphone going off is a
/// consequence; the point is that everybody else in the call can see it and
/// stop waiting for an answer, which is the one thing a plain mute cannot say.
fn call_set_away_for(state: &AppState, away: bool) {
    state.set_call_away(away);
}

#[tauri::command]
pub fn audio_devices(state: State<'_, AppState>) -> AudioDeviceReport {
    audio_devices_for(&state, &CpalHost)
}

#[tauri::command]
pub fn audio_settings(state: State<'_, AppState>) -> AudioSettings {
    audio_settings_for(&state)
}

#[tauri::command]
pub fn set_person_volume(
    state: State<'_, AppState>,
    user_id: String,
    percent: u8,
) -> Result<(), CommandError> {
    set_person_volume_for(&state, user_id, percent).map_err(CommandError::from)
}

#[tauri::command]
pub fn set_audio_settings(
    state: State<'_, AppState>,
    audio: AudioSettings,
) -> Result<(), CommandError> {
    set_audio_settings_for(&state, audio).map_err(CommandError::from)
}

#[tauri::command]
pub fn audio_test_start(state: State<'_, AppState>) {
    audio_test_start_for(&state, &CpalHost, cpal_backends);
}

#[tauri::command]
pub fn audio_test_stop(state: State<'_, AppState>) {
    audio_test_stop_for(&state);
}

#[tauri::command]
pub fn audio_tone_play(state: State<'_, AppState>) {
    audio_tone_play_for(&state, &CpalHost, cpal_backends);
}

#[tauri::command]
pub fn audio_tone_stop(state: State<'_, AppState>) {
    audio_tone_stop_for(&state);
}

#[tauri::command]
pub async fn call_connect(state: State<'_, AppState>, room_id: String) -> Result<(), CommandError> {
    call_connect_for(&state, &CpalHost, cpal_backends, room_id).await
}

#[tauri::command]
pub fn call_disconnect(state: State<'_, AppState>) {
    call_disconnect_for(&state);
}

#[tauri::command]
pub fn call_set_muted(state: State<'_, AppState>, muted: bool) {
    call_set_muted_for(&state, muted);
}

#[tauri::command]
pub fn call_set_deafened(state: State<'_, AppState>, deafened: bool) {
    call_set_deafened_for(&state, deafened);
}

#[tauri::command]
pub fn call_set_away(state: State<'_, AppState>, away: bool) {
    call_set_away_for(&state, away);
}

/// Verify this session with the account's recovery key.
pub async fn verification_recover_for(
    state: &AppState,
    recovery_key: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::recover(&client, &recovery_key).await?)
}

/// Call the verification off.
pub async fn verification_cancel_for(
    state: &AppState,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    let client = signed_in_client(state).await?;
    Ok(verification::cancel(&client, &user_id, &flow_id).await?)
}

/// Report where the access token is being kept.
pub fn token_storage_for(state: &AppState) -> TokenStorage {
    let kind = state.store().backend_kind();
    TokenStorage {
        kind,
        description: kind.description().to_owned(),
        is_preferred: kind.is_preferred(),
    }
}

/// Called once on startup to decide which screen to show.
#[tauri::command]
pub async fn session_status(state: State<'_, AppState>) -> Result<SessionStatus, CommandError> {
    session_status_for(&state).await
}

/// Sign in with a password.
#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    server: String,
    username: String,
    password: String,
) -> Result<Profile, CommandError> {
    login_for(&state, server, username, password).await
}

/// Sign out, locally and on the server.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), CommandError> {
    logout_for(&state).await
}

/// Where the access token lives on this machine.
#[tauri::command]
pub fn token_storage(state: State<'_, AppState>) -> TokenStorage {
    token_storage_for(&state)
}

/// Re-send the current state of every push channel.
///
/// Called by the frontend once its listeners are attached. The background
/// tasks start with the session, which on a restored session is before the
/// webview has run a line of JavaScript, so their first states are published
/// to nobody. These are state channels rather than streams of incidents:
/// missing one is not a missed message, it is an interface stuck on its
/// initial guess until something else happens to change.
#[tauri::command]
pub fn resend_state(state: State<'_, AppState>) {
    state.resend_state();
}

// The five verification actions.
//
// Each takes the pair of identifiers the `verification-flow` event carried,
// rather than the frontend holding a handle to anything. Nothing on either
// side of the boundary owns a flow: the SDK has a registry keyed by exactly
// this pair, and naming the flow every time is what makes two concurrent
// verifications representable, which they are.
//
// The user id looks redundant while only self-verification exists, since it is
// always our own. Taking it anyway costs one string and means verifying
// another person, when it arrives, is not a change to five signatures and the
// TypeScript that calls them.
//
// The two commands after them take nothing, for the opposite reason: neither
// acts on a flow, and the one that starts one always starts the same one.

/// Agree to a verification somebody else asked for.
#[tauri::command]
pub async fn verification_accept(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_accept_for(&state, user_id, flow_id).await
}

/// Start the emoji comparison from this side.
#[tauri::command]
pub async fn verification_start_sas(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_start_sas_for(&state, user_id, flow_id).await
}

/// Say the emoji matched.
#[tauri::command]
pub async fn verification_confirm(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_confirm_for(&state, user_id, flow_id).await
}

/// Say the emoji did not match.
#[tauri::command]
pub async fn verification_mismatch(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_mismatch_for(&state, user_id, flow_id).await
}

/// Call the verification off.
#[tauri::command]
pub async fn verification_cancel(
    state: State<'_, AppState>,
    user_id: String,
    flow_id: String,
) -> Result<(), CommandError> {
    verification_cancel_for(&state, user_id, flow_id).await
}

/// Ask this account's other sessions to verify this one.
///
/// No arguments: it is always this session asking, and always the account's
/// own identity being asked, so there is nothing for the webview to name and
/// nothing it could name wrongly.
#[tauri::command]
pub async fn verification_verify_this_session(
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    verification_verify_this_session_for(&state).await
}

/// Whether there is another signed-in session to compare emoji with.
///
/// Asked before the button is drawn rather than after it is pressed. With
/// nothing else signed in the request can only time out, and offering it
/// anyway wastes ten minutes to arrive at an answer that was known up front.
#[tauri::command]
pub async fn verification_other_sessions_exist(
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    verification_other_sessions_exist_for(&state).await
}

/// Whether this account has a recovery key worth asking for.
///
/// Asked for the same reason as the one above, and it decides a bigger part of
/// the screen: an account with no secret storage has no key anybody could have
/// kept, and an input box for one sends somebody hunting through a password
/// manager for something that was never created.
#[tauri::command]
pub async fn verification_recovery_exists(
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    verification_recovery_exists_for(&state).await
}

/// What to call each of these people in this room.
///
/// The names beside messages, asked for in a batch. See `member_names_for`.
#[tauri::command]
pub async fn member_names(
    state: State<'_, AppState>,
    room_id: String,
    user_ids: Vec<String>,
) -> Result<std::collections::BTreeMap<String, String>, CommandError> {
    member_names_for(&state, room_id, user_ids).await
}

/// Open a room and watch its messages.
///
/// See `timeline_open_for`. What was asked for arrives on the `timeline`
/// channel rather than as an answer here.
#[tauri::command]
pub async fn timeline_open(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<(), CommandError> {
    timeline_open_for(&state, room_id).await;
    Ok(())
}

/// Stop watching whatever room was open.
#[tauri::command]
pub fn timeline_close(state: State<'_, AppState>) {
    timeline_close_for(&state);
}

/// Ask the open room for a page of older messages.
#[tauri::command]
pub fn timeline_earlier(state: State<'_, AppState>) {
    timeline_earlier_for(&state);
}

/// Say something in a room.
#[tauri::command]
pub async fn timeline_send(
    state: State<'_, AppState>,
    room_id: String,
    body: String,
) -> Result<(), CommandError> {
    timeline_send_for(&state, room_id, body).await
}

/// The room to say something to one person in. See `direct_room_for`.
#[tauri::command]
pub async fn direct_room(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<String, CommandError> {
    direct_room_for(&state, user_id).await
}

/// One room's avatar, as a data URL.
///
/// One room at a time, and cached by the SDK on disk, so the second ask does
/// not reach the homeserver. See `room_avatar_for` for why the room list does
/// not simply carry them.
#[tauri::command]
pub async fn room_avatar(
    state: State<'_, AppState>,
    room_id: String,
) -> Result<Option<String>, CommandError> {
    room_avatar_for(&state, room_id).await
}

/// One person's avatar in one room, as a data URL.
///
/// Asked for by the people drawn under a voice channel. See
/// `member_avatar_for` for why the room is part of the question.
#[tauri::command]
pub async fn member_avatar(
    state: State<'_, AppState>,
    room_id: String,
    user_id: String,
) -> Result<Option<String>, CommandError> {
    member_avatar_for(&state, room_id, user_id).await
}

/// What can be said about one person beyond their name.
///
/// Asked for when somebody opens a person's card. See `member_profile_for`.
#[tauri::command]
pub async fn member_profile(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<rooms::MemberProfile, CommandError> {
    member_profile_for(&state, user_id).await
}

/// Verify this session with the account's recovery key.
///
/// The one command in this file that takes a secret. It is not logged, not
/// stored, and not echoed back: it goes to the SDK, which uses it to open
/// secret storage and then drops it. Rejections are the interesting part, and
/// there are four different ones, because "that did not work" is a bad answer
/// to the likeliest mistake in the whole feature.
#[tauri::command]
pub async fn verification_recover(
    state: State<'_, AppState>,
    recovery_key: String,
) -> Result<(), CommandError> {
    verification_recover_for(&state, recovery_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RecordingSink;
    use consort_matrix::Backend;
    use consort_matrix::SessionStore;
    use consort_matrix::secrets::MemoryBackend;
    use std::sync::Arc;

    fn state() -> (tempfile::TempDir, AppState, Arc<MemoryBackend>) {
        let dir = tempfile::tempdir().unwrap();
        let backend = Arc::new(MemoryBackend::new());
        let store = SessionStore::with_backend(dir.path(), backend.clone());
        let settings = crate::settings::SettingsStore::at(dir.path());
        let state = AppState::new(store, settings, Arc::new(RecordingSink::new()));
        (dir, state, backend)
    }

    /// Joining and leaving a voice channel.
    ///
    /// What the call thread then does with the request is `crate::state` and
    /// `consort_call`; this is about the command's own contract.
    mod calls {
        use super::*;
        use crate::events::RecordingSink;
        use crate::testing::{FakeDevices, fake_backends};

        fn state() -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
            let dir = tempfile::tempdir().unwrap();
            let store = SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()));
            let sink = Arc::new(RecordingSink::new());
            let settings = crate::settings::SettingsStore::at(dir.path());
            (dir, AppState::new(store, settings, sink.clone()), sink)
        }

        #[tokio::test]
        async fn joining_a_call_while_signed_out_is_refused_rather_than_attempted() {
            // Not a call that failed. There is no account to publish a
            // membership under, so there is nothing to attempt and nothing to
            // report on the call channel.
            let (_dir, state, sink) = state();

            let error = call_connect_for(
                &state,
                &FakeDevices,
                fake_backends,
                "!a:example.org".to_owned(),
            )
            .await
            .expect_err("a signed-out join was accepted");

            assert!(
                error.message().to_lowercase().contains("signed in"),
                "{error:?}"
            );
            assert!(!state.has_call_thread());
            assert!(sink.events().is_empty());
        }

        #[test]
        fn leaving_a_call_while_signed_out_does_nothing() {
            // A disconnect control that outlived its session. Complaining
            // about it would put an error on screen for a click that asked
            // for the state the application is already in.
            let (_dir, state, sink) = state();

            call_disconnect_for(&state);

            assert!(!state.has_call_thread());
            assert!(sink.events().is_empty());
        }

        #[test]
        fn muting_before_there_is_a_call_thread_does_nothing() {
            // The controls live inside the call panel, so there is nothing to
            // press until a call exists. Reached anyway (an old webview, a
            // command sent by hand) this is the state being asked for, not an
            // error worth drawing.
            let (_dir, state, sink) = state();

            call_set_muted_for(&state, true);
            call_set_deafened_for(&state, true);
            call_set_away_for(&state, true);

            assert!(!state.has_call_thread());
            assert!(
                sink.events().is_empty(),
                "a press with nothing to apply it to was announced as though \
                 something had changed"
            );
        }
    }

    mod audio {
        use super::*;
        use consort_audio::{Device, Direction, GateConfig};

        /// A machine with one microphone and one pair of speakers.
        struct Fake;

        impl AudioDevices for Fake {
            fn enumerate(&self, direction: Direction) -> Vec<Device> {
                let name = match direction {
                    Direction::Input => "Yeti",
                    Direction::Output => "Headphones",
                };
                vec![Device {
                    name: name.to_owned(),
                    is_default: true,
                }]
            }
        }

        #[test]
        fn a_first_run_reports_the_defaults() {
            let (_dir, state, _) = state();

            assert_eq!(audio_settings_for(&state), AudioSettings::default());
        }

        #[test]
        fn what_was_set_is_what_comes_back() {
            let (_dir, state, _) = state();
            let chosen = AudioSettings {
                input: Some("Yeti".to_owned()),
                output: Some("Headphones".to_owned()),
                gate: GateConfig {
                    open_at: 0.8,
                    ..GateConfig::default()
                },
                call_sounds: true,
                call_voices: false,
                output_volume: 70,
                notification_volume: 25,
                ..AudioSettings::default()
            };

            set_audio_settings_for(&state, chosen.clone()).expect("save");

            assert_eq!(audio_settings_for(&state), chosen);
        }

        #[test]
        fn the_device_report_resolves_against_what_was_saved() {
            let (_dir, state, _) = state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("Yeti".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            let report = audio_devices_for(&state, &Fake);

            assert_eq!(report.input.selected.as_deref(), Some("Yeti"));
            assert_eq!(report.input.missing, None);
        }

        #[test]
        fn a_saved_device_that_is_no_longer_here_is_reported_as_missing() {
            let (_dir, state, _) = state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("Some Other Microphone".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            let report = audio_devices_for(&state, &Fake);

            assert_eq!(report.input.selected.as_deref(), Some("Yeti"));
            assert_eq!(
                report.input.missing.as_deref(),
                Some("Some Other Microphone"),
                "somebody whose headset is unplugged has to be told, not \
                 quietly switched"
            );
        }

        #[test]
        fn nothing_saved_reports_the_host_default() {
            let (_dir, state, _) = state();

            let report = audio_devices_for(&state, &Fake);

            assert_eq!(report.input.selected.as_deref(), Some("Yeti"));
            assert_eq!(report.output.selected.as_deref(), Some("Headphones"));
        }

        #[test]
        fn a_persons_volume_is_remembered() {
            // There is nowhere else it could be. No account data says "that one
            // is too loud in my headphones", so this file is the only thing
            // between somebody setting it and setting it again next week.
            let (_dir, state, _) = state();

            set_person_volume_for(&state, "@ada:example.org".to_owned(), 55).expect("save");

            assert_eq!(
                audio_settings_for(&state)
                    .person_volumes
                    .get("@ada:example.org"),
                Some(&55),
            );
        }

        #[test]
        fn putting_somebody_back_to_full_forgets_them_rather_than_writing_it_down() {
            // Full volume is the absence of a choice. Written down, the file
            // would grow a line for every person anybody ever nudged and put
            // back, and the mixer would multiply each of them by one.
            let (_dir, state, _) = state();
            set_person_volume_for(&state, "@ada:example.org".to_owned(), 55).expect("save");

            set_person_volume_for(&state, "@ada:example.org".to_owned(), 100).expect("save");

            assert!(audio_settings_for(&state).person_volumes.is_empty());
        }

        #[test]
        fn a_boost_is_written_down_rather_than_read_as_no_choice() {
            // The regression the "at or above full" test used to allow. While a
            // hundred was the top of the slider, anything at or over it meant
            // "nothing chosen"; now that somebody can be turned up, a level of
            // 150 taken that way would be dropped on the way to the file and
            // put the person straight back to full.
            let (_dir, state, _) = state();

            set_person_volume_for(&state, "@ada:example.org".to_owned(), 150).expect("save");

            assert_eq!(
                audio_settings_for(&state)
                    .person_volumes
                    .get("@ada:example.org"),
                Some(&150),
            );
        }

        #[test]
        fn a_level_above_the_ceiling_is_stored_as_the_ceiling() {
            // The mixer clamps on the way out, so anything higher would be
            // written down, read back, and drawn on a slider as a level
            // nothing is actually playing at.
            let (_dir, state, _) = state();

            set_person_volume_for(&state, "@ada:example.org".to_owned(), 255).expect("save");

            assert_eq!(
                audio_settings_for(&state)
                    .person_volumes
                    .get("@ada:example.org"),
                Some(&consort_audio::MAX_PERSON_VOLUME),
            );
        }

        #[test]
        fn changing_a_device_does_not_erase_everybodys_volume() {
            // The one field the settings screen does not hold and must not be
            // trusted to send back. It writes the whole audio section, so
            // taking its word for this would wipe every per-person level the
            // first time somebody picked a different microphone.
            let (_dir, state, _) = state();
            set_person_volume_for(&state, "@ada:example.org".to_owned(), 55).expect("save");

            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("Yeti".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            assert_eq!(
                audio_settings_for(&state)
                    .person_volumes
                    .get("@ada:example.org"),
                Some(&55),
                "the settings screen wrote over a level it never drew"
            );
        }

        #[test]
        fn the_volumes_are_saved_before_anything_is_told_about_them() {
            // The ordering every setter here shares, and the reason it is
            // worth a test: a mixer turned down against a file that failed to
            // save is a call that is quiet now and loud again on the next
            // launch, with nothing to explain either.
            let (_dir, state, _) = state();

            set_audio_settings_for(
                &state,
                AudioSettings {
                    output_volume: 40,
                    notification_volume: 20,
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            let saved = audio_settings_for(&state);
            assert_eq!(saved.output_volume, 40);
            assert_eq!(saved.notification_volume, 20);
        }

        #[test]
        fn saving_audio_settings_leaves_the_rest_of_the_file_alone() {
            // One section of one file. The call settings are hand-written, so
            // a microphone change that wiped them would take somebody's
            // deployment answer with it and leave a client that connects to
            // calls nobody can hear.
            let (_dir, state, _) = state();
            let stored = crate::settings::Settings {
                audio: AudioSettings::default(),
                calls: crate::settings::CallSettings {
                    fallback_dialect: consort_call::Dialect::State,
                    service_url_fallback: Some("https://example.org/sfu".to_owned()),
                },
            };
            state.settings().save(&stored).expect("save");

            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("Yeti".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            let loaded = state.settings().load();
            assert_eq!(loaded.audio.input.as_deref(), Some("Yeti"));
            assert_eq!(loaded.calls, stored.calls);
        }

        /// Backends that open whatever they are handed and report what that
        /// was, without touching a sound card.
        fn fake_backends() -> Backends {
            Backends {
                capture: Box::new(FakeCapture),
                playback: Box::new(FakePlayback),
            }
        }

        /// A capture backend that opens whatever it is handed and reports
        /// what that was.
        struct FakeCapture;

        /// The same, for the output side.
        struct FakePlayback;

        struct FakeTone {
            device: String,
        }

        impl consort_audio::PlaybackStream for FakeTone {
            fn device_name(&self) -> &str {
                &self.device
            }
        }

        impl consort_audio::AudioPlayback for FakePlayback {
            fn play(
                &self,
                device: Option<&str>,
                _tone: consort_audio::Tone,
                _on_end: consort_audio::ToneEnded,
            ) -> Result<Box<dyn consort_audio::PlaybackStream>, consort_audio::PlaybackError>
            {
                Ok(Box::new(FakeTone {
                    device: device.unwrap_or("<the host default>").to_owned(),
                }))
            }

            fn play_call(
                &self,
                device: Option<&str>,
                _voices: consort_audio::Voices,
            ) -> Result<Box<dyn consort_audio::PlaybackStream>, consort_audio::PlaybackError>
            {
                Ok(Box::new(FakeTone {
                    device: device.unwrap_or("<the host default>").to_owned(),
                }))
            }
        }

        struct FakeStream {
            device: String,
        }

        impl consort_audio::CaptureStream for FakeStream {
            fn device_name(&self) -> &str {
                &self.device
            }
        }

        impl consort_audio::AudioCapture for FakeCapture {
            fn open(
                &self,
                device: Option<&str>,
                _on_frame: consort_audio::FrameSink,
            ) -> Result<Box<dyn consort_audio::CaptureStream>, consort_audio::CaptureError>
            {
                Ok(Box::new(FakeStream {
                    // The name a `None` turns into, so a test can tell "open
                    // the default" apart from "open a device called Default".
                    device: device.unwrap_or("<the host default>").to_owned(),
                }))
            }
        }

        /// A state whose events can be read back.
        fn observable_state() -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
            let dir = tempfile::tempdir().expect("a temporary directory");
            let sink = Arc::new(RecordingSink::new());
            let state = AppState::new(
                SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new())),
                crate::settings::SettingsStore::at(dir.path()),
                sink.clone(),
            );
            (dir, state, sink)
        }

        /// Block until the audio thread has said something matching, or give
        /// up.
        fn audio_event(
            sink: &RecordingSink,
            wanted: impl Fn(&consort_audio::AudioEvent) -> bool,
        ) -> consort_audio::AudioEvent {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                let found = sink.events().into_iter().find_map(|event| match event {
                    crate::events::AppEvent::Audio(event) if wanted(&event) => Some(event),
                    _ => None,
                });
                if let Some(event) = found {
                    return event;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("the audio thread said nothing matching within five seconds");
        }

        /// Block until the audio thread has said something, or give up.
        fn first_audio_event(sink: &RecordingSink) -> consort_audio::AudioEvent {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if let Some(crate::events::AppEvent::Audio(event)) = sink
                    .events()
                    .into_iter()
                    .find(|event| matches!(event, crate::events::AppEvent::Audio(_)))
                {
                    return event;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("the microphone said nothing within five seconds");
        }

        #[test]
        fn a_first_run_opens_whatever_the_host_calls_its_default() {
            // The requirement this whole module exists for. Nothing has been
            // configured, so nothing is named, and the backend picks with
            // everything it knows about the machine rather than being handed a
            // name read out of a list.
            let (_dir, state, sink) = observable_state();

            audio_test_start_for(&state, &Fake, fake_backends);

            assert_eq!(
                first_audio_event(&sink),
                consort_audio::AudioEvent::Started {
                    device: "<the host default>".to_owned()
                }
            );
        }

        #[test]
        fn a_saved_device_that_is_not_the_default_is_opened_by_name() {
            let (_dir, state, sink) = observable_state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("Webcam".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            audio_test_start_for(&state, &TwoInputs, fake_backends);

            assert_eq!(
                first_audio_event(&sink),
                consort_audio::AudioEvent::Started {
                    device: "Webcam".to_owned()
                }
            );
        }

        #[test]
        fn a_saved_device_that_has_gone_falls_back_rather_than_failing() {
            // A headset unplugged between runs. Refusing to open anything
            // would leave somebody staring at a dead meter with no way to
            // work out that the list had moved on without them.
            let (_dir, state, sink) = observable_state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    input: Some("A Headset Somebody Unplugged".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            audio_test_start_for(&state, &Fake, fake_backends);

            assert_eq!(
                first_audio_event(&sink),
                consort_audio::AudioEvent::Started {
                    device: "<the host default>".to_owned()
                }
            );
        }

        #[test]
        fn stopping_releases_the_device() {
            let (_dir, state, sink) = observable_state();
            audio_test_start_for(&state, &Fake, fake_backends);
            first_audio_event(&sink);

            audio_test_stop_for(&state);

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if sink.events().iter().any(|event| {
                    matches!(
                        event,
                        crate::events::AppEvent::Audio(consort_audio::AudioEvent::Stopped)
                    )
                }) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("the microphone was never released");
        }

        #[test]
        fn stopping_a_microphone_that_was_never_started_is_not_an_error() {
            // Which the settings screen does every time it closes, whether or
            // not anybody pressed the test button.
            let (_dir, state, sink) = observable_state();

            audio_test_stop_for(&state);

            assert!(sink.events().is_empty());
        }

        #[test]
        fn the_test_tone_plays_out_of_whatever_the_host_calls_its_default() {
            // Same first-run requirement as the microphone. Nothing has been
            // configured, so nothing is named, and the backend picks.
            let (_dir, state, sink) = observable_state();

            audio_tone_play_for(&state, &Fake, fake_backends);

            assert_eq!(
                audio_event(&sink, |event| matches!(
                    event,
                    consort_audio::AudioEvent::ToneStarted { .. }
                )),
                consort_audio::AudioEvent::ToneStarted {
                    device: "<the host default>".to_owned()
                }
            );
        }

        #[test]
        fn a_saved_output_that_is_not_the_default_is_played_through_by_name() {
            let (_dir, state, sink) = observable_state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    output: Some("HDMI".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            audio_tone_play_for(&state, &TwoOutputs, fake_backends);

            assert_eq!(
                audio_event(&sink, |event| matches!(
                    event,
                    consort_audio::AudioEvent::ToneStarted { .. }
                )),
                consort_audio::AudioEvent::ToneStarted {
                    device: "HDMI".to_owned()
                }
            );
        }

        #[test]
        fn a_saved_output_that_has_gone_falls_back_rather_than_failing() {
            // Speakers unplugged between runs. Refusing to play would leave
            // somebody pressing a button that does nothing and no way to work
            // out why.
            let (_dir, state, sink) = observable_state();
            set_audio_settings_for(
                &state,
                AudioSettings {
                    output: Some("Speakers Somebody Unplugged".to_owned()),
                    ..AudioSettings::default()
                },
            )
            .expect("save");

            audio_tone_play_for(&state, &Fake, fake_backends);

            assert_eq!(
                audio_event(&sink, |event| matches!(
                    event,
                    consort_audio::AudioEvent::ToneStarted { .. }
                )),
                consort_audio::AudioEvent::ToneStarted {
                    device: "<the host default>".to_owned()
                }
            );
        }

        #[test]
        fn the_tone_resolves_the_output_and_not_the_input() {
            // The easy mistake, and an invisible one on a machine whose
            // default output happens to be listed first. `TwoOutputs` has a
            // microphone whose name is nothing like its speakers.
            let (_dir, state, sink) = observable_state();

            audio_tone_play_for(&state, &TwoOutputs, fake_backends);

            let consort_audio::AudioEvent::ToneStarted { device } = audio_event(&sink, |event| {
                matches!(event, consort_audio::AudioEvent::ToneStarted { .. })
            }) else {
                unreachable!()
            };
            assert_ne!(device, "A Microphone", "the chime went to the microphone");
        }

        #[test]
        fn stopping_a_tone_that_was_never_played_is_not_an_error() {
            // Which the settings screen does every time it closes, whether or
            // not anybody pressed the button.
            let (_dir, state, sink) = observable_state();

            audio_tone_stop_for(&state);

            assert!(sink.events().is_empty());
        }

        #[test]
        fn stopping_the_tone_releases_the_output() {
            let (_dir, state, sink) = observable_state();
            audio_tone_play_for(&state, &Fake, fake_backends);
            audio_event(&sink, |event| {
                matches!(event, consort_audio::AudioEvent::ToneStarted { .. })
            });

            audio_tone_stop_for(&state);

            audio_event(&sink, |event| {
                matches!(event, consort_audio::AudioEvent::ToneStopped)
            });
        }

        #[test]
        fn the_chime_and_the_microphone_share_one_thread_without_disturbing_it() {
            // They have to: a cpal stream is `!Send` in either direction, so
            // there is one thread that can hold either. The bug that shape
            // invites is one of them tearing down the other.
            let (_dir, state, sink) = observable_state();
            audio_test_start_for(&state, &Fake, fake_backends);
            audio_event(&sink, |event| {
                matches!(event, consort_audio::AudioEvent::Started { .. })
            });

            audio_tone_play_for(&state, &Fake, fake_backends);
            audio_event(&sink, |event| {
                matches!(event, consort_audio::AudioEvent::ToneStarted { .. })
            });

            assert!(
                !sink.events().iter().any(|event| matches!(
                    event,
                    crate::events::AppEvent::Audio(consort_audio::AudioEvent::Stopped)
                )),
                "playing the chime closed the microphone"
            );
        }

        /// A machine whose speakers and microphone are named nothing alike.
        struct TwoOutputs;

        impl AudioDevices for TwoOutputs {
            fn enumerate(&self, direction: Direction) -> Vec<Device> {
                match direction {
                    Direction::Input => vec![Device {
                        name: "A Microphone".to_owned(),
                        is_default: true,
                    }],
                    Direction::Output => vec![
                        Device {
                            name: "Built-in Speakers".to_owned(),
                            is_default: true,
                        },
                        Device {
                            name: "HDMI".to_owned(),
                            is_default: false,
                        },
                    ],
                }
            }
        }

        /// A machine with a default microphone and a second one.
        struct TwoInputs;

        impl AudioDevices for TwoInputs {
            fn enumerate(&self, direction: Direction) -> Vec<Device> {
                match direction {
                    Direction::Input => vec![
                        Device {
                            name: "Built-in".to_owned(),
                            is_default: true,
                        },
                        Device {
                            name: "Webcam".to_owned(),
                            is_default: false,
                        },
                    ],
                    Direction::Output => Vec::new(),
                }
            }
        }

        #[test]
        fn a_settings_error_reads_as_something_a_person_can_act_on() {
            let error = CommandError::from(crate::settings::SettingsError::Serialise(
                serde_json::from_str::<AudioSettings>("nonsense").unwrap_err(),
            ));

            assert_eq!(error.message(), "Your settings could not be saved.");
            assert!(
                !error.detail().is_empty(),
                "the console half has to carry the reason"
            );
        }
    }

    pub(super) fn status_name(status: &SessionStatus) -> &'static str {
        match status {
            SessionStatus::SignedOut => "signedOut",
            SessionStatus::SignedIn { .. } => "signedIn",
        }
    }

    /// The five verification actions, as `(name, run it)`.
    ///
    /// A list rather than five near-identical tests, because what is being
    /// checked is that none of them behaves differently, and five copies of
    /// one assertion is how the odd one out gets missed.
    #[allow(clippy::type_complexity)]
    pub(super) fn every_action() -> Vec<(
        &'static str,
        fn(
            &AppState,
            String,
            String,
        )
            -> std::pin::Pin<Box<dyn Future<Output = Result<(), CommandError>> + Send + '_>>,
    )> {
        vec![
            ("accept", |state, user, flow| {
                Box::pin(verification_accept_for(state, user, flow))
            }),
            ("start_sas", |state, user, flow| {
                Box::pin(verification_start_sas_for(state, user, flow))
            }),
            ("confirm", |state, user, flow| {
                Box::pin(verification_confirm_for(state, user, flow))
            }),
            ("mismatch", |state, user, flow| {
                Box::pin(verification_mismatch_for(state, user, flow))
            }),
            ("cancel", |state, user, flow| {
                Box::pin(verification_cancel_for(state, user, flow))
            }),
        ]
    }

    #[tokio::test]
    async fn asking_to_verify_this_session_needs_a_signed_in_one() {
        // Same reachability as the five actions below: the webview can invoke
        // this directly, and the sign-in screen is a page too.
        let (_dir, state, _) = state();

        let error = verification_verify_this_session_for(&state)
            .await
            .expect_err("asked to verify nobody");

        assert!(
            error.message().contains("No user is signed in"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn asking_what_there_is_to_verify_against_needs_a_signed_in_session() {
        let (_dir, state, _) = state();

        let error = verification_other_sessions_exist_for(&state)
            .await
            .expect_err("counted the sessions of nobody");

        assert!(
            error.message().contains("No user is signed in"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn asking_whether_there_is_a_recovery_key_needs_a_signed_in_session() {
        let (_dir, state, _) = state();

        let error = verification_recovery_exists_for(&state)
            .await
            .expect_err("asked about the recovery of nobody");

        assert!(
            error.message().contains("No user is signed in"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn offering_a_recovery_key_to_nobody_is_an_error_rather_than_a_panic() {
        let (_dir, state, _) = state();

        let error = verification_recover_for(&state, "a key".to_owned())
            .await
            .expect_err("recovered nobody");

        assert!(
            error.message().contains("No user is signed in"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn a_rejected_recovery_key_never_reaches_the_interface_as_itself() {
        // The one command that takes a secret. Whatever it says back, the key
        // must not be in it: the message is rendered on screen and the detail
        // goes to a console somebody may be screen-sharing.
        let (_dir, state, _) = state();
        let key = "EsTj 3yST y93F SLpB jJsz eAXc 2XzA ygD3 w69H fGaN TKBj jXEd";

        let error = verification_recover_for(&state, key.to_owned())
            .await
            .expect_err("recovered nobody");

        assert!(!error.message().contains("EsTj"), "{}", error.message());
        assert!(!error.detail().contains("EsTj"), "{}", error.detail());
    }

    #[tokio::test]
    async fn no_verification_action_works_without_a_signed_in_session() {
        // Reachable: the webview can invoke a command directly, and a signed
        // out app still has a page that could. Every one of these has to be an
        // error rather than an unwrap on `Option<Client>`.
        let (_dir, state, _) = state();

        for (name, run) in every_action() {
            let error = run(&state, "@bob:example.org".to_owned(), "flow".to_owned())
                .await
                .expect_err("{name} succeeded with nobody signed in");
            assert!(
                error.message().contains("No user is signed in"),
                "{name}: {}",
                error.message()
            );
        }
    }

    #[tokio::test]
    async fn no_stored_session_means_signed_out() {
        let (_dir, state, _) = state();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
    }

    #[tokio::test]
    async fn an_unparseable_session_file_is_discarded_and_reports_signed_out() {
        let (_dir, state, _) = state();
        std::fs::write(state.store().session_file(), b"{ not json").unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
        assert!(
            !state.store().session_file().exists(),
            "an unusable session file should be cleaned up"
        );
    }

    #[tokio::test]
    async fn a_keyring_failure_is_an_error_and_keeps_the_session_file() {
        // The regression guard for finding 5. Before this change, any error
        // reading the session deleted it, so a keyring that was briefly
        // unreachable logged the user out permanently.
        let (_dir, state, backend) = state();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();
        backend.start_failing("the session bus went away");

        let error = session_status_for(&state)
            .await
            .expect_err("a keyring failure should surface, not sign the user out");

        assert!(error.message().contains("keyring"));
        assert!(
            state.store().session_file().exists(),
            "the session file must survive a transient keyring failure"
        );
    }

    #[tokio::test]
    async fn metadata_with_no_tokens_reports_signed_out_without_erroring() {
        let (_dir, state, _) = state();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedOut");
    }

    #[tokio::test]
    async fn logging_out_with_no_client_still_clears_the_stored_session() {
        let (_dir, state, backend) = state();
        backend
            .set(
                "session-tokens:@bob:example.org",
                r#"{"access_token":"syt_x"}"#,
            )
            .unwrap();
        std::fs::write(
            state.store().session_file(),
            br#"{"homeserver":"https://example.org/","store_path":"/tmp/x","user_id":"@bob:example.org","device_id":"DEV"}"#,
        )
        .unwrap();

        logout_for(&state).await.unwrap();

        assert!(!state.store().session_file().exists());
        assert!(backend.is_empty());
    }

    #[tokio::test]
    async fn logging_out_twice_is_not_an_error() {
        let (_dir, state, _) = state();
        logout_for(&state).await.unwrap();
        logout_for(&state).await.unwrap();
    }

    #[tokio::test]
    async fn token_storage_reports_the_backend_in_use() {
        let (_dir, state, _) = state();

        let storage = token_storage_for(&state);

        assert_eq!(storage.kind, BackendKind::Memory);
        assert!(!storage.is_preferred);
        assert!(!storage.description.is_empty());
    }

    #[test]
    fn token_storage_serialises_with_the_field_names_the_frontend_expects() {
        let storage = TokenStorage {
            kind: BackendKind::File,
            description: BackendKind::File.description().to_owned(),
            is_preferred: false,
        };

        let json = serde_json::to_value(&storage).unwrap();

        assert_eq!(json.get("kind").unwrap(), "file");
        assert_eq!(json.get("isPreferred").unwrap(), false);
        assert!(json.get("description").is_some());
    }

    #[test]
    fn a_command_error_splits_the_person_facing_and_developer_facing_text() {
        let error: CommandError = consort_matrix::Error::InvalidServer("bad one".to_owned()).into();

        assert!(error.message().contains("bad one"));
        assert!(error.message().contains("does not look like"));
        assert!(error.detail().contains("bad one"));
    }

    #[test]
    fn a_command_error_never_shows_an_io_path_to_the_user() {
        let error: CommandError = consort_matrix::Error::SessionStore {
            path: std::path::PathBuf::from("/home/someone/.local/share/consort/session.json"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        }
        .into();

        assert!(!error.message().contains("/home/someone"));
        // The detail is for the console, and there the path is what you want.
        assert!(error.detail().contains("/home/someone"));
    }

    #[test]
    fn a_command_error_serialises_both_fields() {
        let error: CommandError = consort_matrix::Error::NotLoggedIn.into();
        let json = serde_json::to_value(&error).unwrap();

        assert!(json.get("message").is_some());
        assert!(json.get("detail").is_some());
    }

    #[test]
    fn session_status_serialises_as_a_tagged_union() {
        let json = serde_json::to_value(SessionStatus::SignedOut).unwrap();
        assert_eq!(json.get("status").unwrap(), "signedOut");

        let json = serde_json::to_value(SessionStatus::SignedIn {
            profile: Profile {
                user_id: "@bob:example.org".to_owned(),
                device_id: "DEV".to_owned(),
                homeserver: "https://example.org/".to_owned(),
                display_name: None,
                avatar_url: None,
            },
        })
        .unwrap();
        assert_eq!(json.get("status").unwrap(), "signedIn");
        assert!(json.get("profile").is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_logout_waits_for_the_first_rather_than_running_beside_it() {
        let (_dir, state, _) = state();
        let state = Arc::new(state);

        let held = state.lock_auth().await;
        let other = {
            let state = state.clone();
            tokio::spawn(async move { logout_for(&state).await })
        };

        // While the gate is held the second call cannot have finished.
        tokio::task::yield_now().await;
        assert!(!other.is_finished());

        drop(held);
        other.await.unwrap().unwrap();
    }
}

/// The command paths that need something answering like a homeserver.
///
/// Split from the unit tests above because they are slower and because they
/// need `MatrixMockServer`, which only exists with matrix-sdk's `testing`
/// feature. Without them `login_for` and the restore half of
/// `session_status_for` are only covered by running the app by hand.
#[cfg(test)]
mod against_a_mock_homeserver {
    use super::tests::status_name;
    use super::*;
    use crate::events::AppEvent;
    use crate::events::RecordingSink;
    use consort_call::CallEvent;
    use consort_matrix::secrets::MemoryBackend;
    use consort_matrix::{Connection, SessionStore, SessionVerification, StopReason};
    use matrix_sdk::ruma;
    use matrix_sdk::test_utils::mocks::{LoginResponseTemplate200, MatrixMockServer};
    use std::sync::Arc;

    const DEVICE: &str = "HZTIUXZKUU";

    fn state() -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()));
        let sink = Arc::new(RecordingSink::new());
        let settings = crate::settings::SettingsStore::at(dir.path());
        (dir, AppState::new(store, settings, sink.clone()), sink)
    }

    async fn mount_login(server: &MatrixMockServer) {
        server.mock_versions().ok().mount().await;
        server.mock_well_known().ok().mount().await;
        server
            .mock_login()
            .ok_with(LoginResponseTemplate200::new(
                "syt_first",
                DEVICE,
                ruma::user_id!("@bob:example.org"),
            ))
            .mount()
            .await;
        server.mock_upload_keys().ok().mount().await;
        server.mock_query_keys().ok().mount().await;
    }

    #[tokio::test]
    async fn joining_a_channel_this_account_is_not_in_says_so_on_the_call_channel() {
        // The command succeeds and the call fails, which is the split the whole
        // channel exists for: joining is a sequence of remote steps, so the
        // thing that starts it cannot be the thing that reports how it went.
        //
        // Also the one end-to-end check that the real transport is reached. A
        // room sync has never delivered is the failure that needs no SFU.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .expect("login");

        call_connect_for(
            &state,
            &crate::testing::FakeDevices,
            crate::testing::fake_backends,
            "!nowhere:example.org".to_owned(),
        )
        .await
        .expect("the command itself should succeed");

        let failure = crate::testing::wait_for_value("the call to fail", || {
            sink.events()
                .into_iter()
                .rev()
                .find_map(|event| match event {
                    AppEvent::Call(CallEvent::Failed { error, .. }) => Some(error),
                    _ => None,
                })
        });
        assert!(failure.contains("!nowhere:example.org"), "{failure}");
        assert!(failure.contains("sync"), "{failure}");

        // And the microphone did not stay open behind it.
        crate::testing::wait_for(
            "the microphone to be given back",
            || !state.microphone_open(),
            || "still open".to_owned(),
        );
    }

    #[tokio::test]
    async fn a_verification_action_on_a_flow_that_has_gone_is_reported_not_swallowed() {
        // Proves the delegation, which is the only thing these commands do:
        // the identifiers reach the SDK, it finds nothing, and the error comes
        // back as something the interface can render rather than a panic in a
        // command thread.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        for (name, run) in super::tests::every_action() {
            let error = run(&state, "@bob:example.org".to_owned(), "gone".to_owned())
                .await
                .expect_err("{name} succeeded on a flow that does not exist");
            assert!(
                error.message().contains("no longer"),
                "{name}: {}",
                error.message()
            );
        }
    }

    #[tokio::test]
    async fn a_successful_login_returns_the_profile() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();

        let profile = login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert_eq!(profile.user_id, "@bob:example.org");
        assert_eq!(profile.device_id, DEVICE);
    }

    #[tokio::test]
    async fn a_successful_login_leaves_the_client_in_state() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.client().await.is_some());
        assert!(
            state.has_refresh_task().await,
            "signing in should start persisting token rotations"
        );
    }

    #[tokio::test]
    async fn a_failed_login_leaves_no_client_behind() {
        let server = MatrixMockServer::new().await;
        server.mock_versions().ok().mount().await;
        server.mock_well_known().ok().mount().await;
        server
            .mock_login()
            .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "Invalid password" }),
            ))
            .mount()
            .await;
        let (_dir, state, _sink) = state();

        let error = login_for(&state, server.uri(), "bob".to_owned(), "wrong".to_owned())
            .await
            .unwrap_err();

        assert_eq!(error.message(), "Incorrect username or password.");
        assert!(state.client().await.is_none());
    }

    #[tokio::test]
    async fn asking_for_the_status_with_a_live_client_does_not_touch_the_store() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        std::fs::remove_file(state.store().session_file()).unwrap();

        let status = session_status_for(&state).await.unwrap();

        assert_eq!(status_name(&status), "signedIn");
    }

    #[tokio::test]
    async fn a_stored_session_is_restored_on_startup() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        // A second run of the app over the same data directory, sharing the
        // secret backend the way a real keyring would be shared.
        let fresh = AppState::new(
            first.store().clone(),
            first.settings().clone(),
            Arc::new(RecordingSink::new()),
        );

        let status = session_status_for(&fresh).await.unwrap();

        assert_eq!(status_name(&status), "signedIn");
        assert!(fresh.client().await.is_some());
        assert!(dir.path().exists());
    }

    #[tokio::test]
    async fn signing_out_clears_the_client_and_the_stored_session() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        logout_for(&state).await.unwrap();

        assert!(state.client().await.is_none());
        assert!(!state.has_refresh_task().await);
        assert!(state.store().load().unwrap().is_none());
        assert_eq!(
            status_name(&session_status_for(&state).await.unwrap()),
            "signedOut"
        );
    }

    /// Poll until the sink has seen a connection state matching `want`.
    ///
    /// The sync loop is a spawned task, so nothing it reports is available on
    /// the line after `login_for` returns.
    async fn wait_for_connection(sink: &RecordingSink, want: impl Fn(&Connection) -> bool) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if sink
                .events()
                .iter()
                .any(|event| matches!(event, crate::events::AppEvent::Connection(c) if want(c)))
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out; saw {:?}",
                sink.events()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Poll until the sink has seen a room list.
    async fn wait_for_rooms(sink: &RecordingSink) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if sink.last_rooms().is_some() {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out; saw {:?}",
                sink.events()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Poll until the sink has seen a verification state matching `want`.
    async fn wait_for_verification(
        sink: &RecordingSink,
        want: impl Fn(&SessionVerification) -> bool,
    ) {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if sink
                .events()
                .iter()
                .any(|event| matches!(event, crate::events::AppEvent::Verification(v) if want(v)))
            {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out; saw {:?}",
                sink.events()
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn signing_in_reports_the_session_unverified() {
        // A brand new device is signed by nobody. Saying so is the whole of
        // this milestone: until it is verified it cannot read encrypted
        // history and no encrypted call will have it.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.has_verification_task().await);
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
    }

    #[tokio::test]
    async fn a_restored_session_reports_its_verification_state_too() {
        // The startup path again. Restoring goes through a different function
        // from logging in, so it is its own way to end up with a session that
        // never says whether it is verified.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        let sink = Arc::new(RecordingSink::new());
        let fresh = AppState::new(
            first.store().clone(),
            first.settings().clone(),
            sink.clone(),
        );
        session_status_for(&fresh).await.unwrap();

        assert!(fresh.has_verification_task().await);
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
    }

    #[tokio::test]
    async fn signing_out_stops_the_verification_watcher() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;

        logout_for(&state).await.unwrap();

        assert!(!state.has_verification_task().await);
    }

    #[tokio::test]
    async fn signing_out_forgets_how_to_start_a_verification() {
        // The supervising task and the channel into it are two fields, and a
        // sign-out that cleared one and left the other would leave this
        // command publishing into a supervisor that has already been aborted:
        // no error, no flow, nothing on screen.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        // While signed in it fails for the honest reason: a mocked
        // `/keys/query` leaves the account with no cross-signing identity.
        // Whatever it says, it does not say nobody is signed in.
        let signed_in = verification_verify_this_session_for(&state)
            .await
            .expect_err("a mocked account has no identity to verify against");
        assert!(
            !signed_in.message().contains("No user is signed in"),
            "{}",
            signed_in.message()
        );

        logout_for(&state).await.unwrap();

        let signed_out = verification_verify_this_session_for(&state)
            .await
            .expect_err("asked to verify after signing out");
        assert!(
            signed_out.message().contains("No user is signed in"),
            "{}",
            signed_out.message()
        );
    }

    #[tokio::test]
    async fn a_signed_in_session_can_ask_what_there_is_to_verify_against() {
        // The mock lists this session and nothing else, which is the answer
        // that sends somebody to a recovery key rather than to a button that
        // can only time out.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server
            .mock_devices()
            .expect_any_access_token()
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "devices": [{ "device_id": DEVICE }] })),
            )
            .mount()
            .await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(!verification_other_sessions_exist_for(&state).await.unwrap());
    }

    #[tokio::test]
    async fn a_signed_in_session_can_ask_whether_there_is_a_recovery_key() {
        // The mocked account has no `m.secret_storage.default_key`, which is
        // what an account nobody has set recovery up on looks like. Paired
        // with the test above it is the dead end the banner has to name: no
        // other session, and no key either.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/_matrix/client/v3/user/@bob:example.org/account_data/m.secret_storage.default_key",
            ))
            .respond_with(
                wiremock::ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "errcode": "M_NOT_FOUND",
                    "error": "Account data not found",
                })),
            )
            .mount(server.server())
            .await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(!verification_recovery_exists_for(&state).await.unwrap());
    }

    #[tokio::test]
    async fn a_late_subscriber_can_ask_for_the_state_it_missed() {
        // The race this exists for: the tasks publish while the webview is
        // still loading, and a listener attached afterwards hears nothing
        // until the next change, which on a healthy session may be never.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_connection(&sink, |c| *c == Connection::Live).await;
        wait_for_verification(&sink, |v| *v == SessionVerification::Unverified).await;
        wait_for_rooms(&sink).await;
        let before = sink.events().len();

        state.resend_state();

        let resent = &sink.events()[before..];
        assert_eq!(
            resent.len(),
            4,
            "expected one state per channel, got {resent:?}"
        );
        assert_eq!(sink.last_connection(), Some(Connection::Live));
        assert_eq!(
            sink.last_verification(),
            Some(SessionVerification::Unverified)
        );
        assert!(sink.last_key_backup().is_some());
        assert!(
            sink.last_rooms().is_some(),
            "an account in no rooms still has a Home, and the shell needs to be told so"
        );
    }

    #[tokio::test]
    async fn signing_in_starts_the_sync_loop() {
        // Without this a signed-in Consort can be talked to and cannot hear:
        // to-device events, and therefore every verification request, arrive
        // through sync and nowhere else.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, _sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.has_sync_task().await);
    }

    #[tokio::test]
    async fn signing_in_starts_the_room_list_watcher() {
        // Its own task rather than a hook on the sync loop, so that the shell
        // has something to draw before the first sync response arrives.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, _sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert!(state.has_rooms_task().await);
    }

    #[tokio::test]
    async fn signing_in_tells_the_frontend_what_rooms_there_are() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        wait_for_rooms(&sink).await;
        let rooms = sink.last_rooms().unwrap();
        assert_eq!(
            rooms.spaces.len(),
            1,
            "an account in no rooms still has a Home: {rooms:?}"
        );
    }

    #[tokio::test]
    async fn asking_for_an_avatar_while_signed_out_says_so() {
        let (_dir, state, _sink) = state();

        let error = room_avatar_for(&state, "!a:example.org".to_owned())
            .await
            .unwrap_err();

        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn asking_for_the_avatar_of_a_room_we_are_not_in_is_not_an_error() {
        // Home is a rail entry rather than a room, and a room the account left
        // between the snapshot and the request is gone. Both end in initials,
        // which is the same place a room with no avatar ends.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert_eq!(
            room_avatar_for(&state, "home".to_owned()).await.unwrap(),
            None
        );
        assert_eq!(
            room_avatar_for(&state, "!gone:example.org".to_owned())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn asking_for_a_direct_message_while_signed_out_says_so() {
        let (_dir, state, _sink) = state();

        let error = direct_room_for(&state, "@ada:example.org".to_owned())
            .await
            .unwrap_err();

        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn asking_for_an_attachment_while_signed_out_says_so() {
        let (_dir, state, _sink) = state();

        let error = attachment_for(&state, "{}").await.unwrap_err();

        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn asking_for_an_attachment_by_something_that_is_not_a_handle_says_so() {
        // Only reachable by handing back something this build never wrote, so
        // what matters is that it is an answer rather than a panic.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        let error = attachment_for(&state, "nonsense").await.unwrap_err();

        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn asking_for_a_member_avatar_while_signed_out_says_so() {
        let (_dir, state, _sink) = state();

        let error = member_avatar_for(
            &state,
            "!a:example.org".to_owned(),
            "@ada:example.org".to_owned(),
        )
        .await
        .unwrap_err();

        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn asking_for_the_avatar_of_somebody_the_room_never_heard_of_is_not_an_error() {
        // A participant can arrive before the `m.room.member` that explains
        // them. The list still draws them, by initial, and asking about their
        // picture has to be as harmless as asking about a room with none.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        assert_eq!(
            member_avatar_for(
                &state,
                "!gone:example.org".to_owned(),
                "@ada:example.org".to_owned(),
            )
            .await
            .unwrap(),
            None
        );
        assert_eq!(
            member_avatar_for(&state, "home".to_owned(), "not a user".to_owned())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn signing_in_tells_the_frontend_the_connection_is_live() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, sink) = state();

        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        wait_for_connection(&sink, |c| *c == Connection::Live).await;
    }

    #[tokio::test]
    async fn a_restored_session_starts_syncing_too() {
        // The startup path. Restoring is the common case and it goes through
        // a different function from logging in, so it is its own way to end
        // up with a client that never syncs.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, first, _sink) = state();
        login_for(&first, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();

        let sink = Arc::new(RecordingSink::new());
        let fresh = AppState::new(
            first.store().clone(),
            first.settings().clone(),
            sink.clone(),
        );
        session_status_for(&fresh).await.unwrap();

        assert!(fresh.has_sync_task().await);
        wait_for_connection(&sink, |c| *c == Connection::Live).await;
    }

    #[tokio::test]
    async fn signing_out_stops_the_sync_loop_and_says_so() {
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().mount().await;
        let (_dir, state, sink) = state();
        login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
            .await
            .unwrap();
        wait_for_connection(&sink, |c| *c == Connection::Live).await;

        logout_for(&state).await.unwrap();

        assert!(!state.has_sync_task().await);
        assert_eq!(
            sink.last_connection(),
            Some(Connection::Stopped {
                reason: StopReason::SignedOut
            }),
            "the UI was left believing it was still connected"
        );
    }

    #[tokio::test]
    async fn a_second_sign_in_does_not_leave_the_first_loop_running() {
        // Two sync loops on one account is two clients holding the same
        // SQLite crypto store and two sets of to-device events being claimed.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (_dir, state, _sink) = state();
        let (client, _) = consort_matrix::auth::login(
            state.store(),
            &consort_matrix::Credentials {
                server: server.uri(),
                username: "bob".to_owned(),
                password: "hunter2".to_owned(),
            },
        )
        .await
        .unwrap();

        state.set_client(client.clone()).await;
        let first = state.sync_task_id().await;
        state.set_client(client).await;
        let second = state.sync_task_id().await;

        assert_ne!(first, second, "the second sign-in reused the first loop");
        assert!(state.has_sync_task().await);
    }

    #[tokio::test]
    async fn a_second_login_that_arrives_after_the_first_reuses_it() {
        // Both calls take the gate. The loser finds a client already in place
        // and must not register a second device on the homeserver.
        let server = MatrixMockServer::new().await;
        mount_login(&server).await;
        let (_dir, state, _sink) = state();
        let state = Arc::new(state);

        let one = {
            let state = state.clone();
            let uri = server.uri();
            tokio::spawn(async move {
                login_for(&state, uri, "bob".to_owned(), "hunter2".to_owned()).await
            })
        };
        let two = {
            let state = state.clone();
            let uri = server.uri();
            tokio::spawn(async move {
                login_for(&state, uri, "bob".to_owned(), "hunter2".to_owned()).await
            })
        };

        let first = one.await.unwrap().unwrap();
        let second = two.await.unwrap().unwrap();

        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.user_id, second.user_id);
    }

    /// Opening and closing a room, which is the only part of the timeline the
    /// app half owns.
    ///
    /// The reading, the ordering and the paging are `consort_matrix`'s and are
    /// tested there. What is here is the state machine around one watcher:
    /// that a room change replaces it rather than adding to it, that
    /// re-opening the room already open leaves it alone, and that signing out
    /// takes the previous account's conversation off the retained channel.
    mod timeline {
        use super::*;
        use consort_matrix::Timeline;

        const GENERAL: &str = "!general:example.org";
        const LOUNGE: &str = "!lounge:example.org";

        /// Every timeline the webview was handed, in order.
        fn published(sink: &RecordingSink) -> Vec<Timeline> {
            sink.events()
                .into_iter()
                .filter_map(|event| match event {
                    AppEvent::Timeline(timeline) => Some(timeline),
                    _ => None,
                })
                .collect()
        }

        /// Wait until `done` holds of what has been published, or give up.
        async fn until(sink: &Arc<RecordingSink>, done: impl Fn(&[Timeline]) -> bool) {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if done(&published(sink)) {
                    return;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "timed out; saw {:?}",
                    published(sink)
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }

        /// A signed-in state whose account is in both rooms above.
        async fn in_two_rooms(
            server: &MatrixMockServer,
        ) -> (tempfile::TempDir, AppState, Arc<RecordingSink>) {
            mount_login(server).await;
            let (dir, state, sink) = state();
            login_for(&state, server.uri(), "bob".to_owned(), "hunter2".to_owned())
                .await
                .unwrap();
            let client = state.client().await.unwrap();
            server
                .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
                .await;
            server
                .sync_joined_room(&client, ruma::room_id!("!lounge:example.org"))
                .await;
            server
                .mock_room_messages()
                .expect_any_access_token()
                .ok(matrix_sdk::test_utils::mocks::RoomMessagesResponseTemplate::default())
                .mount()
                .await;
            (dir, state, sink)
        }

        #[tokio::test]
        async fn opening_a_room_publishes_it() {
            let server = MatrixMockServer::new().await;
            let (_dir, state, sink) = in_two_rooms(&server).await;

            timeline_open_for(&state, GENERAL.to_owned()).await;

            until(&sink, |seen| {
                seen.iter().any(|timeline| timeline.room_id == GENERAL)
            })
            .await;
        }

        #[tokio::test]
        async fn opening_a_second_room_replaces_the_first() {
            // The reason a watcher is held rather than spawned and forgotten.
            // Two of them publishing to one channel would leave the pane
            // showing whichever answered last.
            let server = MatrixMockServer::new().await;
            let (_dir, state, sink) = in_two_rooms(&server).await;
            timeline_open_for(&state, GENERAL.to_owned()).await;
            until(&sink, |seen| !seen.is_empty()).await;

            timeline_open_for(&state, LOUNGE.to_owned()).await;
            until(&sink, |seen| {
                seen.last()
                    .is_some_and(|timeline| timeline.room_id == LOUNGE)
            })
            .await;

            // Long enough for a watcher that was left running to say
            // something, which is the failure this is about.
            let after_the_change = published(&sink).len();
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            let seen = published(&sink);
            assert!(
                seen[after_the_change..]
                    .iter()
                    .all(|timeline| timeline.room_id == LOUNGE),
                "the first room's watcher was still publishing: {seen:?}"
            );
        }

        #[tokio::test]
        async fn opening_the_room_already_open_leaves_it_alone() {
            // The shell re-selects a channel for reasons that are not a click:
            // a room list arriving re-derives the selection. Restarting the
            // watcher for one would throw away every page somebody had
            // scrolled back through.
            let server = MatrixMockServer::new().await;
            let (_dir, state, sink) = in_two_rooms(&server).await;
            timeline_open_for(&state, GENERAL.to_owned()).await;
            until(&sink, |seen| seen.iter().any(|one| !one.loading)).await;
            let settled = published(&sink).len();

            timeline_open_for(&state, GENERAL.to_owned()).await;

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            assert_eq!(
                published(&sink).len(),
                settled,
                "the room was re-read for a selection that had not changed"
            );
        }

        #[tokio::test]
        async fn closing_the_room_clears_what_a_late_subscriber_would_be_told() {
            let server = MatrixMockServer::new().await;
            let (_dir, state, sink) = in_two_rooms(&server).await;
            timeline_open_for(&state, GENERAL.to_owned()).await;
            until(&sink, |seen| !seen.is_empty()).await;

            timeline_close_for(&state);
            state.resend_state();

            assert!(
                !published(&sink)
                    .last()
                    .is_some_and(|timeline| timeline.room_id == GENERAL),
                "a closed room was still being handed to a late subscriber"
            );
        }

        #[tokio::test]
        async fn signing_out_takes_the_previous_account_s_conversation_away() {
            // What is retained here is somebody's messages, in full, waiting
            // for whatever asks to be caught up next. Signing in as a second
            // account must not be handed the first account's room.
            let server = MatrixMockServer::new().await;
            let (_dir, state, sink) = in_two_rooms(&server).await;
            timeline_open_for(&state, GENERAL.to_owned()).await;
            until(&sink, |seen| !seen.is_empty()).await;

            state.clear_client().await;

            assert_eq!(
                published(&sink)
                    .last()
                    .map(|timeline| timeline.room_id.clone()),
                Some(String::new()),
                "the room was not cleared on the way out"
            );
        }

        #[tokio::test]
        async fn opening_a_room_while_signed_out_says_nothing() {
            // A stale click, not something to complain about. There is nothing
            // to read a room out of and no interface to draw it in.
            let (_dir, state, sink) = state();

            timeline_open_for(&state, GENERAL.to_owned()).await;

            assert!(published(&sink).is_empty());
        }

        #[tokio::test]
        async fn asking_for_more_with_no_room_open_does_nothing() {
            // What a scroll landing at the same moment as a room change is.
            let (_dir, state, _sink) = state();

            timeline_earlier_for(&state);
            timeline_close_for(&state);
        }

        #[tokio::test]
        async fn sending_while_signed_out_is_refused_before_anything_is_parsed() {
            let (_dir, state, _sink) = state();

            let refused = timeline_send_for(&state, GENERAL.to_owned(), "hello".to_owned())
                .await
                .unwrap_err();

            assert_eq!(
                refused.message,
                consort_matrix::Error::NotLoggedIn.user_message()
            );
        }
    }
}
