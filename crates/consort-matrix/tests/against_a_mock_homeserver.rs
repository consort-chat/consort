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
use consort_matrix::{Credentials, SessionStore, StoreKey, StoredSession, auth};
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

/// Answer one global account data type with `response`.
///
/// Hand-rolled rather than through `mock_get_default_secret_storage_key`,
/// which only offers the 200 and insists on the mock crate's own access token.
/// Several of the tests below are about what happens when the answer is not a
/// 200.
async fn account_data(
    server: &MatrixMockServer,
    event_type: &str,
    response: wiremock::ResponseTemplate,
) {
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(format!(
            "/_matrix/client/v3/user/{USER}/account_data/{event_type}"
        )))
        .respond_with(response)
        .mount(server.server())
        .await;
}

/// The 404 a homeserver gives for account data that was never set.
///
/// The body matters. matrix-sdk reads `M_NOT_FOUND` and turns it into "no such
/// event"; a bare 404 with no Matrix error in it is a transport failure and
/// comes back as an error instead. Nothing in a mocked login notices the
/// difference until the SDK's own startup work needs an answer, at which point
/// it gives up on the whole of recovery and backup setup.
/// Accept a write of one global account data type.
///
/// Only the ones a test's code path actually writes. An unmounted PUT is a 404
/// with no Matrix error in it, which the SDK reports as a transport failure and
/// which stops whatever was in the middle of happening.
async fn accepting_account_data(server: &MatrixMockServer, event_type: &str) {
    wiremock::Mock::given(wiremock::matchers::method("PUT"))
        .and(wiremock::matchers::path(format!(
            "/_matrix/client/v3/user/{USER}/account_data/{event_type}"
        )))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(server.server())
        .await;
}

fn never_set() -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(404).set_body_json(serde_json::json!({
        "errcode": "M_NOT_FOUND",
        "error": "Account data not found",
    }))
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
    assert_eq!(
        backend.len(),
        2,
        "the token and the store key went to the secret backend"
    );
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
        store_key: StoreKey::generate(),
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
        store_key: StoreKey::generate(),
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
async fn a_login_records_the_key_its_stores_are_encrypted_with() {
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);

    auth::login(&store, &credentials(&server)).await.unwrap();

    let reloaded = store.load().unwrap().expect("the session was saved");
    assert_eq!(
        store
            .store_key(&reloaded.store_path)
            .unwrap()
            .expect("a key was recorded for the store")
            .as_bytes(),
        reloaded.store_key.as_bytes()
    );
}

#[tokio::test]
async fn the_stores_a_login_writes_cannot_be_opened_with_the_wrong_key() {
    // The property the whole store-key mechanism exists for, asserted against
    // the SDK rather than against our own bookkeeping. Passing `None` here
    // would not fail: matrix-sdk would open the databases and read what it
    // found as plaintext, which is exactly the state this replaced.
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();
    let store_path = store.load().unwrap().unwrap().store_path;
    drop(client);

    let error = matrix_sdk::Client::builder()
        .homeserver_url(server.uri())
        .sqlite_store_with_config_and_cache_path(
            matrix_sdk::SqliteStoreConfig::new(&store_path)
                .key(Some(StoreKey::generate().as_bytes())),
            None::<&std::path::Path>,
        )
        .build()
        .await
        .expect_err("a store opened with the wrong key must not open");

    assert!(
        matches!(error, matrix_sdk::ClientBuildError::SqliteStore(_)),
        "expected the store open to fail, got {error:?}"
    );
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
/// is dropped. What has changed since this deferral was written is that the
/// files are no longer readable: `SessionStore::clear` deletes the key they
/// are encrypted with, so what is left is bytes nobody can open. The next test
/// is the one that says so.
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
async fn a_logout_takes_the_key_to_the_store_it_leaves_behind() {
    // What makes the deferral above clutter rather than a privacy bug.
    let server = MatrixMockServer::new().await;
    mount_login(&server, "syt_first").await;
    server.mock_logout().ok().mount().await;
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = store(&dir);
    let (client, _) = auth::login(&store, &credentials(&server)).await.unwrap();
    let store_path = store.load().unwrap().unwrap().store_path;

    auth::logout(&client, &store).await.unwrap();

    assert!(store.store_key(&store_path).unwrap().is_none());
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
        let (flow_task, _) = verification::supervise(client.clone(), flow_sink);

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

        let (task, _) = verification::supervise(client, flow_sink);
        task.abort();

        assert!(task.await.unwrap_err().is_cancelled());
    }
}

/// Starting a verification rather than answering one.
///
/// The same two walls as the module above. `has_devices_to_verify_against` is
/// an ordinary `GET /devices` and is fully testable here, but asking this
/// account to verify us needs a cross-signing identity in the crypto store,
/// and a mocked `/keys/query` has none. So the negative is the one that lives
/// here, and the round trip is in `against_a_real_homeserver.rs`.
mod verifying_this_session {
    use super::*;
    use consort_matrix::{Flow, verification};
    use wiremock::ResponseTemplate;

    /// A `GET /devices` body listing exactly these sessions.
    ///
    /// `device_id` is the only required field, and the only one read.
    fn sessions(ids: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "devices": ids
                .iter()
                .map(|id| serde_json::json!({ "device_id": id }))
                .collect::<Vec<_>>(),
        })
    }

    /// Answer `GET /devices` with `response`.
    ///
    /// `expect_any_access_token` because the prebuilt mock wants the mock
    /// crate's own default token and this harness logs in with its own.
    async fn listing_sessions(server: &MatrixMockServer, response: ResponseTemplate) {
        server
            .mock_devices()
            .expect_any_access_token()
            .respond_with(response)
            .mount()
            .await;
    }

    #[tokio::test]
    async fn a_lone_session_has_nothing_to_compare_emoji_with() {
        // The point of asking. With nothing else signed in there is nobody to
        // show the emoji, and offering the button anyway leads to a request
        // that can only time out.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        listing_sessions(
            &server,
            ResponseTemplate::new(200).set_body_json(sessions(&[DEVICE])),
        )
        .await;

        assert!(
            !verification::has_devices_to_verify_against(&client)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn another_signed_in_session_is_something_to_compare_emoji_with() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        listing_sessions(
            &server,
            ResponseTemplate::new(200).set_body_json(sessions(&[DEVICE, "OTHERDEVICE"])),
        )
        .await;

        assert!(
            verification::has_devices_to_verify_against(&client)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn a_homeserver_that_will_not_list_sessions_says_so_rather_than_guessing() {
        // Neither answer is safe to invent. Guessing "none" sends somebody who
        // has another session to a recovery key they may not have kept, and
        // guessing "some" offers a button that cannot work. The caller decides
        // what to do about not knowing.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        listing_sessions(&server, ResponseTemplate::new(500)).await;

        assert!(
            verification::has_devices_to_verify_against(&client)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn asking_without_a_cross_signing_identity_says_so_instead_of_panicking() {
        // A `/keys/query` with nothing in it leaves the crypto store with no
        // identity for this account, which is also what a real account that
        // has never set cross-signing up looks like.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (_flows, flow_sink) = recorder::<Flow>();
        let (_task, initiator) = verification::supervise(client, flow_sink);

        let error = initiator.verify_this_session().await.unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::NoCrossSigningIdentity),
            "{error}"
        );
        assert!(!error.invalidates_session(), "{error}");
    }
}

/// The recovery-key route, as far as a mock can take it.
///
/// Further than expected. Everything up to the moment the key opens the store
/// is account data over HTTP, so a mock can answer all of it, and the two
/// failures worth telling apart are both decided before any secret is
/// decrypted. What it cannot do is the success: a real recovery holds real
/// cross-signing keys encrypted to a real key, and that lives in the live
/// suite.
mod recovery {
    use super::*;
    use consort_matrix::verification;

    /// The key description from the SDK's own example, and the key it opens.
    ///
    /// Borrowed rather than invented because it has a valid MAC, which is what
    /// makes "the right key" and "a well-formed wrong key" two different
    /// answers rather than both failing at the same check.
    const KEY_ID: &str = "bmur2d9ypPUH1msSwCxQOJkuKRmJI55e";

    fn key_description() -> serde_json::Value {
        serde_json::json!({
            "algorithm": "m.secret_storage.v1.aes-hmac-sha2",
            "iv": "xv5b6/p3ExEw++wTyfSHEg==",
            "mac": "ujBBbXahnTAMkmPUX2/0+VTfUh63pGyVRuBcDMgmJC8=",
        })
    }

    fn found(body: serde_json::Value) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(body)
    }

    /// An account with secret storage set up, ready for a key to be typed.
    async fn with_recovery(server: &MatrixMockServer) {
        account_data(
            server,
            "m.secret_storage.default_key",
            found(serde_json::json!({ "key": KEY_ID })),
        )
        .await;
        account_data(
            server,
            &format!("m.secret_storage.key.{KEY_ID}"),
            found(key_description()),
        )
        .await;
    }

    #[tokio::test]
    async fn an_account_with_secret_storage_has_a_key_worth_asking_for() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        with_recovery(&server).await;

        assert!(verification::has_recovery_set_up(&client).await.unwrap());
    }

    #[tokio::test]
    async fn the_answer_the_sdk_already_has_is_not_asked_for_again() {
        // Mounted before the login rather than after it, so the SDK's own
        // startup task resolves the state and the cached answer is the one
        // read. The other tests here go down the path where it is still
        // `Unknown`, which is what a restored session looks like, and both
        // have to give the same answer.
        let server = MatrixMockServer::new().await;
        mount_login(&server, "syt_first").await;
        with_recovery(&server).await;
        let dir = tempfile::tempdir().unwrap();
        let (session_store, _) = store(&dir);
        let (client, _) = auth::login(&session_store, &credentials(&server))
            .await
            .unwrap();

        assert!(verification::has_recovery_set_up(&client).await.unwrap());
    }

    #[tokio::test]
    async fn a_store_with_no_verification_keys_in_it_says_so() {
        // The silent failure this exists to stop. Secret storage is a bag of
        // secrets rather than a fixed set, so a key can open one holding
        // nothing but a backup key. Without the check the import succeeds,
        // the session stays unverified, and somebody who typed 48 correct
        // characters is shown nothing at all.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        with_recovery(&server).await;
        for secret in [
            "m.cross_signing.master",
            "m.cross_signing.self_signing",
            "m.cross_signing.user_signing",
            "m.megolm_backup.v1",
        ] {
            account_data(&server, secret, never_set()).await;
        }
        // Mounted again here, and this is not redundant. The prebuilt one in
        // `mount_login` insists on the mock crate's own access token, which
        // this harness does not use, so it never matches. Nothing noticed
        // until now because the only caller was a background task that
        // swallows the failure; importing secrets is the first code path that
        // reports it.
        server
            .mock_query_keys()
            .expect_any_access_token()
            .ok()
            .mount()
            .await;

        let error = verification::recover(
            &client,
            "EsTj 3yST y93F SLpB jJsz eAXc 2XzA ygD3 w69H fGaN TKBj jXEd",
        )
        .await
        .unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::RecoveryWithoutIdentity),
            "{error}"
        );
    }

    #[tokio::test]
    async fn an_account_without_secret_storage_has_nothing_to_type() {
        // The screen this decides is a different one. Showing a box for a key
        // that was never created sends somebody through a password manager
        // looking for something that does not exist.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        account_data(&server, "m.secret_storage.default_key", never_set()).await;

        assert!(!verification::has_recovery_set_up(&client).await.unwrap());
    }

    #[tokio::test]
    async fn a_homeserver_that_will_not_answer_says_so_rather_than_guessing() {
        // Same reasoning as counting the account's sessions. Guessing "none"
        // hides the only route a lone session has.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        account_data(
            &server,
            "m.secret_storage.default_key",
            wiremock::ResponseTemplate::new(500),
        )
        .await;

        assert!(verification::has_recovery_set_up(&client).await.is_err());
    }

    #[tokio::test]
    async fn an_empty_box_is_answered_without_asking_the_homeserver() {
        // Nothing is mounted, so any request at all would fail the test with a
        // transport error rather than the answer being asserted.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let error = verification::recover(&client, "   ").await.unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::MalformedRecoveryKey),
            "{error}"
        );
    }

    #[tokio::test]
    async fn something_that_is_not_a_key_is_named_as_such() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        with_recovery(&server).await;

        let error = verification::recover(&client, "hunter2").await.unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::MalformedRecoveryKey),
            "{error}"
        );
        assert!(!error.invalidates_session(), "{error}");
    }

    #[tokio::test]
    async fn a_real_key_for_another_account_is_a_different_answer() {
        // The distinction the whole error mapping exists for. This person has
        // a recovery key; it is just not this one's. Telling them it is
        // malformed sends them to check their typing, which is fine, and then
        // to check it again, which is not.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        with_recovery(&server).await;
        let another_account =
            matrix_sdk_base::crypto::secret_storage::SecretStorageKey::new().to_base58();

        let error = verification::recover(&client, &another_account)
            .await
            .unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::WrongRecoveryKey),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_key_offered_to_an_account_with_no_recovery_is_not_called_wrong() {
        // The race the interface cannot close: recovery was there when the box
        // was drawn and reset before the key was typed. "That key is wrong" is
        // the one answer that is definitely untrue.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        account_data(&server, "m.secret_storage.default_key", never_set()).await;

        let error = verification::recover(
            &client,
            "EsTj 3yST y93F SLpB jJsz eAXc 2XzA ygD3 w69H fGaN TKBj jXEd",
        )
        .await
        .unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::NoRecoverySetUp),
            "{error}"
        );
    }
}

