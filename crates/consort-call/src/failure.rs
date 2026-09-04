// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Why a call did not happen.

use std::time::Duration;

use matrix_rtc_livekit::CallError;

use crate::dialect::Dialect;

/// What went wrong, in categories somebody can act on.
///
/// Five, not one, because they send a person to five different places. A room
/// this account is not in is a sync that has not landed. A homeserver that
/// will not answer is the same outage that is already breaking everything
/// else. No voice server is a deployment that has none, or one this account
/// is not allowed to use. Signalling is the call itself refusing, and is the
/// only one where the network is fine and the call still will not happen. An
/// unreadable dialect is none of those: nothing is broken anywhere, and the
/// call is refused before it is attempted.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CallFailure {
    /// This account is not in that room, or sync has not delivered it yet.
    #[error("this account is not in {room_id}, or sync has not caught up")]
    UnknownRoom { room_id: String },

    /// The homeserver refused, or could not be reached.
    #[error("the homeserver could not be reached: {0}")]
    Homeserver(String),

    /// No voice server could be found, or it would not authorise this
    /// session.
    #[error("no voice server would take this call: {0}")]
    NoTransport(String),

    /// Announcing this session in the call failed.
    #[error("this session could not be announced in the call: {0}")]
    Signalling(String),

    /// A call in this dialect would connect and be heard by nobody.
    ///
    /// The one failure here that is neither the network nor the deployment. It
    /// is a limit of this build, it will not clear by retrying, and the only
    /// thing that changes it is the setting the message names. See
    /// [`Dialect::readable`] for why, and for when this goes away.
    #[error(
        "this build cannot hold a call in the {} dialect: set \"fallbackDialect\" to \"{}\" in settings.json",
        .0.name(),
        Dialect::default().name()
    )]
    UnreadableDialect(Dialect),

    /// The join did not finish in time.
    ///
    /// Ours rather than the transport's. Every step of a join can block on
    /// something remote, and a thread waiting forever on one of them is a
    /// voice channel that stays on "Connecting" with no way back.
    #[error("the call did not connect within {}s", .0.as_secs())]
    TimedOut(Duration),
}

