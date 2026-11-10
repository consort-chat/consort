// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Why a call that looks fine cannot be heard.
//!
//! Phase 0 of `docs/PLAN-voice-call.md` found the failure this module exists
//! for, by producing it: two sessions joined, both memberships published, both
//! rosters correct, RTP flowing in both directions, and neither could decrypt
//! a word the other said. Everything an interface normally draws said the call
//! was working.
//!
//! It is not silent underneath, which is the whole opportunity. The media
//! layer reports the frame cryptor's verdict per participant, and the core
//! reports a media key that arrived and was refused, with the reason. So the
//! honest answer to "why can nobody hear me" is available, and printing
//! nothing would be a choice rather than a limitation.
//!
//! ## One sentence, and nobody's name in it
//!
//! What comes out of here is one sentence for the whole call. Naming the
//! person would mean mapping a membership onto the roster, and the roster is
//! per person while these reports are per membership, so the mapping is not
//! total. In a voice channel with three people in it, "somebody" is enough to
//! act on, and being sure of what is said matters more than being specific
//! about who. Phase 5 is where that gets revisited against a real client.
//!
//! ## The half that is not wired up
//!
//! Everything above is about keys arriving. The other direction, our own key
//! failing to reach the call, is the failure with no other symptom, and
//! [`what_a_rollout_says`] is the whole of Consort's answer to it. Nothing
//! feeds it today: see that function, and
//! docs/PLAN-off-the-pinned-forks.md's stage 4.

use std::collections::BTreeMap;

use matrix_rtc_call::CallStatus;
use matrix_rtc_call_sdk::{CallEvent, FrameEncryptionDiagnostic};
use matrix_rtc_transport::FrameEncryptionState;

/// What a fault about this session itself is filed under.
///
/// Not a membership. [`what_a_rollout_says`] names nobody, because it is about
/// our own key failing to reach the call rather than about anybody in it. It
/// still needs a key so it can be replaced and cleared like the rest, and that
/// key must not collide with a real one. Real ones are `_{user}_{device}`, so
/// a string with spaces in it cannot be mistaken for one.
const THIS_SESSION: &str = "this session";

/// Something wrong with a call's audio.
///
/// Ordered by how much it matters, most first, because a call can have several
/// at once and only one sentence goes on screen. Nobody hearing you outranks
/// you not hearing one person.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fault {
    /// This session's own media key did not reach the call, so what it sends
    /// is encrypted with a key nobody has.
    ///
    /// First because it is the failure with no other symptom. Our own frame
    /// cryptor is perfectly happy here: it has a key and encrypts with it, so
    /// every local diagnostic reads healthy while every peer decodes our audio
    /// as noise. Nothing else in this list can be true without something
    /// looking wrong somewhere.
    NotDistributed {
        /// What the layer that tried to send it said, verbatim.
        reason: String,
        /// Whether it was the join's own distribution that failed, meaning
        /// nobody in this call has ever held our key.
        at_join: bool,
    },
    /// This session's own frames will not encrypt, so nothing it sends is
    /// usable to anybody.
    NothingSent,
    /// A media key arrived from somebody and was refused. A trust or
    /// configuration problem rather than a delivery one, and the only fault
    /// here that carries its own explanation.
    KeyRefused { reason: String },
    /// Somebody's frames arrive and no key for them has ever been installed.
    /// Either none was sent, or it was sent under an identity this session
    /// does not use.
    NoKey,
    /// Keys are installed and the frames still will not decrypt: the two
    /// sessions disagree about the key material, or a rotation is in flight.
    WrongKey,
    /// The transport's frame cryptor failed on its own account.
    CryptorBroken,
}

