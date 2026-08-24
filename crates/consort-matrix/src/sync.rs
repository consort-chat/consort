// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The sync loop.
//!
//! Everything push-driven in Matrix arrives through `/sync`: to-device
//! messages, room keys, device-list changes, and the verification requests the
//! next milestone is built on. Until this module existed Consort never called
//! it, so a signed-in Consort was a client that could be talked to and could
//! not hear.
//!
//! The loop reports its own health rather than running silently. A sync loop
//! that has quietly died looks exactly like a homeserver with nothing to say,
//! and the difference matters: one of them means the next message arrives in a
//! moment and the other means it never arrives at all.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::sync::SyncResponse;
use matrix_sdk::{Client, LoopCtrl};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

/// How long the server may hold a sync request open with nothing to say.
///
/// Thirty seconds is the conventional value and what Element uses. Shorter
/// means more requests for no benefit; much longer starts running into proxies
/// and NAT tables that drop an idle connection and turn a working sync into a
/// timeout.
const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest gap between reconnection attempts.
///
/// A minute, so a homeserver that comes back after an outage is noticed within
/// a minute rather than at the end of an exponential curve that has run away.
const MAX_BACKOFF_SECONDS: u64 = 60;

/// Why a sync loop is not running any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// The user signed out, or another account signed in. Ours to report, and
    /// never inferred from anything the homeserver said.
    SignedOut,
    /// The homeserver no longer accepts this session's access token. The
    /// session is over and no amount of retrying brings it back.
    SessionEnded,
    /// The loop ended for some other reason. Should not happen; reported
    /// rather than swallowed so that it is visible when it does.
    Failed,
}

/// What the sync loop is currently doing.
///
/// This crosses the IPC boundary, so the wire format is part of the contract
/// with `app/src/lib/api.ts` and the tests below pin it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Connection {
    /// Started, and no sync has succeeded yet.
    Connecting,
    /// A sync has completed. Events are flowing.
    Live,
    /// A sync failed and another attempt is scheduled.
    Offline { attempt: u32, retry_in_seconds: u64 },
    /// Not running, and it will not restart on its own.
    Stopped { reason: StopReason },
}

/// What to do about a failed sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reaction {
    Retry,
    Stop(StopReason),
}

/// How long to wait before the next attempt.
///
/// Doubling from one second up to [`MAX_BACKOFF_SECONDS`]. The SDK already
/// refuses to issue two syncs within a second of each other, so this is not
/// what stops a hot loop; it is what stops a homeserver that is down from
/// receiving one request per second from every Consort on the network.
fn backoff_seconds(attempt: u32) -> u64 {
    1u64.checked_shl(attempt.saturating_sub(1))
        .unwrap_or(MAX_BACKOFF_SECONDS)
        .min(MAX_BACKOFF_SECONDS)
}

/// Decide what a failed sync means.
///
/// Only one thing is fatal. Everything else, a 502 from a reverse proxy, a
/// homeserver mid-restart, a laptop whose wifi dropped, is a reason to wait
/// and ask again. Treating any of those as a logout is how a client signs
/// people out for being on a train.
fn reaction_for_kind(kind: Option<&ErrorKind>) -> Reaction {
    match kind {
        Some(ErrorKind::UnknownToken { .. }) => Reaction::Stop(StopReason::SessionEnded),
        _ => Reaction::Retry,
    }
}

/// Passes states to the sink, skipping the ones that change nothing.
///
/// A sync that keeps working reports success every thirty seconds forever.
/// Forwarding each one wakes the webview and re-renders for no new
/// information, so only transitions go out.
struct Reporter<F> {
    sink: F,
    last: Mutex<Option<Connection>>,
}

impl<F: Fn(Connection)> Reporter<F> {
    fn new(sink: F) -> Self {
        Self {
            sink,
            last: Mutex::new(None),
        }
    }

