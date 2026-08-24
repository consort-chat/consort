// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Driving one verification flow from arrival to outcome.
//!
//! A flow is not held anywhere. Nothing in this crate or in the application
//! keeps a `SasVerification` in a field: the SDK already has a registry keyed
//! by user and flow id, and a second one next to it would be a lifetime to
//! manage and a reason two concurrent requests could not both exist. Every
//! action resolves through the SDK's registry instead, and the only thing kept
//! is one task per flow, driving the stream that reports its progress.
//!
//! Those tasks are owned rather than detached. A flow task holds a
//! `SasVerification`, which holds the `Client`, and it watches a stream
//! belonging to that same client, so nothing about a signed-out session can
//! make one end. [`supervise`] owns them all in a set, and dropping that set
//! is what stops the lot.

use std::future::Future;

use futures_util::StreamExt;
use matrix_sdk::Client;
use matrix_sdk::encryption::verification::{
    SasVerification, VerificationRequest, VerificationRequestState,
};
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::{JoinHandle, JoinSet};

use super::Changes;
use super::dto::{Flow, FlowState};
use crate::{Error, Result};

/// A verification request the sync loop has just noticed.
///
/// Only the two identifiers, not the SDK's request object. The object is
/// re-resolved inside the flow task, so nothing here has to be `Send` across
/// an event handler boundary or kept alive by a channel that might back up.
#[derive(Clone, Debug)]
struct Arrival {
    user_id: OwnedUserId,
    flow_id: String,
}

/// The flow tasks belonging to one session.
///
/// A `JoinSet` rather than loose `tokio::spawn`s because dropping it aborts
/// everything in it. That is the whole point: a detached flow task outlives
/// the sign-out that should have ended it.
struct Flows(JoinSet<()>);

impl Flows {
    fn new() -> Self {
        Self(JoinSet::new())
    }

    fn start(&mut self, task: impl Future<Output = ()> + Send + 'static) {
        self.0.spawn(task);
    }

    /// Wait for one flow to finish and forget it.
    ///
    /// Waits forever when there is nothing outstanding, so it can sit in a
    /// `select!` without a guard. Returning immediately instead would spin the
    /// supervisor for the whole life of a session that is not verifying
    /// anything, which is nearly all of them.
    async fn reap(&mut self) {
        if self.0.is_empty() {
            std::future::pending::<()>().await;
        }
        let _ = self.0.join_next().await;
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// Watch for verification requests, reporting each flow as it progresses.
///
/// Registers the event handlers on `client` and returns the task that owns
/// every flow they start. Aborting it ends the flows with it.
///
/// # Lifetime
///
/// The handlers live on the `Client` and hold the only sender, so the task
/// does not end while the client exists. Same ownership story as
/// [`crate::sync::start`]: the caller aborts it when the session ends.
pub fn supervise<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(Flow) + Send + Sync + 'static,
{
    let (arrivals, inbox) = tokio::sync::mpsc::unbounded_channel();
    watch_for_requests(&client, arrivals);

    let on_change = std::sync::Arc::new(on_change);
    tokio::spawn(run(inbox, move |arrival| {
        drive(client.clone(), arrival, on_change.clone())
    }))
}

/// Register the two handlers a request can arrive through.
///
/// Both of them, and not just the to-device one. Element sends the in-room
/// flow for self-verification in some versions, and a client that handles only
/// to-device requests shows the user nothing at all when it does: no error, no
/// screen, just a request on the other device that is never answered.
fn watch_for_requests(client: &Client, arrivals: UnboundedSender<Arrival>) {
    client.add_event_handler({
        let arrivals = arrivals.clone();
        move |event: ToDeviceKeyVerificationRequestEvent| {
            let arrivals = arrivals.clone();
            async move {
                announce(
                    &arrivals,
                    Arrival {
                        user_id: event.sender,
                        flow_id: event.content.transaction_id.to_string(),
                    },
                );
            }
        }
    });

    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent| {
        let arrivals = arrivals.clone();
        async move {
            if !matches!(event.content.msgtype, MessageType::VerificationRequest(_)) {
                return;
            }
            announce(
                &arrivals,
                Arrival {
                    // The in-room flow is identified by the event that started
                    // it rather than by a transaction id of its own.
                    flow_id: event.event_id.to_string(),
                    user_id: event.sender,
                },
            );
        }
    });
}

fn announce(arrivals: &UnboundedSender<Arrival>, arrival: Arrival) {
    let flow_id = arrival.flow_id.clone();
    if arrivals.send(arrival).is_err() {
        // The supervisor is gone, which means the session is. Logged rather
        // than ignored because a request the user can see on their other
        // device and not on this one is a confusing thing to debug.
        tracing::warn!(
            flow_id,
            "a verification request arrived after the session ended"
        );
    }
}

/// Accept arrivals, start a flow for each, and clean up after them.
///
/// Generic over what a flow *is* so the ownership can be tested without a
/// homeserver: the leak this closes is about task lifetimes, and none of it is
/// about olm.
async fn run<S, Fut>(mut arrivals: UnboundedReceiver<Arrival>, start: S)
where
    S: Fn(Arrival) -> Fut + Send,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut flows = Flows::new();

