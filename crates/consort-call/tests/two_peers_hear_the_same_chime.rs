// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Two Consort peers in one call, and the chime one of them sent arriving at
//! the other as the chime rather than as a connection.
//!
//! The gate ADR-0021 asks for, and the only coverage `livekit.rs` has.
//! `docs/CALL-HARNESS.md` has the stack, how to run it, and how it is proven
//! able to fail.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use consort_audio::{FRAME_SAMPLES, SAMPLE_RATE, Tone};
use consort_call::{
    CallSession, CallTransport, Cue, Dialect, Ears, Heard, LiveKitTransport, PublishedAudio,
};
use consort_matrix::secrets::MemoryBackend;
use consort_matrix::{CallReadiness, Connection, Credentials, SessionStore, sync};

/// Where `testing/call/up.sh` puts the homeserver and the JWT service.
const HOMESERVER: &str = "CONSORT_TEST_CALL_HOMESERVER";
const SFU: &str = "CONSORT_TEST_CALL_SFU";

/// What the CI job greps the output for, so a renamed variable above fails the
/// job instead of skipping quietly.
const RECEIPT: &str = "the chime arrived:";

/// The accounts are registered per run, so the password is only ever typed
/// here.
const PASSWORD: &str = "consort-test-only";

// ─── what the chime looks like on arrival ────────────────────────────────────

/// Deciding whether a buffer of PCM is [`Tone`], by what is in it rather than
/// by how much of it there is.
///
/// Opus is lossy and an SFU may resample, so sample equality is not available
/// at any price. Frequency content is, and it is the stronger assertion
/// anyway: it refuses silence, noise, a steady hum and the wrong notes.
mod chime {
    use consort_audio::SAMPLE_RATE;

    /// The two notes [`consort_audio::Tone`] plays, in hertz.
    const NOTES: [f32; 2] = [440.0, 659.25];

    /// Frequencies the chime does not contain, and a harmonic of neither note.
    ///
    /// A harmonic would be found in the real signal, so one in here would
    /// quietly stop the margin below from meaning anything.
    const CONTROLS: [f32; 4] = [311.0, 523.25, 1100.0, 1550.0];

    /// One window, in samples: a quarter of a note, so a window sits inside
    /// one note rather than straddling the join between the two.
    const WINDOW: usize = SAMPLE_RATE as usize * 40 / 1000;

    /// How far above every control a note has to be in a window to count.
    const MARGIN: f64 = 4.0;

    /// How loud a note has to be in a window to count, as an `i16` amplitude.
    ///
    /// `Tone` peaks near 7200, so this is about a ninth of what arrives
    /// intact: low enough to survive a codec, high enough to refuse a buffer
    /// that is nearly quiet.
    const FLOOR: f64 = 800.0;

    /// How many windows each note needs. Two seconds of the chime is about
    /// twenty-four of each, so this is comfortable but not free.
    const ENOUGH: usize = 10;

    /// What a buffer of PCM turned out to contain.
    #[derive(Debug)]
    pub struct Verdict {
        /// Windows carrying each note, in the order of `NOTES`.
        pub notes: [usize; 2],
        /// Windows examined.
        pub windows: usize,
        /// The loudest either note got in any window.
        pub loudest_note: f64,
        /// The loudest any control got in any window.
        pub loudest_control: f64,
    }

