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

use std::collections::BTreeMap;

use matrix_rtc_media::{CallEvent, FrameEncryptionDiagnostic, FrameEncryptionState};

/// Something wrong with a call's audio.
///
/// Ordered by how much it matters, most first, because a call can have several
/// at once and only one sentence goes on screen. Nobody hearing you outranks
/// you not hearing one person.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fault {
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
        CallEvent::ParticipantLeft { member_id } => Some((member_id, None)),
        _ => None,
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

    fn refused() -> Fault {
        Fault::KeyRefused {
            reason: "the sending device is not cross-signed".to_owned(),
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
    fn the_two_most_likely_faults_name_cross_signing() {
        // The cause phase 0 reproduced, and the one that is fixed from the
        // settings screen rather than by waiting. Leaving it out would send
        // somebody looking at their network.
        assert!(Fault::NoKey.sentence().contains("cross-signed"));
        assert!(refused().sentence().contains("cross-signed"));
    }

    #[test]
    fn a_noted_fault_becomes_the_call_s_answer() {
        let mut faults = Faults::default();

        assert!(faults.note("_@ada:example.org_LAPTOP", Some(Fault::NoKey)));

        assert_eq!(faults.sentence(), Some(Fault::NoKey.sentence()));
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
            reason: matrix_rtc_core::KeyRejection::NotCrossSigned,
        };

        let said = what_it_says(&event);

        let Some((_, Some(Fault::KeyRefused { reason }))) = said else {
            panic!("{said:?}");
        };
        assert!(reason.to_lowercase().contains("cross-signed"), "{reason}");
    }

    #[test]
    fn somebody_leaving_takes_whatever_was_wrong_with_them() {
        let event = CallEvent::ParticipantLeft {
            member_id: "_@ada:example.org_LAPTOP".to_owned(),
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
        };

        let said = what_it_says(&event);

        assert_eq!(said, None);
    }
}
