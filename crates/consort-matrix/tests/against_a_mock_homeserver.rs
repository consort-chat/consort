// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The half of the crate that talks to a homeserver.
//!
//! Everything here needs an HTTP server that answers like Synapse, which is
//! what `MatrixMockServer` is. Without these the login, restore and logout
//! paths are only covered by running the app by hand against a real account,
//! which is not a test.
//!
//! Endpoints the SDK reaches during `wait_for_e2ee_initialization_tasks` and
//! are deliberately left unmocked return 404. That is on purpose: the SDK logs
//! and carries on, and asserting that a login still succeeds when the crypto
//! setup is unhappy is worth having, because a homeserver with key backup
//! disabled behaves exactly that way.

use std::sync::Arc;

use consort_matrix::secrets::MemoryBackend;
use consort_matrix::{Credentials, SessionStore, StoredSession, auth};
use matrix_sdk::authentication::SessionTokens;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::test_utils::mocks::{LoginResponseTemplate200, MatrixMockServer};
use matrix_sdk::{SessionMeta, ruma};

const USER: &str = "@bob:example.org";
const DEVICE: &str = "HZTIUXZKUU";

fn store(dir: &tempfile::TempDir) -> (SessionStore, Arc<MemoryBackend>) {
    let backend = Arc::new(MemoryBackend::new());
    (
        SessionStore::with_backend(dir.path(), backend.clone()),
        backend,
    )
}

/// The endpoints a login touches before it can return.
async fn mount_login(server: &MatrixMockServer, access_token: &str) {
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server
        .mock_login()
        .ok_with(LoginResponseTemplate200::new(
            access_token,
            DEVICE,
            ruma::user_id!("@bob:example.org"),
        ))
        .mount()
        .await;
    server.mock_upload_keys().ok().mount().await;
    server.mock_query_keys().ok().mount().await;
}

fn credentials(server: &MatrixMockServer) -> Credentials {
    Credentials {
        server: server.uri(),
        username: "bob".to_owned(),
        password: "hunter2".to_owned(),
    }
}

/// A signed-in client against a freshly mocked server.
async fn signed_in(server: &MatrixMockServer) -> (tempfile::TempDir, matrix_sdk::Client) {
    mount_login(server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(server)).await.unwrap();
    (dir, client)
}

/// A sink that keeps everything it was handed.
fn recorder<T: Send + 'static>() -> (
    Arc<std::sync::Mutex<Vec<T>>>,
    impl Fn(T) + Send + Sync + 'static,
) {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = {
        let seen = seen.clone();
        move |state: T| seen.lock().unwrap().push(state)
    };
    (seen, sink)
}

/// Poll until the recorded states satisfy `done`, or give up loudly.
///
/// Polling rather than a channel because the assertions are about the whole
/// sequence, including what did *not* appear in it.
async fn wait_until<T: Clone + std::fmt::Debug>(
    seen: &Arc<std::sync::Mutex<Vec<T>>>,
    done: impl Fn(&[T]) -> bool,
) -> Vec<T> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        {
            let states = seen.lock().unwrap();
            if done(&states) {
                return states.clone();
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out; saw {:?}",
            seen.lock().unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_successful_login_returns_the_profile_and_persists_the_session() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, backend) = store(&dir);

    let (_client, profile) = auth::login(&store, &credentials(&server)).await.unwrap();

    assert_eq!(profile.user_id, USER);
    assert_eq!(profile.device_id, DEVICE);

    let reloaded = store.load().unwrap().expect("the session was persisted");
    assert_eq!(reloaded.session.tokens.access_token, "syt_first");
    assert_eq!(reloaded.session.meta.device_id.as_str(), DEVICE);
    assert_eq!(backend.len(), 1, "the token went to the secret backend");
}

#[tokio::test]
async fn a_login_never_writes_the_token_into_the_metadata_file() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_secret_value").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    auth::login(&store, &credentials(&server)).await.unwrap();

    let on_disk = std::fs::read_to_string(store.session_file()).unwrap();
    assert!(!on_disk.contains("syt_secret_value"));
}

