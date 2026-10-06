// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The room to say something to one person in.
//!
//! A direct message is an ordinary Matrix room. What makes it one is the
//! account's `m.direct` account data, which maps a person to the rooms shared
//! only with them, and the SDK keeps that mapping for us.
//!
//! ## Why this creates
//!
//! Because the alternative does nothing for almost everybody who presses the
//! button. Most people opening somebody's card have never messaged them, so a
//! version that only opened an existing room would be the disabled button it
//! replaced with extra steps.
//!
//! It is a real side effect and it is the one every other client has. Element,
//! Fluffychat and Cinny all create on the first message rather than on the
//! first click; Consort creates on the click, which costs an empty room in the
//! case where somebody changes their mind. That is the price of the button
//! working, and the room is one the person can leave.
//!
//! ## Why an invite counts as already having one
//!
//! `m.direct` is per account and written by whoever made the room, so the
//! person on the receiving end has nothing in theirs. `get_dm_room` reads that
//! mapping and walks only joined rooms, so for them it misses twice over and
//! both sides end up making a room. That is issue #184.
//!
//! The room is not missing on their side, it is sitting there as an invite,
//! carrying `is_direct` and the name of whoever sent it. Taking it is what
//! answers the question somebody asked by pressing the button, and the SDK
//! writes their `m.direct` on the way in, so the next press costs nothing.
//!
//! Only an invite from this person, and only a direct one. Nothing here makes
//! a second room with somebody impossible: one made and invited into by hand
//! is untouched, and this reuses a room rather than choosing between two.

use matrix_sdk::ruma::UserId;
use matrix_sdk::{Client, Room};

use crate::error::{Error, Result};

/// The room to talk to one person in, made if there is not one already.
///
/// Never picks between several. `get_dm_room` takes the first room shared only
/// with them, which is what every client does with an account that has somehow
/// ended up with two: choosing by activity would need read receipts, and
/// choosing by creation time would move somebody's conversation the day a bot
/// makes a second room.
pub async fn direct(client: &Client, user_id: &str) -> Result<String> {
    let user_id = UserId::parse(user_id).map_err(|_| Error::NoSuchUser {
        user_id: user_id.to_owned(),
    })?;

    // Local, and the case that costs nothing: the mapping is account data the
    // sync already brought in.
    if let Some(room) = client.get_dm_room(&user_id) {
        return Ok(room.room_id().to_string());
    }

    if let Some(room) = asked_in_by(client, &user_id).await {
        room.join().await?;
        return Ok(room.room_id().to_string());
    }

    let room = client.create_dm(&user_id).await?;
    Ok(room.room_id().to_string())
}

/// The direct room this person has already asked this account into, if any.
///
/// Both halves of the test matter. `is_direct` is the only thing that
/// distinguishes the room somebody made to message you from the room somebody
/// made for a project, and the sender is what keeps an invite from a third
/// person out of an answer about this one.
///
/// An invite neither question can be answered about is not a match, which is
/// the same answer as an invite from somebody else: there is still a room to
/// make, and one unreadable invite is no reason for the button to do nothing.
async fn asked_in_by(client: &Client, user_id: &UserId) -> Option<Room> {
    for room in client.invited_rooms() {
        if room.is_direct().await.unwrap_or(false)
            && room
                .invite_details()
                .await
                .is_ok_and(|invite| invite.inviter_id == user_id)
        {
            return Some(room);
        }
    }

    None
}