/// What is happening to this session's room keys.
///
/// Every one of these is decided by a single request the mock can answer, so
/// the whole state machine is testable without a homeserver. The one thing
/// that is not is a backup being read from, which needs real keys in a real
/// backup and lives in the live suite.
mod key_backup {
    use super::*;
    use consort_matrix::{KeyBackup, backup};

    /// Start the watcher and wait for its first report.
    ///
    /// The first is the one worth asserting on: the SDK's stream yields the
    /// current state before any update, so it is the answer as of now rather
    /// than whatever happened to change next.
    async fn first_report(client: matrix_sdk::Client) -> KeyBackup {
        let (seen, sink) = recorder::<KeyBackup>();
        let task = backup::watch(client, sink);

        let states = wait_until(&seen, |states| !states.is_empty()).await;

        task.abort();
        states[0]
    }

    #[tokio::test]
    async fn a_backup_this_session_cannot_read_is_not_the_same_as_no_backup() {
        // The distinction the SDK does not draw and a person needs. Its own
        // `Unknown` covers both, and they are opposite pieces of news: one
        // says verify this session and your history comes back, the other
        // says nothing is coming back for anybody.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .mock_room_keys_version()
            .expect_any_access_token()
            .exists()
            .mount()
            .await;

        assert_eq!(first_report(client).await, KeyBackup::Unusable);
    }

    #[tokio::test]
    async fn an_account_with_no_backup_at_all_says_so() {
        // The one worth interrupting somebody about. Every room key this
        // session holds is on this machine and nowhere else.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .mock_room_keys_version()
            .expect_any_access_token()
            .none()
            .mount()
            .await;

        assert_eq!(first_report(client).await, KeyBackup::Missing);
    }

    #[tokio::test]
    async fn a_homeserver_that_will_not_answer_is_reported_as_not_known() {
        // Neither guess is safe. One tells somebody their messages are safe
        // when nothing has checked, and the other raises an alarm about a
        // backup that is probably fine.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .mock_room_keys_version()
            .expect_any_access_token()
            .error429()
            .mount()
            .await;

        assert_eq!(first_report(client).await, KeyBackup::Unknown);
    }

    #[tokio::test]
    async fn signing_in_to_an_account_with_no_backup_creates_one() {
        // The half that cannot be seen from the state afterwards, because the
        // mock still says there is no backup. What is asserted is the request:
        // wiremock checks the expectation when the server drops, so a login
        // that quietly skipped the creation fails here.
        let server = MatrixMockServer::new().await;
        mount_login(&server, "syt_first").await;
        // Mounted because the SDK asks before it decides. Without an answer it
        // abandons the whole of recovery and backup setup, and the creation
        // this test is about never happens for a reason that has nothing to do
        // with backups.
        account_data(&server, "m.secret_storage.default_key", never_set()).await;
        account_data(&server, "m.key_backup", never_set()).await;
        account_data(&server, "m.org.matrix.custom.backup_disabled", never_set()).await;
        accepting_account_data(&server, "m.key_backup").await;
        accepting_account_data(&server, "m.org.matrix.custom.backup_disabled").await;
        server
            .mock_room_keys_version()
            .expect_any_access_token()
            .none()
            .mount()
            .await;
        server
            .mock_add_room_keys_version()
            .expect_any_access_token()
            .ok()
            .expect(1)
            .named("the backup this login should create")
            .mount()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (session_store, _) = store(&dir);
        auth::login(&session_store, &credentials(&server))
            .await
            .unwrap();
    }
}

/// The room list watcher, wired to a sync loop.
///
/// The grouping, the ordering and the orphan detection are unit-tested against
/// hand-built facts, because that is pure and every branch of it is reachable
/// without a homeserver. What is left, and what needs a server, is the wiring:
/// that a room arriving in a sync response comes out the other end, and that a
/// sync carrying nothing does not.
///
/// The sync responses below are written out rather than built, for the same
/// reason the verification tests write out a to-device event: the builders
/// live in `matrix-sdk-test`, which matrix-sdk does not re-export, and one
/// JSON literal is cheaper than a second git dependency pinned to the same
/// rev. It also puts the shape of a space on the page, which is the thing
/// being read.
mod room_list {
    use super::*;
    use consort_matrix::{Channel, ChannelKind, Client, Connection, Rooms, Space, rooms, sync};
    use std::time::Duration;

    const ROOM: &str = "!general:example.org";

    fn home(rooms: &Rooms) -> &Space {
        &rooms.spaces[0]
    }

    fn ids(channels: &[Channel]) -> Vec<&str> {
        channels.iter().map(|channel| channel.id.as_str()).collect()
    }

    #[tokio::test]
    async fn the_first_report_arrives_without_waiting_for_a_sync() {
        // By the time this is spawned the first sync may already have
        // happened. A shell that stays empty until the next one looks broken
        // for thirty seconds after every restart.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (seen, sink) = recorder::<Rooms>();

        let task = rooms::watch(client, sink);
        let reports = wait_until(&seen, |reports| !reports.is_empty()).await;
        task.abort();

        assert_eq!(reports.len(), 1);
        assert_eq!(home(&reports[0]).id, "home");
        assert!(
            home(&reports[0]).channels.is_empty(),
            "an account in no rooms should have an empty Home, not a missing one"
        );
    }

    #[tokio::test]
    async fn a_room_arriving_in_a_sync_reaches_the_room_list() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        let (seen, sink) = recorder::<Rooms>();

        let task = rooms::watch(client.clone(), sink);
        wait_until(&seen, |reports| !reports.is_empty()).await;

        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;

        let reports = wait_until(&seen, |reports| {
            reports
                .last()
                .is_some_and(|rooms| !home(rooms).channels.is_empty())
        })
        .await;
        task.abort();