#[tokio::test]
async fn a_login_creates_a_per_account_store_directory() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    auth::login(&store, &credentials(&server)).await.unwrap();

    let stored = store.load().unwrap().unwrap();
    assert!(stored.store_path.starts_with(dir.path()));
    assert!(stored.store_path.is_dir());
}

#[tokio::test]
#[cfg(unix)]
async fn the_sqlite_store_directory_is_not_readable_by_other_users() {
    use std::os::unix::fs::PermissionsExt;

    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    auth::login(&store, &credentials(&server)).await.unwrap();

    let stored = store.load().unwrap().unwrap();
    let mode = std::fs::metadata(&stored.store_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode was {mode:o}");
}

#[tokio::test]
async fn a_rejected_login_reports_a_message_for_a_person_and_stores_nothing() {
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
    let dir = tempfile::tempdir().unwrap();
    let (store, backend) = store(&dir);

    let error = auth::login(&store, &credentials(&server))
        .await
        .expect_err("a 403 must not look like a success");

    assert_eq!(error.user_message(), "Incorrect username or password.");
    assert!(!error.invalidates_session());
    assert!(backend.is_empty());
    assert!(store.load().unwrap().is_none());
}

#[tokio::test]
async fn signing_in_again_after_the_session_is_gone_is_not_blocked_by_the_old_crypto_store() {
    // The bug this exists for. matrix-sdk-crypto binds a store to the device
    // that created it, and a password login always mints a new device. Any
    // route that drops the session but leaves the store behind (a sign out, a
    // session file from an older format being discarded, a crash between the
    // two) therefore left an account that could never be signed into again:
    // every attempt died with "the account in the store doesn't match the
    // account in the constructor", which is not a message any user can act on.
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server.mock_upload_keys().ok().mount().await;
    server.mock_query_keys().ok().mount().await;

    let first = server
        .mock_login()
        .ok_with(LoginResponseTemplate200::new(
            "syt_first",
            "AAAAAAAAAA",
            ruma::user_id!("@bob:example.org"),
        ))
        .mount_as_scoped()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let (store, _backend) = store(&dir);

    let (_client, first_profile) = auth::login(&store, &credentials(&server)).await.unwrap();
    assert_eq!(first_profile.device_id, "AAAAAAAAAA");
    let store_path = store.load().unwrap().unwrap().store_path;
    assert!(
        store_path.exists(),
        "the first login should have made a store"
    );

    // However the session goes away, the store outlives it.
    store.clear().unwrap();
    assert!(store_path.exists(), "clearing the session leaves the store");

    drop(first);
    server
        .mock_login()
        .ok_with(LoginResponseTemplate200::new(
            "syt_second",
            "BBBBBBBBBB",
            ruma::user_id!("@bob:example.org"),
        ))
        .mount()
        .await;

    let (_client, second_profile) = auth::login(&store, &credentials(&server))
        .await
        .expect("a second sign-in must not be blocked by the previous device's store");

    assert_eq!(second_profile.device_id, "BBBBBBBBBB");
    // Same account, so the same directory, now belonging to the new device.
    assert_eq!(store.load().unwrap().unwrap().store_path, store_path);
}

#[tokio::test]
async fn a_second_sign_in_does_not_reuse_the_previous_device_s_keys() {
    // The discard has to be a real removal, not a fresh file alongside the old
    // one. A leftover sqlite from the previous device is what fails the next
    // open, so the check is that nothing from before survives.
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server.mock_upload_keys().ok().mount().await;
    server.mock_query_keys().ok().mount().await;
    let first = server
        .mock_login()
        .ok_with(LoginResponseTemplate200::new(
            "syt_first",
            "AAAAAAAAAA",
            ruma::user_id!("@bob:example.org"),
        ))
        .mount_as_scoped()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _backend) = store(&dir);

    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();
    let store_path = store.load().unwrap().unwrap().store_path;
    drop(client);
    store.clear().unwrap();

    let marker = store_path.join("left-behind-by-the-old-device");
    std::fs::write(&marker, b"stale").unwrap();

    drop(first);
    server
        .mock_login()
        .ok_with(LoginResponseTemplate200::new(
            "syt_second",
            "BBBBBBBBBB",
            ruma::user_id!("@bob:example.org"),
        ))
        .mount()
        .await;

    auth::login(&store, &credentials(&server)).await.unwrap();

    assert!(!marker.exists(), "the previous device's files must be gone");
}