    impl std::fmt::Display for Verdict {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                out,
                "{} windows, {} at 440 Hz and {} at 659 Hz, loudest note {:.0} \
                 against loudest control {:.0}",
                self.windows, self.notes[0], self.notes[1], self.loudest_note, self.loudest_control
            )
        }
    }

    /// The amplitude of `hz` in `window`, on the scale the samples are on.
    ///
    /// Goertzel rather than an FFT: six lines, no dependency, and only two
    /// frequencies are ever asked about.
    fn amplitude(window: &[i16], hz: f32) -> f64 {
        let turn = 2.0 * std::f64::consts::PI * f64::from(hz) / f64::from(SAMPLE_RATE);
        let coefficient = 2.0 * turn.cos();

        let (mut last, mut before) = (0.0_f64, 0.0_f64);
        for &sample in window {
            let next = f64::from(sample) + coefficient * last - before;
            before = last;
            last = next;
        }

        let real = last - before * turn.cos();
        let imaginary = before * turn.sin();
        2.0 * real.hypot(imaginary) / window.len() as f64
    }

    /// Examine `samples` window by window.
    pub fn examine(samples: &[i16]) -> Verdict {
        let mut verdict = Verdict {
            notes: [0, 0],
            windows: 0,
            loudest_note: 0.0,
            loudest_control: 0.0,
        };

        for window in samples.as_chunks::<WINDOW>().0 {
            verdict.windows += 1;

            let controls = CONTROLS
                .iter()
                .map(|&hz| amplitude(window, hz))
                .fold(0.0_f64, f64::max);
            verdict.loudest_control = verdict.loudest_control.max(controls);

            for (nth, &hz) in NOTES.iter().enumerate() {
                let note = amplitude(window, hz);
                verdict.loudest_note = verdict.loudest_note.max(note);
                if note > FLOOR && note > MARGIN * controls {
                    verdict.notes[nth] += 1;
                }
            }
        }

        verdict
    }

    /// Whether `samples` are the chime, and what was measured either way.
    ///
    /// Both notes, each in enough windows of its own, is what makes this the
    /// chime rather than any loud thing: a steady tone at one of them fails,
    /// and so does noise, which puts as much in the controls as in the notes.
    pub fn heard_in(samples: &[i16]) -> Result<Verdict, String> {
        let verdict = examine(samples);

        if samples.len() < SAMPLE_RATE as usize {
            return Err(format!(
                "only {} samples arrived, under a second of audio ({verdict})",
                samples.len()
            ));
        }
        if verdict.notes[0] < ENOUGH || verdict.notes[1] < ENOUGH {
            return Err(format!(
                "that is not the chime: each note needs {ENOUGH} windows ({verdict})"
            ));
        }
        if verdict.loudest_control * MARGIN > verdict.loudest_note {
            return Err(format!(
                "the controls are as loud as the notes, so that is noise ({verdict})"
            ));
        }

        Ok(verdict)
    }
}

/// Fill `out` with `count` samples of the chime, looping it.
fn chime_pcm(count: usize) -> Vec<i16> {
    let mut out = vec![0_i16; count];
    let mut tone = Tone::check();
    for frame in out.chunks_mut(FRAME_SAMPLES) {
        if !tone.fill(frame) {
            tone = Tone::check();
        }
    }
    out
}

// ─── the detector can tell the difference, with no harness at all ────────────
//
// These run in every `cargo test --workspace`. They are what stops the live
// test below from being a green light that would stay green on silence: the
// predicate it asserts is the predicate they prove discriminates.

#[test]
fn the_chime_is_recognised() {
    let verdict = chime::heard_in(&chime_pcm(SAMPLE_RATE as usize * 2))
        .expect("the chime itself should be recognised as the chime");

    // Eleven to one when measured, and the floor is here so that a change
    // which quietly narrowed it would be noticed rather than tolerated. The
    // leakage is the envelope's: 523 Hz sits between the two notes and picks
    // up the transition.
    assert!(
        verdict.loudest_note > 8.0 * verdict.loudest_control,
        "the margin is thinner than expected: {verdict}"
    );
}

#[test]
fn silence_is_not_the_chime() {
    let error = chime::heard_in(&vec![0_i16; SAMPLE_RATE as usize * 2])
        .expect_err("silence must not pass for the chime");

    assert!(error.contains("not the chime"), "{error}");
}

#[test]
fn noise_as_loud_as_the_chime_is_not_the_chime() {
    // A buffer nobody could mistake for quiet, so only the shape of the
    // spectrum can refuse it.
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let noise: Vec<i16> = (0..SAMPLE_RATE as usize * 2)
        .map(|_| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 40) as i16).wrapping_sub(4096)
        })
        .collect();

    let error = chime::heard_in(&noise).expect_err("noise must not pass for the chime");

    assert!(
        error.contains("not the chime") || error.contains("noise"),
        "{error}"
    );
}