        let channels = &home(reports.last().unwrap()).channels;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, ROOM);
        assert!(channels[0].joined);
        assert_eq!(
            channels[0].kind,
            ChannelKind::Text,
            "a room that does not announce itself as a call is not a voice channel"
        );
    }

    #[tokio::test]
    async fn a_sync_that_changes_nothing_is_not_reported_again() {
        // The regression this guards is a watcher that recomputes and reports
        // on every sync response. Sync fires every thirty seconds forever, so
        // that is a webview wake-up and a full re-render of the shell, twice a
        // minute, carrying exactly what it carried last time.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server.mock_sync().ok(|_| {}).mount().await;

        let (seen, sink) = recorder::<Rooms>();
        let task = rooms::watch(client.clone(), sink);
        wait_until(&seen, |reports| !reports.is_empty()).await;

        let (connections, connection_sink) = recorder();
        let sync_task = sync::start(client, connection_sink);
        wait_until(&connections, |states| states.contains(&Connection::Live)).await;

        // Long enough for several more syncs against a mock that answers at
        // once.
        tokio::time::sleep(Duration::from_secs(3)).await;
        sync_task.abort();
        task.abort();

        let reports = seen.lock().unwrap().clone();
        assert_eq!(reports.len(), 1, "{reports:?}");
    }

    /// One state event, with the fields the SDK insists on.
    ///
    /// The event ID is derived from the event so that no two of them collide,
    /// and stripped down to letters and digits because it has to survive
    /// ruma's parsing. Everything after a colon in an event ID is read as a
    /// server name, and a state key can hold both a colon and characters no
    /// server name is allowed to have. An ID that fails to parse takes the
    /// whole event with it, silently, which is a long way to look for a
    /// fixture that simply never arrives.
    fn state_event(
        event_type: &str,
        state_key: &str,
        timestamp: u64,
        content: serde_json::Value,
    ) -> serde_json::Value {
        let event_id: String = format!("{event_type}{state_key}{timestamp}")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();

        serde_json::json!({
            "type": event_type,
            "state_key": state_key,
            "content": content,
            "event_id": format!("$e{event_id}"),
            "sender": USER,
            "origin_server_ts": timestamp,
        })
    }

    /// The `m.room.create` every room needs before the SDK will believe in it.
    fn created(room_type: Option<&str>) -> serde_json::Value {
        let mut content = serde_json::json!({
            "creator": USER,
            "room_version": "10",
        });
        if let Some(room_type) = room_type {
            content["type"] = room_type.into();
        }
        state_event("m.room.create", "", 1, content)
    }

    fn named(name: &str) -> serde_json::Value {
        state_event("m.room.name", "", 2, serde_json::json!({ "name": name }))
    }

    /// A space claiming a child, at the timestamp the ordering falls back to.
    fn child(room_id: &str, timestamp: u64, via: &[&str]) -> serde_json::Value {
        state_event(
            "m.space.child",
            room_id,
            timestamp,
            serde_json::json!({ "via": via }),
        )
    }

    /// Answer every sync with the same set of joined rooms.
    async fn sync_with(server: &MatrixMockServer, joined: serde_json::Value) {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/v3/sync"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "next_batch": "s1", "rooms": { "join": joined } }),
            ))
            .mount(server.server())
            .await;
    }

    /// The account this milestone was designed against, in miniature.
    ///
    /// One space with four claimed children, of which one is a call room, one
    /// is an ordinary room, one was never joined, and one carries no `via` and
    /// is therefore not a child at all. Plus a room in no space.
    fn an_account_with_a_space() -> serde_json::Value {
        serde_json::json!({
            "!space:example.org": { "state": { "events": [
                created(Some("m.space")),
                named("Kahu HQ"),
                child("!general:example.org", 2_000, &["example.org"]),
                child("!lounge:example.org", 1_000, &["example.org"]),
                child("!never:example.org", 3_000, &["example.org"]),
                child("!dropped:example.org", 500, &[]),
            ] } },
            "!general:example.org": { "state": { "events": [created(None), named("general")] } },
            "!lounge:example.org": { "state": { "events": [
                created(Some("org.matrix.msc3417.call")),
                named("Lounge"),
            ] } },
            "!dm:example.org": { "state": { "events": [created(None)] } },
        })
    }

    async fn synced(
        server: &MatrixMockServer,
        joined: serde_json::Value,
    ) -> (tempfile::TempDir, Client) {
        let (dir, client) = signed_in(server).await;
        sync_with(server, joined).await;
        client
            .sync_once(matrix_sdk::config::SyncSettings::default())
            .await
            .unwrap();
        (dir, client)
    }

    #[tokio::test]
    async fn a_space_becomes_a_rail_entry_holding_its_channels_in_order() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = synced(&server, an_account_with_a_space()).await;

        let rooms = rooms::snapshot(&client).await;

        assert_eq!(rooms.spaces.len(), 2, "{rooms:?}");
        let space = &rooms.spaces[1];
        assert_eq!(space.id, "!space:example.org");
        assert_eq!(space.name, "Kahu HQ");
        assert_eq!(
            space
                .channels
                .iter()
                .map(|channel| channel.id.as_str())
                .collect::<Vec<_>>(),
            [
                "!lounge:example.org",
                "!general:example.org",
                "!never:example.org"
            ],
            "children with no order sort by when the space claimed them"
        );
    }

    #[tokio::test]
    async fn a_call_room_in_a_space_is_a_voice_channel() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = synced(&server, an_account_with_a_space()).await;

        let rooms = rooms::snapshot(&client).await;

        let lounge = &rooms.spaces[1].channels[0];
        assert_eq!(lounge.name.as_deref(), Some("Lounge"));
        assert_eq!(lounge.kind, ChannelKind::Voice);
        assert!(lounge.joined);
    }

    #[tokio::test]
    async fn a_child_the_account_never_joined_is_listed_without_a_name() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = synced(&server, an_account_with_a_space()).await;

        let rooms = rooms::snapshot(&client).await;

        let never = &rooms.spaces[1].channels[2];
        assert_eq!(never.id, "!never:example.org");
        assert_eq!(never.name, None, "nothing local knows what it is called");
        assert!(!never.joined);
    }

    #[tokio::test]
    async fn a_child_with_no_via_is_not_a_child() {
        // The spec is explicit: without a server to join through the entry is
        // unusable and should be ignored. It is also what removing a child
        // from a space looks like on the wire.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = synced(&server, an_account_with_a_space()).await;

        let rooms = rooms::snapshot(&client).await;

        assert!(
            !rooms.spaces[1]
                .channels
                .iter()
                .any(|channel| channel.id == "!dropped:example.org"),
            "{:?}",
            rooms.spaces[1].channels
        );
    }

    #[tokio::test]
    async fn a_room_no_space_claims_lands_in_home_with_a_name_that_is_not_its_id() {
        // A direct message has no `m.room.name`, so the name has to come from
        // the SDK's own calculation. Whatever that produces, showing somebody
        // a room ID is not it.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = synced(&server, an_account_with_a_space()).await;

        let rooms = rooms::snapshot(&client).await;

        let channels = &home(&rooms).channels;
        assert_eq!(ids(channels), ["!dm:example.org"]);
        assert!(channels[0].name.is_some());
        assert_ne!(channels[0].name.as_deref(), Some("!dm:example.org"));
    }

    /// Naming the children a space lists and this account has never joined.
    ///
    /// The one request the room list makes. These are about when it happens
    /// rather than what it returns: once per space per distinct set of
    /// unjoined children, after the snapshot has already gone out, and never
    /// again on a client where nothing changed.
    mod hierarchy {
        use super::*;

        /// A `/hierarchy` response naming the child nobody joined.
        fn names_the_unjoined_child(room_type: Option<&str>) -> serde_json::Value {
            let mut child = serde_json::json!({
                "room_id": "!never:example.org",
                "name": "announcements",
                "num_joined_members": 3,
                "world_readable": false,
                "guest_can_join": false,
                "children_state": [],
            });
            if let Some(room_type) = room_type {
                child["room_type"] = room_type.into();
            }

            serde_json::json!({ "rooms": [
                {
                    "room_id": "!space:example.org",
                    "name": "Kahu HQ",
                    "num_joined_members": 1,
                    "world_readable": false,
                    "guest_can_join": false,
                    "children_state": [],
                },
                child,
            ] })
        }

        async fn mount_hierarchy(server: &MatrixMockServer, body: serde_json::Value) {
            server
                .mock_get_hierarchy()
                .expect_any_access_token()
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(body))
                .mount()
                .await;
        }

        /// A watcher running against a client that has synced once.
        async fn watching(
            server: &MatrixMockServer,
            joined: serde_json::Value,
        ) -> (
            tempfile::TempDir,
            Client,
            Arc<std::sync::Mutex<Vec<Rooms>>>,
            tokio::task::JoinHandle<()>,
        ) {
            let (dir, client) = signed_in(server).await;
            sync_with(server, joined).await;

            let (seen, sink) = recorder::<Rooms>();
            let task = rooms::watch(client.clone(), sink);
            wait_until(&seen, |reports| !reports.is_empty()).await;

            client
                .sync_once(matrix_sdk::config::SyncSettings::default())
                .await
                .unwrap();

            (dir, client, seen, task)
        }

        /// The name of the child nobody joined, in the most recent report.
        fn unjoined_name(reports: &[Rooms]) -> Option<String> {
            reports
                .last()?
                .spaces
                .iter()
                .flat_map(|space| &space.channels)
                .find(|channel| channel.id == "!never:example.org")?
                .name
                .clone()
        }

        #[tokio::test]
        async fn a_child_nobody_joined_is_named_by_the_space() {
            let server = MatrixMockServer::new().await;
            mount_hierarchy(&server, names_the_unjoined_child(None)).await;
            let (_dir, _client, seen, task) = watching(&server, an_account_with_a_space()).await;

            let reports = wait_until(&seen, |reports| unjoined_name(reports).is_some()).await;
            task.abort();

            assert_eq!(unjoined_name(&reports).as_deref(), Some("announcements"));
        }

        #[tokio::test]
        async fn the_snapshot_goes_out_before_the_request_does() {
            // A slow homeserver should delay two channel names, not the whole
            // room list. The report carrying the unnamed child has to arrive
            // before the one carrying its name.
            let server = MatrixMockServer::new().await;
            mount_hierarchy(&server, names_the_unjoined_child(None)).await;
            let (_dir, _client, seen, task) = watching(&server, an_account_with_a_space()).await;

            let reports = wait_until(&seen, |reports| unjoined_name(reports).is_some()).await;
            task.abort();

            let unnamed = reports
                .iter()
                .position(|tree| {
                    tree.spaces
                        .iter()
                        .flat_map(|space| &space.channels)
                        .any(|channel| channel.id == "!never:example.org" && channel.name.is_none())
                })
                .expect("no report ever carried the unnamed child");
            let named = reports.len() - 1;

            assert!(unnamed < named, "{reports:?}");
        }

        #[tokio::test]
        async fn an_unjoined_call_room_is_still_a_voice_channel() {
            // The room type comes back in the same response as the name, so
            // there is no reason to draw it as a text channel.
            let server = MatrixMockServer::new().await;
            mount_hierarchy(
                &server,
                names_the_unjoined_child(Some("org.matrix.msc3417.call")),
            )
            .await;
            let (_dir, _client, seen, task) = watching(&server, an_account_with_a_space()).await;

            let reports = wait_until(&seen, |reports| unjoined_name(reports).is_some()).await;
            task.abort();

            let channel = reports
                .last()
                .unwrap()
                .spaces
                .iter()
                .flat_map(|space| &space.channels)
                .find(|channel| channel.id == "!never:example.org")
                .unwrap();

            assert_eq!(channel.kind, ChannelKind::Voice);
            assert!(!channel.joined);
        }

        #[tokio::test]
        async fn the_space_is_asked_once_rather_than_once_per_sync() {
            // The regression this guards is a room list that reaches the
            // homeserver every thirty seconds for the life of the session.
            let server = MatrixMockServer::new().await;
            server
                .mock_get_hierarchy()
                .expect_any_access_token()
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .set_body_json(names_the_unjoined_child(None)),
                )
                .expect(1)
                .named("one hierarchy request for one unchanged child set")
                .mount()
                .await;

            let (_dir, client, seen, task) = watching(&server, an_account_with_a_space()).await;
            wait_until(&seen, |reports| unjoined_name(reports).is_some()).await;

            for _ in 0..4 {
                client
                    .sync_once(matrix_sdk::config::SyncSettings::default())
                    .await
                    .unwrap();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            task.abort();

            // Fails here if more than the one request arrived.
            server.verify_and_reset().await;
        }

        #[tokio::test]
        async fn a_space_with_nothing_unjoined_is_never_asked_about() {
            // No mock is mounted, so a request would 404 rather than pass
            // quietly. Most accounts look like this.
            let server = MatrixMockServer::new().await;
            let joined = serde_json::json!({
                "!space:example.org": { "state": { "events": [
                    created(Some("m.space")),
                    named("Kahu HQ"),
                    child("!general:example.org", 1_000, &["example.org"]),
                ] } },
                "!general:example.org": { "state": { "events": [
                    created(None),
                    named("general"),
                ] } },
            });
            let (_dir, _client, seen, task) = watching(&server, joined).await;

            let reports = wait_until(&seen, |reports| {
                reports.last().is_some_and(|tree| tree.spaces.len() == 2)
            })
            .await;
            task.abort();

            let channels = &reports.last().unwrap().spaces[1].channels;
            assert_eq!(channels.len(), 1);
            assert!(channels[0].joined);
        }

        #[tokio::test]
        async fn a_homeserver_that_will_not_answer_is_asked_once_and_not_again() {
            // A failure still counts as having asked. Retrying on every sync
            // of a busy account is the poll this whole arrangement exists to
            // avoid, and two channels reading "Unknown channel" until the
            // child list changes is the better of the two bad outcomes.
            let server = MatrixMockServer::new().await;
            server
                .mock_get_hierarchy()
                .expect_any_access_token()
                .respond_with(wiremock::ResponseTemplate::new(502))
                .expect(1)
                .named("one failed hierarchy request, and no retry storm")
                .mount()
                .await;

            let (_dir, client, seen, task) = watching(&server, an_account_with_a_space()).await;
            wait_until(&seen, |reports| {
                reports.last().is_some_and(|tree| tree.spaces.len() == 2)
            })
            .await;

            for _ in 0..4 {
                client
                    .sync_once(matrix_sdk::config::SyncSettings::default())
                    .await
                    .unwrap();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            task.abort();

            server.verify_and_reset().await;
            assert_eq!(unjoined_name(&seen.lock().unwrap()), None);
        }
    }

    /// Who is connected to a voice channel, without joining it.
    ///
    /// Element Call announces a connection by writing an
    /// `org.matrix.msc3401.call.member` state event into the call room, so
    /// every client in the room can see who is there without touching an SFU.
    /// These are that read: the fixtures are the shapes the account this was
    /// built against actually carries, down to the underscore state key and
    /// the four hour `expires`.
    mod voice_presence {
        use super::*;
        use consort_matrix::Participant;

        const ADA: &str = "@ada:example.org";
        const BOB: &str = "@bob:example.org";
        const BEN: &str = "@ben:example.org";

        /// Four hours, which is what Element Call asks for.
        const FOUR_HOURS: u64 = 14_400_000;

        /// The wall clock, in milliseconds.
        ///
        /// A membership expires against the real clock rather than against
        /// anything in the event, so a fixture with a hardcoded join time is a
        /// fixture that starts failing on its own.
        fn now() -> u64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after 1970")
                .as_millis() as u64
        }

        /// One device's membership in a room call.
        ///
        /// `created_ts` is deliberately absent, because Element Call leaves it
        /// out of the first event of a session and the SDK fills it in from
        /// `origin_server_ts`. That fill-in is what decides expiry, so a
        /// fixture that pre-empted it would be testing the wrong thing.
        fn membership(user: &str, device: &str, at: u64, expires: u64) -> serde_json::Value {
            state_event(
                "org.matrix.msc3401.call.member",
                &format!("_{user}_{device}"),
                at,
                serde_json::json!({
                    "application": "m.call",
                    "call_id": "",
                    "scope": "m.room",
                    "device_id": device,
                    "foci_preferred": [],
                    "focus_active": {
                        "type": "livekit",
                        "focus_selection": "oldest_membership",
                    },
                    "expires": expires,
                }),
            )
        }

        /// Somebody who is in the call right now.
        fn connected(user: &str, device: &str) -> serde_json::Value {
            membership(user, device, now(), FOUR_HOURS)
        }

        /// Somebody who left. An empty content is how this dialect says so.
        fn left(user: &str, device: &str) -> serde_json::Value {
            state_event(
                "org.matrix.msc3401.call.member",
                &format!("_{user}_{device}"),
                now(),
                serde_json::json!({}),
            )
        }

        fn member(user: &str, display_name: Option<&str>) -> serde_json::Value {
            let mut content = serde_json::json!({ "membership": "join" });
            if let Some(display_name) = display_name {
                content["displayname"] = display_name.into();
            }
            state_event("m.room.member", user, 3, content)
        }

        /// A joined call room carrying `events` on top of the usual state.
        fn a_voice_channel_with(events: Vec<serde_json::Value>) -> serde_json::Value {
            let mut state = vec![created(Some("org.matrix.msc3417.call")), named("Lounge")];
            state.extend(events);

            serde_json::json!({ "!lounge:example.org": { "state": { "events": state } } })
        }

        /// Who the snapshot says is in the one voice channel.
        async fn participants(events: Vec<serde_json::Value>) -> Vec<Participant> {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, a_voice_channel_with(events)).await;

            let rooms = rooms::snapshot(&client).await;
            let channels = &home(&rooms).channels;

            assert_eq!(ids(channels), ["!lounge:example.org"], "{rooms:?}");
            assert_eq!(channels[0].kind, ChannelKind::Voice);

            channels[0].participants.clone()
        }

        fn names(participants: &[Participant]) -> Vec<&str> {
            participants
                .iter()
                .map(|participant| participant.name.as_str())
                .collect()
        }

        /// Name `user_ids` in the one voice channel, the way a live call
        /// roster is named.
        async fn named_roster(
            events: Vec<serde_json::Value>,
            user_ids: &[String],
        ) -> (consort_matrix::Client, Vec<Participant>) {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, a_voice_channel_with(events)).await;

            let people = rooms::name_participants(&client, "!lounge:example.org", user_ids).await;
            (client, people)
        }

        #[tokio::test]
        async fn a_live_roster_is_named_out_of_the_room_the_call_is_in() {
            // The other source a voice channel's list can come from. It
            // arrives from MatrixRTC signalling with user IDs and no names,
            // because a Matrix profile is per room and only a room can answer.
            let (_client, people) = named_roster(
                vec![member(ADA, Some("Ada")), member(BOB, Some("Bob"))],
                &[ADA.to_owned(), BOB.to_owned()],
            )
            .await;

            assert_eq!(names(&people), ["Ada", "Bob"]);
        }

        #[tokio::test]
        async fn a_live_roster_keeps_the_order_it_arrived_in() {
            // Oldest membership first is a stable order, and re-sorting would
            // make the list move under the pointer whenever anybody joined.
            let (_client, people) = named_roster(
                vec![member(ADA, Some("Ada")), member(BOB, Some("Bob"))],
                &[BOB.to_owned(), ADA.to_owned()],
            )
            .await;

            assert_eq!(names(&people), ["Bob", "Ada"]);
        }

        #[tokio::test]
        async fn a_live_roster_names_each_person_once() {
            // A roster is per membership and a membership is per device, so
            // somebody on a laptop and a phone arrives twice.
            let (_client, people) = named_roster(
                vec![member(ADA, Some("Ada"))],
                &[ADA.to_owned(), ADA.to_owned()],
            )
            .await;

            assert_eq!(names(&people), ["Ada"]);
        }

        #[tokio::test]
        async fn somebody_in_the_call_the_room_has_not_heard_of_is_their_user_id() {
            // The membership arriving before the `m.room.member` that explains
            // it. Unhelpful, honest, and fixed by the next sync.
            let (_client, people) = named_roster(Vec::new(), &[ADA.to_owned()]).await;

            assert_eq!(names(&people), [ADA]);
        }

        #[tokio::test]
        async fn a_roster_carrying_something_that_is_not_a_user_id_drops_it() {
            // Nothing local can be looked up under it, and putting it on
            // screen as somebody's name would be worse than the absence.
            let (_client, people) = named_roster(
                vec![member(ADA, Some("Ada"))],
                &["not a user id".to_owned(), ADA.to_owned()],
            )
            .await;

            assert_eq!(names(&people), ["Ada"]);
        }

        #[tokio::test]
        async fn a_roster_for_a_room_this_account_is_not_in_is_empty() {
            // There is no local store to read names out of, and inventing them
            // would be worse than the absence.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, a_voice_channel_with(Vec::new())).await;

            let elsewhere =
                rooms::name_participants(&client, "!nowhere:example.org", &[ADA.to_owned()]).await;
            let nonsense = rooms::name_participants(&client, "not a room", &[ADA.to_owned()]).await;

            assert!(elsewhere.is_empty(), "{elsewhere:?}");
            assert!(nonsense.is_empty(), "{nonsense:?}");
        }

        #[tokio::test]
        async fn somebody_connected_from_another_client_shows_up() {
            // The whole point. Nothing here joined a call, asked an SFU
            // anything, or clicked the channel.
            let people =
                participants(vec![member(ADA, Some("Ada")), connected(ADA, "LAPTOP")]).await;

            assert_eq!(people, [Participant::named(ADA, "Ada")]);
        }

        #[tokio::test]
        async fn somebody_who_left_is_gone() {
            // A leave is an empty content rather than a removed event, so the
            // state key stays in the room forever. Twenty-five of the
            // twenty-seven call member events on the real account are these.
            let people = participants(vec![member(ADA, Some("Ada")), left(ADA, "LAPTOP")]).await;

            assert!(people.is_empty(), "{people:?}");
        }

        #[tokio::test]
        async fn a_membership_that_ran_out_is_gone_without_anything_saying_so() {
            // The failure mode this guards is somebody whose client was killed
            // rather than closed. No event announces it: the membership simply
            // stops being valid, and every read after that has to agree.
            let people = participants(vec![
                member(ADA, Some("Ada")),
                membership(ADA, "LAPTOP", 1_000, FOUR_HOURS),
            ])
            .await;

            assert!(people.is_empty(), "{people:?}");
        }

        #[tokio::test]
        async fn one_person_on_two_devices_is_one_person() {
            // Memberships are per device. A laptop and a phone are two events,
            // and drawing both would put the same face in the channel twice.
            let people = participants(vec![
                member(ADA, Some("Ada")),
                connected(ADA, "LAPTOP"),
                connected(ADA, "PHONE"),
            ])
            .await;

            assert_eq!(names(&people), ["Ada"]);
        }

        #[tokio::test]
        async fn the_oldest_membership_is_drawn_first() {
            // Not for its own sake: an order that comes out of a map is an
            // order that changes between renders, and a list that reshuffles
            // under the pointer is worse than one in an arbitrary but fixed
            // order.
            let people = participants(vec![
                member(ADA, Some("Ada")),
                member(BEN, Some("Ben")),
                membership(BEN, "LAPTOP", now() - 60_000, FOUR_HOURS),
                membership(ADA, "LAPTOP", now() - 600_000, FOUR_HOURS),
            ])
            .await;

            assert_eq!(names(&people), ["Ada", "Ben"]);
        }

        #[tokio::test]
        async fn somebody_with_no_display_name_is_shown_by_their_user_id() {
            // Unhelpful and honest. The SDK's own fallback is the localpart,
            // which drops the server and can therefore show two different
            // people identically.
            let people = participants(vec![member(ADA, None), connected(ADA, "LAPTOP")]).await;

            assert_eq!(names(&people), [ADA]);
        }

        #[tokio::test]
        async fn somebody_the_room_has_never_mentioned_is_shown_by_their_user_id() {
            // A membership can arrive before the `m.room.member` that explains
            // it. Showing the ID is right until the next sync fixes it, and
            // leaving them out of the channel entirely would not be.
            let people = participants(vec![connected(ADA, "LAPTOP")]).await;

            assert_eq!(names(&people), [ADA]);
        }

        #[tokio::test]
        async fn two_people_who_picked_the_same_name_are_told_apart() {
            // Otherwise one of them is impersonating the other in the only
            // place the channel names either of them.
            let people = participants(vec![
                member(ADA, Some("Ada")),
                member(BEN, Some("Ada")),
                membership(ADA, "LAPTOP", now() - 600_000, FOUR_HOURS),
                membership(BEN, "LAPTOP", now() - 60_000, FOUR_HOURS),
            ])
            .await;

            assert_eq!(
                names(&people),
                [
                    "Ada (@ada:example.org)".to_owned(),
                    "Ada (@ben:example.org)".to_owned(),
                ]
            );
        }

        #[tokio::test]
        async fn a_text_room_is_never_asked_who_is_in_it() {
            // Being a call room is not the same as carrying call membership
            // state, and this read runs on every sync for every room. A text
            // channel has to pay nothing for it.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(
                &server,
                serde_json::json!({ "!general:example.org": { "state": { "events": [
                    created(None),
                    named("general"),
                    member(ADA, Some("Ada")),
                    connected(ADA, "LAPTOP"),
                ] } } }),
            )
            .await;

            let rooms = rooms::snapshot(&client).await;
            let channels = &home(&rooms).channels;

            assert_eq!(channels[0].kind, ChannelKind::Text);
            assert!(channels[0].participants.is_empty(), "{channels:?}");
        }

        #[tokio::test]
        async fn a_voice_channel_nobody_is_in_looks_exactly_as_it_did_before() {
            let people = participants(Vec::new()).await;

            assert!(people.is_empty());
        }

        #[tokio::test]
        async fn somebody_connecting_reaches_the_watcher_without_being_asked() {
            // The end to end shape of the acceptance test, minus the browser:
            // a call membership arriving in a sync is a room update, so the
            // watcher re-derives and the shell hears about it.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = signed_in(&server).await;
            sync_with(
                &server,
                a_voice_channel_with(vec![member(ADA, Some("Ada")), connected(ADA, "LAPTOP")]),
            )
            .await;

            let (seen, sink) = recorder::<Rooms>();
            let task = rooms::watch(client.clone(), sink);

            // Nothing in the watcher syncs; it only listens. The membership
            // has to arrive the way it would in the app, through the sync
            // loop, for this to be testing the path the browser exercises.
            let (connections, connection_sink) = recorder();
            let sync_task = sync::start(client, connection_sink);
            wait_until(&connections, |states| states.contains(&Connection::Live)).await;

            let reports = wait_until(&seen, |reports| {
                reports.last().is_some_and(|rooms| {
                    home(rooms)
                        .channels
                        .first()
                        .is_some_and(|channel| !channel.participants.is_empty())
                })
            })
            .await;
            sync_task.abort();
            task.abort();

            let channel = &home(reports.last().unwrap()).channels[0];
            assert_eq!(names(&channel.participants), ["Ada"]);
        }
    }

    mod avatars {
        use super::*;
        use matrix_sdk::ruma::api::client::media::get_content_thumbnail::v3::Method;

        /// The eight bytes every PNG starts with, and nothing after them.
        ///
        /// Enough for the sniffing, which is all this is testing. A real image
        /// would prove nothing extra and would put a blob in the file.
        const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

        fn with_an_avatar() -> serde_json::Value {
            serde_json::json!({
                "!general:example.org": { "state": { "events": [
                    created(None),
                    named("general"),
                    state_event(
                        "m.room.avatar",
                        "",
                        3,
                        serde_json::json!({ "url": "mxc://example.org/abc" }),
                    ),
                    // A person in the room is drawn under a voice channel, and
                    // their picture is per room rather than per account.
                    state_event(
                        "m.room.member",
                        "@ada:example.org",
                        4,
                        serde_json::json!({
                            "membership": "join",
                            "displayname": "Ada",
                            "avatar_url": "mxc://example.org/ada",
                        }),
                    ),
                    state_event(
                        "m.room.member",
                        "@ben:example.org",
                        4,
                        serde_json::json!({ "membership": "join", "displayname": "Ben" }),
                    ),
                ] } },
                "!plain:example.org": { "state": { "events": [created(None), named("plain")] } },
            })
        }

        /// Answer a thumbnail request with `bytes`, on either endpoint.
        ///
        /// Both, because which one the SDK reaches for depends on the
        /// versions the homeserver advertises, and this test is not about
        /// that choice.
        async fn mount_thumbnail(server: &MatrixMockServer, bytes: &'static [u8]) {
            let png = || wiremock::ResponseTemplate::new(200).set_body_raw(bytes, "image/png");

            server
                .mock_media_thumbnail(Method::Crop, 96, 96, false)
                .expect_any_access_token()
                .respond_with(png())
                .mount()
                .await;
            server
                .mock_authed_media_thumbnail(Method::Crop, 96, 96, false)
                .expect_any_access_token()
                .respond_with(png())
                .mount()
                .await;
        }

        #[tokio::test]
        async fn an_avatar_comes_back_as_a_data_url() {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;
            mount_thumbnail(&server, PNG).await;

            let url = rooms::avatar(&client, "!general:example.org").await;

            assert_eq!(
                url.as_deref(),
                Some("data:image/png;base64,iVBORw0KGgo="),
                "an img src has to carry the type, and base64 is what a data url is"
            );
        }

        #[tokio::test]
        async fn a_room_with_no_avatar_is_not_asked_about() {
            // Four rooms in ten on the account this was built against have no
            // avatar. Asking anyway is a request per room per launch for an
            // answer already on disk. With no thumbnail endpoint mounted at
            // all, a request here would come back as a 404 rather than a None.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(rooms::avatar(&client, "!plain:example.org").await, None);
        }

        #[tokio::test]
        async fn something_that_is_not_a_room_id_is_refused_rather_than_fetched() {
            // Home is a rail entry, not a room, and the interface asks about
            // rail entries.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(rooms::avatar(&client, "home").await, None);
        }

        #[tokio::test]
        async fn a_room_the_account_is_not_in_has_no_avatar() {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(rooms::avatar(&client, "!gone:example.org").await, None);
        }

        #[tokio::test]
        async fn bytes_that_are_not_an_image_are_refused_rather_than_shown_broken() {
            // A homeserver that answers a thumbnail request with an error page
            // would otherwise put a broken image icon in the rail. Initials
            // are better.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;
            mount_thumbnail(&server, b"<html>not an image</html>").await;

            assert_eq!(rooms::avatar(&client, "!general:example.org").await, None);
        }

        #[tokio::test]
        async fn a_member_avatar_comes_back_as_a_data_url() {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;
            mount_thumbnail(&server, PNG).await;

            let url =
                rooms::member_avatar(&client, "!general:example.org", "@ada:example.org").await;

            assert_eq!(url.as_deref(), Some("data:image/png;base64,iVBORw0KGgo="));
        }

        #[tokio::test]
        async fn a_member_with_no_avatar_is_not_asked_about() {
            // Most people in most rooms have no per-room picture, so this is
            // the ordinary case rather than the exception. With no thumbnail
            // endpoint mounted, a request here would come back a 404.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(
                rooms::member_avatar(&client, "!general:example.org", "@ben:example.org").await,
                None
            );
        }

        #[tokio::test]
        async fn a_member_the_room_has_never_mentioned_has_no_avatar() {
            // A call membership can arrive before the `m.room.member` that
            // explains it. The list draws them by initial, and asking about
            // their picture has to be harmless rather than an error.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(
                rooms::member_avatar(&client, "!general:example.org", "@nobody:example.org").await,
                None
            );
        }

        #[tokio::test]
        async fn something_that_is_not_a_user_id_is_refused_rather_than_fetched() {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;

            assert_eq!(
                rooms::member_avatar(&client, "!general:example.org", "ada").await,
                None
            );
            assert_eq!(
                rooms::member_avatar(&client, "home", "@ada:example.org").await,
                None
            );
        }

        #[tokio::test]
        async fn a_thumbnail_the_homeserver_will_not_produce_is_not_an_error() {
            let server = MatrixMockServer::new().await;
            let (_dir, client) = synced(&server, with_an_avatar()).await;
            server
                .mock_media_thumbnail(Method::Crop, 96, 96, false)
                .expect_any_access_token()
                .respond_with(wiremock::ResponseTemplate::new(502))
                .mount()
                .await;
            server
                .mock_authed_media_thumbnail(Method::Crop, 96, 96, false)
                .expect_any_access_token()
                .respond_with(wiremock::ResponseTemplate::new(502))
                .mount()
                .await;

            assert_eq!(rooms::avatar(&client, "!general:example.org").await, None);
        }
    }
}