#[tokio::test]
async fn a_rate_limited_login_does_not_tell_the_user_their_password_is_wrong() {
    // Synapse refuses after a few failed attempts, per account and per address,
    // and it answers 429 rather than 403. Reported as a wrong password, this
    // sends a user who has already fixed their password back to fix it again.
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server
        .mock_login()
        .respond_with(
            wiremock::ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "errcode": "M_LIMIT_EXCEEDED",
                "error": "Too Many Requests",
                "retry_after_ms": 12_000,
            })),
        )
        .mount()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (store, backend) = store(&dir);

    let error = auth::login(&store, &credentials(&server))
        .await
        .expect_err("a 429 must not look like a success");

    let message = error.user_message();
    assert!(message.contains("Too many sign-in attempts"), "{message}");
    assert!(!message.to_lowercase().contains("password"), "{message}");
    assert!(message.contains("12 seconds"), "{message}");
    // A rate limit is a reason to wait, never a reason to bin the session.
    assert!(!error.invalidates_session());
    assert!(backend.is_empty());
}

#[tokio::test]
async fn a_login_with_a_nonsense_server_never_reaches_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    let error = auth::login(
        &store,
        &Credentials {
            server: "  ".to_owned(),
            username: "bob".to_owned(),
            password: "hunter2".to_owned(),
        },
    )
    .await
    .expect_err("an empty server should be rejected up front");

    assert!(error.user_message().contains("does not look like"));
}

#[tokio::test]
async fn a_login_keeps_the_refresh_token_the_server_sent() {
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server
        .mock_login()
        .ok_with(
            LoginResponseTemplate200::new("syt_first", DEVICE, ruma::user_id!("@bob:example.org"))
                .refresh_token("refresh_me"),
        )
        .mount()
        .await;
    server.mock_upload_keys().ok().mount().await;
    server.mock_query_keys().ok().mount().await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    auth::login(&store, &credentials(&server)).await.unwrap();

    let reloaded = store.load().unwrap().unwrap();
    assert_eq!(
        reloaded.session.tokens.refresh_token.as_deref(),
        Some("refresh_me")
    );
}

#[tokio::test]
async fn a_restore_brings_back_a_client_without_a_password() {
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    let dir = tempfile::tempdir().unwrap();
    let stored = StoredSession {
        homeserver: server.uri(),
        store_path: dir.path().join("account"),
        session: MatrixSession {
            meta: SessionMeta {
                user_id: ruma::user_id!("@bob:example.org").to_owned(),
                device_id: DEVICE.into(),
            },
            tokens: SessionTokens {
                access_token: "syt_restored".to_owned(),
                refresh_token: None,
            },
        },
    };

    let (client, profile) = auth::restore(&stored).await.unwrap();

    assert_eq!(profile.user_id, USER);
    assert_eq!(profile.device_id, DEVICE);
    assert_eq!(client.user_id().unwrap().as_str(), USER);
}