    loop {
        tokio::select! {
            arrival = arrivals.recv() => match arrival {
                Some(arrival) => {
                    tracing::info!(flow_id = arrival.flow_id, "a verification request arrived");
                    flows.start(start(arrival));
                }
                // Only when every sender is gone, which means the client is.
                None => break,
            },
            () = flows.reap() => {}
        }
    }
}

/// Follow one flow from the request through to its outcome.
async fn drive<F>(client: Client, arrival: Arrival, on_change: std::sync::Arc<F>)
where
    F: Fn(Flow) + Send + Sync + 'static,
{
    let Some(request) = client
        .encryption()
        .get_verification_request(&arrival.user_id, &arrival.flow_id)
        .await
    else {
        // The SDK garbage-collects a request that was already cancelled or
        // already answered by another of our devices, and the event announcing
        // it can arrive in the same sync batch. Nothing to show.
        tracing::info!(
            flow_id = arrival.flow_id,
            "a verification request was gone before it could be read"
        );
        return;
    };

    let mut report = Report::about(&request, on_change);
    report.state(state_of(&request.state()));

    let mut changes = request.changes();
    while let Some(state) = changes.next().await {
        if let VerificationRequestState::Transitioned { verification } = state {
            match verification.sas() {
                Some(sas) => return follow_sas(sas, report).await,
                None => {
                    // Only reachable with the `qrcode` feature on, which this
                    // workspace does not enable. Logged rather than ignored so
                    // that turning it on does not silently strand a flow.
                    tracing::warn!("a verification transitioned into a method we do not support");
                    return;
                }
            }
        }

        let mapped = state_of(&state);
        let is_final = mapped.is_final();
        report.state(mapped);
        if is_final {
            return;
        }
    }
}

/// Follow the short auth string exchange the request turned into.
async fn follow_sas<F>(sas: SasVerification, mut report: Report<F>)
where
    F: Fn(Flow) + Send + Sync + 'static,
{
    // Not a decision anybody is asked about, which is why it is here rather
    // than behind a command. `m.key.verification.accept` settles which hash
    // and which MAC the two devices will use; the human decision was made when
    // they accepted the request, and putting a second button in front of a
    // choice between `hkdf-hmac-sha256` and `hkdf-hmac-sha256.v2` would be
    // asking somebody a question they cannot have an opinion about.
    //
    // Only as the responder. When we started the exchange there is nothing to
    // accept, and Phase 3 arrives here with `we_started()` true.
    if !sas.we_started()
        && let Err(error) = sas.accept().await
    {
        // Deliberately not reported as a cancellation. Nothing has been
        // cancelled: the other side is still waiting and will time the flow
        // out in its own time, and that cancellation is the one worth showing
        // because it is the one that actually happened.
        tracing::error!(%error, "could not accept a verification");
    }

    report.state(FlowState::from(&sas.state()));

    let mut changes = sas.changes();
    while let Some(state) = changes.next().await {
        let mapped = FlowState::from(&state);
        let is_final = mapped.is_final();
        report.state(mapped);
        if is_final {
            return;
        }
    }
}

/// Where a request has got to, before any concrete method has started.
///
/// `Transitioned` is handled by the caller rather than here, because it is the
/// one that changes which stream is being watched.
fn state_of(state: &VerificationRequestState) -> FlowState {
    match state {
        // We are the responder in this phase, so `Created` is not reached from
        // here. Mapped rather than left to a catch-all so that the initiator
        // path does not have to come back and find out why its first state was
        // wrong.
        VerificationRequestState::Created { .. } => FlowState::Waiting,
        VerificationRequestState::Requested { .. } => FlowState::Requested,
        VerificationRequestState::Ready { .. } => FlowState::Ready,
        VerificationRequestState::Transitioned { .. } => FlowState::Waiting,
        VerificationRequestState::Done => FlowState::Done,
        VerificationRequestState::Cancelled(info) => {
            FlowState::cancelled(info.cancel_code(), info.cancelled_by_us(), info.reason())
        }
    }
}