/// Whether this session could be heard if it joined a call.
///
/// A mock reaches further here than it looks like it should. Login runs the
/// SDK's cross-signing bootstrap as a background task, so whether the account
/// ends up with an identity is decided by whether the upload endpoint answers,
/// which is a thing a mock controls exactly. That makes both ends of the
/// question reachable without a real homeserver: mount the endpoint and this
/// session has an identity it trusts, leave it unmounted and it has none, and
/// those are the two states a real account is in before and after somebody
/// sets up recovery.
mod call_readiness {
    use super::*;
    use consort_matrix::calls::{self, CallReadiness};

    /// Answer the keys query this account's own identity is looked up with.
    ///
    /// `mount_login` already mounts one, and it wants the mock crate's own
    /// default access token while this harness signs in with its own, so that
    /// one never matches. This is the one that does.
    async fn answering_key_queries(server: &MatrixMockServer) {
        server
            .mock_query_keys()
            .expect_any_access_token()
            .ok()
            .mount()
            .await;
    }

    /// A signed-in client that has not bootstrapped cross-signing.
    ///
    /// Built by hand rather than through [`auth::login`] or [`auth::restore`],
    /// because neither of those can end up here: `base_builder` sets
    /// `auto_enable_cross_signing`, so both create an identity when the
    /// account has none. What this stands in for is a bootstrap that has not
    /// finished, or one the homeserver refused, which is what real Synapse
    /// does to `/keys/device_signing/upload` on an account that already has
    /// keys and wants interactive auth for replacing them.
    async fn without_cross_signing(server: &MatrixMockServer) -> matrix_sdk::Client {
        answering_key_queries(server).await;
        server
            .client_builder()
            .logged_in_with_token(
                "syt_no_identity".to_owned(),
                ruma::user_id!("@bob:example.org").to_owned(),
                DEVICE.into(),
            )
            .build()
            .await
    }