#[tokio::test]
async fn a_restore_survives_a_homeserver_that_answers_nothing() {
    // Everything a restore needs is local, so an unreachable homeserver is not
    // a reason to sign the user out. The regression guard for the review's
    // finding 5, at the level below the command.
    let server = MatrixMockServer::new().await;
    let dir = tempfile::tempdir().unwrap();
    let stored = StoredSession {
        homeserver: server.uri(),
        store_path: dir.path().join("account"),
        session: MatrixSession {
            meta: SessionMeta {
                user_id: ruma::user_id!("@bob:example.org").to_owned(),
                device_id: DEVICE.into(),
            },
            tokens: SessionTokens {
                access_token: "syt_restored".to_owned(),
                refresh_token: None,
            },
        },
    };

    let (_client, profile) = auth::restore(&stored)
        .await
        .expect("a restore must not need the network");

    assert_eq!(profile.user_id, USER);
    // No display name came back, so the UI falls back to the user ID.
    assert_eq!(profile.display_name, None);
}

#[tokio::test]
async fn a_logout_clears_the_local_session() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    server.mock_logout().ok().mount().await;
    let dir = tempfile::tempdir().unwrap();
    let (store, backend) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();

    auth::logout(&client, &store).await.unwrap();

    assert!(store.load().unwrap().is_none());
    assert!(backend.is_empty());
    assert!(!store.session_file().exists());
}

#[tokio::test]
async fn a_logout_clears_locally_even_when_the_server_refuses() {
    // A user who clicked sign out is signed out. A token we have thrown away
    // is one we could not have invalidated later anyway.
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, backend) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();

    // No logout endpoint is mounted, so the server answers 404.
    auth::logout(&client, &store)
        .await
        .expect("a failed server logout is still a successful local one");

    assert!(store.load().unwrap().is_none());
    assert!(backend.is_empty());
}

/// Documents a deferral, not a virtue.
///
/// The justification this test used to carry, that deleting the store would
/// make old history undecryptable, is wrong: logging out destroys the device
/// server-side, and no future device can use the keys left here. Removing them
/// on sign out is what Element does and is the right end state, because "sign
/// out" leaving decrypted room keys on disk is a privacy question.
///
/// It is not done here because the client is still alive at this point and
/// holds open sqlite handles, so the removal has to move to after the client
/// is dropped. Until then `discard_previous_device_store` clears it on the way
/// into the next sign-in, which is what stops the leftovers from being a bug
/// rather than just clutter.
#[tokio::test]
async fn a_logout_leaves_the_crypto_store_on_disk_for_now() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    server.mock_logout().ok().mount().await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();
    let store_path = store.load().unwrap().unwrap().store_path;

    auth::logout(&client, &store).await.unwrap();

    assert!(
        store_path.is_dir(),
        "if this starts failing the deferral above has been resolved; update it"
    );
}

#[tokio::test]
async fn the_profile_picks_up_a_display_name_when_the_server_has_one() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/_matrix/client/v3/profile/@bob:example.org/displayname",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "displayname": "Bob" })),
        )
        .mount(server.server())
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    let (_client, profile) = auth::login(&store, &credentials(&server)).await.unwrap();

    assert_eq!(profile.display_name.as_deref(), Some("Bob"));
}

#[tokio::test]
async fn a_failed_profile_lookup_does_not_fail_the_login() {
    // A profile request that times out should cost the display name, not the
    // whole login.
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/_matrix/client/v3/profile/@bob:example.org/displayname",
        ))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(server.server())
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    let (_client, profile) = auth::login(&store, &credentials(&server)).await.unwrap();

    assert_eq!(profile.user_id, USER);
    assert_eq!(profile.display_name, None);
}