#[test]
fn a_steady_tone_at_one_of_the_notes_is_not_the_chime() {
    // The sharpest case, and the reason both notes are required. A badly
    // grounded audio interface produces exactly this on its own, which is why
    // `Tone` has two notes in the first place.
    let hum: Vec<i16> = (0..SAMPLE_RATE as usize * 2)
        .map(|at| {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * at as f32 / SAMPLE_RATE as f32;
            (0.22 * phase.sin() * f32::from(i16::MAX)) as i16
        })
        .collect();

    let error = chime::heard_in(&hum).expect_err("a steady hum must not pass for the chime");

    assert!(error.contains("not the chime"), "{error}");
}

#[test]
fn the_chime_at_the_wrong_pitch_is_not_the_chime() {
    let wrong: Vec<i16> = (0..SAMPLE_RATE as usize * 2)
        .map(|at| {
            let hz = if (at / 7680) % 2 == 0 { 300.0 } else { 900.0 };
            let phase = 2.0 * std::f32::consts::PI * hz * at as f32 / SAMPLE_RATE as f32;
            (0.22 * phase.sin() * f32::from(i16::MAX)) as i16
        })
        .collect();

    let error = chime::heard_in(&wrong).expect_err("the wrong notes must not pass for the chime");

    assert!(error.contains("not the chime"), "{error}");
}

// ─── the live call ───────────────────────────────────────────────────────────

/// Everything the other peer was heard saying, by queue.
#[derive(Default)]
struct Recorder {
    heard: Mutex<Vec<(String, Vec<i16>)>>,
}

impl Recorder {
    /// Throw away what has been collected so far.
    ///
    /// Called once the media key has come into use. A key is not used for five
    /// seconds after it is distributed (MSC4143's `delayBeforeUse`), so the
    /// start of any real call is silence that nothing is wrong with.
    fn clear(&self) {
        self.heard.lock().unwrap().clear();
    }

    /// The samples of the one queue that was heard from, and its name.
    fn only(&self) -> Option<(String, Vec<i16>)> {
        let heard = self.heard.lock().unwrap();
        match heard.len() {
            1 => Some(heard[0].clone()),
            _ => None,
        }
    }

    fn queues(&self) -> Vec<String> {
        self.heard
            .lock()
            .unwrap()
            .iter()
            .map(|(who, _)| who.clone())
            .collect()
    }
}

impl Heard for Recorder {
    fn hear(&self, who: &str, samples: &[i16]) {
        let mut heard = self.heard.lock().unwrap();
        match heard.iter_mut().find(|(name, _)| name == who) {
            Some((_, collected)) => collected.extend_from_slice(samples),
            None => heard.push((who.to_owned(), samples.to_vec())),
        }
    }

    // A call that ends while this is collecting would otherwise erase the
    // evidence, and the assertion is about what arrived rather than what is
    // still queued.
    fn forget(&self, _who: &str) {}
    fn silence(&self) {}
    fn cue(&self, _cue: Cue) {}
    fn attribute(&self, _whose: &[(String, String)]) {}
}

/// Where the harness is, when there is one.
struct Harness {
    homeserver: String,
    sfu: String,
}

impl Harness {
    fn from_env() -> Option<Self> {
        Some(Self {
            homeserver: std::env::var(HOMESERVER).ok()?,
            sfu: std::env::var(SFU).ok()?,
        })
    }
}

/// A store in a fresh directory, so a login is a new device.
fn store(dir: &tempfile::TempDir) -> SessionStore {
    SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()))
}

