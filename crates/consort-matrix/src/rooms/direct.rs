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

use matrix_sdk::Client;
use matrix_sdk::ruma::UserId;

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

    let room = client.create_dm(&user_id).await?;
    Ok(room.room_id().to_string())
}