#[tokio::test]
async fn a_rotated_access_token_is_written_back_to_the_store() {
    // The bug this exists to prevent: the SDK rotates the pair, nothing
    // persists the new one, and the next launch restores an expired access
    // token alongside a refresh token the server has already spent. The user
    // is signed out with nothing in the log to explain it.
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server
        .mock_login()
        .ok_with(
            LoginResponseTemplate200::new("syt_first", DEVICE, ruma::user_id!("@bob:example.org"))
                .refresh_token("refresh_one"),
        )
        .mount()
        .await;
    server.mock_upload_keys().ok().mount().await;
    server.mock_query_keys().ok().mount().await;

    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();
    assert_eq!(
        store.load().unwrap().unwrap().session.tokens.access_token,
        "syt_first"
    );

    // Only now, with the login finished and the token on disk, start watching
    // and make the server reject that token. Mounting these before the login
    // would rotate during it, before anything was listening, and the test
    // would prove nothing.
    let task = tokio::spawn(auth::persist_token_refreshes(client.clone(), store.clone()));

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/_matrix/client/v3/profile/@bob:example.org/displayname",
        ))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer syt_first",
        ))
        .respond_with(wiremock::ResponseTemplate::new(401).set_body_json(
            serde_json::json!({ "errcode": "M_UNKNOWN_TOKEN", "error": "expired", "soft_logout": false }),
        ))
        .mount(server.server())
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/_matrix/client/v3/refresh"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({ "access_token": "syt_rotated", "refresh_token": "refresh_two" }),
        ))
        .mount(server.server())
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(
            "/_matrix/client/v3/profile/@bob:example.org/displayname",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "displayname": "Bob" })),
        )
        .mount(server.server())
        .await;

    // Any authenticated call will do. This one 401s on the old token, which is
    // what drives the refresh.
    let _ = client.account().get_display_name().await;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let stored = store.load().unwrap().unwrap().session.tokens;
        if stored.access_token == "syt_rotated" {
            assert_eq!(stored.refresh_token.as_deref(), Some("refresh_two"));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the rotated token never reached the store; it is still {}",
            stored.access_token
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    task.abort();
}