    #[tokio::test]
    async fn an_account_with_no_cross_signing_identity_is_not_ready() {
        // The failure the spike measured, at the moment it can still be said
        // out loud. Joining from here connects, publishes membership and
        // fills in a roster, and hands a media key to nobody.
        let server = MatrixMockServer::new().await;
        let client = without_cross_signing(&server).await;

        let readiness = calls::readiness(&client).await.unwrap();

        assert_eq!(readiness, CallReadiness::NoIdentity);
        assert!(!readiness.is_ready());
    }

    #[tokio::test]
    async fn an_identity_missing_only_from_the_store_is_asked_about_before_being_ruled_out() {
        // The reason for the second lookup. Nothing has run a keys query on
        // this client, so the local store is empty whichever is true of the
        // account, and an answer taken from it alone would tell somebody who
        // has cross-signing to go and set it up.
        let server = MatrixMockServer::new().await;
        let client = without_cross_signing(&server).await;
        let user_id = client.user_id().unwrap().to_owned();
        assert!(
            client
                .encryption()
                .get_user_identity(&user_id)
                .await
                .unwrap()
                .is_none(),
            "the local store was expected to be cold"
        );

        // Reaching a verdict at all means the homeserver was asked: there was
        // nothing local to answer from, and the mock 404s anything it was not
        // set up for, which would have surfaced as an error instead.
        assert_eq!(
            calls::readiness(&client).await.unwrap(),
            CallReadiness::NoIdentity
        );
    }

