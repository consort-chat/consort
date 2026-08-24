// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The half that needs a homeserver rather than a mock.
//!
//! `MatrixMockServer` covers more than it looks like it should: it can inject
//! a to-device `m.key.verification.request` into a sync response, so most of
//! the verification work is testable without any of this. What it cannot do is
//! olm. A SAS handshake is real cryptography between two real devices, and
//! there is no way to fake one side of it.
//!
//! So these are `#[ignore]`d and gated on `CONSORT_TEST_HOMESERVER`. A plain
//! `cargo test` skips them, CI skips them, and nobody needs Docker to
//! contribute. Run them deliberately:
//!
//! ```sh
//! testing/synapse/up.sh
//! export CONSORT_TEST_HOMESERVER=http://localhost:8008
//! cargo test --workspace -- --ignored
//! ```
//!
//! Running one with the variable unset fails rather than passing quietly. A
//! test that reports success because it did nothing is worse than no test.
//!
//! Every test that verifies anything registers an account of its own. Two
//! reasons, and both of them bite. A verification request goes to every device
//! on the account and `cargo test` runs these at the same time, so two tests
//! sharing an account would answer each other's requests. And the first login
//! to an account is the one that bootstraps a cross-signing identity and keeps
//! the private keys: log in twice to an account that already has an identity
//! and neither device can sign anything, so the session under test can never
//! become verified. A reused account makes those tests pass once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use consort_matrix::secrets::MemoryBackend;
use consort_matrix::{
    Connection, Credentials, SessionStore, SessionVerification, sync, verification,
};

/// The accounts `testing/synapse/up.sh` creates.
const PASSWORD: &str = "consort-test-only";

fn homeserver() -> String {
    std::env::var("CONSORT_TEST_HOMESERVER").expect(
        "these tests need a homeserver. Start one with testing/synapse/up.sh and export \
         CONSORT_TEST_HOMESERVER=http://localhost:8008",
    )
}

/// A store in a fresh directory, which is what makes a second login a second
/// device rather than a reuse of the first.
fn store(dir: &tempfile::TempDir) -> SessionStore {
    SessionStore::with_backend(dir.path(), Arc::new(MemoryBackend::new()))
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        server: homeserver(),
        username: username.to_owned(),
        password: PASSWORD.to_owned(),
    }
}

/// Register an account nobody has used before, and return its credentials.
///
/// `inhibit_login` so registering does not also create a device: the tests
/// count devices, and a third one that never syncs would be a device list
/// entry nothing explains.
async fn a_brand_new_account(prefix: &str) -> Credentials {
    // Unique per run rather than random, so a failed run leaves an account
    // whose name says which test made it.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let username = format!("{prefix}-{stamp}");

    let client = matrix_sdk::Client::builder()
        .homeserver_url(homeserver())
        .build()
        .await
        .expect("could not reach the test homeserver");

    let mut request = matrix_sdk::ruma::api::client::account::register::v3::Request::new();
    request.username = Some(username.clone());
    request.password = Some(PASSWORD.to_owned());
    request.inhibit_login = true;
    // Synapse offers a single dummy stage when registration is open, and
    // sending it up front saves the round trip that would otherwise come back
    // as a 401 carrying the flows.
    request.auth = Some(matrix_sdk::ruma::api::client::uiaa::AuthData::Dummy(
        matrix_sdk::ruma::api::client::uiaa::Dummy::new(),
    ));

    client.matrix_auth().register(request).await.expect(
        "registration failed. testing/synapse/up.sh turns it on; a homeserver started before \
         that change has it off and needs down.sh first",
    );

    credentials(&username)
}

async fn wait_for(seen: &Arc<Mutex<Vec<Connection>>>, want: Connection) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if seen.lock().unwrap().contains(&want) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited for {want:?}; saw {:?}",
            seen.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn recorder() -> (
    Arc<Mutex<Vec<Connection>>>,
    impl Fn(Connection) + Send + Sync + 'static,
) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let seen = seen.clone();
        move |state: Connection| seen.lock().unwrap().push(state)
    };
    (seen, sink)
}