/// Sends flow states out, skipping the ones that change nothing.
///
/// The identity half of a `Flow` is fixed for the life of the flow, so it is
/// read once here rather than at every state.
struct Report<F> {
    flow_id: String,
    other_user_id: String,
    is_self_verification: bool,
    changes: Changes<FlowState>,
    on_change: std::sync::Arc<F>,
}

impl<F: Fn(Flow)> Report<F> {
    /// Takes the three strings rather than the request they came from.
    ///
    /// `VerificationRequest` has no public constructor and needs a live olm
    /// machine, so a `Report` built from one could only be exercised against a
    /// homeserver. The dedup and the identity handling are ordinary logic and
    /// deserve ordinary tests; reading the fields off a request is one line in
    /// [`Report::about`].
    fn new(
        flow_id: String,
        other_user_id: String,
        is_self_verification: bool,
        on_change: std::sync::Arc<F>,
    ) -> Self {
        Self {
            flow_id,
            other_user_id,
            is_self_verification,
            changes: Changes::new(),
            on_change,
        }
    }

    fn about(request: &VerificationRequest, on_change: std::sync::Arc<F>) -> Self {
        Self::new(
            request.flow_id().to_owned(),
            request.other_user_id().to_string(),
            request.is_self_verification(),
            on_change,
        )
    }

    fn state(&mut self, state: FlowState) {
        let Some(state) = self.changes.accept(state) else {
            return;
        };
        tracing::info!(
            flow_id = self.flow_id,
            ?state,
            "a verification flow moved on"
        );
        (self.on_change)(Flow {
            flow_id: self.flow_id.clone(),
            other_user_id: self.other_user_id.clone(),
            is_self_verification: self.is_self_verification,
            state,
        });
    }
}

/// Look up a request the frontend named, or say plainly that it is gone.
async fn request_for(client: &Client, user_id: &str, flow_id: &str) -> Result<VerificationRequest> {
    let user_id = OwnedUserId::try_from(user_id).map_err(|_| Error::NoSuchFlow {
        flow_id: flow_id.to_owned(),
    })?;

    client
        .encryption()
        .get_verification_request(&user_id, flow_id)
        .await
        .ok_or_else(|| Error::NoSuchFlow {
            flow_id: flow_id.to_owned(),
        })
}

/// Look up the short auth string exchange the frontend named.
async fn sas_for(client: &Client, user_id: &str, flow_id: &str) -> Result<SasVerification> {
    let user_id = OwnedUserId::try_from(user_id).map_err(|_| Error::NoSuchFlow {
        flow_id: flow_id.to_owned(),
    })?;

    client
        .encryption()
        .get_verification(&user_id, flow_id)
        .await
        .and_then(|verification| verification.sas())
        .ok_or_else(|| Error::NoSuchFlow {
            flow_id: flow_id.to_owned(),
        })
}

/// Agree to verify. The other side is then free to start the comparison.
pub async fn accept(client: &Client, user_id: &str, flow_id: &str) -> Result<()> {
    Ok(request_for(client, user_id, flow_id)
        .await?
        .accept()
        .await?)
}

/// Start the emoji comparison ourselves.
///
/// Usually unnecessary as the responder: whoever asked normally starts it as
/// soon as we are ready. It exists for the case where they do not, which
/// otherwise leaves both sides waiting for the other.
pub async fn start_sas(client: &Client, user_id: &str, flow_id: &str) -> Result<()> {
    request_for(client, user_id, flow_id)
        .await?
        .start_sas()
        .await?;
    Ok(())
}

/// Say the short auth strings matched.
pub async fn confirm(client: &Client, user_id: &str, flow_id: &str) -> Result<()> {
    Ok(sas_for(client, user_id, flow_id).await?.confirm().await?)
}

/// Say the short auth strings did not match.
///
/// A different call from [`cancel`], and the difference is not cosmetic: this
/// sends `m.key.verification.cancel` with `m.mismatched_sas`, which tells the
/// other side that somebody may be intercepting rather than that somebody
/// changed their mind.
pub async fn mismatch(client: &Client, user_id: &str, flow_id: &str) -> Result<()> {
    Ok(sas_for(client, user_id, flow_id).await?.mismatch().await?)
}

