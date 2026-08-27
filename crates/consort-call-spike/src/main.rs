#![recursion_limit = "512"]

//! Phase 0 of `docs/PLAN-voice-call.md`. Throwaway.
//!
//! Four questions, each of which changes the plan if it goes the wrong way:
//!
//! 1. Does the matrix-sdk rev unify between this workspace and
//!    matrix-rtc-livekit? Answered at compile time by [`one_sdk_only`] and by
//!    handing `Call::join` a `Room` that came through consort-matrix.
//! 2. Does the `[patch.crates-io]` block have to be copied? Answered by this
//!    crate building at all, since nothing here replicates it.
//! 3. Does `Call::join` need `matrix_sdk_ui::sync_service::SyncService`, or
//!    will an ordinary `Client::sync` do? `SYNC=plain` against `SYNC=service`.
//! 4. Does the pre-MSC4354 dialect need an open slot? `OPEN_SLOT=1` opens one;
//!    leaving it unset says whether joining works without.
//!
//! And the fifth question the plan raised separately: what an uncross-signed
//! device does about MSC4153. This binary logs in fresh, so its device is
//! exactly that until somebody verifies it.
//!
//! ```sh
//! HOMESERVER_URL=https://example.org MX_USER=someone MX_PASSWORD=... \
//! ROOM_ID='!room:example.org' LIVEKIT_SERVICE_URL=https://rtc.example.org/livekit/jwt \
//! COMPAT=state SYNC=plain PUBLISH_TONE=1 \
//! cargo run -p consort-call-spike
//! ```

use std::env;
use std::error::Error;
use std::f32::consts::TAU;
use std::time::Duration;

use matrix_rtc_livekit::compat::ElementCallCompat;
use matrix_rtc_livekit::{Call, CallOptions, open_slot};
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::api::client::account::register;
use matrix_sdk::ruma::api::client::room::create_room::v3::Request as CreateRoomRequest;
use matrix_sdk::ruma::api::client::uiaa;
use matrix_sdk::ruma::events::InitialStateEvent;
use matrix_sdk::ruma::events::room::history_visibility::{
    HistoryVisibility, RoomHistoryVisibilityEventContent,
};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};
use matrix_rtc_media::{AudioFrame, PublishOptions};

use consort_audio::{FRAME_SAMPLES, SAMPLE_RATE};

/// The compile-time half of question one.
///
/// `consort_matrix` re-exports the `Client` it resolves; this crate resolves
/// its own. If cargo put two copies of matrix-sdk in the graph, these are two
/// unrelated types and this identity function does not compile. That is the
/// only reason the spike depends on consort-matrix at all.
fn one_sdk_only(client: consort_matrix::Client) -> matrix_sdk::Client {
    client
}

fn required(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("missing required env var {name}").into())
}

fn main() -> Result<(), Box<dyn Error>> {
    // Two crypto backends are in this graph (livekit/reqwest and matrix-sdk),
    // so rustls cannot pick a process default on its own.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install the rustls aws-lc-rs provider");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // `Call::join` spawns `!Send` futures and panics outside a `LocalSet`.
    // This skeleton is the shape Consort's call thread has to take.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(tokio::task::LocalSet::new().run_until(run()))
}