    #[tokio::test]
    async fn a_session_holding_its_own_cross_signing_keys_is_ready() {
        // The other end, reached the way the app reaches it. `auth::login`
        // bootstraps, so a first sign-in onto an account with no
        // cross-signing can be heard without anybody verifying anything.
        let server = MatrixMockServer::new().await;
        server
            .mock_upload_cross_signing_keys()
            .expect_any_access_token()
            .ok()
            .mount()
            .await;
        answering_key_queries(&server).await;
        let (_dir, client) = signed_in(&server).await;

        let readiness = calls::readiness(&client).await.unwrap();

        assert_eq!(readiness, CallReadiness::Ready);
        assert!(readiness.is_ready());
    }

    #[tokio::test]
    async fn asking_before_anybody_is_signed_in_is_not_a_verdict() {
        // Reaching this is a bug in our own state handling, and the one thing
        // it must not do is answer. Both `NoIdentity` and `Ready` would be
        // inventions, and one of the two is the call nobody can hear.
        let server = MatrixMockServer::new().await;
        let client = server.client_builder().unlogged().build().await;

        let error = calls::readiness(&client).await.unwrap_err();

        assert!(
            matches!(error, consort_matrix::Error::NotLoggedIn),
            "{error}"
        );
    }
}

/// What a person's card can say about them beyond their name.
///
/// Presence is one request and its failure is routine rather than exceptional:
/// most homeservers have it switched off, and every one of these paths ends in
/// a card that still draws. That is exactly the sort of thing a unit test
/// cannot check, because there is nothing to degrade from without a server.
mod member_profiles {
    use super::*;
    use consort_matrix::rooms::{Presence, Standing, member_profile};

    const OTHER: &str = "@ada:example.org";
    const ROOM: &str = "!room:example.org";

