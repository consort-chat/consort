// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for one verification flow.
//!
//! None of the SDK's own state enums cross the IPC boundary. `SasState` and
//! `VerificationRequestState` carry `DeviceData`, protocol lists and accepted
//! algorithm sets that the interface has no use for, and pinning the wire
//! format to an upstream enum means an SDK bump can silently change what the
//! webview receives. Everything here is owned, small, and ours.
//!
//! The mappings below are the most valuable tests in this module: they are
//! pure functions over enums, so every branch is reachable without a
//! homeserver, and getting one wrong shows the user the wrong thing about
//! their own encryption.

use matrix_sdk::encryption::verification::{Emoji, EmojiShortAuthString, SasState};
use matrix_sdk::ruma::events::key::verification::cancel::CancelCode;
use serde::{Deserialize, Serialize};

/// One of the seven pictures both sides compare.
///
/// The description is the English word from the spec's table, which is what
/// makes the comparison work when two people are reading over a phone line
/// rather than looking at the same screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmojiPair {
    pub symbol: String,
    pub description: String,
}

impl From<&Emoji> for EmojiPair {
    fn from(emoji: &Emoji) -> Self {
        Self {
            symbol: emoji.symbol.to_owned(),
            description: emoji.description.to_owned(),
        }
    }
}

/// Why a flow ended without verifying anything.
///
/// A deliberate narrowing of ruma's `CancelCode`, which has eleven variants
/// plus an open-ended custom one and is written for the protocol rather than
/// for a person. What the interface needs to decide is only what to say and
/// how alarmed to look, and these four answers cover that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelReason {
    /// Somebody pressed the equivalent of "no". Ordinary, and not a fault.
    Declined,
    /// The two sides were not looking at the same short auth string. This is
    /// the one that matters: it is what the whole exchange exists to detect.
    Mismatch,
    /// Nobody answered inside the protocol's ten minutes.
    TimedOut,
    /// Another of the account's own sessions took the request first.
    ///
    /// Worth its own variant rather than folding into `Declined`. A request
    /// goes to every device on the account, so every device that did not
    /// answer is told `m.accepted`, and calling that a refusal reports a
    /// problem to somebody whose verification is going fine on their phone.
    AcceptedElsewhere,
    /// Something else went wrong. Named in the detail rather than here.
    Other,
}

impl From<&CancelCode> for CancelReason {
    fn from(code: &CancelCode) -> Self {
        match code {
            CancelCode::User => Self::Declined,
            CancelCode::MismatchedSas
            | CancelCode::KeyMismatch
            | CancelCode::MismatchedCommitment => Self::Mismatch,
            CancelCode::Timeout => Self::TimedOut,
            CancelCode::Accepted => Self::AcceptedElsewhere,
            // `CancelCode` is `#[non_exhaustive]` and has an open custom
            // variant, so this arm is required and is genuinely reachable: any
            // client may cancel with a code of its own invention.
            _ => Self::Other,
        }
    }
}

/// Where a verification flow has got to.
///
/// Seven states, and the two that look redundant are not. `Ready` means both
/// sides have agreed to verify but nobody has started the comparison, which is
/// where a "show me the emoji" button belongs. `Waiting` means the comparison
/// has started and the keys are still in flight, which is a spinner. Merging
/// them would put a button on a screen where pressing it does nothing.
///
/// This crosses the IPC boundary, so the wire format is part of the contract
/// with `app/src/lib/api.ts` and the tests below pin it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FlowState {
    /// The other side asked, and we have not answered.
    Requested,
    /// Both sides agreed to verify. Nobody has started the comparison.
    Ready,
    /// The comparison has started and there is nothing to show yet.
    Waiting,
    /// Compare these against the other device.
    Comparing {
        /// Empty when the other side cannot do emoji, in which case the
        /// decimals are the whole of the short auth string.
        emoji: Vec<EmojiPair>,
        decimals: [u16; 3],
    },
    /// We said they matched. Waiting for the other side to say the same.
    Confirmed,
    /// Both sides agreed. The device is verified.
    Done,
    /// Over, and not successfully.
    Cancelled {
        reason: CancelReason,
        /// The SDK's own sentence, for the console. Developer English, and
        /// never rendered: the interface phrases `reason` itself.
        detail: String,
        by_us: bool,
    },
}

impl FlowState {
    /// The comparison screen, from whatever the SDK managed to produce.
    ///
    /// `emojis` is `None` when the agreed protocols did not include the emoji
    /// method. That is not a failure and not an empty screen: the decimals are
    /// always there, and comparing three numbers verifies exactly as well.
    pub(crate) fn comparing(
        emojis: Option<&EmojiShortAuthString>,
        decimals: (u16, u16, u16),
    ) -> Self {
        Self::Comparing {
            emoji: emojis
                .map(|sas| sas.emojis.iter().map(EmojiPair::from).collect())
                .unwrap_or_default(),
            decimals: [decimals.0, decimals.1, decimals.2],
        }
    }