#[tokio::test]
async fn the_token_refresh_task_stops_when_it_is_aborted() {
    // It cannot end on its own, because it holds the client whose channel it
    // is watching. Aborting is how the app stops it at sign-out, so aborting
    // has to actually work.
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();

    let task = tokio::spawn(auth::persist_token_refreshes(client, store));
    task.abort();

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("an aborted task should finish promptly");
    assert!(outcome.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn a_rejected_access_token_is_the_one_failure_that_invalidates_the_session() {
    // `invalidates_session` is what decides whether the stored credential gets
    // deleted. Unit tests cover the classifier; this proves the SDK really
    // does surface M_UNKNOWN_TOKEN in the shape it reads.
    let server = MatrixMockServer::new().await;
    server.mock_versions().ok().mount().await;
    server.mock_well_known().ok().mount().await;
    server.mock_login().error_unknown_token(false).mount().await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    let error = auth::login(&store, &credentials(&server))
        .await
        .unwrap_err();

    assert!(
        error.invalidates_session(),
        "a rejected token must be recognised as a dead session"
    );
}

/// The sync loop.
///
/// Everything here needs a homeserver that answers `/sync`, and the point of
/// most of it is what happens when that answer is not a good one. A loop that
/// only works against a server that is up is not a loop worth having.
mod sync_loop {
    use super::*;
    use consort_matrix::{Connection, StopReason, sync};
    use std::time::Duration;
    use wiremock::ResponseTemplate;

    #[tokio::test]
    async fn a_loop_that_reaches_the_server_reports_itself_live() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (seen, sink) = recorder();

        let task = sync::start(client, sink);
        let states = wait_until(&seen, |s| s.contains(&Connection::Live)).await;
        task.abort();

        assert_eq!(
            states.first(),
            Some(&Connection::Connecting),
            "the loop should say it is trying before it says it succeeded: {states:?}"
        );
    }

    #[tokio::test]
    async fn a_loop_reports_live_once_rather_than_on_every_sync() {
        // Sync fires every thirty seconds forever. An event per response is a
        // webview wake-up and a re-render that carry no news.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;
        let (seen, sink) = recorder();

        let task = sync::start(client, sink);
        wait_until(&seen, |s| s.contains(&Connection::Live)).await;
        // Long enough for several more syncs against a mock that answers at once.
        tokio::time::sleep(Duration::from_secs(3)).await;
        task.abort();

        let states = seen.lock().unwrap().clone();
        let live = states.iter().filter(|s| **s == Connection::Live).count();
        assert_eq!(live, 1, "{states:?}");
    }

    #[tokio::test]
    async fn a_server_that_is_failing_moves_the_loop_to_offline() {
        // The failure this exists for. Without it a sync loop that cannot
        // reach anything is indistinguishable from a quiet homeserver, and
        // the UI keeps saying "Connected" at someone whose messages are not
        // arriving.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .mock_sync()
            .respond_with(ResponseTemplate::new(502))
            .mount()
            .await;
        let (seen, sink) = recorder();

        let task = sync::start(client, sink);
        let states = wait_until(&seen, |s| {
            s.iter().any(|c| matches!(c, Connection::Offline { .. }))
        })
        .await;
        task.abort();

        assert_eq!(states.first(), Some(&Connection::Connecting));
        assert!(
            !states.contains(&Connection::Live),
            "nothing ever succeeded: {states:?}"
        );
        let Some(Connection::Offline { attempt, .. }) = states
            .iter()
            .find(|c| matches!(c, Connection::Offline { .. }))
        else {
            unreachable!()
        };
        assert_eq!(*attempt, 1, "the first failure is attempt one");
    }

    #[tokio::test]
    async fn a_rejected_access_token_stops_the_loop_instead_of_retrying_forever() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .mock_sync()
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "errcode": "M_UNKNOWN_TOKEN",
                "error": "Invalid access token",
                "soft_logout": false,
            })))
            .mount()
            .await;
        let (seen, sink) = recorder();

        let task = sync::start(client, sink);
        let states = wait_until(&seen, |s| {
            s.iter().any(|c| matches!(c, Connection::Stopped { .. }))
        })
        .await;

        assert!(
            states.contains(&Connection::Stopped {
                reason: StopReason::SessionEnded
            }),
            "{states:?}"
        );
        assert!(
            !states
                .iter()
                .any(|c| matches!(c, Connection::Offline { .. })),
            "a dead session is not a retryable failure: {states:?}"
        );

        // And the task ends by itself rather than waiting to be aborted.
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the task should finish on its own")
            .expect("and not by panicking");
    }

    #[tokio::test]
    async fn a_to_device_verification_request_reaches_an_event_handler() {
        // Phase 0 exists for this. Verification is delivered as to-device
        // events and nothing else delivers them, so proving one arrives
        // through the loop is what says the next milestone can be built.
        use matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent;
        use std::sync::atomic::{AtomicBool, Ordering};

        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let arrived = Arc::new(AtomicBool::new(false));
        client.add_event_handler({
            let arrived = arrived.clone();
            move |_: ToDeviceKeyVerificationRequestEvent| {
                let arrived = arrived.clone();
                async move {
                    arrived.store(true, Ordering::SeqCst);
                }
            }
        });

        server
            .mock_sync()
            .ok(|builder| {
                builder.add_to_device_event(serde_json::json!({
                    "sender": "@bob:example.org",
                    "type": "m.key.verification.request",
                    "content": {
                        "from_device": "OTHERDEVICE",
                        "transaction_id": "the-only-flow",
                        "methods": ["m.sas.v1"],
                        "timestamp": 1_600_000_000_000u64,
                    },
                }));
            })
            .mount()
            .await;

        let (seen, sink) = recorder();
        let task = sync::start(client, sink);
        wait_until(&seen, |s| s.contains(&Connection::Live)).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !arrived.load(Ordering::SeqCst) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the verification request never reached the handler"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        task.abort();
    }

    #[tokio::test]
    async fn a_failing_homeserver_gives_up_promptly_instead_of_retrying_for_minutes() {
        // matrix-sdk retries a 5xx on its own for fifteen minutes by default,
        // with no way for a caller to see it happening. Left alone that makes
        // the offline state above unreachable in practice: the loop would sit
        // inside one `sync_once` for a quarter of an hour while the UI went on
        // claiming everything was fine.
        let server = MatrixMockServer::new().await;
        server.mock_versions().ok().mount().await;
        server.mock_well_known().ok().mount().await;
        server
            .mock_login()
            .respond_with(ResponseTemplate::new(502))
            .mount()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = store(&dir);

        let outcome = tokio::time::timeout(
            Duration::from_secs(30),
            auth::login(&store, &credentials(&server)),
        )
        .await;

        assert!(
            outcome.is_ok(),
            "the login was still retrying after thirty seconds"
        );
        assert!(outcome.unwrap().is_err());
    }
}