    /// Answer the presence endpoint for `OTHER` with `response`.
    async fn presence(server: &MatrixMockServer, response: wiremock::ResponseTemplate) {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/_matrix/client/v3/presence/{OTHER}/status"
            )))
            .respond_with(response)
            .mount(server.server())
            .await;
    }

    fn saying(body: serde_json::Value) -> wiremock::ResponseTemplate {
        wiremock::ResponseTemplate::new(200).set_body_json(body)
    }

    #[tokio::test]
    async fn presence_the_homeserver_reports_reaches_the_card() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(
            &server,
            saying(serde_json::json!({
                "presence": "online",
                "status_msg": "in a meeting",
                "last_active_ago": 4_000,
            })),
        )
        .await;

        let profile = member_profile(&client, ROOM, OTHER).await;

        assert_eq!(profile.presence, Presence::Online);
        assert_eq!(profile.status.as_deref(), Some("in a meeting"));
        assert_eq!(profile.last_active_ago, Some(4_000));
    }

    #[tokio::test]
    async fn matrixs_unavailable_is_drawn_as_idle() {
        // The wire word and the word a person reads are not the same. Nobody
        // outside the spec calls "at their desk but not typing" unavailable.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(
            &server,
            saying(serde_json::json!({ "presence": "unavailable" })),
        )
        .await;

        let profile = member_profile(&client, ROOM, OTHER).await;

        assert_eq!(profile.presence, Presence::Idle);
    }

    #[tokio::test]
    async fn a_homeserver_with_presence_switched_off_yields_unknown() {
        // The ordinary case, not an edge one. Synapse ships with presence
        // disabled and most servers of any size leave it that way. Reading
        // that silence as "offline" would put a grey dot on somebody who is
        // sitting right there in the call.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(
            &server,
            wiremock::ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "errcode": "M_UNKNOWN",
                "error": "Presence is disabled on this server",
            })),
        )
        .await;

        let profile = member_profile(&client, ROOM, OTHER).await;

        assert_eq!(profile.presence, Presence::Unknown);
        assert_eq!(profile.status, None);
        assert_eq!(profile.last_active_ago, None);
    }

    #[tokio::test]
    async fn an_empty_status_message_is_not_a_status_message() {
        // Synapse returns whatever was set, and clients have been known to set
        // an empty string. A blank line under somebody's name reads as a
        // rendering fault.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(
            &server,
            saying(serde_json::json!({ "presence": "offline", "status_msg": "   " })),
        )
        .await;

        let profile = member_profile(&client, ROOM, OTHER).await;

        assert_eq!(profile.presence, Presence::Offline);
        assert_eq!(profile.status, None);
    }

    #[tokio::test]
    async fn somebody_in_no_room_this_session_knows_is_an_ordinary_member() {
        // No power levels to read, so no evidence of authority, which is
        // exactly what an ordinary member looks like.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(&server, saying(serde_json::json!({ "presence": "online" }))).await;

        let profile = member_profile(&client, ROOM, OTHER).await;

        assert_eq!(profile.standing, Standing::Member);
    }

    #[tokio::test]
    async fn something_that_is_not_a_user_id_does_not_reach_the_homeserver() {
        // Nothing is mounted for presence here, so a request would 404 and the
        // answer would be right for the wrong reason. The point is that the
        // parse fails first.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let profile = member_profile(&client, ROOM, "not a user id").await;

        assert_eq!(profile.presence, Presence::Unknown);
        assert_eq!(profile.standing, Standing::Member);
    }

    #[tokio::test]
    async fn something_that_is_not_a_room_id_is_still_an_ordinary_member() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        presence(&server, saying(serde_json::json!({ "presence": "online" }))).await;

        let profile = member_profile(&client, "not a room id", OTHER).await;

        assert_eq!(profile.standing, Standing::Member);
        assert_eq!(profile.presence, Presence::Online);
    }
}

/// One room's messages, wired to a sync loop and a paginating homeserver.
///
/// The ordering, the deduplication and the rules about which events are
/// messages are unit-tested, because all three are pure and every branch is
/// reachable without a server. What needs one, and what is here, is the wiring
/// nothing else can reach: that a page comes back the right way round, that
/// paging stops when the homeserver says there is no more, that a message
/// arriving in a sync reaches the same list, and that sending puts the right
/// thing on the wire.
///
/// The sync responses are written out rather than built, on the same terms as
/// the room list tests above: the builders live in `matrix-sdk-test`, which
/// matrix-sdk does not re-export, and one JSON literal is cheaper than a
/// second git dependency pinned to the same rev.
mod timeline {
    use super::*;
    use consort_matrix::timeline::{self, MessageKind, Timeline};
    use consort_matrix::{Connection, sync};
    use matrix_sdk::test_utils::mocks::RoomMessagesResponseTemplate;

    const ROOM: &str = "!general:example.org";
    const OTHER: &str = "@ada:example.org";

    /// One message as a homeserver returns it from `/messages`.
    ///
    /// That endpoint answers with full events, which carry a `room_id`, unlike
    /// the sync-shaped ones below. Getting the two mixed up is not a
    /// compilation error; it is a page that silently deserialises to nothing.
    fn said(id: &str, body: &str, at: u64) -> serde_json::Value {
        serde_json::json!({
            "type": "m.room.message",
            "event_id": id,
            "room_id": ROOM,
            "sender": OTHER,
            "origin_server_ts": at,
            "content": { "msgtype": "m.text", "body": body },
        })
    }

    fn raw(
        value: serde_json::Value,
    ) -> matrix_sdk::ruma::serde::Raw<ruma::events::AnyTimelineEvent> {
        matrix_sdk::ruma::serde::Raw::new(&value)
            .expect("the fixture is valid JSON")
            .cast_unchecked()
    }

    /// Answer `/messages` with `chunk`, newest first, and `end` as the token
    /// for the page behind it.
    async fn paginating(
        server: &MatrixMockServer,
        chunk: Vec<serde_json::Value>,
        end: Option<&str>,
    ) {
        let mut template = RoomMessagesResponseTemplate::default()
            .events(chunk.into_iter().map(raw).collect::<Vec<_>>());
        template = match end {
            Some(end) => template.end_token(end),
            // The start of the room, which is how the homeserver says there is
            // nothing older.
            None => RoomMessagesResponseTemplate {
                end: None,
                ..template
            },
        };
        server
            .mock_room_messages()
            .expect_any_access_token()
            .ok(template)
            .mount()
            .await;
    }