/// Sort a join failure into something worth putting in front of a person.
///
/// Four categories out of four variants, and one of them is a merge:
/// `Transport` is the authorisation service and the SFU socket, `Media` is the
/// media session on top of that socket, and from where somebody is sitting
/// both are the voice server not working. Splitting them would offer a choice
/// between two sentences that ask for the same thing.
pub fn classify(error: &CallError) -> CallFailure {
    let said = error.to_string();

    match error {
        CallError::Sdk(_) => CallFailure::Homeserver(said),
        CallError::Transport(_) | CallError::Media(_) => CallFailure::NoTransport(said),
        // Nothing here sends one: Consort draws no in-call reactions and has
        // no raised hand. The arm exists because the match is exhaustive, and
        // that is worth keeping, since being exhaustive is what said this
        // variant had appeared at all. Signalling is where it belongs if it
        // ever does arrive: a reaction is an event send, like the membership
        // beside it.
        CallError::Signalling(_) | CallError::Reaction(_) => CallFailure::Signalling(said),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each failure says what it is without the caller adding anything.
    ///
    /// These strings reach a person. A message that reads as a type name
    /// with a colon after it is a message that gets screenshotted into a bug
    /// report nobody can act on.
    #[test]
    fn every_failure_reads_as_a_sentence() {
        let failures = [
            CallFailure::UnknownRoom {
                room_id: "!general:example.org".to_owned(),
            },
            CallFailure::Homeserver("connection refused".to_owned()),
            CallFailure::NoTransport("no focus advertised".to_owned()),
            CallFailure::Signalling("membership rejected".to_owned()),
            CallFailure::UnreadableDialect(Dialect::Current),
            CallFailure::TimedOut(Duration::from_secs(30)),
        ];

        for failure in failures {
            let said = failure.to_string();
            assert!(
                said.len() > 20 && said.contains(' '),
                "{failure:?} said {said:?}"
            );
        }
    }

    #[test]
    fn a_timeout_says_how_long_it_waited_in_seconds() {
        assert_eq!(
            CallFailure::TimedOut(Duration::from_secs(30)).to_string(),
            "the call did not connect within 30s"
        );
    }

    #[test]
    fn an_unreadable_dialect_names_both_the_one_it_got_and_the_one_to_write() {
        // This message is the entire remedy. A person reading it has to be
        // able to go to one file and type one word without asking anybody,
        // so it has to carry the setting's name and a value that parses.
        let said = CallFailure::UnreadableDialect(Dialect::Current).to_string();

        assert!(said.contains("current"), "{said}");
        assert!(said.contains("fallbackDialect"), "{said}");
        assert!(said.contains(Dialect::default().name()), "{said}");
    }

    #[test]
    fn the_dialect_an_unreadable_join_is_told_to_use_is_one_that_works() {
        // The advice is generated from the default rather than written out, so
        // that moving the default moves the advice. This is what makes that
        // safe: a default nobody can hold a call in would otherwise become a
        // message telling people to configure exactly that.
        assert!(Dialect::default().readable());
    }

    #[test]
    fn an_unknown_room_names_the_room() {
        // The one failure whose cause is local. Naming the room is what makes
        // "the click landed before sync did" tellable apart from "that
        // channel is gone".
        assert!(
            CallFailure::UnknownRoom {
                room_id: "!general:example.org".to_owned(),
            }
            .to_string()
            .contains("!general:example.org")
        );
    }
}

#[cfg(test)]
mod classifying {
    use super::*;
    use matrix_rtc_core::reactions::ReactionError;
    use matrix_rtc_media::TransportError;

    fn sdk_error() -> CallError {
        CallError::Sdk(matrix_sdk::Error::UnknownError(Box::new(
            std::io::Error::other("the homeserver hung up"),
        )))
    }

    #[test]
    fn a_matrix_error_is_the_homeserver() {
        assert!(matches!(classify(&sdk_error()), CallFailure::Homeserver(_)));
    }

    #[test]
    fn the_authorisation_handshake_failing_is_no_transport() {
        // Which leg of the handshake failed does not reach `classify`, which
        // matches the outer `Transport` arm: getting the OpenID token, the
        // token service refusing, and the socket never opening are one
        // sentence to whoever is trying to talk.
        let error = CallError::Transport(matrix_rtc_livekit::Error::OpenIdToken(
            matrix_rtc_livekit::OpenIdTokenError("the homeserver said no".to_owned()),
        ));

        assert!(matches!(classify(&error), CallFailure::NoTransport(_)));
    }

    #[test]
    fn the_media_session_failing_is_also_no_transport() {
        // The merge, asserted rather than assumed. Both legs are the voice
        // server from where somebody is sitting, and a build that starts
        // telling them apart should have to change this test to do it.
        let error = CallError::Media(TransportError::Connect("no route".to_owned()));

        assert!(matches!(classify(&error), CallFailure::NoTransport(_)));
    }

    #[test]
    fn membership_being_refused_is_signalling() {
        let error = CallError::Signalling("membership rejected".to_owned());

        assert!(matches!(classify(&error), CallFailure::Signalling(_)));
    }

    /// Unreachable today, and classified rather than ignored.
    ///
    /// Consort has no way to send one, so this arrives from nowhere. It is
    /// here because a variant with no arm is a compile error and a variant
    /// swept into a catch-all is the next upstream addition going unnoticed.
    #[test]
    fn a_reaction_failing_is_signalling() {
        let error = CallError::Reaction(ReactionError::NotJoined);

        assert!(matches!(classify(&error), CallFailure::Signalling(_)));
    }

    #[test]
    fn every_classification_keeps_what_the_error_said() {
        // The category is for choosing a sentence. The original text is what
        // makes a bug report actionable, and dropping it leaves four generic
        // messages and no way back to the cause.
        let error = CallError::Signalling("slot m.call#ROOM is closed".to_owned());

        assert!(
            classify(&error)
                .to_string()
                .contains("slot m.call#ROOM is closed")
        );
    }
}