/// Register an account nobody has used before.
///
/// Per run, and that is not tidiness. Only a first login keeps the
/// cross-signing private keys, and only a session that holds them can
/// distribute a media key: `consort_matrix::calls::readiness` has the
/// measurement.
async fn a_brand_new_account(homeserver: &str, prefix: &str) -> Credentials {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let username = format!("{prefix}-{stamp}");

    let client = matrix_sdk::Client::builder()
        .homeserver_url(homeserver)
        .build()
        .await
        .expect("could not reach the test homeserver");

    let mut request = matrix_sdk::ruma::api::client::account::register::v3::Request::new();
    request.username = Some(username.clone());
    request.password = Some(PASSWORD.to_owned());
    request.inhibit_login = true;
    request.auth = Some(matrix_sdk::ruma::api::client::uiaa::AuthData::Dummy(
        matrix_sdk::ruma::api::client::uiaa::Dummy::new(),
    ));

    client
        .matrix_auth()
        .register(request)
        .await
        .expect("registration failed; testing/call/up.sh turns it on");

    Credentials {
        server: homeserver.to_owned(),
        username,
        password: PASSWORD.to_owned(),
    }
}

/// Wait for `check` to hold, or fail saying what was being waited for.
async fn until(what: &str, deadline: Duration, mut check: impl FnMut() -> bool) {
    let giving_up = tokio::time::Instant::now() + deadline;
    loop {
        if check() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < giving_up,
            "gave up waiting for {what} after {deadline:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[test]
fn two_peers_in_one_call_hear_the_chime() {
    let Some(harness) = Harness::from_env() else {
        println!("skipped: testing/call/up.sh prints the {HOMESERVER} and {SFU} to export");
        return;
    };

    // Both the ring and the aws-lc-rs backends are linked once livekit is in
    // the graph, and rustls refuses to pick for itself.
    consort_matrix::install_crypto_provider();

    // A current-thread runtime with a `LocalSet`, because `listen` pumps
    // audio with `spawn_local`. This is the shape `app/src-tauri/src/state.rs`
    // gives a call for the same reason.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let local = tokio::task::LocalSet::new();

    local.block_on(&runtime, a_call(harness));
}

async fn a_call(harness: Harness) {
    let (sender_dir, listener_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let sender_account = a_brand_new_account(&harness.homeserver, "chime-sender").await;
    let listener_account = a_brand_new_account(&harness.homeserver, "chime-listener").await;

    let (sender, _) = consort_matrix::auth::login(&store(&sender_dir), &sender_account)
        .await
        .expect("the sender could not sign in");
    let (listener, listener_profile) =
        consort_matrix::auth::login(&store(&listener_dir), &listener_account)
            .await
            .expect("the listener could not sign in");

    let sink = |seen: Arc<Mutex<Vec<Connection>>>| {
        move |state: Connection| seen.lock().unwrap().push(state)
    };
    let sender_live = Arc::new(Mutex::new(Vec::new()));
    let listener_live = Arc::new(Mutex::new(Vec::new()));
    let sender_sync = sync::start(sender.clone(), sink(sender_live.clone()));
    let listener_sync = sync::start(listener.clone(), sink(listener_live.clone()));

    until("both peers to be syncing", Duration::from_secs(60), || {
        sender_live.lock().unwrap().contains(&Connection::Live)
            && listener_live.lock().unwrap().contains(&Connection::Live)
    })
    .await;

    // A precondition rather than the thing under test, and worth asserting
    // separately: a session that cannot distribute a media key joins a call
    // that connects perfectly and is heard by nobody, which is the exact
    // failure this test would otherwise be reporting as a transport fault.
    for (who, client) in [("sender", &sender), ("listener", &listener)] {
        let readiness = consort_matrix::calls::readiness(client)
            .await
            .expect("could not ask whether media keys will be distributed");
        assert_eq!(
            readiness,
            CallReadiness::Ready,
            "the {who} cannot distribute media keys, so nothing could be heard"
        );
    }

    let room_id = shared_room(&sender, &listener, &listener_profile.user_id).await;

    // The sender first, into an empty room, so detection has nothing to go on
    // and both peers take the fallback. Then the listener, which sees one live
    // membership and reaches the same answer the other way.
    let sending = LiveKitTransport::new(sender.clone(), Dialect::State, Some(harness.sfu.clone()));
    let listening =
        LiveKitTransport::new(listener.clone(), Dialect::State, Some(harness.sfu.clone()));

    let sender_call = sending
        .join(&room_id)
        .await
        .expect("the sender could not join the call");
    let track = sender_call
        .publish_microphone()
        .await
        .expect("the sender could not publish a microphone");

    // Looping, and seamlessly: `Tone::SAMPLES` is a whole number of frames, so
    // the restart lands on the first sample of the first note. The transport
    // paces this, so there is no sleep in here and must not be one.
    let publishing = tokio::task::spawn_local(async move {
        let mut tone = Tone::check();
        let mut frame = vec![0_i16; FRAME_SAMPLES];
        loop {
            if !tone.fill(&mut frame) {
                tone = Tone::check();
            }
            if track.send(frame.clone()).await.is_err() {
                return;
            }
        }
    });

    let listener_call = listening
        .join(&room_id)
        .await
        .expect("the listener could not join the call");

    let recorder = Arc::new(Recorder::default());
    let ears: Ears = Arc::clone(&recorder) as Ears;

    // `listen` is idempotent by contract and is called on every roster change
    // in the application, so calling it on a tick is what the application
    // does, not a shortcut around it.
    let attached = {
        let mut attached = 0;
        until(
            "the listener to attach to the sender",
            Duration::from_secs(60),
            || {
                attached = listener_call.listen(&ears).playing;
                attached > 0
            },
        )
        .await;
        attached
    };
    assert_eq!(attached, 1, "expected one stream to pull, not {attached}");

    // Long enough for `delayBeforeUse` plus a rotation, then start again from
    // nothing so the measurement is of a settled call.
    tokio::time::sleep(Duration::from_secs(12)).await;
    recorder.clear();

    tokio::time::sleep(Duration::from_secs(4)).await;

    let queues = recorder.queues();
    let (who, samples) = recorder.only().unwrap_or_else(|| {
        panic!("expected to hear exactly one peer, heard {queues:?}");
    });

    let verdict = chime::heard_in(&samples).unwrap_or_else(|why| panic!("{why}"));

    // And the other half of the same measurement: what the SFU is carrying is
    // not what the peer is hearing. Without this, everything above would pass
    // just as well on a call with no encryption at all, which is how the
    // unencrypted room this test used to make went unnoticed.
    let wire = eavesdrop(&harness.sfu, &room_id, &listener).await;
    let on_the_wire = chime::examine(&wire);
    // Before the refusal, so that "not the chime" is a statement about real
    // traffic rather than about an empty buffer.
    assert!(
        wire.len() >= SAMPLE_RATE as usize,
        "the eavesdropper collected under a second, so it proves nothing"
    );
    assert!(
        chime::heard_in(&wire).is_err(),
        "the chime is audible on the wire, so the media is not encrypted: {on_the_wire}"
    );

    println!(
        "{RECEIPT} {} samples from {who} in a {} call, and {verdict}. \
         On the wire it is not the chime: {on_the_wire}",
        samples.len(),
        Dialect::State.name(),
    );

    publishing.abort();
    sender_call
        .leave()
        .await
        .expect("the sender could not leave");
    listener_call
        .leave()
        .await
        .expect("the listener could not leave");
    sender_sync.abort();
    listener_sync.abort();
}

/// Join the same LiveKit room with no end to end encryption at all, and report
/// what is on the wire.
///
/// A token for this account under a device id of its own, which is all the SFU
/// asks for. The point is what a colluding SFU, or anybody who can get a token
/// for the room, can hear.
async fn eavesdrop(sfu: &str, room_id: &str, as_client: &matrix_sdk::Client) -> Vec<i16> {
    use futures_util::StreamExt;
    use matrix_sdk::ruma::api::client::account::request_openid_token;

    let user_id = as_client.user_id().unwrap().to_owned();
    let openid = as_client
        .send(request_openid_token::v3::Request::new(user_id))
        .await
        .expect("an openid token");

    let body = serde_json::json!({
        "room": room_id,
        "openid_token": {
            "access_token": openid.access_token,
            "matrix_server_name": openid.matrix_server_name.to_string(),
            "token_type": "Bearer",
            "expires_in": 3600,
        },
        "device_id": "EAVESDROP",
    });
    let text = matrix_sdk::reqwest::Client::new()
        .post(format!("{sfu}/sfu/get"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&body).unwrap())
        .send()
        .await
        .expect("the jwt service answered")
        .text()
        .await
        .expect("a body");
    let answer: serde_json::Value = serde_json::from_str(&text).expect("json");
    let url = answer["url"].as_str().expect("a url").to_owned();
    let jwt = answer["jwt"].as_str().expect("a jwt").to_owned();

    let (_room, mut events) = livekit::Room::connect(&url, &jwt, livekit::RoomOptions::default())
        .await
        .expect("the eavesdropper could not connect");

    let mut stream = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
            Ok(Some(livekit::RoomEvent::TrackSubscribed { track, .. })) => {
                if let livekit::track::RemoteTrack::Audio(audio) = track {
                    stream = Some(
                        livekit::webrtc::audio_stream::native::NativeAudioStream::new(
                            audio.rtc_track(),
                            48_000,
                            1,
                        ),
                    );
                    break;
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    let mut stream = stream.expect("the eavesdropper never got an audio track");
    let mut collected = Vec::new();
    let until = tokio::time::Instant::now() + Duration::from_secs(4);
    while tokio::time::Instant::now() < until {
        match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
            Ok(Some(frame)) => collected.extend_from_slice(&frame.data),
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    collected
}

/// One room both peers have joined.
async fn shared_room(
    sender: &matrix_sdk::Client,
    listener: &matrix_sdk::Client,
    listener_user_id: &str,
) -> String {
    use matrix_sdk::ruma::api::client::room::create_room;

    let mut request = create_room::v3::Request::new();
    // A state event defaults to needing power level 50, and the second peer
    // joins as an ordinary member, so without this its membership is refused
    // with a 403 and the call it joined has nobody in it. Element Call writes
    // the same entry when it makes a room.
    request.power_level_content_override = Some(
        matrix_sdk::ruma::serde::Raw::from_json_string(
            r#"{"events":{"org.matrix.msc3401.call.member":0}}"#.to_owned(),
        )
        .expect("a literal"),
    );
    request.invite = vec![
        matrix_sdk::ruma::UserId::parse(listener_user_id)
            .expect("the listener's own user id")
            .to_owned(),
    ];
    let room = sender
        .create_room(request)
        .await
        .expect("could not make a room for the call");

    // Encrypted, and that is the whole point rather than caution. Frame
    // encryption follows the room's own encryption state (spec 038 R11), so in
    // a plain room there is no media key to distribute, nothing to get wrong,
    // and no assertion here would notice. Found by mutation: in an unencrypted
    // room a sender with no media key at all is heard perfectly.
    room.enable_encryption()
        .await
        .expect("could not turn encryption on");

    until("the invite to arrive", Duration::from_secs(60), || {
        listener.get_room(room.room_id()).is_some()
    })
    .await;

    listener
        .join_room_by_id(room.room_id())
        .await
        .expect("the listener could not join the room");

    until(
        "both peers to be in the room",
        Duration::from_secs(60),
        || {
            listener
                .get_room(room.room_id())
                .is_some_and(|room| room.state() == matrix_sdk::RoomState::Joined)
        },
    )
    .await;

    // Both sides, because the sender decides whether to carry a media key and
    // the listener decides whether to expect one.
    for (who, client) in [("sender", sender), ("listener", listener)] {
        let theirs = client
            .get_room(room.room_id())
            .expect("the room this peer just joined");
        let state = theirs
            .latest_encryption_state()
            .await
            .expect("could not read the room's encryption state");
        assert!(
            state.is_encrypted(),
            "the {who} does not see an encrypted room, so no media key would be carried"
        );
    }

    room.room_id().to_string()
}