    /// A sync response carrying `events` in this room's timeline.
    ///
    /// Mounted straight onto wiremock rather than through `mock_sync`, whose
    /// builder takes a `JoinedRoomBuilder` this crate cannot name. What it
    /// costs is writing the envelope out, which also puts the shape of a sync
    /// on the page.
    async fn syncing(server: &MatrixMockServer, events: Vec<serde_json::Value>) {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/v3/sync"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "next_batch": "s2",
                    "rooms": {
                        "join": {
                            ROOM: {
                                "timeline": { "events": events, "limited": false },
                            },
                        },
                    },
                })),
            )
            .mount(server.server())
            .await;
    }

    /// The same message as it arrives in a sync, which carries no `room_id`.
    fn arriving(id: &str, body: &str, at: u64) -> serde_json::Value {
        serde_json::json!({
            "type": "m.room.message",
            "event_id": id,
            "sender": OTHER,
            "origin_server_ts": at,
            "content": { "msgtype": "m.text", "body": body },
        })
    }

    fn bodies(timeline: &Timeline) -> Vec<&str> {
        timeline
            .messages
            .iter()
            .map(|message| message.body.as_str())
            .collect()
    }

    /// The last report that was not a spinner going up.
    fn settled(reports: &[Timeline]) -> Option<&Timeline> {
        reports.iter().rev().find(|report| !report.loading)
    }

    #[tokio::test]
    async fn opening_a_room_reads_a_page_of_its_history() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        // Newest first, which is what a backwards pagination answers with.
        paginating(
            &server,
            vec![said("$2", "second", 2_000), said("$1", "first", 1_000)],
            None,
        )
        .await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);

        let settled = settled(&reports).unwrap();
        assert_eq!(settled.room_id, ROOM);
        assert_eq!(
            bodies(settled),
            vec!["first", "second"],
            "a backwards page has to be turned round, or every page reads backwards"
        );
    }

    #[tokio::test]
    async fn a_room_at_the_start_of_its_history_offers_nothing_more() {
        // The homeserver said there is no page behind this one, so the
        // interface must not draw a control that asks for one.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        paginating(&server, vec![said("$1", "first", 1_000)], None).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);

        assert!(!settled(&reports).unwrap().more_before);
    }

    #[tokio::test]
    async fn a_room_with_more_behind_it_says_so() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        paginating(&server, vec![said("$1", "first", 1_000)], Some("t-older")).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);

        assert!(settled(&reports).unwrap().more_before);
    }

    #[tokio::test]
    async fn a_room_this_account_is_not_in_reports_itself_as_empty() {
        // Left from another session between the room list being drawn and
        // somebody clicking a channel in it. Reported rather than silent, or
        // the pane keeps whatever room was open before.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| !reports.is_empty()).await;
        drop(watch);

        assert_eq!(reports[0].room_id, ROOM);
        assert!(reports[0].messages.is_empty());
    }

    #[tokio::test]
    async fn something_that_is_not_a_room_id_still_answers() {
        // A reader waiting for a timeline that never arrives is a pane stuck
        // on the room before it, which is worse than an empty one.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, "not a room id", sink);
        let reports = wait_until(&seen, |reports| !reports.is_empty()).await;
        drop(watch);

        assert_eq!(reports[0].room_id, "not a room id");
    }

    #[tokio::test]
    async fn a_message_arriving_in_a_sync_reaches_the_timeline() {
        // The path that makes this a chat client rather than an archive
        // viewer, and the one no unit test can reach: a sync response, through
        // the SDK's update channel, into the list somebody is reading.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        paginating(&server, Vec::new(), None).await;
        syncing(&server, vec![arriving("$new", "just said", 5_000)]).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client.clone(), ROOM, sink);
        let (connections, connection_sink) = recorder();
        let syncing = sync::start(client, connection_sink);
        wait_until(&connections, |states| states.contains(&Connection::Live)).await;

        let reports = wait_until(&seen, |reports| {
            reports
                .last()
                .is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);
        syncing.abort();

        assert_eq!(bodies(reports.last().unwrap()), vec!["just said"]);
    }

    #[tokio::test]
    async fn a_sync_that_says_nothing_about_this_room_does_not_republish_it() {
        // A sync delivers one update whether or not this room was in it, and
        // sync fires forever. Republishing on every one of them would wake the
        // webview twice a minute to hand it the list it already has.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        paginating(&server, vec![said("$1", "first", 1_000)], None).await;
        syncing(&server, Vec::new()).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client.clone(), ROOM, sink);
        wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        let (connections, connection_sink) = recorder();
        let syncing = sync::start(client, connection_sink);
        wait_until(&connections, |states| states.contains(&Connection::Live)).await;

        let after_first = seen.lock().unwrap().len();
        // Long enough for several more syncs against a mock that answers at
        // once.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let after_several = seen.lock().unwrap().len();
        drop(watch);
        syncing.abort();

        assert_eq!(after_first, after_several, "an idle sync redrew the room");
    }

    #[tokio::test]
    async fn asking_for_more_puts_the_older_page_in_front() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;

        // Scoped, so the second page can replace it. Newest first, and a token
        // saying there is more behind it.
        let first = server
            .mock_room_messages()
            .expect_any_access_token()
            .ok(RoomMessagesResponseTemplate::default()
                .events(vec![raw(said("$2", "second", 2_000))])
                .end_token("t-older"))
            .mount_as_scoped()
            .await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;

        drop(first);
        paginating(&server, vec![said("$1", "first", 1_000)], None).await;
        watch.earlier();

        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| report.messages.len() == 2)
        })
        .await;
        drop(watch);

        assert_eq!(bodies(settled(&reports).unwrap()), vec!["first", "second"]);
    }

    #[tokio::test]
    async fn asking_for_more_at_the_start_of_the_room_asks_nobody() {
        // The homeserver already said there is nothing older. Asking again
        // would be a request per press of a control that should not be drawn.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;

        let page = server
            .mock_room_messages()
            .expect_any_access_token()
            .ok(RoomMessagesResponseTemplate::default()
                .events(vec![raw(said("$1", "first", 1_000))]))
            .expect(1)
            .mount_as_scoped()
            .await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;

        watch.earlier();
        watch.earlier();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        drop(watch);
        // `expect(1)` is checked here: a second request would fail this.
        drop(page);
    }

    #[tokio::test]
    async fn a_page_of_nothing_but_state_events_keeps_looking() {
        // The beginning of every room is a dozen state events before the first
        // word. An ask that fetched exactly one page would answer a scroll
        // with nothing and look broken.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;

        let joins = server
            .mock_room_messages()
            .expect_any_access_token()
            .ok(RoomMessagesResponseTemplate::default()
                .events(vec![raw(serde_json::json!({
                    "type": "m.room.member",
                    "event_id": "$join",
                    "room_id": ROOM,
                    "sender": OTHER,
                    "state_key": OTHER,
                    "origin_server_ts": 500,
                    "content": { "membership": "join" },
                }))])
                .end_token("t-older"))
            .up_to_n_times(1)
            .mount_as_scoped()
            .await;
        paginating(&server, vec![said("$1", "first", 1_000)], None).await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);
        drop(joins);

        assert_eq!(bodies(settled(&reports).unwrap()), vec!["first"]);
    }

    #[tokio::test]
    async fn sending_puts_the_text_on_the_wire() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        // Plain, so the send is not held up trying to establish a megolm
        // session against a mock with no devices in it. What is being tested
        // is that the text reaches the wire; whether the SDK encrypts it is
        // the SDK's own decision from the room's state and its own tests.
        server
            .mock_room_state_encryption()
            .expect_any_access_token()
            .plain()
            .mount()
            .await;
        server
            .mock_room_send()
            .expect_any_access_token()
            .body_matches_partial_json(serde_json::json!({
                "msgtype": "m.text",
                "body": "hello",
            }))
            .ok(ruma::event_id!("$sent:example.org"))
            .expect(1)
            .mount()
            .await;

        timeline::send(&client, ROOM, "hello").await.unwrap();
    }

    #[tokio::test]
    async fn sending_markdown_puts_the_formatting_on_the_wire_beside_the_text() {
        // What "###" in the box has to become. The plain body is kept as the
        // fallback every client without HTML draws, and the formatting rides
        // beside it; sending only the first is what made a heading arrive as
        // three hashes.
        //
        // The HTML itself is not asserted. Which tags pulldown-cmark emits is
        // ruma's business and it has its own tests for it; what is ours is
        // that the format is declared and the source survives as the fallback.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        server
            .mock_room_state_encryption()
            .expect_any_access_token()
            .plain()
            .mount()
            .await;
        server
            .mock_room_send()
            .expect_any_access_token()
            .body_matches_partial_json(serde_json::json!({
                "msgtype": "m.text",
                "body": "### Heading",
                "format": "org.matrix.custom.html",
            }))
            .ok(ruma::event_id!("$sent:example.org"))
            .expect(1)
            .mount()
            .await;

        timeline::send(&client, ROOM, "### Heading").await.unwrap();
    }

    #[tokio::test]
    async fn sending_nothing_never_reaches_the_homeserver() {
        // Nothing is mounted for a send here, so a request would fail and the
        // answer would be right for the wrong reason. The point is that the
        // emptiness is caught first.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;

        let refused = timeline::send(&client, ROOM, "   \n  ").await.unwrap_err();

        assert!(matches!(refused, consort_matrix::Error::EmptyMessage));
        assert!(!refused.user_message().is_empty());
    }

    #[tokio::test]
    async fn sending_to_a_room_this_account_is_not_in_says_so() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let refused = timeline::send(&client, ROOM, "hello").await.unwrap_err();

        assert!(matches!(
            refused,
            consort_matrix::Error::NoSuchRoom { ref room_id } if room_id == ROOM
        ));
        assert!(!refused.user_message().is_empty());
    }

    #[tokio::test]
    async fn sending_to_something_that_is_not_a_room_id_never_reaches_the_homeserver() {
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;

        let refused = timeline::send(&client, "not a room id", "hello")
            .await
            .unwrap_err();

        assert!(matches!(refused, consort_matrix::Error::NoSuchRoom { .. }));
    }

    #[tokio::test]
    async fn an_image_arrives_with_a_handle_to_fetch_it_by() {
        // The end-to-end half of the unit test on the same rule. What crosses
        // the boundary is the address of the picture and not the picture, on
        // the same terms as an avatar: a timeline is re-sent in full whenever
        // anything in it changes.
        let server = MatrixMockServer::new().await;
        let (_dir, client) = signed_in(&server).await;
        server
            .sync_joined_room(&client, ruma::room_id!("!general:example.org"))
            .await;
        paginating(
            &server,
            vec![serde_json::json!({
                "type": "m.room.message",
                "event_id": "$image",
                "room_id": ROOM,
                "sender": OTHER,
                "origin_server_ts": 1_000,
                "content": {
                    "msgtype": "m.image",
                    "body": "screenshot.png",
                    "url": "mxc://example.org/abc",
                },
            })],
            None,
        )
        .await;

        let (seen, sink) = recorder::<Timeline>();
        let watch = timeline::watch(client, ROOM, sink);
        let reports = wait_until(&seen, |reports| {
            settled(reports).is_some_and(|report| !report.messages.is_empty())
        })
        .await;
        drop(watch);

        let drawn = &settled(&reports).unwrap().messages[0];
        assert_eq!(drawn.kind, MessageKind::Image);
        assert_eq!(drawn.body, "screenshot.png");
        assert!(
            drawn
                .media
                .as_ref()
                .is_some_and(|media| media.source.contains("mxc://example.org/abc")),
            "an image must reach the interface with something to fetch it by"
        );
    }

    mod attachments {
        use super::*;

        /// The eight bytes every PNG starts with, and nothing after them.
        ///
        /// Enough for the sniffing, which is what these are about. A real
        /// image would prove nothing extra and would put a blob in the file.
        const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

        /// The handle a plain, unencrypted attachment reaches the interface as.
        const HANDLE: &str = r#"{"url":"mxc://example.org/abc"}"#;

        /// Answer a download with `bytes`, on either endpoint.
        ///
        /// Both, because which one the SDK reaches for depends on the versions
        /// the homeserver advertises, and these tests are not about that
        /// choice.
        async fn mount_download(server: &MatrixMockServer, bytes: &'static [u8]) {
            let body = || wiremock::ResponseTemplate::new(200).set_body_raw(bytes, "image/png");

            server
                .mock_media_download()
                .expect_any_access_token()
                .respond_with(body())
                .mount()
                .await;
            server
                .mock_authed_media_download()
                .expect_any_access_token()
                .respond_with(body())
                .mount()
                .await;
        }

        #[tokio::test]
        async fn a_picture_comes_back_as_the_bytes_it_was_sent_as() {
            // Byte for byte, and not encoded on the way. The interface makes a
            // blob out of these, which is the whole reason they are not a data
            // URL like an avatar.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = signed_in(&server).await;
            mount_download(&server, PNG).await;

            let bytes = timeline::media(&client, HANDLE).await.unwrap();

            assert_eq!(bytes, PNG);
        }

        #[tokio::test]
        async fn something_that_is_not_media_is_refused_rather_than_handed_over() {
            // What a homeserver answers with when the file has been removed,
            // and what a sender who lied about their upload produces.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = signed_in(&server).await;
            mount_download(&server, b"<!doctype html>").await;

            let error = timeline::media(&client, HANDLE).await.unwrap_err();

            assert!(!error.user_message().is_empty());
        }

        #[tokio::test]
        async fn a_homeserver_that_will_not_hand_it_over_is_an_answer_not_a_panic() {
            // Nothing mounted, so the download is a 404.
            let server = MatrixMockServer::new().await;
            let (_dir, client) = signed_in(&server).await;

            assert!(timeline::media(&client, HANDLE).await.is_err());
        }
    }
}