    /// The end of a flow that did not verify anything.
    ///
    /// Takes the pieces rather than a `CancelInfo` because `CancelInfo` has
    /// private fields and no public constructor, so a mapping written against
    /// it could not be tested without a live handshake. The one-line caller
    /// that unpacks a real `CancelInfo` is in `flow.rs`.
    pub(crate) fn cancelled(code: &CancelCode, by_us: bool, detail: &str) -> Self {
        Self::Cancelled {
            reason: CancelReason::from(code),
            detail: detail.to_owned(),
            by_us,
        }
    }

    /// Whether nothing further will happen in this flow.
    ///
    /// Public because the Tauri layer needs it too: a flow that is over is
    /// history rather than state, and should not be replayed to a webview
    /// that subscribed after it ended.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Done | Self::Cancelled { .. })
    }
}

/// Where the short auth string exchange has got to.
impl From<&SasState> for FlowState {
    fn from(state: &SasState) -> Self {
        match state {
            // Three protocol steps that all look the same from outside: the
            // devices are agreeing on algorithms and there is nothing a person
            // can do about any of it.
            SasState::Created { .. } | SasState::Started { .. } | SasState::Accepted { .. } => {
                Self::Waiting
            }
            SasState::KeysExchanged { emojis, decimals } => {
                Self::comparing(emojis.as_ref(), *decimals)
            }
            SasState::Confirmed => Self::Confirmed,
            SasState::Done { .. } => Self::Done,
            SasState::Cancelled(info) => {
                Self::cancelled(info.cancel_code(), info.cancelled_by_us(), info.reason())
            }
        }
    }
}

