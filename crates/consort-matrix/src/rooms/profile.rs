// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What can honestly be said about one person, asked for one at a time.
//!
//! Everything else about a person reaches the interface inside a snapshot,
//! because it is drawn without anybody asking. This is not: it is read when
//! somebody clicks a name, so it can afford the one request that presence
//! costs and the roster cannot.
//!
//! The rule throughout is that not knowing is a state with a name. A
//! homeserver with presence switched off, and a great many of them have it
//! switched off, answers nothing at all, and inventing "offline" from that
//! would put a grey dot on somebody who is sitting right there. So the
//! [`Presence::Unknown`] variant exists and the interface draws it as what it
//! is.

use matrix_sdk::Client;
use matrix_sdk::ruma::UserId;
use matrix_sdk::ruma::api::client::presence::get_presence;
use matrix_sdk::ruma::presence::PresenceState;
use serde::Serialize;

/// Where somebody's own client says they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Presence {
    Online,
    /// Matrix calls this `unavailable`, and means "their client is running and
    /// they are not touching it".
    Idle,
    Offline,
    /// Nobody would say. Either the homeserver does not track presence, or it
    /// declined to answer for this person.
    Unknown,
}

/// One person, as far as anything outside the call can describe them.
///
/// `MemberProfile` rather than `Profile`: [`crate::Profile`] is the signed-in
/// account, which is a different thing asked in a different place.
///
/// Not per room, despite the name. Everything on it is account-wide, which is
/// what is left after the power level came off: presence, a status line and a
/// last-seen time all belong to the person rather than to where they are
/// standing.
///
/// Nothing here duplicates the roster. Who they are, what they are called,
/// whether they are muted and when they joined the call all arrive with the
/// channel and are already on screen by the time this is asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberProfile {
    pub presence: Presence,
    /// Their own status line, when they set one.
    pub status: Option<String>,
    /// Milliseconds since they last did anything, as the homeserver counts it.
    ///
    /// `None` whenever presence is unknown, and often `None` even when it is
    /// not: the field is optional in the spec and Synapse omits it for people
    /// it has not heard from.
    pub last_active_ago: Option<u64>,
}

/// Everything worth saying about one person in one room.
///
/// Never fails. Every part of this degrades to "nothing known" on its own,
/// because the alternative is a dialog in front of somebody who clicked a name
/// out of curiosity, and none of these facts is worth interrupting anybody
/// over. Every degraded case is logged.
pub async fn member_profile(client: &Client, user_id: &str) -> MemberProfile {
    MemberProfile {
        presence: presence(client, user_id).await,
        status: status(client, user_id).await,
        last_active_ago: last_active_ago(client, user_id).await,
    }
}

/// The presence request, made once and read three ways.
async fn presence_response(client: &Client, user_id: &str) -> Option<get_presence::v3::Response> {
    let user_id = match UserId::parse(user_id) {
        Ok(user_id) => user_id,
        Err(error) => {
            tracing::warn!(%error, user_id, "asked for the presence of something that is not a user");
            return None;
        }
    };

    match client.send(get_presence::v3::Request::new(user_id)).await {
        Ok(response) => Some(response),
        Err(error) => {
            // Routine rather than exceptional. Presence is off by default on
            // Synapse and stays off on most homeservers that run at any size,
            // because it is the single most expensive thing in the protocol.
            tracing::debug!(%error, "the homeserver would not say where somebody is");
            None
        }
    }
}

/// Read the presence state, or [`Presence::Unknown`].
async fn presence(client: &Client, user_id: &str) -> Presence {
    presence_response(client, user_id)
        .await
        .map_or(Presence::Unknown, |response| {
            as_presence(&response.presence)
        })
}

async fn status(client: &Client, user_id: &str) -> Option<String> {
    presence_response(client, user_id)
        .await
        .and_then(|response| response.status_msg)
        .filter(|status| !status.trim().is_empty())
}

async fn last_active_ago(client: &Client, user_id: &str) -> Option<u64> {
    presence_response(client, user_id)
        .await
        .and_then(|response| response.last_active_ago)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

/// Map the wire's presence vocabulary onto the one an interface draws.
///
/// The enum is non-exhaustive upstream, so anything new arrives as unknown
/// rather than as a guess.
fn as_presence(state: &PresenceState) -> Presence {
    match state {
        PresenceState::Online => Presence::Online,
        PresenceState::Unavailable => Presence::Idle,
        PresenceState::Offline => Presence::Offline,
        _ => Presence::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_presence_states_map_across() {
        assert_eq!(as_presence(&PresenceState::Online), Presence::Online);
        assert_eq!(as_presence(&PresenceState::Unavailable), Presence::Idle);
        assert_eq!(as_presence(&PresenceState::Offline), Presence::Offline);
    }

    #[test]
    fn presence_serialises_as_the_lowercase_name() {
        // The interface matches on these strings, so the mapping is part of
        // the contract rather than an implementation detail.
        assert_eq!(serde_json::to_string(&Presence::Idle).unwrap(), "\"idle\"");
        assert_eq!(
            serde_json::to_string(&Presence::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn a_profile_carries_its_fields_under_the_names_the_interface_reads() {
        let json = serde_json::to_value(MemberProfile {
            presence: Presence::Online,
            status: Some("in a meeting".to_owned()),
            last_active_ago: Some(4_000),
        })
        .unwrap();

        assert_eq!(json["presence"], "online");
        assert_eq!(json["status"], "in a meeting");
        assert_eq!(json["lastActiveAgo"], 4_000);
    }
}