impl Fault {
    /// What to put in front of somebody.
    ///
    /// Each one says what is happening and what would change it. "Not
    /// cross-signed" appears twice and is meant to: it is by a wide margin the
    /// most likely cause, it is the one phase 0 reproduced, and it is fixed
    /// from the settings screen rather than by waiting.
    fn sentence(&self) -> String {
        match self {
            // Two sentences rather than one, because the two cases send
            // somebody to different places. At the join nothing has ever
            // worked and rejoining is part of the fix; later, the people
            // already here can still hear us and only new arrivals cannot.
            Self::NotDistributed {
                reason,
                at_join: true,
            } => format!(
                "Nobody in this call can hear you: your audio key never reached them ({reason}). \
                 That usually means this session is not cross-signed."
            ),
            Self::NotDistributed {
                reason,
                at_join: false,
            } => format!(
                "Anybody joining from now on will not hear you: your new audio key could not be \
                 sent ({reason}). That usually means this session is not cross-signed."
            ),
            Self::NothingSent => "Your audio could not be encrypted, so nobody in this call \
                 can hear you."
                .to_owned(),
            Self::KeyRefused { reason } => {
                format!("Somebody's audio cannot be read: their media key was refused ({reason}).")
            }
            Self::NoKey => "Somebody's audio cannot be read: their media key never reached \
                 this session. That usually means one of the two sessions is not \
                 cross-signed."
                .to_owned(),
            Self::WrongKey => "Somebody's audio cannot be read: this session has a key for \
                 them and it does not fit."
                .to_owned(),
            Self::CryptorBroken => {
                "Somebody's audio cannot be read: the voice layer failed to decrypt it.".to_owned()
            }
        }
    }
}

/// What is currently wrong with a call, per membership.
///
/// A map rather than a single value, because these arrive and clear per
/// participant: one person's key turning up does not mean the next person's
/// did. Keyed by membership, which is what the reports carry, and never read
/// back out per person.
#[derive(Debug, Default)]
pub struct Faults {
    by_member: BTreeMap<String, Fault>,
}

impl Faults {
    /// Record what is wrong with one membership, or that nothing is.
    ///
    /// Says whether the call's answer changed, so a caller can stay quiet when
    /// a report repeats. They do repeat: the cryptor reports its state per
    /// frame run rather than only on a transition.
    pub fn note(&mut self, member_id: &str, fault: Option<Fault>) -> bool {
        let before = self.worst().cloned();

        match fault {
            Some(fault) => self.by_member.insert(member_id.to_owned(), fault),
            None => self.by_member.remove(member_id),
        };

        self.worst() != before.as_ref()
    }

    /// Forget a membership entirely, because it left.
    ///
    /// Not the same as noting that nothing is wrong with it, only in what it
    /// means, and the same in what it does. Kept separate so a caller reading
    /// the code can see that a participant leaving clears their fault rather
    /// than leaving it on screen for a call they are no longer in.
    pub fn forget(&mut self, member_id: &str) -> bool {
        self.note(member_id, None)
    }

    /// The one sentence to put on screen, if there is one.
    pub fn sentence(&self) -> Option<String> {
        self.worst().map(|fault| fault.sentence())
    }

    /// The fault that matters most out of everything currently wrong.
    fn worst(&self) -> Option<&Fault> {
        self.by_member.values().min()
    }
}

/// Turn one thing the call said into what it says about a membership's audio.
///
/// `None` for an event that says nothing about encryption at all, which is
/// most of them. `Some(member, None)` for one that says a membership is fine
/// now, which is how a fault clears.
///
/// Here rather than beside the transport that produces these, because this is
/// the only decision in the whole path: `livekit.rs` is excluded from coverage
/// on the grounds that CI has no SFU, and a mapping table hidden behind that
/// exclusion is an untested mapping table.
pub fn what_it_says(event: &CallEvent) -> Option<(&str, Option<Fault>)> {
    match event {
        CallEvent::FrameEncryptionState {
            member_id,
            state,
            diagnostic,
        } => Some((member_id, from_cryptor(state, diagnostic))),
        // A key that arrived and was refused, which knows why, unlike a key
        // that never arrived, which is guessing.
        CallEvent::KeyDiscarded {
            member_id, reason, ..
        } => Some((
            member_id,
            Some(Fault::KeyRefused {
                reason: reason.to_string(),
            }),
        )),
        // Their fault goes with them. Leaving it up would explain a call
        // somebody is no longer in.
        CallEvent::ParticipantLeft { member_id, .. } => Some((member_id, None)),
        _ => None,
    }
}

