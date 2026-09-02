// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Error type for the Matrix layer.
//!
//! Every variant carries a `user_message()` string, because a login form has to
//! say something to a human and `matrix_sdk::Error`'s `Display` is written for a
//! log line. The two audiences want different text, so both are kept: the
//! `Display` impl for logs, `user_message` for the screen.
//!
//! `user_message` never interpolates an underlying error. Server-generated text
//! is written for a developer, may name internal hosts or codes, and is not
//! translated. It goes to the log through `Display`, which is where somebody
//! debugging will look for it.

use std::io;
use std::path::{Path, PathBuf};

use matrix_sdk::ruma::api::error::{ErrorKind, RetryAfter};

/// Errors produced by the Matrix layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The server the user typed is not a valid server name or URL.
    #[error("`{0}` is not a valid homeserver name or URL")]
    InvalidServer(String),

    /// Building the client failed, which in practice means homeserver discovery
    /// failed: no `.well-known/matrix/client`, no server at that name, or TLS
    /// the platform would not accept.
    #[error("could not reach a Matrix homeserver: {0}")]
    Discovery(#[from] matrix_sdk::ClientBuildError),

    /// The homeserver rejected the login, or the request never landed.
    #[error("login failed: {0}")]
    Login(#[source] matrix_sdk::Error),

    /// An SDK call after login failed.
    #[error("matrix error: {0}")]
    Sdk(#[from] matrix_sdk::Error),

    /// An operation that needs a logged-in client was given one that is not.
    /// Reaching this is a bug in our own state handling, not a user error.
    #[error("no user is signed in")]
    NotLoggedIn,

    /// Reading or writing the on-disk session failed.
    #[error("session store at {path}: {source}")]
    SessionStore {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The platform keyring, or the file standing in for it, could not be used.
    #[error("{backend} secret store: {message}")]
    SecretStore {
        backend: &'static str,
        message: String,
    },

    /// The stored session exists but is not parseable, typically because the
    /// format changed across versions.
    #[error("stored session is unreadable and will be discarded: {0}")]
    CorruptSession(#[source] serde_json::Error),

    /// The stored session parsed, but a field in it is not a valid Matrix
    /// identifier. Same practical outcome as `CorruptSession`, different cause.
    #[error("stored session contains an invalid {field}: {value}")]
    InvalidStoredIdentifier { field: &'static str, value: String },

    /// A command named a room this account is not in.
    ///
    /// Reachable without a bug: a room can be left from another session
    /// between the room list being drawn and somebody clicking a channel in
    /// it.
    #[error("this account is not in room {room_id}")]
    NoSuchRoom { room_id: String },

    /// Somebody pressed send with nothing typed.
    #[error("a message with no text in it")]
    EmptyMessage,

    /// An attachment larger than this build will carry into the webview.
    ///
    /// The bytes are held whole on both sides of the IPC boundary for a
    /// moment, so there is a size past which drawing one costs more than any
    /// room is worth.
    #[error("attachment is {bytes} bytes, past the {limit} this build will carry")]
    MediaTooLarge { bytes: usize, limit: usize },

    /// An attachment whose bytes are neither a picture nor a clip.
    ///
    /// The type an event claims is written by whoever sent it, so what
    /// actually arrives is sniffed and refused when the two disagree. Also
    /// what a homeserver that has lost the file answers with.
    #[error("attachment is not something this build can draw")]
    UndrawableMedia,

    /// A command named a verification flow the SDK no longer has.
    ///
    /// Not necessarily a bug. Flows expire after ten minutes, either side can
    /// cancel at any point, and a request that another of the account's
    /// devices answered is dropped as soon as that is known. Any of those can
    /// happen between the interface drawing a button and somebody pressing it.
    #[error("verification flow {flow_id} is no longer active")]
    NoSuchFlow { flow_id: String },

    /// This session was asked to start a verification, and the account has no
    /// cross-signing identity to address it to.
    ///
    /// Either the account has never had cross-signing set up, or this session
    /// has not learned about it yet, which is a `/keys/query` away. Both look
    /// the same from here and both mean the same thing to the person waiting:
    /// there is nothing to ask right now.
    #[error("this account has no cross-signing identity to verify against")]
    NoCrossSigningIdentity,

    /// What was typed into the recovery key box is not a recovery key at all.
    ///
    /// Not base58, or the wrong length, or the parity byte does not match. The
    /// distinction from `WrongRecoveryKey` is the whole point: this one means
    /// look at what you pasted, that one means look at which account you are
    /// on.
    #[error("that is not a recovery key")]
    MalformedRecoveryKey,

    /// A well-formed recovery key, or a passphrase, that this account's secret
    /// storage does not open.
    #[error("that recovery key does not open this account's secret storage")]
    WrongRecoveryKey,

    /// A recovery key was offered and the account has no secret storage to
    /// open with it.
    ///
    /// The interface asks before drawing the box, so reaching this means the
    /// account changed underneath somebody: recovery was reset or turned off
    /// between the question and the answer.
    #[error("this account has no recovery set up")]
    NoRecoverySetUp,

    /// The key opened secret storage, and what came out did not include this
    /// account's cross-signing keys.
    ///
    /// Secret storage is a bag of secrets rather than a fixed set, and an
    /// account can have one holding only the megolm backup key. Importing that
    /// leaves the session exactly as unverified as it was, which without this
    /// looks to the person who just typed 48 correct characters like nothing
    /// happened at all.
    #[error("this account's recovery does not hold its cross-signing keys")]
    RecoveryWithoutIdentity,

    /// The account's secret storage is described in a way this client cannot
    /// use: an algorithm it does not implement, or a key description whose own
    /// fields are the wrong size.
    ///
    /// Nothing about the input. Carried as text because the only useful thing
    /// left to do with it is put it in a log.
    #[error("this account's recovery cannot be used: {0}")]
    UnsupportedRecovery(String),
}

impl Error {
    /// Convenience for the many io failures that name a path.
    pub(crate) fn secret_file(path: &Path, source: io::Error) -> Self {
        Self::SessionStore {
            path: path.to_path_buf(),
            source,
        }
    }

    /// A sentence safe and useful to put in front of a person.
    ///
    /// Deliberately never includes the underlying error text. Homeservers
    /// return "M_FORBIDDEN" for both a wrong password and an unknown user, and
    /// echoing a raw code teaches the user nothing while leaking whatever else
    /// the server chose to say.
    pub fn user_message(&self) -> String {
        match self {
            Self::InvalidServer(server) => {
                format!(
                    "`{server}` does not look like a server address. Try something like `example.org`."
                )
            }
            Self::Discovery(_) => {
                "Could not reach that homeserver. Check the address and your connection.".to_owned()
            }
            Self::Login(error) => login_message(error),
            Self::Sdk(_) => {
                "The homeserver could not complete that request. Please try again.".to_owned()
            }
            Self::NotLoggedIn => "No user is signed in.".to_owned(),
            Self::SessionStore { .. } => {
                "Could not read or write the saved session on disk.".to_owned()
            }
            Self::SecretStore { .. } => {
                "Could not reach the system keyring to save your sign-in.".to_owned()
            }
            Self::CorruptSession(_) | Self::InvalidStoredIdentifier { .. } => {
                "The saved session was unreadable, so you have been signed out.".to_owned()
            }
            Self::NoSuchRoom { .. } => {
                "That room is not one this account is in. It may have been left from another \
                 session."
                    .to_owned()
            }
            Self::EmptyMessage => "There is nothing to send.".to_owned(),
            Self::MediaTooLarge { .. } => {
                "That attachment is too large for Consort to show.".to_owned()
            }
            Self::UndrawableMedia => {
                "Consort cannot show that attachment.".to_owned()
            }
            Self::NoSuchFlow { .. } => {
                "That verification is no longer waiting for an answer. Start a new one.".to_owned()
            }
            Self::NoCrossSigningIdentity => {
                "This account has no verification keys set up yet, so there is nothing to compare against."
                    .to_owned()
            }
            Self::MalformedRecoveryKey => {
                "That does not look like a recovery key. It is 48 characters, usually shown in \
                 groups of four."
                    .to_owned()
            }
            Self::WrongRecoveryKey => {
                "That is not this account's recovery key. Check you are signing in to the right \
                 account."
                    .to_owned()
            }
            Self::NoRecoverySetUp => {
                "This account has no recovery key set up, so there is nothing to enter.".to_owned()
            }
            Self::RecoveryWithoutIdentity => {
                "That key worked, but this account's verification keys are not stored with it, so \
                 this session cannot verify itself that way."
                    .to_owned()
            }
            Self::UnsupportedRecovery(_) => {
                "This account's recovery was set up in a way Consort cannot use. Verify from \
                 another session instead."
                    .to_owned()
            }
        }
    }

    /// Whether the stored session should be thrown away in response to this.
    ///
    /// Only true when the session itself is the problem. A keyring that is
    /// temporarily unreachable, a locked store because another copy of Consort
    /// is running, or a homeserver that is down are all reasons to retry, not
    /// reasons to delete the one credential we hold. Deleting on any error is
    /// how a transient failure turns into "type your password again".
    pub fn invalidates_session(&self) -> bool {
        match self {
            Self::CorruptSession(_) | Self::InvalidStoredIdentifier { .. } => true,
            // The homeserver rejecting the token is the one network failure
            // that does mean the session is gone.
            Self::Login(error) | Self::Sdk(error) => is_unknown_token(error),
            _ => false,
        }
    }
}

/// Turn a failed login into a sentence that is actually true.
///
/// Split in two because the two halves fail for unrelated reasons. A login can
/// die inside our own sqlite before the homeserver is ever asked, and that is
/// not something the user can fix by checking their password or their network.
fn login_message(error: &matrix_sdk::Error) -> String {
    if matches!(
        error,
        matrix_sdk::Error::CryptoStoreError(_) | matrix_sdk::Error::StateStore(_)
    ) {
        return "Could not open this account's local encryption store. Signing out and in again \
                usually clears it."
            .to_owned();
    }

    login_message_for_kind(error.client_api_error_kind())
}

/// Turn the homeserver's error code into a sentence that is actually true.
///
/// This used to answer "Incorrect username or password." to every login
/// failure, which is a guess presented as a fact. A homeserver that is rate
/// limiting, an account that has been deactivated, and a gateway that returned
/// 502 all reach this function, and telling a user with the right password to
/// check their password sends them looking in the wrong place.
///
/// `None` means the failure never produced a Matrix error body at all: a
/// connection reset, a TLS failure, a timeout, or a proxy's HTML error page.
fn login_message_for_kind(kind: Option<&ErrorKind>) -> String {
    let Some(kind) = kind else {
        return "Could not reach the homeserver to sign in. Check your connection and try again."
            .to_owned();
    };

    match kind {
        // The one case the old blanket message was right about. Homeservers
        // deliberately return the same code for an unknown user and a wrong
        // password, so this stays vague on purpose.
        ErrorKind::Forbidden | ErrorKind::Unauthorized => {
            "Incorrect username or password.".to_owned()
        }
        // Synapse starts refusing after a handful of failed attempts, per
        // account and per address. Without naming it, a user who has since
        // typed the right password keeps being told the password is wrong.
        ErrorKind::LimitExceeded(data) => match data.retry_after {
            Some(RetryAfter::Delay(delay)) => format!(
                "Too many sign-in attempts. Wait about {} seconds and try again.",
                delay.as_secs().max(1)
            ),
            _ => "Too many sign-in attempts. Wait a minute and try again.".to_owned(),
        },
        ErrorKind::UserDeactivated => "That account has been deactivated.".to_owned(),
        ErrorKind::UserLocked => {
            "That account is locked. Contact your homeserver admin.".to_owned()
        }
        ErrorKind::UserSuspended => {
            "That account is suspended. Contact your homeserver admin.".to_owned()
        }
        ErrorKind::InvalidUsername => "That is not a valid Matrix username.".to_owned(),
        // Reached when the homeserver does not offer password login at all,
        // which is what a server configured for SSO or OIDC only will say.
        ErrorKind::Unrecognized => "That homeserver does not accept password sign-in.".to_owned(),
        // Everything else is the server's problem, not the user's.
        _ => "The homeserver refused the sign-in. Please try again.".to_owned(),
    }
}

/// Whether the SDK is telling us the access token is no longer valid.
fn is_unknown_token(error: &matrix_sdk::Error) -> bool {
    error
        .client_api_error_kind()
        .is_some_and(|kind| matches!(kind, ErrorKind::UnknownToken { .. }))
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    fn all_variants() -> Vec<Error> {
        vec![
            Error::InvalidServer("not a server".to_owned()),
            Error::Login(matrix_sdk::Error::AuthenticationRequired),
            Error::Sdk(matrix_sdk::Error::AuthenticationRequired),
            Error::NotLoggedIn,
            Error::SessionStore {
                path: PathBuf::from("/tmp/consort/session.json"),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            },
            Error::SecretStore {
                backend: "keyring",
                message: "no session bus".to_owned(),
            },
            Error::CorruptSession(serde_json::from_str::<i32>("nonsense").unwrap_err()),
            Error::InvalidStoredIdentifier {
                field: "user_id",
                value: "nonsense".to_owned(),
            },
            Error::MediaTooLarge {
                bytes: 40_000_000,
                limit: 33_554_432,
            },
            Error::UndrawableMedia,
            Error::NoSuchFlow {
                flow_id: "the-only-flow".to_owned(),
            },
            Error::NoCrossSigningIdentity,
            Error::MalformedRecoveryKey,
            Error::WrongRecoveryKey,
            Error::NoRecoverySetUp,
            Error::RecoveryWithoutIdentity,
            Error::UnsupportedRecovery("unsupported algorithm m.made.up".to_owned()),
        ]
    }

    #[test]
    fn a_verification_that_has_already_ended_does_not_sign_anybody_out() {
        // Flows expire after ten minutes and either side can cancel at any
        // point, so naming a flow that has gone is ordinary. Treating it as a
        // broken session would sign somebody out for pressing a button a
        // moment too late.
        let error = Error::NoSuchFlow {
            flow_id: "the-only-flow".to_owned(),
        };

        assert!(!error.invalidates_session());
    }

    /// Every branch of `login_message`, driven directly.
    ///
    /// The classifier takes the error kind rather than a `matrix_sdk::Error`
    /// precisely so these can be written: constructing a real SDK error for
    /// each Matrix error code means hand-rolling an HTTP response per case,
    /// and the branch logic is the part worth testing.
    mod login_messages {
        use super::*;
        use matrix_sdk::ruma::api::error::LimitExceededErrorData;
        use std::time::Duration;

        #[test]
        fn a_forbidden_login_is_the_only_one_that_blames_the_password() {
            assert_eq!(
                login_message_for_kind(Some(&ErrorKind::Forbidden)),
                "Incorrect username or password."
            );
            assert_eq!(
                login_message_for_kind(Some(&ErrorKind::Unauthorized)),
                "Incorrect username or password."
            );
        }

        #[test]
        fn rate_limiting_says_so_instead_of_blaming_the_password() {
            // The regression that started this. Synapse rate limits after a
            // few failures, so a user who has since fixed their password was
            // told to fix it again, forever.
            let message = login_message_for_kind(Some(&ErrorKind::LimitExceeded(
                LimitExceededErrorData::new(),
            )));

            assert!(message.contains("Too many sign-in attempts"), "{message}");
            assert!(!message.to_lowercase().contains("password"), "{message}");
        }

        #[test]
        fn a_rate_limit_with_a_delay_tells_the_user_how_long() {
            let mut data = LimitExceededErrorData::new();
            data.retry_after = Some(RetryAfter::Delay(Duration::from_secs(43)));

            let message = login_message_for_kind(Some(&ErrorKind::LimitExceeded(data)));

            assert!(message.contains("43 seconds"), "{message}");
        }

        #[test]
        fn a_sub_second_delay_never_says_zero_seconds() {
            let mut data = LimitExceededErrorData::new();
            data.retry_after = Some(RetryAfter::Delay(Duration::from_millis(400)));

            let message = login_message_for_kind(Some(&ErrorKind::LimitExceeded(data)));

            assert!(message.contains("1 seconds"), "{message}");
            assert!(!message.contains("0 seconds"), "{message}");
        }

        #[test]
        fn an_account_the_server_will_not_let_in_is_named_as_such() {
            for (kind, expected) in [
                (ErrorKind::UserDeactivated, "deactivated"),
                (ErrorKind::UserLocked, "locked"),
                (ErrorKind::UserSuspended, "suspended"),
            ] {
                let message = login_message_for_kind(Some(&kind));
                assert!(message.contains(expected), "{kind:?} gave {message}");
                assert!(!message.to_lowercase().contains("password"), "{message}");
            }
        }

        #[test]
        fn a_server_without_password_login_says_that_rather_than_wrong_password() {
            let message = login_message_for_kind(Some(&ErrorKind::Unrecognized));

            assert!(message.contains("does not accept password"), "{message}");
        }

        #[test]
        fn an_invalid_username_is_distinguished_from_a_wrong_one() {
            let message = login_message_for_kind(Some(&ErrorKind::InvalidUsername));

            assert!(message.contains("valid Matrix username"), "{message}");
        }

        #[test]
        fn no_error_body_at_all_is_reported_as_a_connection_problem() {
            // A TLS failure, a reset, or a proxy returning HTML. Nothing here
            // says anything about the credentials, because nothing checked them.
            let message = login_message_for_kind(None);

            assert!(
                message.contains("Could not reach the homeserver"),
                "{message}"
            );
            assert!(!message.to_lowercase().contains("password"), "{message}");
        }

        #[test]
        fn an_unmapped_server_error_does_not_blame_the_user() {
            let message = login_message_for_kind(Some(&ErrorKind::NotJson));

            assert!(!message.to_lowercase().contains("password"), "{message}");
            assert!(message.ends_with('.'), "{message}");
        }

        #[test]
        fn a_local_store_failure_is_not_reported_as_a_network_problem() {
            // The failure that started this: matrix-sdk-crypto refusing to open
            // a store belonging to a previous device. It carries no Matrix
            // error code, so the kind-based half sees `None` and would call it
            // a connection failure, sending the user to check their wifi over
            // a problem entirely on their own disk.
            let error = Error::Login(matrix_sdk::Error::CryptoStoreError(Box::new(
                matrix_sdk::encryption::CryptoStoreError::AccountUnset,
            )));

            let message = error.user_message();

            assert!(message.contains("local encryption store"), "{message}");
            assert!(!message.contains("connection"), "{message}");
            assert!(!message.to_lowercase().contains("password"), "{message}");
        }

        #[test]
        fn a_store_failure_is_still_not_a_reason_to_bin_the_session() {
            let error = Error::Login(matrix_sdk::Error::CryptoStoreError(Box::new(
                matrix_sdk::encryption::CryptoStoreError::AccountUnset,
            )));

            assert!(!error.invalidates_session());
        }

        #[test]
        fn every_branch_reads_like_a_sentence() {
            let kinds = [
                Some(ErrorKind::Forbidden),
                Some(ErrorKind::Unauthorized),
                Some(ErrorKind::LimitExceeded(LimitExceededErrorData::new())),
                Some(ErrorKind::UserDeactivated),
                Some(ErrorKind::UserLocked),
                Some(ErrorKind::UserSuspended),
                Some(ErrorKind::InvalidUsername),
                Some(ErrorKind::Unrecognized),
                Some(ErrorKind::NotJson),
                None,
            ];

            for kind in kinds {
                let message = login_message_for_kind(kind.as_ref());
                assert!(!message.is_empty());
                assert!(message.ends_with('.'), "{message}");
                assert!(
                    message.chars().next().is_some_and(char::is_uppercase),
                    "{message}"
                );
            }
        }
    }

    #[test]
    fn every_variant_has_a_user_message_that_reads_like_a_sentence() {
        for error in all_variants() {
            let message = error.user_message();
            assert!(!message.is_empty(), "{error:?} has an empty user message");
            assert!(
                message.ends_with('.'),
                "{error:?} produced {message:?}, which is not a sentence"
            );
            assert!(
                message.chars().next().unwrap().is_uppercase() || message.starts_with('`'),
                "{error:?} produced {message:?}, which does not start with a capital"
            );
        }
    }

    #[test]
    fn a_user_message_never_leaks_the_underlying_error_text() {
        // The whole point of having two strings. If the SDK's wording ever
        // reaches `user_message`, this fails.
        let sdk_wording = matrix_sdk::Error::AuthenticationRequired.to_string();

        for error in [
            Error::Login(matrix_sdk::Error::AuthenticationRequired),
            Error::Sdk(matrix_sdk::Error::AuthenticationRequired),
        ] {
            assert!(
                !error.user_message().contains(&sdk_wording),
                "{error:?} leaked the SDK error into its user message"
            );
        }
    }

    #[test]
    fn an_io_error_is_not_shown_to_the_user() {
        let error = Error::SessionStore {
            path: PathBuf::from("/home/someone/.local/share/consort/session.json"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        };

        let message = error.user_message();

        assert!(!message.contains("/home/someone"));
        assert!(!message.contains("denied"));
    }

    #[test]
    fn the_invalid_server_message_quotes_what_the_user_typed() {
        let error = Error::InvalidServer("exa mple.org".to_owned());
        assert!(error.user_message().contains("exa mple.org"));
    }

    #[test]
    fn display_does_include_the_detail_because_it_is_for_the_log() {
        let error = Error::SessionStore {
            path: PathBuf::from("/tmp/session.json"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        };

        let logged = error.to_string();

        assert!(logged.contains("/tmp/session.json"));
        assert!(logged.contains("denied"));
    }

    #[test]
    fn a_corrupt_session_invalidates_the_stored_session() {
        let error = Error::CorruptSession(serde_json::from_str::<i32>("nonsense").unwrap_err());
        assert!(error.invalidates_session());
    }

    #[test]
    fn an_invalid_stored_identifier_invalidates_the_stored_session() {
        let error = Error::InvalidStoredIdentifier {
            field: "user_id",
            value: "nonsense".to_owned(),
        };
        assert!(error.invalidates_session());
    }

    #[test]
    fn a_transient_failure_never_invalidates_the_stored_session() {
        // This is the regression guard for the bug where any restore failure
        // deleted the token. A locked store or an absent keyring must not.
        let transient = [
            Error::SecretStore {
                backend: "keyring",
                message: "no session bus".to_owned(),
            },
            Error::SessionStore {
                path: PathBuf::from("/tmp/session.json"),
                source: io::Error::new(io::ErrorKind::WouldBlock, "database is locked"),
            },
            Error::NotLoggedIn,
            Error::InvalidServer("x".to_owned()),
        ];

        for error in transient {
            assert!(
                !error.invalidates_session(),
                "{error:?} would have deleted the session"
            );
        }
    }

    #[test]
    fn an_sdk_error_that_is_not_a_token_rejection_does_not_invalidate() {
        assert!(!Error::Sdk(matrix_sdk::Error::AuthenticationRequired).invalidates_session());
        assert!(!Error::Login(matrix_sdk::Error::AuthenticationRequired).invalidates_session());
    }

    #[test]
    fn the_secret_file_helper_names_the_path() {
        let error = Error::secret_file(
            Path::new("/tmp/consort/x.secret"),
            io::Error::new(io::ErrorKind::NotFound, "missing"),
        );

        match error {
            Error::SessionStore { path, .. } => {
                assert_eq!(path, PathBuf::from("/tmp/consort/x.secret"));
            }
            other => panic!("expected SessionStore, got {other:?}"),
        }
    }
}