    fn report(&self, state: Connection) {
        let mut last = self
            .last
            .lock()
            .expect("the reporter mutex is never poisoned");
        if last.as_ref() == Some(&state) {
            return;
        }
        *last = Some(state.clone());
        // Held across the call on purpose. The sink is a synchronous send into
        // the webview, and releasing first would let two threads reorder two
        // states the frontend then applies backwards.
        (self.sink)(state);
    }

    /// Whether the last state reported was a stop.
    fn has_stopped(&self) -> bool {
        matches!(
            *self
                .last
                .lock()
                .expect("the reporter mutex is never poisoned"),
            Some(Connection::Stopped { .. })
        )
    }
}

/// Start syncing, reporting the loop's health as it changes.
///
/// # Lifetime
///
/// The returned task holds the `Client` and never ends on its own, exactly
/// like [`crate::auth::persist_token_refreshes`]. The caller owns the handle
/// and aborts it when the session ends. An abort skips the final report, which
/// is correct: the only thing that aborts this is a teardown that already
/// knows what it is doing.
pub fn start<F>(client: Client, on_change: F) -> JoinHandle<()>
where
    F: Fn(Connection) + Send + Sync + 'static,
{
    tokio::spawn(run(client, Arc::new(Reporter::new(on_change))))
}

/// The loop itself.
async fn run<F>(client: Client, reporter: Arc<Reporter<F>>)
where
    F: Fn(Connection) + Send + Sync + 'static,
{
    reporter.report(Connection::Connecting);

    // An `Fn` closure cannot own mutable state, and the future it returns has
    // to be `'static`, so the attempt counter is shared rather than captured.
    let failures = Arc::new(AtomicU32::new(0));

    let outcome = {
        let reporter = reporter.clone();
        let failures = failures.clone();
        client
            .sync_with_result_callback(
                SyncSettings::default().timeout(SYNC_TIMEOUT),
                move |result| {
                    let reporter = reporter.clone();
                    let failures = failures.clone();
                    async move { after_sync(result, &reporter, &failures).await }
                },
            )
            .await
    };

    // `sync_with_result_callback` only returns `Err` if our own callback did,
    // which it never does. Logged rather than ignored so that a future change
    // to that is not silent.
    if let Err(error) = outcome {
        tracing::error!(%error, "the sync loop returned an error");
    }

    if !reporter.has_stopped() {
        tracing::warn!("the sync loop ended without being asked to");
        reporter.report(Connection::Stopped {
            reason: StopReason::Failed,
        });
    }
}