/// Whether the call is over and nothing will follow.
///
/// Reads a status rather than an event because `CallEvent` has no `Ended`
/// variant any more: upstream moved a call's own lifecycle onto
/// `LiveKitCall::subscribe_status`, away from the stream of things that happen
/// to other people in it. That is the better place for it, and it means the
/// end arrives on a watch channel which holds its value rather than on a
/// broadcast a slow reader can miss.
///
/// Here rather than beside the transport, for the reason [`what_it_says`] is:
/// `livekit.rs` is excluded from coverage on the grounds that CI has no SFU,
/// and a decision hidden behind that exclusion is an untested decision.
pub fn is_the_end(status: &CallStatus) -> bool {
    matches!(status, CallStatus::Ended { .. })
}

/// What one media key rollout says about whether this session can be heard.
///
/// The other direction from [`what_it_says`], and the only fault in this module
/// that is about us rather than about somebody else: our own key failing to
/// reach the rest of the call. Filed under a name no membership can have,
/// because it names nobody. An empty `undelivered` is how the fault clears, and
/// it is the only thing that can clear it: no report about a membership says
/// anything about our own outgoing key.
///
/// `at_join` separates the two sentences. At the join nothing has ever worked
/// and rejoining is part of the fix; later, the people already here can still
/// hear us and only new arrivals cannot.
///
/// ## Nothing calls this yet
///
/// Upstream computes exactly this and will not quite let go of it.
/// `EncryptionManager` records which recipients a rollout actually reached,
/// keeps the rest in an `undelivered` map, retries them on backoff, and
/// exposes the list on `RtcSession::undelivered_key_members`. `RtcCall::session`
/// is public too. The one missing link is that `LiveKitCall` holds its
/// `RtcCall` privately and offers no accessor, so there is no route from a
/// joined call to the list; `LiveKitCall::session` is the SFU session, not that
/// one.
///
/// So this is the surface kept warm for a one-line upstream addition, and the
/// whole of docs/PLAN-off-the-pinned-forks.md's stage 4 is now that one line.
/// Before the move off the forks the same capability arrived as two `CallEvent`
/// variants Consort carried three fork commits for. The decision below is what
/// those commits were worth, and it is still tested.
pub fn what_a_rollout_says(undelivered: &[String], at_join: bool) -> (&'static str, Option<Fault>) {
    if undelivered.is_empty() {
        return (THIS_SESSION, None);
    }

    (
        THIS_SESSION,
        Some(Fault::NotDistributed {
            reason: format!("{} did not get it", a_count_of(undelivered.len())),
            at_join,
        }),
    )
}

/// "one member" or "3 members", for a sentence somebody reads.
fn a_count_of(members: usize) -> String {
    if members == 1 {
        "one member".to_owned()
    } else {
        format!("{members} members")
    }
}