/// Call the whole thing off.
///
/// Cancelling the request cancels whatever it turned into, so this is the one
/// action that works at every stage of a flow.
pub async fn cancel(client: &Client, user_id: &str, flow_id: &str) -> Result<()> {
    Ok(request_for(client, user_id, flow_id)
        .await?
        .cancel()
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn arrival(flow_id: &str) -> Arrival {
        Arrival {
            user_id: matrix_sdk::ruma::user_id!("@bob:example.org").to_owned(),
            flow_id: flow_id.to_owned(),
        }
    }

    /// Poll until `done`, or give up loudly.
    async fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(tokio::time::Instant::now() < deadline, "waited for {what}");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn an_arriving_request_starts_a_flow() {
        let started = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = tokio::spawn(run(rx, {
            let started = started.clone();
            move |arrival: Arrival| {
                let started = started.clone();
                async move {
                    started.lock().unwrap().push(arrival.flow_id);
                }
            }
        }));

        tx.send(arrival("the-only-flow")).unwrap();

        wait_until("the flow to start", || !started.lock().unwrap().is_empty()).await;
        assert_eq!(*started.lock().unwrap(), vec!["the-only-flow".to_owned()]);
        supervisor.abort();
    }

    #[tokio::test]
    async fn two_requests_at_once_both_get_a_flow() {
        // A request goes to every device on the account, and nothing stops two
        // arriving. The shape that made this unrepresentable, one
        // `Mutex<Option<SasVerification>>` in application state, is the shape
        // this design exists to avoid.
        let started = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = tokio::spawn(run(rx, {
            let started = started.clone();
            move |_| {
                let started = started.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    // Never finishes, so the second cannot be waiting on the
                    // first having got out of the way.
                    std::future::pending::<()>().await;
                }
            }
        }));

        tx.send(arrival("first")).unwrap();
        tx.send(arrival("second")).unwrap();

        wait_until("both flows to start", || {
            started.load(Ordering::SeqCst) == 2
        })
        .await;
        supervisor.abort();
    }

    #[tokio::test]
    async fn stopping_the_supervisor_stops_the_flows_it_started() {
        // The leak this closes. A flow task holds a `SasVerification`, which
        // holds the `Client`, and it watches a stream belonging to that same
        // client, so it cannot end on its own. Signing out with a request
        // still open would otherwise leave the previous account's task running
        // for the life of the process, still holding its SQLite handles.
        let running = Arc::new(AtomicUsize::new(0));

        struct Counted(Arc<AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = tokio::spawn(run(rx, {
            let running = running.clone();
            move |_| {
                let running = running.clone();
                async move {
                    running.fetch_add(1, Ordering::SeqCst);
                    let _counted = Counted(running.clone());
                    std::future::pending::<()>().await;
                }
            }
        }));

        tx.send(arrival("the-only-flow")).unwrap();
        wait_until("the flow to start", || running.load(Ordering::SeqCst) == 1).await;

        supervisor.abort();

        wait_until("the flow to stop", || running.load(Ordering::SeqCst) == 0).await;
    }

    mod flows {
        use super::*;

        #[tokio::test]
        async fn a_started_flow_is_outstanding_until_it_is_reaped() {
            let mut flows = Flows::new();
            flows.start(async {});

            assert_eq!(flows.len(), 1);
            flows.reap().await;
            assert_eq!(flows.len(), 0);
        }

        #[tokio::test]
        async fn reaping_an_empty_set_waits_rather_than_returning() {
            // It sits in a `select!` beside the arrivals channel. Returning
            // immediately when there is nothing to reap would turn that select
            // into a hot loop for the whole life of a session with no
            // verification going on, which is nearly all of them.
            let mut flows = Flows::new();

            let outcome = tokio::time::timeout(Duration::from_millis(50), flows.reap()).await;

            assert!(outcome.is_err(), "reaping an empty set returned");
        }

        #[tokio::test]
        async fn dropping_the_set_stops_everything_in_it() {
            let running = Arc::new(AtomicUsize::new(1));
            let mut flows = Flows::new();
            flows.start({
                let running = running.clone();
                async move {
                    std::future::pending::<()>().await;
                    running.store(2, Ordering::SeqCst);
                }
            });

            drop(flows);

            // The task never got to change it, and never will.
            tokio::task::yield_now().await;
            assert_eq!(running.load(Ordering::SeqCst), 1);
        }
    }

    mod reporting {
        use super::*;
        use matrix_sdk::ruma::events::key::verification::cancel::CancelCode;

        /// Everything the report was handed, and the report itself.
        type Recorded<F> = (Arc<Mutex<Vec<Flow>>>, Report<F>);

        fn reporter() -> Recorded<impl Fn(Flow)> {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let sink = {
                let seen = seen.clone();
                move |flow: Flow| seen.lock().unwrap().push(flow)
            };
            (
                seen.clone(),
                Report::new(
                    "the-only-flow".to_owned(),
                    "@bob:example.org".to_owned(),
                    true,
                    std::sync::Arc::new(sink),
                ),
            )
        }

        #[test]
        fn every_state_carries_the_identity_the_actions_need() {
            // The frontend addresses a flow by exactly this pair. A state that
            // arrived without them would draw a button that cannot name what
            // it acts on.
            let (seen, mut report) = reporter();

            report.state(FlowState::Requested);

            let flow = seen.lock().unwrap()[0].clone();
            assert_eq!(flow.flow_id, "the-only-flow");
            assert_eq!(flow.other_user_id, "@bob:example.org");
            assert!(flow.is_self_verification);
        }

        #[test]
        fn a_repeated_state_is_not_reported_twice() {
            // Both streams re-publish on their own schedule, and every
            // duplicate is a webview wake-up and a re-render carrying nothing.
            let (seen, mut report) = reporter();

            report.state(FlowState::Waiting);
            report.state(FlowState::Waiting);

            assert_eq!(seen.lock().unwrap().len(), 1);
        }

        #[test]
        fn each_real_change_is_reported() {
            let (seen, mut report) = reporter();

            report.state(FlowState::Requested);
            report.state(FlowState::Ready);
            report.state(FlowState::Waiting);

            let kinds: Vec<FlowState> = seen
                .lock()
                .unwrap()
                .iter()
                .map(|flow| flow.state.clone())
                .collect();
            assert_eq!(
                kinds,
                vec![FlowState::Requested, FlowState::Ready, FlowState::Waiting]
            );
        }

        #[test]
        fn two_comparisons_showing_different_emoji_are_two_states() {
            // Nothing produces this today, but folding two different short
            // auth strings into one state would leave the second unshown, and
            // "the emoji on screen are stale" is the worst possible bug here.
            let one = FlowState::Comparing {
                emoji: Vec::new(),
                decimals: [1, 2, 3],
            };
            let two = FlowState::Comparing {
                emoji: Vec::new(),
                decimals: [4, 5, 6],
            };
            let (seen, mut report) = reporter();

            report.state(one);
            report.state(two);

            assert_eq!(seen.lock().unwrap().len(), 2);
        }

        #[test]
        fn an_ending_is_reported_even_when_it_follows_a_similar_one() {
            let (seen, mut report) = reporter();

            report.state(FlowState::cancelled(&CancelCode::User, false, "x"));
            report.state(FlowState::cancelled(&CancelCode::Timeout, false, "x"));

            assert_eq!(seen.lock().unwrap().len(), 2);
        }
    }

    /// Where a request is, before any concrete method has started.
    ///
    /// Two of the six arms are reachable without a live olm machine, because
    /// the rest carry `DeviceData`, a `Verification` or a `CancelInfo` and none
    /// of those has a public constructor. The four that are not reachable here
    /// are covered by the live suite in `against_a_real_homeserver.rs`.
    mod request_states {
        use super::*;
        use matrix_sdk::ruma::events::key::verification::VerificationMethod;

        #[test]
        fn a_request_we_made_is_not_yet_asking_the_user_anything() {
            // `Created` is our own side of the initiator path. Mapping it to
            // `Requested` would put "somebody wants to verify this session" in
            // front of the person who just pressed the button themselves.
            let state = state_of(&VerificationRequestState::Created {
                our_methods: vec![VerificationMethod::SasV1],
            });

            assert_eq!(state, FlowState::Waiting);
        }

        #[test]
        fn a_finished_request_is_final() {
            let state = state_of(&VerificationRequestState::Done);

            assert_eq!(state, FlowState::Done);
            assert!(state.is_final());
        }
    }

    #[tokio::test]
    async fn the_supervisor_ends_when_nothing_can_send_to_it_any_more() {
        // The sender lives on the `Client`, so in the application this happens
        // when the client is dropped. Ending rather than spinning is what
        // makes an abort unnecessary in that case.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Arrival>();
        let supervisor = tokio::spawn(run(rx, |_| async {}));

        drop(tx);

        tokio::time::timeout(Duration::from_secs(5), supervisor)
            .await
            .expect("the supervisor kept running with nobody able to reach it")
            .unwrap();
    }
}