/// Handle one sync result and decide whether to keep going.
///
/// `sync_with_result_callback` rather than `sync` is what makes this reachable
/// at all: `sync` swallows the failed iterations and retries behind our back,
/// so a client that has been offline for an hour looks identical to one that
/// is connected and idle.
async fn after_sync<F>(
    result: Result<SyncResponse, matrix_sdk::Error>,
    reporter: &Reporter<F>,
    failures: &AtomicU32,
) -> Result<LoopCtrl, matrix_sdk::Error>
where
    F: Fn(Connection),
{
    let error = match result {
        Ok(_) => {
            failures.store(0, Ordering::Relaxed);
            reporter.report(Connection::Live);
            return Ok(LoopCtrl::Continue);
        }
        Err(error) => error,
    };

    match reaction_for_kind(error.client_api_error_kind()) {
        Reaction::Stop(reason) => {
            tracing::warn!(%error, "the homeserver rejected our access token; stopping the sync loop");
            reporter.report(Connection::Stopped { reason });
            Ok(LoopCtrl::Break)
        }
        Reaction::Retry => {
            let attempt = failures.fetch_add(1, Ordering::Relaxed) + 1;
            let retry_in_seconds = backoff_seconds(attempt);
            tracing::warn!(%error, attempt, retry_in_seconds, "sync failed; retrying");
            reporter.report(Connection::Offline {
                attempt,
                retry_in_seconds,
            });
            tokio::time::sleep(Duration::from_secs(retry_in_seconds)).await;
            Ok(LoopCtrl::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> (Arc<Mutex<Vec<Connection>>>, impl Fn(Connection)) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = seen.clone();
            move |state: Connection| seen.lock().unwrap().push(state)
        };
        (seen, sink)
    }

    mod backoff {
        use super::*;

        #[test]
        fn the_first_retry_comes_quickly() {
            assert!((1..=2).contains(&backoff_seconds(1)));
        }

        #[test]
        fn waiting_grows_with_consecutive_failures() {
            let waits: Vec<u64> = (1..=10).map(backoff_seconds).collect();
            for pair in waits.windows(2) {
                assert!(pair[1] >= pair[0], "{waits:?} is not monotonic");
            }
            assert!(waits[9] > waits[0], "{waits:?} never grew");
        }

        #[test]
        fn waiting_is_capped_so_a_long_outage_still_reconnects_promptly() {
            // A server down for an hour should not leave us waiting an hour
            // after it comes back.
            assert_eq!(backoff_seconds(1000), backoff_seconds(20));
            assert!(backoff_seconds(1000) <= 60);
        }

        #[test]
        fn a_zeroth_attempt_still_waits() {
            // Nothing should call it with 0, but a wait of 0 would turn the
            // loop into a hot spin against a dead server.
            assert!(backoff_seconds(0) >= 1);
        }
    }

    mod reactions {
        use super::*;
        use matrix_sdk::ruma::api::error::UnknownTokenErrorData;

        #[test]
        fn an_expired_token_stops_the_loop() {
            let kind = ErrorKind::UnknownToken(UnknownTokenErrorData::new());
            assert_eq!(
                reaction_for_kind(Some(&kind)),
                Reaction::Stop(StopReason::SessionEnded)
            );
        }

        #[test]
        fn a_soft_logout_stops_the_loop_too() {
            let mut data = UnknownTokenErrorData::new();
            data.soft_logout = true;
            let kind = ErrorKind::UnknownToken(data);
            assert_eq!(
                reaction_for_kind(Some(&kind)),
                Reaction::Stop(StopReason::SessionEnded)
            );
        }

        #[test]
        fn a_server_side_failure_is_retried_rather_than_ending_the_session() {
            // The regression this guards: treating any sync failure as a
            // logout signs people out because their homeserver restarted.
            use matrix_sdk::ruma::api::error::LimitExceededErrorData;

            for kind in [
                ErrorKind::NotJson,
                ErrorKind::LimitExceeded(LimitExceededErrorData::new()),
                ErrorKind::Unknown,
            ] {
                assert_eq!(
                    reaction_for_kind(Some(&kind)),
                    Reaction::Retry,
                    "{kind:?} should be retried"
                );
            }
        }

        #[test]
        fn a_transport_failure_with_no_matrix_body_is_retried() {
            // A reset, a TLS failure, or a proxy's HTML error page. Being
            // offline is the single most likely cause and is not fatal.
            assert_eq!(reaction_for_kind(None), Reaction::Retry);
        }

        #[test]
        fn nothing_the_server_says_can_produce_a_signed_out_stop() {
            // `SignedOut` is ours to report when the user asks. A homeserver
            // must not be able to claim it, because the UI treats it as an
            // action the user took.
            use matrix_sdk::ruma::api::error::LimitExceededErrorData;

            let kinds = [
                Some(ErrorKind::UnknownToken(UnknownTokenErrorData::new())),
                Some(ErrorKind::Forbidden),
                Some(ErrorKind::LimitExceeded(LimitExceededErrorData::new())),
                Some(ErrorKind::NotJson),
                None,
            ];

            for kind in kinds {
                assert_ne!(
                    reaction_for_kind(kind.as_ref()),
                    Reaction::Stop(StopReason::SignedOut),
                    "{kind:?}"
                );
            }
        }
    }

    mod reporting {
        use super::*;

        #[test]
        fn the_first_state_is_always_reported() {
            let (seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Connecting);

            assert_eq!(*seen.lock().unwrap(), vec![Connection::Connecting]);
        }

        #[test]
        fn repeating_a_state_reports_nothing() {
            // A sync that keeps succeeding fires every thirty seconds. Each
            // one saying "still live" is a wake-up for the webview and a
            // re-render for nothing.
            let (seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Live);
            reporter.report(Connection::Live);
            reporter.report(Connection::Live);

            assert_eq!(seen.lock().unwrap().len(), 1);
        }

        #[test]
        fn a_change_is_reported() {
            let (seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Connecting);
            reporter.report(Connection::Live);

            assert_eq!(
                *seen.lock().unwrap(),
                vec![Connection::Connecting, Connection::Live]
            );
        }

        #[test]
        fn going_live_again_after_an_outage_is_reported() {
            let (seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Live);
            reporter.report(Connection::Offline {
                attempt: 1,
                retry_in_seconds: 1,
            });
            reporter.report(Connection::Live);

            assert_eq!(seen.lock().unwrap().len(), 3);
        }

        #[test]
        fn each_retry_is_its_own_state() {
            // The attempt count is part of what the UI shows, so two retries
            // are not the same state even though both say "offline".
            let (seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Offline {
                attempt: 1,
                retry_in_seconds: 1,
            });
            reporter.report(Connection::Offline {
                attempt: 2,
                retry_in_seconds: 2,
            });

            assert_eq!(seen.lock().unwrap().len(), 2);
        }

        #[test]
        fn a_fresh_reporter_has_not_stopped() {
            let (_seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            assert!(!reporter.has_stopped());
        }

        #[test]
        fn a_reporter_knows_it_has_stopped() {
            let (_seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Stopped {
                reason: StopReason::SessionEnded,
            });

            assert!(reporter.has_stopped());
        }

        #[test]
        fn a_running_reporter_has_not_stopped() {
            let (_seen, sink) = recorder();
            let reporter = Reporter::new(sink);

            reporter.report(Connection::Live);

            assert!(!reporter.has_stopped());
        }
    }

    mod wire_format {
        use super::*;

        #[test]
        fn a_connection_serialises_as_a_tagged_union() {
            // api.ts switches on `state`. An externally tagged enum would
            // arrive as `{"offline": {...}}` and match none of its branches.
            for (state, expected) in [
                (Connection::Connecting, "connecting"),
                (Connection::Live, "live"),
                (
                    Connection::Offline {
                        attempt: 1,
                        retry_in_seconds: 2,
                    },
                    "offline",
                ),
                (
                    Connection::Stopped {
                        reason: StopReason::Failed,
                    },
                    "stopped",
                ),
            ] {
                let json = serde_json::to_value(&state).unwrap();
                assert_eq!(json.get("state").unwrap(), expected, "{state:?}");
            }
        }

        #[test]
        fn offline_carries_the_field_names_the_frontend_expects() {
            let json = serde_json::to_value(Connection::Offline {
                attempt: 3,
                retry_in_seconds: 8,
            })
            .unwrap();

            assert_eq!(json.get("attempt").unwrap(), 3);
            assert_eq!(json.get("retryInSeconds").unwrap(), 8);
        }

        #[test]
        fn a_stop_reason_is_camel_case_like_every_other_discriminant() {
            for (reason, expected) in [
                (StopReason::SignedOut, "signedOut"),
                (StopReason::SessionEnded, "sessionEnded"),
                (StopReason::Failed, "failed"),
            ] {
                let json = serde_json::to_value(Connection::Stopped { reason }).unwrap();
                assert_eq!(json.get("reason").unwrap(), expected);
            }
        }

        #[test]
        fn a_connection_survives_a_json_round_trip() {
            let states = [
                Connection::Connecting,
                Connection::Live,
                Connection::Offline {
                    attempt: 2,
                    retry_in_seconds: 4,
                },
                Connection::Stopped {
                    reason: StopReason::SignedOut,
                },
            ];

            for state in states {
                let json = serde_json::to_string(&state).unwrap();
                let back: Connection = serde_json::from_str(&json).unwrap();
                assert_eq!(back, state);
            }
        }
    }
}