/// What the frame cryptor's verdict means.
fn from_cryptor(
    state: &FrameEncryptionState,
    diagnostic: &FrameEncryptionDiagnostic,
) -> Option<Fault> {
    match (state, diagnostic) {
        (FrameEncryptionState::Ok, _) => None,
        // Ours, not theirs. The upstream documentation is explicit: this is
        // our outgoing frames failing to encrypt, so it is everybody who
        // cannot hear rather than one person we cannot.
        (FrameEncryptionState::EncryptionFailed, _) => Some(Fault::NothingSent),
        (FrameEncryptionState::MissingKey, FrameEncryptionDiagnostic::NoKeyInstalled) => {
            Some(Fault::NoKey)
        }
        // Frames carrying an index we have not been given: keys are installed,
        // they are simply not these keys.
        (FrameEncryptionState::MissingKey, _) => Some(Fault::WrongKey),
        (FrameEncryptionState::DecryptionFailed, _) => Some(Fault::WrongKey),
        (FrameEncryptionState::InternalError, _) => Some(Fault::CryptorBroken),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_rtc_call::CallEnd;

    fn refused() -> Fault {
        Fault::KeyRefused {
            reason: "the sending device is not cross-signed".to_owned(),
        }
    }

    fn undistributed(at_join: bool) -> Fault {
        Fault::NotDistributed {
            reason: "1 of 1 member(s) did not get key index 0: Encryption failed because \
                     cross-signing is not set up on your account"
                .to_owned(),
            at_join,
        }
    }

    #[test]
    fn a_call_with_nothing_wrong_says_nothing() {
        // The overwhelmingly common case, and the one where a permanent line
        // of reassurance would be worse than silence.
        let faults = Faults::default();

        assert_eq!(faults.sentence(), None);
    }

    #[test]
    fn every_fault_reads_as_a_sentence_a_person_could_act_on() {
        // These strings reach somebody who has just found that a call is not
        // working. A message that reads as a type name with a colon after it
        // is a message that gets screenshotted into a bug report nobody can
        // act on.
        let faults = [
            undistributed(true),
            undistributed(false),
            Fault::NothingSent,
            refused(),
            Fault::NoKey,
            Fault::WrongKey,
            Fault::CryptorBroken,
        ];

        for fault in faults {
            let said = fault.sentence();
            assert!(said.len() > 40, "{fault:?} said {said:?}");
            assert!(said.ends_with('.'), "{fault:?} said {said:?}");
            assert!(!said.contains("Fault"), "{fault:?} said {said:?}");
            // The sentences are written across several source lines with a
            // trailing backslash, which is one indentation slip away from a
            // sentence with a paragraph's worth of spaces in the middle of it.
            assert!(!said.contains("  "), "{fault:?} said {said:?}");
        }
    }

    #[test]
    fn every_fault_with_a_known_cause_names_it() {
        // The cause phase 0 reproduced, and the one that is fixed from the
        // settings screen rather than by waiting. Leaving it out would send
        // somebody looking at their network.
        //
        // Both halves of `NotDistributed` say it, and that is the correction
        // rather than the original intent: the mid-call one said what had
        // happened and nothing about what to do, which is the shape of message
        // somebody screenshots because there is nothing else to do with it.
        assert!(Fault::NoKey.sentence().contains("cross-signed"));
        assert!(refused().sentence().contains("cross-signed"));
        assert!(undistributed(true).sentence().contains("cross-signed"));
        assert!(undistributed(false).sentence().contains("cross-signed"));
    }

    #[test]
    fn a_noted_fault_becomes_the_call_s_answer() {
        let mut faults = Faults::default();

        assert!(faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey)));

        assert_eq!(faults.sentence(), Some(Fault::NoKey.sentence()));
    }

    #[test]
    fn our_own_key_working_again_takes_its_notice_down() {
        // The one report that clears `THIS_SESSION`, and the reason it had to
        // be added upstream rather than derived here. Nothing about a
        // membership says anything about our own outgoing key, so before this
        // there was no event in the stream that could take the notice off the
        // screen: the fault is a fact about one rollout, and a call somebody
        // fixed mid-way through went on being described as broken.
        let mut faults = Faults::default();
        let (member, fault) = what_a_rollout_says(&["_@bob:example.org_PHONE".to_owned()], false);
        faults.note(member, fault);
        assert!(faults.sentence().is_some());

        let (member, fault) = what_a_rollout_says(&[], false);
        faults.note(member, fault);

        assert_eq!(faults.sentence(), None);
    }

    #[test]
    fn a_recovery_is_filed_where_the_failure_was() {
        // Both under `THIS_SESSION`, because a key filed under one name and
        // cleared under another is a notice that never comes down, which is
        // exactly the bug this pair exists to fix and would be invisible in
        // the test above if either side used a real member id.
        let (failed, _) = what_a_rollout_says(&["_@bob:example.org_PHONE".to_owned()], true);
        let (recovered, _) = what_a_rollout_says(&[], true);

        assert_eq!(failed, THIS_SESSION);
        assert_eq!(recovered, THIS_SESSION);
    }

    #[test]
    fn a_fault_that_clears_takes_the_sentence_with_it() {
        // A key arriving late is the ordinary way this resolves, and a warning
        // that stays up after the thing it warned about is fixed is a warning
        // people learn to ignore.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        assert!(faults.note("_@ada:example.org_LAPTOP", None));

        assert_eq!(faults.sentence(), None);
    }

    #[test]
    fn the_same_report_twice_is_not_a_change() {
        // The cryptor reports its state per frame run rather than only on a
        // transition, so this is the common case rather than an edge one.
        // Without it every call with one bad key redraws the interface a
        // hundred times a second.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        assert!(!faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey)));
        assert!(!faults.note("_@bob:example.org_PHONE", None));
    }

    #[test]
    fn a_key_that_never_left_outranks_everything_else() {
        // The failure with no other symptom, so it is also the one most likely
        // to be true at the same time as a symptom that misleads: our cryptor
        // reads healthy, so a peer's frames failing looks like their problem.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));
        faults.note("_@bob:example.org_PHONE", Some(Fault::NothingSent));

        faults.note(THIS_SESSION, Some(undistributed(true)));

        assert_eq!(faults.sentence(), Some(undistributed(true).sentence()));
    }

    #[test]
    fn the_join_s_own_failure_and_a_later_one_are_different_sentences() {
        // The join's own means nobody in the call has ever held our key; a
        // later one leaves everybody already here with the last one that
        // worked. Saying the first for both would tell people mid-call that
        // nobody can hear them when most of them can.
        let at_join = undistributed(true).sentence();
        let later = undistributed(false).sentence();

        assert_ne!(at_join, later);
        assert!(at_join.starts_with("Nobody in this call"), "{at_join}");
        assert!(later.starts_with("Anybody joining"), "{later}");
    }

    #[test]
    fn a_fault_about_this_session_does_not_displace_one_about_somebody() {
        // Filed under its own key, so it clears on its own. Sharing a
        // membership's slot would mean our key arriving late wiped out
        // whatever was separately wrong with that person.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::WrongKey));
        faults.note(THIS_SESSION, Some(undistributed(true)));

        faults.note(THIS_SESSION, None);

        assert_eq!(faults.sentence(), Some(Fault::WrongKey.sentence()));
    }

    #[test]
    fn nobody_hearing_you_outranks_you_not_hearing_somebody() {
        // Both at once is a real state: a session with no cross-signing
        // identity cannot send its key and cannot be sent one. Only one
        // sentence fits, and it is the one about everybody.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        faults.note("_@me:example.org_THIS", Some(Fault::NothingSent));

        assert_eq!(faults.sentence(), Some(Fault::NothingSent.sentence()));
    }

    #[test]
    fn a_refused_key_outranks_one_that_never_arrived() {
        // A refusal knows why and a missing key is guessing, so the specific
        // one is the better sentence when both are true.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        faults.note("_@bob:example.org_PHONE", Some(refused()));

        assert_eq!(faults.sentence(), Some(refused().sentence()));
    }

    #[test]
    fn one_person_recovering_leaves_the_other_s_fault_up() {
        // Per membership rather than per call, because that is how they
        // arrive and clear: one key turning up says nothing about the next.
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));
        faults.note("_@bob:example.org_PHONE", Some(Fault::WrongKey));

        faults.note("_@ada:example.org_LAPTOP", None);

        assert_eq!(faults.sentence(), Some(Fault::WrongKey.sentence()));
    }

    #[test]
    fn somebody_leaving_takes_their_fault_with_them() {
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        assert!(faults.forget("_@ada:example.org_LAPTOP"));

        assert_eq!(faults.sentence(), None);
    }

    #[test]
    fn forgetting_somebody_who_was_never_in_trouble_changes_nothing() {
        let mut faults = Faults::default();
        faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey));

        assert!(!faults.forget("_@bob:example.org_PHONE"));

        assert_eq!(faults.sentence(), Some(Fault::NoKey.sentence()));
    }

    /// What the cryptor says about one membership.
    fn cryptor(state: FrameEncryptionState, diagnostic: FrameEncryptionDiagnostic) -> CallEvent {
        CallEvent::FrameEncryptionState {
            member_id: "_@ada:example.org_LAPTOP".to_owned(),
            state,
            diagnostic,
        }
    }

    #[test]
    fn frames_that_decrypt_are_a_membership_with_nothing_wrong() {
        // Not "nothing to say". This is how a fault clears, so it has to name
        // the membership it clears for.
        let event = cryptor(
            FrameEncryptionState::Ok,
            FrameEncryptionDiagnostic::NotApplicable,
        );

        let said = what_it_says(&event);

        assert_eq!(said, Some(("_@ada:example.org_LAPTOP", None)));
    }

    #[test]
    fn a_key_that_never_arrived_is_told_apart_from_one_that_does_not_fit() {
        // The two behind a `MissingKey`, and they send somebody to different
        // places: no key at all is an identity or signalling problem, while
        // frames carrying an index we have not been given is a rotation still
        // in flight.
        let no_key = cryptor(
            FrameEncryptionState::MissingKey,
            FrameEncryptionDiagnostic::NoKeyInstalled,
        );
        let wrong_index = cryptor(
            FrameEncryptionState::MissingKey,
            FrameEncryptionDiagnostic::KeysInstalled {
                key_indices: vec![0],
            },
        );

        assert_eq!(what_it_says(&no_key).unwrap().1, Some(Fault::NoKey));
        assert_eq!(what_it_says(&wrong_index).unwrap().1, Some(Fault::WrongKey));
    }

    #[test]
    fn our_own_frames_failing_to_encrypt_is_about_everybody() {
        // The one state that is not about the membership it names. Upstream is
        // explicit that this is our outgoing frames, so it means nobody can
        // hear us rather than that we cannot hear one person.
        let event = cryptor(
            FrameEncryptionState::EncryptionFailed,
            FrameEncryptionDiagnostic::NotApplicable,
        );

        let said = what_it_says(&event);

        assert_eq!(said.unwrap().1, Some(Fault::NothingSent));
    }

    #[test]
    fn a_cryptor_failing_on_its_own_account_is_not_blamed_on_a_key() {
        let event = cryptor(
            FrameEncryptionState::InternalError,
            FrameEncryptionDiagnostic::NotApplicable,
        );

        let said = what_it_says(&event);

        assert_eq!(said.unwrap().1, Some(Fault::CryptorBroken));
    }

    #[test]
    fn keys_that_do_not_decrypt_are_the_two_sides_disagreeing() {
        let event = cryptor(
            FrameEncryptionState::DecryptionFailed,
            FrameEncryptionDiagnostic::KeysInstalled {
                key_indices: vec![0, 1],
            },
        );

        let said = what_it_says(&event);

        assert_eq!(said.unwrap().1, Some(Fault::WrongKey));
    }

    #[test]
    fn a_refused_key_carries_the_reason_it_was_refused() {
        // The reason is the whole value of this event over a missing key: it
        // knows, where the absence is guessing.
        let event = CallEvent::KeyDiscarded {
            member_id: "_@ada:example.org_LAPTOP".to_owned(),
            key_index: Some(0),
            sender_user_id: Some("@ada:example.org".to_owned()),
            sender_device_id: Some("LAPTOP".to_owned()),
            reason: matrix_rtc_core::KeyRejection::SenderNotConfirmed,
        };

        let said = what_it_says(&event);

        let Some((_, Some(Fault::KeyRefused { reason }))) = said else {
            panic!("{said:?}");
        };
        assert!(reason.to_lowercase().contains("confirm"), "{reason}");
    }

    #[test]
    fn our_own_key_failing_to_send_is_reported_against_no_membership() {
        // It names nobody, because it is about us. It still needs a key it can
        // be cleared under, and that key must not be one a real membership
        // could take: every member id in this generation is `{user}:{device}`.
        let (member_id, fault) = what_a_rollout_says(&["_@bob:example.org_PHONE".to_owned()], true);

        let Some(Fault::NotDistributed { at_join, .. }) = fault else {
            panic!("{fault:?}");
        };
        assert!(!member_id.starts_with('_'), "{member_id}");
        assert!(member_id.contains(' '), "{member_id}");
        assert!(at_join);
    }

    #[test]
    fn a_later_distribution_failure_keeps_saying_it_was_later() {
        // The flag is the whole reason the caller has to pass it, and dropping
        // it would collapse the two sentences into one.
        let (_, fault) = what_a_rollout_says(&["_@bob:example.org_PHONE".to_owned()], false);

        assert_eq!(
            fault,
            Some(Fault::NotDistributed {
                reason: "one member did not get it".to_owned(),
                at_join: false,
            })
        );
    }

    #[test]
    fn somebody_leaving_takes_whatever_was_wrong_with_them() {
        let event = CallEvent::ParticipantLeft {
            member_id: "_@ada:example.org_LAPTOP".to_owned(),
            participant_count: 1,
        };

        let said = what_it_says(&event);

        assert_eq!(said, Some(("_@ada:example.org_LAPTOP", None)));
    }

    #[test]
    fn everything_else_the_call_says_is_not_about_encryption() {
        // Most of this stream is phase 4's, and treating an unrelated event as
        // a verdict would clear or invent a fault on every speaker change.
        let event = CallEvent::ParticipantJoined {
            member_id: "_@ada:example.org_LAPTOP".to_owned(),
            user_id: "@ada:example.org".to_owned(),
            participant_count: 2,
        };

        let said = what_it_says(&event);

        assert_eq!(said, None);
    }
    #[test]
    fn the_end_of_a_call_is_recognised() {
        // Nothing follows an `Ended`, so a reader that keeps waiting draws a
        // dead call as connected for as long as the application runs.
        assert!(is_the_end(&CallStatus::Ended {
            reason: CallEnd::Left,
        }));
    }

    #[test]
    fn a_call_in_progress_is_not_the_end() {
        // A status channel holds its value and is read on every wake, so
        // treating any of the live states as an ending would close the call
        // panel on a call that is fine.
        assert!(!is_the_end(&CallStatus::Connecting));
        assert!(!is_the_end(&CallStatus::Connected {
            since_ms: 1,
            degraded: false,
        }));
    }

    #[test]
    fn a_rollout_that_reached_everybody_says_nothing_is_wrong() {
        // The clear, and the shape the notice comes down in. An empty list is
        // a healthy rollout rather than an absent one: upstream reports the
        // recipients it missed, so "none" is the good answer.
        assert_eq!(what_a_rollout_says(&[], false), (THIS_SESSION, None));
        assert_eq!(what_a_rollout_says(&[], true), (THIS_SESSION, None));
    }

    #[test]
    fn a_rollout_at_the_join_and_one_after_it_read_differently() {
        // Two sentences rather than one, because they send somebody to
        // different places, and the flag is the only thing that picks between
        // them.
        let missed = ["_@bob:example.org_PHONE".to_owned()];

        let (_, at_join) = what_a_rollout_says(&missed, true);
        let (_, later) = what_a_rollout_says(&missed, false);

        assert!(
            at_join
                .expect("a missed recipient is a fault")
                .sentence()
                .contains("Nobody in this call can hear you")
        );
        assert!(
            later
                .expect("a missed recipient is a fault")
                .sentence()
                .contains("joining from now on")
        );
    }

    #[test]
    fn how_many_went_unreached_is_in_the_sentence() {
        // The count is the shape of the problem: one person who cannot hear us
        // and a whole call that cannot are the same fault and not the same
        // news.
        let one = what_a_rollout_says(&["_@bob:example.org_PHONE".to_owned()], false)
            .1
            .expect("a fault")
            .sentence();
        let three = what_a_rollout_says(
            &[
                "_@bob:example.org_PHONE".to_owned(),
                "_@cyd:example.org_DESK".to_owned(),
                "_@eve:example.org_TAB".to_owned(),
            ],
            false,
        )
        .1
        .expect("a fault")
        .sentence();

        assert!(one.contains("one member"), "{one}");
        assert!(three.contains("3 members"), "{three}");
    }
}
