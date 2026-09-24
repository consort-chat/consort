// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Leaving a room, and asking somebody into one.
//!
//! The two ends of belonging to a room, and the first things Consort does that
//! change who is in one. [`super::direct`] is the nearest existing neighbour: a
//! room operation with a side effect, reached from a click.
//!
//! ## Leaving does not come back
//!
//! [`leave`] is one request and no cleverness, and that is the whole of it.
//! What it costs is elsewhere: an invite-only room left by mistake needs
//! somebody still in it to ask you back, and nothing here can undo it. The step
//! between the press and the request is the interface's job, because a
//! confirmation this function asked for would be a confirmation no test could
//! answer and no caller could skip.
//!
//! There is no variant for a refused leave. Any joined member may leave, so
//! what actually reaches [`crate::Error::Sdk`] here is the network, and its
//! sentence ends in "please try again", which for a network is the right
//! advice.
//!
//! ## Inviting is refused four different ways
//!
//! All four are decided here, before the request, and that is not an
//! optimisation. A homeserver answers "they are already in the room", "they are
//! banned", and "you may not invite anybody" with one `M_FORBIDDEN` and a
//! sentence written for whoever reads its logs. A client that waited to be told
//! would have one message for four situations that want four, and the one
//! people actually hit is the one that message helps least: "that did not work"
//! is useless when the reason is that somebody here banned them.
//!
//! What is left over is [`crate::Error::InviteRefused`]: everything local said
//! yes and the homeserver said no anyway.

use matrix_sdk::Client;
use matrix_sdk::Room;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::{OwnedUserId, RoomId, UserId};

use crate::error::{Error, Result};

/// Leave `room_id`.
///
/// [`matrix_sdk::Room::leave`] also leaves the room's predecessors, which is
/// what somebody means by leaving a room that has been upgraded: the old one
/// is the same conversation under an older room version, and staying in it
/// would keep the account in a room it thinks it has left.
pub async fn leave(client: &Client, room_id: &str) -> Result<()> {
    let room = room_of(client, room_id)?;
    room.leave().await?;
    Ok(())
}

/// Ask `user_id` into `room_id`.
///
/// The permission is read before the member list, so the case the interface
/// has already drawn as disabled costs no request at all. See the module note
/// for why all four refusals are decided here rather than read off the
/// homeserver's.
pub async fn invite(client: &Client, room_id: &str, user_id: &str) -> Result<()> {
    let room = room_of(client, room_id)?;
    let user = user_of(user_id)?;

    if !may_invite(client, &room).await? {
        return Err(Error::NotAllowedToInvite {
            room_id: room_id.to_owned(),
        });
    }

    /*
        A member list that could not be read lets the invitation through, on
        the same terms as the call gate in `commands::call_connect_for`: the
        error is about not being able to ask rather than about the answer, and
        there is no honest refusal to draw from a request that timed out. The
        homeserver decides in that case, which is what it would have done
        anyway.
    */
    match membership_of(&room, &user).await {
        Some(MembershipState::Join) => {
            return Err(Error::AlreadyInRoom {
                room_id: room_id.to_owned(),
                user_id: user_id.to_owned(),
            });
        }
        Some(MembershipState::Invite) => {
            return Err(Error::AlreadyInvited {
                room_id: room_id.to_owned(),
                user_id: user_id.to_owned(),
            });
        }
        Some(MembershipState::Ban) => {
            return Err(Error::BannedFromRoom {
                room_id: room_id.to_owned(),
                user_id: user_id.to_owned(),
            });
        }
        // Somebody who left, somebody who knocked, and somebody this room has
        // never heard of. All three can be asked in.
        _ => {}
    }

    room.invite_user_by_id(&user).await.map_err(|error| {
        tracing::warn!(%error, %room_id, "the homeserver refused an invitation");
        Error::InviteRefused {
            room_id: room_id.to_owned(),
            user_id: user_id.to_owned(),
        }
    })
}

/// Whether this account's power level lets it invite anybody into `room_id`.
///
/// Asked by the panel so that the control can be drawn disabled with a reason
/// rather than left out. Leaving it out would answer a different question from
/// the one somebody is asking: a control that is not there reads as a thing
/// Consort cannot do, and this is a thing this room will not let them do.
///
/// An error rather than `false` for a room this account is not in, because the
/// interface draws `false` as a permission and that would be the wrong reason
/// on screen.
pub async fn can_invite(client: &Client, room_id: &str) -> Result<bool> {
    let room = room_of(client, room_id)?;
    may_invite(client, &room).await
}

/// Whether this account may invite into a room it is in.
///
/// `power_levels_or_default` rather than the event alone, because most small
/// rooms never write an `m.room.power_levels` and the specification's default
/// is that anybody may invite. Reading a missing event as a refusal would
/// disable the control in exactly the rooms where it should work.
async fn may_invite(client: &Client, room: &Room) -> Result<bool> {
    let us = client.user_id().ok_or(Error::NotLoggedIn)?;
    Ok(room.power_levels_or_default().await.user_can_invite(us))
}

/// What `user` currently is in `room`, as far as anything can tell.
///
/// `None` covers both somebody the room has never heard of and a member list
/// that could not be read, which is deliberate: the caller does the same thing
/// with either, and telling them apart would be a distinction with one
/// outcome.
async fn membership_of(room: &Room, user: &UserId) -> Option<MembershipState> {
    match room.get_member(user).await {
        Ok(member) => member.map(|member| member.membership().clone()),
        Err(error) => {
            tracing::warn!(%error, room_id = %room.room_id(), "inviting without a member list");
            None
        }
    }
}

/// The room this account is in, by ID.
fn room_of(client: &Client, room_id: &str) -> Result<Room> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;
    client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })
}

/// The person a user ID names.
fn user_of(user_id: &str) -> Result<OwnedUserId> {
    UserId::parse(user_id).map_err(|_| Error::NoSuchUser {
        user_id: user_id.to_owned(),
    })
}