async fn run() -> Result<(), Box<dyn Error>> {
    let homeserver =
        env::var("HOMESERVER_URL").unwrap_or_else(|_| "http://localhost:8009".to_owned());
    let slot_id = env::var("SLOT_ID").unwrap_or_else(|_| "m.call#ROOM".to_owned());
    let livekit_service_url = env::var("LIVEKIT_SERVICE_URL").ok();

    let compat = match env::var("COMPAT").as_deref() {
        Ok("state") | Err(_) => ElementCallCompat::StateEvents,
        Ok("sticky") => ElementCallCompat::StickyEvents,
        Ok("off") => ElementCallCompat::Off,
        Ok(other) => return Err(format!("COMPAT must be state, sticky or off, not {other:?}").into()),
    };

    println!("== phase 0 spike ==");
    println!("compat mode: {compat:?}");

    let client = matrix_sdk::Client::builder()
        .homeserver_url(&homeserver)
        .build()
        .await?;
    if env::var("REGISTER").is_ok() {
        // A throwaway account on the local stack, so nothing here touches a
        // real homeserver or a real device list.
        let user = format!("spike{}", std::process::id());
        let mut request = register::v3::Request::new();
        request.username = Some(user.clone());
        request.password = Some("spike-password".to_owned());
        request.auth = Some(uiaa::AuthData::Dummy(uiaa::Dummy::new()));
        request.initial_device_display_name = Some("consort phase 0 spike".to_owned());
        client.matrix_auth().register(request).await?;
        println!("registered {user}");
    } else {
        let user = required("MX_USER")?;
        let password = required("MX_PASSWORD")?;
        client
            .matrix_auth()
            .login_username(&user, &password)
            .initial_device_display_name("consort phase 0 spike")
            .send()
            .await?;
    }
    let device_id = client.device_id().ok_or("no device id after login")?.to_string();
    println!("logged in, device {device_id}");

    // Question one, at runtime as well as at compile time: this `Client` goes
    // through consort-matrix's re-export and comes back out usable here.
    let client = one_sdk_only(client);

    // Question five, asked before anything can fail for another reason.
    match client.encryption().get_own_device().await? {
        Some(device) => println!(
            "own device: cross_signed_by_owner={} verified={}",
            device.is_cross_signed_by_owner(),
            device.is_verified(),
        ),
        None => println!("own device: not found in the store yet"),
    }

    // Question three. `plain` is what Consort's own sync loop amounts to;
    // `service` is what `Call::join`'s preconditions actually name.
    let use_sync_service = env::var("SYNC").as_deref() != Ok("plain");
    println!("sync: {}", if use_sync_service { "SyncService" } else { "plain Client::sync" });

    client.sync_once(SyncSettings::default()).await?;
    let _sync_guard = if use_sync_service {
        let service = matrix_sdk_ui::sync_service::SyncService::builder(client.clone())
            .build()
            .await?;
        service.start().await;
        Some(service)
    } else {
        let syncing = client.clone();
        tokio::spawn(async move {
            if let Err(error) = syncing.sync(SyncSettings::default()).await {
                eprintln!("plain sync stopped: {error}");
            }
        });
        None
    };

    let room_id: OwnedRoomId = if env::var("CREATE_ROOM").is_ok() {
        let id = create_call_room(&client).await?;
        println!("created room {id}");
        id
    } else {
        RoomId::parse(required("ROOM_ID")?)?
    };
    if client.get_room(&room_id).is_none() {
        println!("not in {room_id} yet; joining");
        client.join_room_by_id(&room_id).await?;
        client.sync_once(SyncSettings::default()).await?;
    }
    let room = client
        .get_room(&room_id)
        .ok_or("this account is not in that room, or sync has not delivered it yet")?;
    println!("room: {} (encrypted: {:?})", room.room_id(), room.latest_encryption_state().await.map(|s| s.is_encrypted()));

    // Question four.
    if env::var("OPEN_SLOT").is_ok() {
        println!("opening slot {slot_id}...");
        open_slot(&client, room_id.as_str(), &slot_id, "m.call", None).await?;
        println!("slot opened");
    } else {
        println!("not opening a slot; if the join fails on one, that is the answer to question four");
    }

    println!("joining...");
    let call = Call::join(
        &room,
        CallOptions {
            slot_id: slot_id.clone(),
            livekit_service_url_fallback: livekit_service_url,
            element_call_compat: compat,
            ..CallOptions::default()
        },
    )
    .await?;
    println!("JOINED as {}", call.local_identity());
    println!("membership id {}", call.membership_id());

    let mut events = call.subscribe_call_events();

    let _track = if env::var("PUBLISH_TONE").is_ok() {
        let handle = call.publish(PublishOptions::microphone()).await?;
        println!("publishing a 440 Hz tone in {FRAME_SAMPLES}-sample frames at {SAMPLE_RATE} Hz");
        tokio::task::spawn_local(async move {
            let mut at = 0usize;
            loop {
                let data: Vec<i16> = (0..FRAME_SAMPLES)
                    .map(|n| {
                        let phase = TAU * 440.0 * (at + n) as f32 / SAMPLE_RATE as f32;
                        (0.25 * phase.sin() * f32::from(i16::MAX)) as i16
                    })
                    .collect();
                at += FRAME_SAMPLES;
                let frame = AudioFrame {
                    data,
                    sample_rate: SAMPLE_RATE,
                    num_channels: 1,
                    samples_per_channel: FRAME_SAMPLES as u32,
                };
                if let Err(error) = handle.capture_audio(frame).await {
                    eprintln!("capture_audio stopped: {error}");
                    return;
                }
            }
        });
        Some(())
    } else {
        None
    };

    let run_for = Duration::from_secs(
        env::var("SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(120),
    );
    println!("watching for {run_for:?}; join the same channel from Element Call");

    let deadline = tokio::time::sleep(run_for);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.recv() => match event {
                Ok(event) => println!("event: {event:?}"),
                Err(error) => {
                    println!("event stream: {error}");
                    break;
                }
            },
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                println!("roster: {:?}", call.participants());
            }
        }
    }

    println!("leaving...");
    call.leave().await?;
    println!("left cleanly");
    Ok(())
}

/// An encrypted room whose call-member state event is open to PL 0.
///
/// `org.matrix.msc3401.call.member` is a state event, so `state_default` (50)
/// would gate it and an ordinary member could never announce that they had
/// connected. Real Element Call rooms ship exactly this override, which is why
/// nobody hits it there.
async fn create_call_room(client: &matrix_sdk::Client) -> Result<OwnedRoomId, Box<dyn Error>> {
    let mut request = CreateRoomRequest::new();
    request.name = Some("consort phase 0 spike".to_owned());
    // Public so a second spike can join by id without an invite. The room is
    // still encrypted; only who may walk in changes.
    request.preset = Some(matrix_sdk::ruma::api::client::room::create_room::v3::RoomPreset::PublicChat);
    request.visibility = matrix_sdk::ruma::api::client::room::Visibility::Public;
    request.power_level_content_override = Some(
        Raw::new(&serde_json::json!({
            "events": { matrix_rtc_livekit::compat::STATE_MEMBER_EVENT_TYPE: 0 },
        }))?
        .cast_unchecked(),
    );
    request.initial_state = vec![
        InitialStateEvent::with_empty_state_key(RoomHistoryVisibilityEventContent::new(
            HistoryVisibility::Shared,
        ))
        .to_raw_any(),
    ];
    let room = client.create_room(request).await?;
    room.enable_encryption().await?;
    Ok(room.room_id().to_owned())
}