/// One verification flow, as the webview sees it.
///
/// Carries its own identity because flows are addressed rather than held: no
/// `Mutex<Option<SasVerification>>` in application state, so every command
/// names the flow it means and the SDK's own registry resolves it. That is
/// what makes two concurrent requests representable, which they are: a
/// request goes to every device on the account and two of them can answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub flow_id: String,
    pub other_user_id: String,
    /// Whether this is another of our own sessions rather than someone else.
    ///
    /// Phase 2 only ever produces `true` in practice, but the interface has to
    /// choose between "another of your sessions" and naming a person, and
    /// guessing from a string comparison at the far end would be a second
    /// place for that logic to be wrong.
    pub is_self_verification: bool,
    /// Whether this session asked for the verification rather than being asked.
    ///
    /// Not derivable from the state once the request has turned into a key
    /// exchange: from `Comparing` onwards the two sides look identical, and
    /// they still need different sentences and different buttons.
    pub we_started: bool,
    pub state: FlowState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::encryption::verification::Emoji;
    use matrix_sdk::ruma::events::key::verification::cancel::CancelCode;

    /// A short auth string in the shape `SasState::KeysExchanged` carries one.
    ///
    /// The indices are what a client with its own translated emoji table would
    /// use instead of the symbols; nothing here reads them, but leaving them
    /// inconsistent with the symbols would make this a misleading fixture.
    fn seven() -> EmojiShortAuthString {
        let emojis = [
            Emoji {
                symbol: "🐶",
                description: "Dog",
            },
            Emoji {
                symbol: "🐱",
                description: "Cat",
            },
            Emoji {
                symbol: "🦁",
                description: "Lion",
            },
            Emoji {
                symbol: "🐎",
                description: "Horse",
            },
            Emoji {
                symbol: "🦄",
                description: "Unicorn",
            },
            Emoji {
                symbol: "🐷",
                description: "Pig",
            },
            Emoji {
                symbol: "🐘",
                description: "Elephant",
            },
        ];
        EmojiShortAuthString {
            indices: [0, 1, 2, 3, 4, 5, 6],
            emojis,
        }
    }

    mod comparing {
        use super::*;

        #[test]
        fn all_seven_emoji_are_carried_with_their_descriptions() {
            let state = FlowState::comparing(Some(&seven()), (1, 2, 3));

            let FlowState::Comparing { emoji, .. } = state else {
                panic!("expected a comparison, got {state:?}");
            };
            assert_eq!(emoji.len(), 7);
            assert_eq!(emoji[0].symbol, "🐶");
            assert_eq!(emoji[0].description, "Dog");
            assert_eq!(emoji[6].description, "Elephant");
        }

        #[test]
        fn a_device_that_cannot_do_emoji_still_gets_its_decimals() {
            // `supports_emoji()` is genuinely false for some clients, and the
            // decimals are the fallback the spec provides. Showing nothing
            // would strand the flow.
            let state = FlowState::comparing(None, (4242, 1337, 9001));

            let FlowState::Comparing { emoji, decimals } = state else {
                panic!("expected a comparison, got {state:?}");
            };
            assert!(emoji.is_empty());
            assert_eq!(decimals, [4242, 1337, 9001]);
        }
    }

    mod cancellation {
        use super::*;

        #[test]
        fn a_mismatch_is_named_as_one() {
            for code in [
                CancelCode::MismatchedSas,
                CancelCode::KeyMismatch,
                CancelCode::MismatchedCommitment,
            ] {
                let state = FlowState::cancelled(&code, false, "whatever the sdk said");
                assert_eq!(reason_of(&state), CancelReason::Mismatch, "{code:?}");
            }
        }

        #[test]
        fn a_user_cancelling_is_a_decline_and_not_a_failure() {
            let state = FlowState::cancelled(&CancelCode::User, false, "x");
            assert_eq!(reason_of(&state), CancelReason::Declined);
        }

        #[test]
        fn running_out_of_time_is_its_own_reason() {
            let state = FlowState::cancelled(&CancelCode::Timeout, false, "x");
            assert_eq!(reason_of(&state), CancelReason::TimedOut);
        }

        #[test]
        fn another_of_our_own_sessions_answering_first_is_not_a_refusal() {
            // The common case in self-verification, and the one most easily got
            // wrong. Element sends the request to every device on the account,
            // so each device that did not answer receives `m.accepted`.
            // Rendering that as "cancelled" tells the user something went wrong
            // when in fact the verification is proceeding elsewhere.
            let state = FlowState::cancelled(&CancelCode::Accepted, false, "x");
            assert_eq!(reason_of(&state), CancelReason::AcceptedElsewhere);
        }

        #[test]
        fn a_code_we_do_not_recognise_still_produces_a_cancellation() {
            for code in [
                CancelCode::UnknownMethod,
                CancelCode::UnknownTransaction,
                CancelCode::UnexpectedMessage,
                CancelCode::InvalidMessage,
                CancelCode::UserMismatch,
                CancelCode::from("org.example.something.new"),
            ] {
                let state = FlowState::cancelled(&code, false, "x");
                assert_eq!(reason_of(&state), CancelReason::Other, "{code:?}");
            }
        }

        #[test]
        fn who_cancelled_is_carried_through() {
            let ours = FlowState::cancelled(&CancelCode::User, true, "x");
            let theirs = FlowState::cancelled(&CancelCode::User, false, "x");

            assert!(matches!(ours, FlowState::Cancelled { by_us: true, .. }));
            assert!(matches!(theirs, FlowState::Cancelled { by_us: false, .. }));
        }

        #[test]
        fn the_sdks_own_sentence_is_kept_as_detail() {
            // For the console, not the interface. The UI phrases the reason
            // itself, because "The SAS did not match." is developer English.
            let state =
                FlowState::cancelled(&CancelCode::MismatchedSas, false, "The SAS did not match.");

            let FlowState::Cancelled { detail, .. } = state else {
                panic!("expected a cancellation");
            };
            assert_eq!(detail, "The SAS did not match.");
        }
    }

    mod from_sas_state {
        use super::*;

        #[test]
        fn exchanged_keys_become_the_comparison_screen() {
            let state = FlowState::from(&SasState::KeysExchanged {
                emojis: Some(seven()),
                decimals: (1, 2, 3),
            });

            let FlowState::Comparing { emoji, decimals } = state else {
                panic!("expected a comparison, got {state:?}");
            };
            assert_eq!(emoji.len(), 7);
            assert_eq!(decimals, [1, 2, 3]);
        }

        #[test]
        fn exchanged_keys_without_emoji_still_become_the_comparison_screen() {
            // The regression this guards is an `emoji()` that returns `None`
            // being read as "nothing to show yet", which strands the flow on a
            // spinner that never resolves.
            let state = FlowState::from(&SasState::KeysExchanged {
                emojis: None,
                decimals: (7, 8, 9),
            });

            assert!(matches!(state, FlowState::Comparing { .. }), "{state:?}");
        }

        #[test]
        fn our_own_confirmation_is_not_the_end_of_the_flow() {
            // `Confirmed` means we said yes and the other side has not. An
            // interface that treated it as done would tell somebody their
            // device was verified while it was still waiting.
            let state = FlowState::from(&SasState::Confirmed);

            assert_eq!(state, FlowState::Confirmed);
            assert!(!state.is_final());
        }
    }

    mod finality {
        use super::*;

        #[test]
        fn nothing_follows_a_completed_or_cancelled_flow() {
            assert!(FlowState::Done.is_final());
            assert!(FlowState::cancelled(&CancelCode::User, false, "x").is_final());
        }

        #[test]
        fn everything_else_is_still_in_progress() {
            // The flow task ends on a final state. Calling one of these final
            // would drop the flow half way through and leave the screen frozen
            // on whatever it last showed.
            for state in [
                FlowState::Requested,
                FlowState::Ready,
                FlowState::Waiting,
                FlowState::comparing(Some(&seven()), (1, 2, 3)),
                FlowState::Confirmed,
            ] {
                assert!(!state.is_final(), "{state:?}");
            }
        }
    }

    mod wire_format {
        use super::*;

        fn kind(state: &FlowState) -> String {
            serde_json::to_value(state).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        }

        #[test]
        fn every_state_is_tagged_the_way_the_frontend_reads_it() {
            assert_eq!(kind(&FlowState::Requested), "requested");
            assert_eq!(kind(&FlowState::Ready), "ready");
            assert_eq!(kind(&FlowState::Waiting), "waiting");
            assert_eq!(
                kind(&FlowState::comparing(Some(&seven()), (1, 2, 3))),
                "comparing"
            );
            assert_eq!(kind(&FlowState::Confirmed), "confirmed");
            assert_eq!(kind(&FlowState::Done), "done");
            assert_eq!(
                kind(&FlowState::cancelled(&CancelCode::User, true, "x")),
                "cancelled"
            );
        }

        #[test]
        fn a_comparison_carries_the_field_names_the_frontend_expects() {
            let json =
                serde_json::to_value(FlowState::comparing(Some(&seven()), (1, 2, 3))).unwrap();

            assert_eq!(json["emoji"][0]["symbol"], "🐶");
            assert_eq!(json["emoji"][0]["description"], "Dog");
            assert_eq!(json["decimals"], serde_json::json!([1, 2, 3]));
        }

        #[test]
        fn a_cancellation_carries_the_field_names_the_frontend_expects() {
            let json =
                serde_json::to_value(FlowState::cancelled(&CancelCode::Timeout, true, "gone"))
                    .unwrap();

            assert_eq!(json["reason"], "timedOut");
            assert_eq!(json["byUs"], true);
            assert_eq!(json["detail"], "gone");
        }

        #[test]
        fn a_flow_carries_the_field_names_the_frontend_expects() {
            let flow = Flow {
                flow_id: "the-only-flow".to_owned(),
                other_user_id: "@bob:example.org".to_owned(),
                is_self_verification: true,
                we_started: false,
                state: FlowState::Requested,
            };

            let json = serde_json::to_value(&flow).unwrap();

            assert_eq!(json["flowId"], "the-only-flow");
            assert_eq!(json["otherUserId"], "@bob:example.org");
            assert_eq!(json["isSelfVerification"], true);
            assert_eq!(json["weStarted"], false);
            assert_eq!(json["state"]["kind"], "requested");
        }

        /// The two directions have to be distinguishable on the wire.
        ///
        /// The interface says different things about a flow it asked for and
        /// one it was asked about, and after the request turns into a key
        /// exchange the states are identical, so the direction is the only
        /// thing left to tell them apart.
        #[test]
        fn a_flow_we_started_says_so() {
            let flow = Flow {
                flow_id: "the-only-flow".to_owned(),
                other_user_id: "@alice:example.org".to_owned(),
                is_self_verification: true,
                we_started: true,
                state: FlowState::Waiting,
            };

            let json = serde_json::to_value(&flow).unwrap();

            assert_eq!(json["weStarted"], true);
        }

        #[test]
        fn every_state_survives_a_round_trip() {
            let states = [
                FlowState::Requested,
                FlowState::Ready,
                FlowState::Waiting,
                FlowState::comparing(Some(&seven()), (1, 2, 3)),
                FlowState::comparing(None, (1, 2, 3)),
                FlowState::Confirmed,
                FlowState::Done,
                FlowState::cancelled(&CancelCode::User, true, "x"),
            ];

            for state in states {
                let json = serde_json::to_string(&state).unwrap();
                let back: FlowState = serde_json::from_str(&json).unwrap();
                assert_eq!(back, state, "{json} did not come back the same");
            }
        }
    }

    /// The reason out of a state that is known to be a cancellation.
    fn reason_of(state: &FlowState) -> CancelReason {
        match state {
            FlowState::Cancelled { reason, .. } => *reason,
            other => panic!("expected a cancellation, got {other:?}"),
        }
    }
}
