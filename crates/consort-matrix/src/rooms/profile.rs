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
use matrix_sdk::ruma::api::client::presence::get_presence;
use matrix_sdk::ruma::events::room::power_levels::UserPowerLevel;
use matrix_sdk::ruma::presence::PresenceState;
use matrix_sdk::ruma::{RoomId, UserId};
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

/// What somebody is allowed to do in this room, at the granularity a person
/// cares about.
///
/// The three labels every Matrix client already uses, from the numbers the
/// spec defines: 100 and 50 are conventions rather than rules, but they are
/// conventions every server and client is built around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    Admin,
    Moderator,
    Member,
}

/// One person, as far as anything outside the call can describe them.
///
/// `MemberProfile` rather than `Profile`: [`crate::Profile`] is the signed-in
/// account, which is a different thing asked in a different place.
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
    pub standing: Standing,
}

/// Everything worth saying about one person in one room.
///
/// Never fails. Every part of this degrades to "nothing known" on its own,
/// because the alternative is a dialog in front of somebody who clicked a name
/// out of curiosity, and none of these facts is worth interrupting anybody
/// over. Every degraded case is logged.
pub async fn member_profile(client: &Client, room_id: &str, user_id: &str) -> MemberProfile {
    MemberProfile {
        presence: presence(client, user_id).await,
        status: status(client, user_id).await,
        last_active_ago: last_active_ago(client, user_id).await,
        standing: standing(client, room_id, user_id).await,
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

/// What this person is allowed to do in this room.
///
/// [`Standing::Member`] where nothing is known, which is the same answer as
/// "an ordinary person" and is right for the same reason the presence default
/// is not: an unreadable power level means no evidence of authority, and no
/// evidence of authority is exactly what an ordinary member looks like.
async fn standing(client: &Client, room_id: &str, user_id: &str) -> Standing {
    let Ok(room_id) = RoomId::parse(room_id) else {
        return Standing::Member;
    };
    let Ok(user_id) = UserId::parse(user_id) else {
        return Standing::Member;
    };
    let Some(room) = client.get_room(&room_id) else {
        return Standing::Member;
    };

    // Local: reads the member store rather than pulling the room's whole
    // member list, matching how avatars are looked up.
    match room.get_member_no_sync(&user_id).await {
        Ok(Some(member)) => as_standing(member.power_level()),
        Ok(None) => Standing::Member,
        Err(error) => {
            tracing::warn!(%error, %room_id, "could not read somebody's standing in a room");
            Standing::Member
        }
    }
}

/// The two thresholds every Matrix client draws these labels at, and the one
/// case that is above both of them by construction.
///
/// From room version 12 a creator has no number at all: they outrank every
/// power level there is, and the type says so rather than picking a large
/// integer.
fn as_standing(power: UserPowerLevel) -> Standing {
    let power = match power {
        UserPowerLevel::Infinite => return Standing::Admin,
        UserPowerLevel::Int(power) => i64::from(power),
        // The enum is non-exhaustive upstream. A rank nothing here has heard
        // of is not evidence of authority, which is what an ordinary member
        // looks like.
        _ => return Standing::Member,
    };
    if power >= 100 {
        Standing::Admin
    } else if power >= 50 {
        Standing::Moderator
    } else {
        Standing::Member
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power(level: i32) -> UserPowerLevel {
        UserPowerLevel::Int(level.into())
    }

    #[test]
    fn the_three_standings_come_from_the_conventional_thresholds() {
        assert_eq!(as_standing(power(100)), Standing::Admin);
        assert_eq!(as_standing(power(50)), Standing::Moderator);
        assert_eq!(as_standing(power(0)), Standing::Member);
    }

    #[test]
    fn power_above_a_threshold_still_reads_as_that_standing() {
        // Rooms exist with a 101 in them, and a homeserver admin can set any
        // number at all. Nothing between the thresholds is a fourth kind of
        // person.
        assert_eq!(as_standing(power(1000)), Standing::Admin);
        assert_eq!(as_standing(power(99)), Standing::Moderator);
        assert_eq!(as_standing(power(49)), Standing::Member);
    }

    #[test]
    fn a_negative_power_level_is_still_a_member() {
        // Rooms that mute newcomers by default do this. They are restricted,
        // not a category of their own, and there is no label worth inventing.
        assert_eq!(as_standing(power(-1)), Standing::Member);
    }

    #[test]
    fn a_room_creator_outranks_every_number() {
        // Room version 12 onwards. They have no power level, they are simply
        // above all of them, and there is no integer to compare.
        assert_eq!(as_standing(UserPowerLevel::Infinite), Standing::Admin);
    }

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
        assert_eq!(
            serde_json::to_string(&Standing::Moderator).unwrap(),
            "\"moderator\""
        );
    }

    #[test]
    fn a_profile_carries_its_fields_under_the_names_the_interface_reads() {
        let json = serde_json::to_value(MemberProfile {
            presence: Presence::Online,
            status: Some("in a meeting".to_owned()),
            last_active_ago: Some(4_000),
            standing: Standing::Admin,
        })
        .unwrap();

        assert_eq!(json["presence"], "online");
        assert_eq!(json["status"], "in a meeting");
        assert_eq!(json["lastActiveAgo"], 4_000);
        assert_eq!(json["standing"], "admin");
    }
}