#[tokio::test]
#[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
async fn a_real_login_syncs() {
    // The whole harness, end to end: a real Synapse, a real login, a real
    // /sync. Everything else in this file assumes this works.
    let dir = tempfile::tempdir().unwrap();
    let (client, profile) = consort_matrix::auth::login(&store(&dir), &credentials("alice"))
        .await
        .unwrap();

    assert!(profile.user_id.starts_with("@alice:"));

    let (seen, sink) = recorder();
    let task = sync::start(client, sink);
    wait_for(&seen, Connection::Live).await;
    task.abort();
}

#[tokio::test]
#[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
async fn one_account_can_hold_two_devices_that_can_see_each_other() {
    // What self-verification needs, and the reason the harness creates
    // accounts rather than a single one. Each login gets its own data
    // directory, which is what makes it a second device instead of the SDK
    // reopening the first one's store.
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();

    let account = a_brand_new_account("two-devices").await;
    let (first, first_profile) = consort_matrix::auth::login(&store(&first_dir), &account)
        .await
        .unwrap();
    let (second, second_profile) = consort_matrix::auth::login(&store(&second_dir), &account)
        .await
        .unwrap();

    assert_eq!(first_profile.user_id, second_profile.user_id);
    assert_ne!(
        first_profile.device_id, second_profile.device_id,
        "two logins produced one device, so there is nothing to verify against"
    );

    // Both need to be syncing before either can see the other's keys: device
    // lists arrive through /sync like everything else.
    let (first_seen, first_sink) = recorder();
    let (second_seen, second_sink) = recorder();
    let first_task = sync::start(first.clone(), first_sink);
    let second_task = sync::start(second.clone(), second_sink);
    wait_for(&first_seen, Connection::Live).await;
    wait_for(&second_seen, Connection::Live).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let ready = second
            .encryption()
            .has_devices_to_verify_against()
            .await
            .unwrap();
        if ready {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the second device never saw the first one"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    first_task.abort();
    second_task.abort();
}

#[tokio::test]
#[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
async fn a_real_login_is_reported_unverified() {
    // The mock server says the same thing, but it says it from a crypto store
    // that never met a homeserver. This is the one that proves the banner is
    // right about a real account on a real Synapse, which is what Phase 2 will
    // be watching change.
    let dir = tempfile::tempdir().unwrap();
    let (client, _) =
        consort_matrix::auth::login(&store(&dir), &a_brand_new_account("unverified").await)
            .await
            .unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let seen = seen.clone();
        move |state: SessionVerification| seen.lock().unwrap().push(state)
    };
    let task = verification::watch(client, sink);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let states = seen.lock().unwrap().clone();
        if states.contains(&SessionVerification::Unverified) {
            assert!(
                !states.contains(&SessionVerification::Verified),
                "a session nobody has verified was reported verified: {states:?}"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the watcher never reported an unverified session; saw {states:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    task.abort();
}

// The pieces every live verification test needs.
//
// The other device is another matrix-sdk client driven from the test rather
// than Element, so the whole flow runs unattended in one `cargo test`. That is
// not the same claim as interoperating: "works against another copy of the
// same SDK" leaves room for both copies agreeing on something the spec does
// not say, so Element still gets tried by hand before the milestone closes.
// What this does prove is that our half is correct against a real homeserver,
// real to-device delivery and real olm, which no mock can.
use consort_matrix::{Flow, FlowState};
use futures_util::StreamExt;
use matrix_sdk::Client;
use matrix_sdk::encryption::verification::{
    SasState, SasVerification, VerificationRequest, VerificationRequestState,
};

/// What the device on the other end of the flow decides to do.
enum Choice {
    /// Say the emoji matched.
    Confirm,
    /// Say they did not, which is the answer the whole exchange exists to
    /// make possible.
    Mismatch,
}

/// Two devices on one account, both syncing and both aware of the other.
///
/// The awareness is not decoration. A verification request from a device
/// the crypto store has never seen is dropped without a word, so a test
/// that starts the flow before the device lists have caught up fails for a
/// reason that has nothing to do with verification.
async fn two_devices(prefix: &str) -> (Device, Device) {
    let account = a_brand_new_account(prefix).await;

    // The other device first, deliberately. The first login to an account
    // is the one that bootstraps the cross-signing identity and keeps the
    // private keys, and it is those keys that sign the other device at the
    // end of a successful flow. Log Consort in first and there would be
    // nothing on the far end able to sign it.
    let theirs = Device::new(&account).await;
    let ours = Device::new(&account).await;

    assert_ne!(
        ours.client.device_id(),
        theirs.client.device_id(),
        "two logins produced one device"
    );

    for (device, other) in [(&ours, &theirs), (&theirs, &ours)] {
        let wanted = other.client.device_id().unwrap().to_owned();
        let user_id = device.client.user_id().unwrap().to_owned();

        // A request from a device the crypto store has never seen is
        // dropped without a word, so starting the flow before the device
        // lists have caught up fails for a reason that has nothing to do
        // with verification.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let known = device
                .client
                .encryption()
                .get_user_devices(&user_id)
                .await
                .unwrap()
                .devices()
                .any(|device| device.device_id() == wanted);
            if known {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "a device never saw the other one"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    (ours, theirs)
}

/// One signed-in, syncing client, with its directory kept alive.
struct Device {
    client: Client,
    _dir: tempfile::TempDir,
    sync: tokio::task::JoinHandle<()>,
}

impl Device {
    async fn new(account: &Credentials) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (client, _) = consort_matrix::auth::login(&store(&dir), account)
            .await
            .unwrap();

        let (seen, sink) = recorder();
        let sync = sync::start(client.clone(), sink);
        wait_for(&seen, Connection::Live).await;

        Self {
            client,
            _dir: dir,
            sync,
        }
    }

    /// Ask to verify another of this account's own sessions.
    async fn ask_to_verify_us(&self) -> VerificationRequest {
        let user_id = self.client.user_id().unwrap();
        self.client
            .encryption()
            .get_user_identity(user_id)
            .await
            .unwrap()
            .expect("the account has no cross-signing identity to verify against")
            .request_verification()
            .await
            .unwrap()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.sync.abort();
    }
}

/// Everything the supervisor has reported so far.
fn flow_recorder() -> (Arc<Mutex<Vec<Flow>>>, impl Fn(Flow) + Send + Sync + 'static) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let seen = seen.clone();
        move |flow: Flow| seen.lock().unwrap().push(flow)
    };
    (seen, sink)
}

/// Wait for the supervisor to report a state the predicate likes.
async fn wait_for_flow(
    seen: &Arc<Mutex<Vec<Flow>>>,
    what: &str,
    matches: impl Fn(&FlowState) -> bool,
) -> Flow {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let found = seen
            .lock()
            .unwrap()
            .iter()
            .find(|flow| matches(&flow.state))
            .cloned();
        if let Some(flow) = found {
            return flow;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited for {what}; saw {:?}",
            seen.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The emoji symbols, in order, out of a comparison state.
fn symbols(state: &FlowState) -> Vec<String> {
    let FlowState::Comparing { emoji, .. } = state else {
        panic!("expected a comparison, got {state:?}");
    };
    emoji.iter().map(|pair| pair.symbol.clone()).collect()
}

/// Drive the far end of the flow the way a person with a phone would.
/// Drive the far end of the flow the way a person with a phone would.
///
/// Returns the emoji it was shown, so the test can check both sides were
/// looking at the same seven. Comparing them is the point of the exchange, and
/// a client that showed a different set from its counterparty would pass every
/// state-machine assertion while being useless.
async fn play_the_other_device(request: VerificationRequest, choice: Choice) -> Vec<String> {
    // Bounded, because every wait in here is on the far end of a real
    // network and a test that hangs tells nobody anything. Sixty seconds
    // is the same budget the rest of this file uses.
    tokio::time::timeout(Duration::from_secs(60), play(request, choice))
        .await
        .expect("the other device never finished its half of the flow")
}

async fn play(request: VerificationRequest, choice: Choice) -> Vec<String> {
    let sas = wait_for_sas(&request).await;

    // Same reason Consort does it: whoever did not start the exchange has
    // to settle the algorithms before anything else happens.
    if !sas.we_started() {
        sas.accept().await.unwrap();
    }

    answer(sas, choice).await
}

/// Watch a comparison, answer it, and report the emoji it was shown.
async fn answer(sas: SasVerification, choice: Choice) -> Vec<String> {
    let mut seen = Vec::new();
    let mut state = Some(sas.state());
    let mut changes = sas.changes();

    loop {
        let Some(current) = state.take() else {
            match changes.next().await {
                Some(next) => {
                    state = Some(next);
                    continue;
                }
                None => panic!("the other device's stream ended early"),
            }
        };

        match current {
            SasState::KeysExchanged { emojis, .. } => {
                seen = emojis
                    .map(|sas| {
                        sas.emojis
                            .iter()
                            .map(|emoji| emoji.symbol.to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                match choice {
                    Choice::Confirm => sas.confirm().await.unwrap(),
                    Choice::Mismatch => sas.mismatch().await.unwrap(),
                }
            }
            SasState::Done { .. } | SasState::Cancelled(_) => return seen,
            _ => {}
        }
    }
}

/// Wait until the request has turned into an emoji comparison, starting
/// one unless the other side gets there first.
async fn wait_for_sas(request: &VerificationRequest) -> SasVerification {
    let mut state = Some(request.state());
    let mut changes = request.changes();

    loop {
        let Some(current) = state.take() else {
            match changes.next().await {
                Some(next) => {
                    state = Some(next);
                    continue;
                }
                None => panic!("the request stream ended before a comparison started"),
            }
        };

        match current {
            // We asked, so starting the comparison is ours to do. Consort
            // as the responder deliberately waits for this rather than
            // racing to send its own `m.key.verification.start`.
            VerificationRequestState::Ready { .. } => {
                if let Some(sas) = request.start_sas().await.unwrap() {
                    return sas;
                }
            }
            VerificationRequestState::Transitioned { verification } => {
                return verification.sas().expect("not an emoji verification");
            }
            VerificationRequestState::Cancelled(info) => {
                panic!(
                    "the flow was cancelled before it started: {}",
                    info.reason()
                )
            }
            _ => {}
        }
    }
}

/// Follow a flow to the end without ever starting the comparison.
///
/// The far end of the one test where Consort is the one that starts it.
async fn follow_without_starting(request: VerificationRequest) -> Vec<String> {
    tokio::time::timeout(Duration::from_secs(60), async move {
        let mut state = Some(request.state());
        let mut changes = request.changes();

        let sas = loop {
            let Some(current) = state.take() else {
                state = Some(changes.next().await.expect("the request stream ended"));
                continue;
            };
            match current {
                VerificationRequestState::Transitioned { verification } => {
                    break verification.sas().expect("not an emoji verification");
                }
                VerificationRequestState::Cancelled(info) => {
                    panic!("the flow was cancelled: {}", info.reason())
                }
                _ => {}
            }
        };

        sas.accept().await.unwrap();
        answer(sas, Choice::Confirm).await
    })
    .await
    .expect("the other device never finished its half of the flow")
}

/// The emoji handshake, both sides of it.
mod emoji {
    use super::*;

    #[tokio::test]
    #[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
    async fn another_session_can_verify_this_one_by_emoji() {
        let (ours, theirs) = two_devices("emoji").await;

        let (flows, sink) = flow_recorder();
        let supervisor = verification::supervise(ours.client.clone(), sink);

        let their_request = theirs.ask_to_verify_us().await;

        // The request reaches us and is waiting for an answer.
        let asked = wait_for_flow(&flows, "the request", |state| {
            matches!(state, FlowState::Requested)
        })
        .await;
        assert!(asked.is_self_verification);
        verification::accept(&ours.client, &asked.other_user_id, &asked.flow_id)
            .await
            .unwrap();

        let their_side = tokio::spawn(play_the_other_device(their_request, Choice::Confirm));

        // Seven emoji, and the same seven the other device is looking at.
        let comparing = wait_for_flow(&flows, "the emoji", |state| {
            matches!(state, FlowState::Comparing { .. })
        })
        .await;
        let ours_saw = symbols(&comparing.state);
        assert_eq!(ours_saw.len(), 7, "{:?}", comparing.state);

        verification::confirm(&ours.client, &comparing.other_user_id, &comparing.flow_id)
            .await
            .unwrap();

        let theirs_saw = their_side.await.unwrap();
        assert_eq!(
            ours_saw, theirs_saw,
            "the two devices were shown different emoji"
        );

        wait_for_flow(&flows, "both sides to agree", |state| {
            matches!(state, FlowState::Done)
        })
        .await;

        supervisor.abort();
    }

    #[tokio::test]
    #[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
    async fn verifying_this_way_makes_the_session_report_itself_verified() {
        // The reason any of this exists. Reaching `Done` is the protocol
        // working; the banner changing is the milestone working, and the two
        // are not the same event: the session only becomes verified once the
        // other device has signed this one and sent over the cross-signing
        // secrets, which happens after the handshake ends.
        let (ours, theirs) = two_devices("verified").await;

        let states = Arc::new(Mutex::new(Vec::new()));
        let watcher = verification::watch(ours.client.clone(), {
            let states = states.clone();
            move |state: SessionVerification| states.lock().unwrap().push(state)
        });

        let (flows, sink) = flow_recorder();
        let supervisor = verification::supervise(ours.client.clone(), sink);

        let their_request = theirs.ask_to_verify_us().await;
        let asked = wait_for_flow(&flows, "the request", |state| {
            matches!(state, FlowState::Requested)
        })
        .await;
        verification::accept(&ours.client, &asked.other_user_id, &asked.flow_id)
            .await
            .unwrap();

        let their_side = tokio::spawn(play_the_other_device(their_request, Choice::Confirm));

        let comparing = wait_for_flow(&flows, "the emoji", |state| {
            matches!(state, FlowState::Comparing { .. })
        })
        .await;
        verification::confirm(&ours.client, &comparing.other_user_id, &comparing.flow_id)
            .await
            .unwrap();
        their_side.await.unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if states
                .lock()
                .unwrap()
                .contains(&SessionVerification::Verified)
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the session never reported itself verified; saw {:?}",
                states.lock().unwrap()
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        supervisor.abort();
        watcher.abort();
    }

    #[tokio::test]
    #[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
    async fn saying_the_emoji_do_not_match_ends_the_flow_and_names_why() {
        // The answer the exchange exists to make possible. It has to be
        // distinguishable from an ordinary cancellation, because one of them
        // means somebody changed their mind and the other means somebody may
        // be sitting in the middle.
        let (ours, theirs) = two_devices("mismatch").await;

        let (flows, sink) = flow_recorder();
        let supervisor = verification::supervise(ours.client.clone(), sink);

        let their_request = theirs.ask_to_verify_us().await;
        let asked = wait_for_flow(&flows, "the request", |state| {
            matches!(state, FlowState::Requested)
        })
        .await;
        verification::accept(&ours.client, &asked.other_user_id, &asked.flow_id)
            .await
            .unwrap();

        let their_side = tokio::spawn(play_the_other_device(their_request, Choice::Mismatch));

        let ended = wait_for_flow(&flows, "the cancellation", |state| {
            matches!(state, FlowState::Cancelled { .. })
        })
        .await;
        their_side.await.unwrap();
        supervisor.abort();

        let FlowState::Cancelled { reason, by_us, .. } = ended.state else {
            unreachable!()
        };
        assert_eq!(reason, consort_matrix::verification::CancelReason::Mismatch);
        assert!(!by_us, "the other device said no, not this one");

        let states: Vec<_> = flows
            .lock()
            .unwrap()
            .iter()
            .map(|flow| flow.state.clone())
            .collect();
        assert!(
            !states.contains(&FlowState::Done),
            "a rejected verification reported success along the way: {states:?}"
        );
    }
}

/// The two ways a flow ends without verifying anything.
mod refusal {
    use super::*;

    use consort_matrix::verification::CancelReason;

    #[tokio::test]
    #[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
    async fn declining_a_request_ends_it_for_both_sides() {
        // Somebody presses "not now". Distinct from a mismatch, which is what
        // the emoji module covers: this one is the ordinary answer and should
        // not look like a security event to anybody.
        let (ours, theirs) = two_devices("declined").await;

        let (flows, sink) = flow_recorder();
        let supervisor = verification::supervise(ours.client.clone(), sink);

        let their_request = theirs.ask_to_verify_us().await;

        let asked = wait_for_flow(&flows, "the request", |state| {
            matches!(state, FlowState::Requested)
        })
        .await;
        verification::cancel(&ours.client, &asked.other_user_id, &asked.flow_id)
            .await
            .unwrap();

        let ended = wait_for_flow(&flows, "the cancellation", |state| {
            matches!(state, FlowState::Cancelled { .. })
        })
        .await;
        supervisor.abort();

        let FlowState::Cancelled { reason, by_us, .. } = ended.state else {
            unreachable!()
        };
        assert_eq!(reason, CancelReason::Declined);
        assert!(by_us, "this device is the one that said no");

        // And the other device is told, rather than being left waiting for an
        // answer that is never coming.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        while !their_request.is_cancelled() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the other device was never told the request was declined"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[tokio::test]
    #[ignore = "needs testing/synapse/up.sh and CONSORT_TEST_HOMESERVER"]
    async fn this_session_can_start_the_comparison_when_the_other_side_will_not() {
        // The case `start_sas` exists for. Whoever asked normally starts the
        // comparison as soon as the other side is ready, and if both wait for
        // that then both wait forever. Here the far end deliberately does not,
        // and Consort gets it moving.
        let (ours, theirs) = two_devices("started-here").await;

        let (flows, sink) = flow_recorder();
        let supervisor = verification::supervise(ours.client.clone(), sink);

        let their_request = theirs.ask_to_verify_us().await;
        let asked = wait_for_flow(&flows, "the request", |state| {
            matches!(state, FlowState::Requested)
        })
        .await;
        verification::accept(&ours.client, &asked.other_user_id, &asked.flow_id)
            .await
            .unwrap();

        // Only now, and from this side. The far end is following its stream
        // and answering, not starting.
        let ready = wait_for_flow(&flows, "both sides ready", |state| {
            matches!(state, FlowState::Ready)
        })
        .await;
        verification::start_sas(&ours.client, &ready.other_user_id, &ready.flow_id)
            .await
            .unwrap();

        let their_side = tokio::spawn(follow_without_starting(their_request));

        let comparing = wait_for_flow(&flows, "the emoji", |state| {
            matches!(state, FlowState::Comparing { .. })
        })
        .await;
        verification::confirm(&ours.client, &comparing.other_user_id, &comparing.flow_id)
            .await
            .unwrap();
        their_side.await.unwrap();

        wait_for_flow(&flows, "both sides to agree", |state| {
            matches!(state, FlowState::Done)
        })
        .await;

        supervisor.abort();
    }
}