/// Reporting whether this session is verified.
mod verification_watcher {
    use super::*;
    use consort_matrix::{SessionVerification, verification};

    #[tokio::test]
    async fn a_new_login_is_reported_unverified() {
        // Nothing has signed this device, and until something does it cannot
        // read encrypted history or join an encrypted call. The whole point of
        // the channel is that the user is told so.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (seen, sink) = recorder();

        let task = verification::watch(client, sink);
        let states = wait_until(&seen, |s: &[SessionVerification]| {
            s.contains(&SessionVerification::Unverified)
        })
        .await;
        task.abort();

        assert!(
            !states.contains(&SessionVerification::Verified),
            "a session nobody has verified was reported verified: {states:?}"
        );
    }

    #[tokio::test]
    async fn the_first_report_does_not_wait_for_a_sync() {
        // No `mock_sync` here and no sync loop running. The state is read from
        // the crypto store, so a client that is signed in but not yet syncing
        // still knows the answer, and the banner does not sit on "checking"
        // until the first sync response arrives.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (seen, sink) = recorder();

        let task = verification::watch(client, sink);
        let states = wait_until(&seen, |s: &[SessionVerification]| !s.is_empty()).await;
        task.abort();

        assert!(!states.is_empty());
    }

    #[tokio::test]
    async fn aborting_the_watcher_stops_it() {
        // `AppState` aborts this on sign-out and on a second sign-in. A task
        // that survived that would keep a whole `Client` alive, SQLite handles
        // included, for as long as the process runs.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (_seen, sink) = recorder::<SessionVerification>();

        let task = verification::watch(client, sink);
        task.abort();

        assert!(task.await.unwrap_err().is_cancelled());
    }
}

/// Verification flows, as far as a mock can drive them.
///
/// Less far than it first looks. `MatrixMockServer` can put an
/// `m.key.verification.request` into a sync response, and Phase 0 proved one
/// reaches an event handler, but the crypto machine will not build a request
/// object out of it: it looks the sender's device up in its own store first,
/// and a mocked `/keys/query` has no devices in it. Producing one means a
/// device key blob carrying a signature the SDK verifies, which is a real
/// olm identity and not something a JSON literal can fake.
///
/// So what is testable here is the wiring that does not need a counterparty:
/// the supervisor's lifetime, the actions' behaviour when the flow they name
/// has gone, and the fact that nothing is announced when nothing happened. The
/// handshake, and a request that actually produces a flow, are in
/// `against_a_real_homeserver.rs`.
mod verification_flows {
    use super::*;
    use consort_matrix::{Connection, Flow, sync, verification};
    use std::time::Duration;

    const FLOW: &str = "the-only-flow";

