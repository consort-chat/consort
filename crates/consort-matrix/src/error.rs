// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Error type for the Matrix layer.
//!
//! Every variant carries a `user_message()` string, because a login form has to
//! say something to a human and `matrix_sdk::Error`'s `Display` is written for a
//! log line. The two audiences want different text, so both are kept: the
//! `Display` impl for logs, `user_message` for the screen.

use std::io;
use std::path::PathBuf;

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

    /// The stored session exists but is not parseable, typically because the
    /// format changed across versions.
    #[error("stored session is unreadable and will be discarded: {0}")]
    CorruptSession(#[source] serde_json::Error),
}

impl Error {
    /// A sentence safe and useful to put in front of a person.
    ///
    /// Deliberately does not include the underlying error text for the login
    /// case: homeservers return "M_FORBIDDEN" for both a wrong password and an
    /// unknown user, and echoing the raw code teaches the user nothing.
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
            Self::Login(_) => "Incorrect username or password.".to_owned(),
            Self::Sdk(error) => format!("The homeserver returned an error: {error}"),
            Self::NotLoggedIn => "No user is signed in.".to_owned(),
            Self::SessionStore { .. } => {
                "Could not read or write the saved session on disk.".to_owned()
            }
            Self::CorruptSession(_) => {
                "The saved session was unreadable, so you have been signed out.".to_owned()
            }
        }
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
