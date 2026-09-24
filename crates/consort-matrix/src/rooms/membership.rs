// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Joining a room, leaving one, and asking somebody into one.
//!
//! The ends of belonging to a room, and the things Consort does that change
//! who is in one. [`super::direct`] is the nearest existing neighbour: a room
//! operation with a side effect, reached from a click.
//!
//! ## Joining needs a server name, not just a room ID
//!
//! [`join`] is the one thing here reached for a room the account is not in, so
//! it is the one thing that cannot start from a [`Room`]: there is no local
//! record of a room nobody has joined. What there is instead is the
//! `m.space.child` event that put it in the list, and that carries the servers
//! the space says are in it.
//!
//! Those matter. A room ID is not an address, and a homeserver asked to join a
//! room it has never heard of by ID alone has nowhere to ask. Every other
//! client sends the `via` list for this reason, which is why [`join`] gathers
//! it from every joined space rather than sending the ID on its own.
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

use std::collections::BTreeSet;

use matrix_sdk::Client;
use matrix_sdk::Room;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::space::child::SpaceChildEventContent;
use matrix_sdk::ruma::{OwnedServerName, OwnedUserId, RoomId, RoomOrAliasId, UserId};

use crate::error::{Error, Result};

/// Join `room_id`, through whichever servers a space says it is on.
///
/// The refusal is its own error rather than the SDK's, whose sentence ends in
/// "please try again". A space listing a room is not a promise that anybody
/// may walk into it, so the ordinary failure here is a room that is invite
/// only, and trying again does nothing about that.
///
/// Nothing is done about what is selected afterwards, on [`leave`]'s terms:
/// the room turning up in the list as joined is what the interface reads, and
/// a command with an opinion about the selection would be a second answer to a
/// question the room list already answers.
pub async fn join(client: &Client, room_id: &str) -> Result<()> {
    let room = RoomId::parse(room_id).map_err(|_| Error::NoSuchAddress {
        address: room_id.to_owned(),
    })?;

    let via = via_for(client, &room).await;

    client
        .join_room_by_id_or_alias(<&RoomOrAliasId>::from(&*room), &via)
        .await
        .map_err(|error| {
            tracing::warn!(%error, %room_id, "the homeserver refused a join");
            Error::JoinRefused {
                room_id: room_id.to_owned(),
            }
        })?;

    Ok(())
}

/// The servers every joined space says `room_id` is on.
///
/// Read out of the local store, so a join costs no request beyond itself. Only
/// spaces are asked: an ordinary room holds no `m.space.child`, and walking
/// every room on a large account to learn that would be a page of store reads
/// for one click.
///
/// Empty for a room nothing lists, which is the honest answer rather than a
/// guess. The homeserver then does what it would have done with the ID alone.
async fn via_for(client: &Client, room_id: &RoomId) -> Vec<OwnedServerName> {
    let mut servers = BTreeSet::new();

    for space in client.joined_rooms().iter().filter(|room| room.is_space()) {
        let Ok(Some(raw)) = space
            .get_state_event_static_for_key::<SpaceChildEventContent, _>(room_id)
            .await
        else {
            continue;
        };
        let Ok(event) = raw.deserialize() else {
            continue;
        };
        let Some(original) = event.as_sync().and_then(|event| event.as_original()) else {
            continue;
        };

        servers.extend(original.content.via.iter().cloned());
    }

    servers.into_iter().collect()
}

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