    /// A to-device request, as if from another of this account's own devices.
    ///
    /// Written out rather than built with a helper because
    /// `SyncResponseBuilder` lives in `matrix-sdk-test`, which matrix-sdk does
    /// not re-export, and one JSON literal is cheaper than another
    /// dev-dependency pinned to the same rev.
    fn a_request_from_a_device_we_have_never_seen() -> serde_json::Value {
        // Current, not a constant. The crypto machine drops a request whose
        // timestamp is far from now, and a fixed one from 2020 would make this
        // test pass for a reason it is not about.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        serde_json::json!({
            "sender": USER,
            "type": "m.key.verification.request",
            "content": {
                "from_device": "OTHERDEVICE",
                "transaction_id": FLOW,
                "methods": ["m.sas.v1"],
                "timestamp": now,
            },
        })
    }

    /// A signed-in client with its supervisor and sync loop running.
    async fn watching(
        server: &MatrixMockServer,
    ) -> (
        tempfile::TempDir,
        Arc<std::sync::Mutex<Vec<Flow>>>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let (dir, client) = signed_in(server).await;

        let (flows, flow_sink) = recorder::<Flow>();
        let flow_task = verification::supervise(client.clone(), flow_sink);

        let (connections, sink) = recorder();
        let sync_task = sync::start(client, sink);
        wait_until(&connections, |s| s.contains(&Connection::Live)).await;

        (dir, flows, flow_task, sync_task)
    }

    #[tokio::test]
    async fn a_request_from_a_device_we_cannot_identify_is_not_shown() {
        // The SDK drops it, and we take the SDK's word for that rather than
        // building a flow out of the raw event ourselves. The difference
        // matters: synthesising one would put "somebody wants to verify this
        // session" in front of the user for a device nobody can verify, which
        // is both useless and exactly the shape of a nuisance.
        let server = MatrixMockServer::new().await;
        server
            .mock_sync()
            .ok(|builder| {
                builder.add_to_device_event(a_request_from_a_device_we_have_never_seen());
            })
            .mount()
            .await;

        let (_dir, flows, flow_task, sync_task) = watching(&server).await;

        // Two sync round trips' worth, so this is not just winning a race.
        tokio::time::sleep(Duration::from_millis(500)).await;
        sync_task.abort();
        flow_task.abort();

        let seen = flows.lock().unwrap().clone();
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[tokio::test]
    async fn nothing_is_reported_when_no_request_arrives() {
        // The regression this guards is a supervisor that announces a flow on
        // every sync, which would put a "somebody wants to verify" prompt in
        // front of the user for the life of the session.
        let server = MatrixMockServer::new().await;
        server.mock_sync().ok(|_| {}).mount().await;

        let (_dir, flows, flow_task, sync_task) = watching(&server).await;

        tokio::time::sleep(Duration::from_millis(500)).await;
        sync_task.abort();
        flow_task.abort();

        let seen = flows.lock().unwrap().clone();
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[tokio::test]
    async fn acting_on_a_flow_that_does_not_exist_says_so_instead_of_panicking() {
        // The interface draws a button from an event, and by the time somebody
        // presses it the flow may have expired, been cancelled by the other
        // side, or been answered on another of the account's devices. All
        // three are ordinary and none of them is a broken session.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        for outcome in [
            verification::accept(&client, USER, "gone").await,
            verification::start_sas(&client, USER, "gone").await,
            verification::confirm(&client, USER, "gone").await,
            verification::mismatch(&client, USER, "gone").await,
            verification::cancel(&client, USER, "gone").await,
        ] {
            let error = outcome.expect_err("acting on a missing flow should be an error");
            assert!(!error.invalidates_session(), "{error}");
            assert!(
                error.user_message().contains("no longer"),
                "{}",
                error.user_message()
            );
        }
    }

    #[tokio::test]
    async fn a_malformed_user_id_is_an_error_rather_than_a_panic() {
        // Nothing in our own interface can send one, but a command is reachable
        // from anything running in the webview, and `OwnedUserId::try_from`
        // would otherwise be an unwrap on attacker-shaped input.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let error = verification::accept(&client, "not a user id", FLOW)
            .await
            .expect_err("a malformed user id should be rejected");

        assert!(!error.invalidates_session(), "{error}");
    }

    #[tokio::test]
    async fn stopping_the_supervisor_stops_it() {
        // `AppState` aborts this on sign-out, and it has to be an abort that
        // takes the flow tasks with it: each one holds the `Client` and
        // watches a stream belonging to that same client, so none of them can
        // end on its own.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (_flows, flow_sink) = recorder::<Flow>();

        let task = verification::supervise(client, flow_sink);
        task.abort();

        assert!(task.await.unwrap_err().is_cancelled());
    }
}
