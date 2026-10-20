// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The custom emoji an account can reach, read out of MSC2545 image packs.
//!
//! Three sources, and the only thing they have in common is the content:
//! the account's own pack, which is global account data; the packs somebody
//! has switched on for every room, which is a list in account data naming
//! packs that live in other rooms' state; and the packs belonging to the room
//! being read. [`packs`] returns them in that order.
//!
//! Split the way [`crate::rooms`] is. [`pack_of`] applies every rule over
//! plain data and is where the tests are; the async half does the store reads
//! and nothing else.
//!
//! ## The names on the wire
//!
//! MSC2545 has been through a rewrite. The revision that is current names
//! `m.room.image_pack` and `m.image_pack.rooms`, each with an `im.ponies.`
//! unstable twin, and both are read here. It also dropped the per-account pack
//! the earlier revisions had, which every client that implements any of this
//! still writes as `im.ponies.user_emotes`: Cinny, FluffyChat, nheko and
//! gotktrix all do. So that type is read too, under its unstable name only,
//! because the rewrite left it with no stable one to read.
//!
//! ## Why every URL is checked here as well
//!
//! A pack is room state, which means anybody with permission to set state in
//! any room somebody has switched on can put a string in it. The only `src`
//! that may reach an `img` is an `mxc://`, for the reason
//! `app/src/components/FormattedBody.tsx` gives: any other scheme is a request
//! the reader's machine makes to a server a stranger chose. `mxcUrl` in
//! `app/src/lib/api.ts` refuses one too, and neither is a reason to drop the
//! other.

use std::collections::BTreeMap;

use matrix_sdk::Client;
use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::ruma::RoomId;
use matrix_sdk::ruma::events::{GlobalAccountDataEventType, StateEventType};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The usage that makes an image an emoji rather than a sticker.
const EMOTICON: &str = "emoticon";

/// The account data holding the account's own pack.
///
/// Unstable only. See the module note: the current MSC revision has no
/// per-account pack and so no stable name to read.
const USER_PACK: &str = "im.ponies.user_emotes";

/// The account data listing packs somebody switched on for every room.
const ENABLED_ROOMS: [&str; 2] = ["m.image_pack.rooms", "im.ponies.emote_rooms"];

/// The room state a pack lives in.
const ROOM_PACK: [&str; 2] = ["m.room.image_pack", "im.ponies.room_emotes"];

/// The ID the account's own pack is drawn under.
///
/// Not a room ID and not a state key, because it is neither. Every other pack
/// is named by the room and state key it came from, and no room ID can collide
/// with this: one starts with `!`.
const USER_PACK_ID: &str = "account";

/// One image pack, with only the images that are emoji in it.
///
/// A sticker pack is read and then dropped: this build has no sticker picker,
/// and offering somebody a sticker as an emoji would send a 512 pixel picture
/// into the middle of a sentence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePack {
    /// Stable across reads, so it can be a React key: `account` for the
    /// account's own, and `<room id>/<state key>` for a room's.
    pub id: String,
    /// What to call it. The pack's own name, or the room's where it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// An `mxc://` for the pack's icon, its own or the room's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// Who made the images, where the pack says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    /// The emoji in it, by shortcode.
    pub images: Vec<PackImage>,
}

/// One custom emoji.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackImage {
    /// What somebody types to reach it, without the surrounding colons.
    ///
    /// Taken as written. MSC2545 gives a grammar for one and then says to draw
    /// an image whose shortcode breaks it anyway, so that somebody can see the
    /// mistake and fix it.
    pub shortcode: String,
    /// The `mxc://` the image is at, checked to be one.
    pub url: String,
    /// What the image is of, for somebody who cannot see it. Not the
    /// shortcode: MSC2545 is explicit that those are different jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// One pack's content as it arrives.
#[derive(Clone, Debug, Default, Deserialize)]
struct PackContent {
    #[serde(default)]
    images: BTreeMap<String, ImageEntry>,
    #[serde(default)]
    pack: PackMeta,
}

/// What a pack says about itself.
#[derive(Clone, Debug, Default, Deserialize)]
struct PackMeta {
    display_name: Option<String>,
    avatar_url: Option<String>,
    attribution: Option<String>,
    usage: Option<Vec<String>>,
}

/// One entry in a pack's `images` map.
#[derive(Clone, Debug, Deserialize)]
struct ImageEntry {
    /// Required by MSC2545, optional here: anybody can write a pack, and one
    /// entry missing its address must not cost the three hundred beside it.
    #[serde(default)]
    url: Option<String>,
    body: Option<String>,
    /// What this one image is for, overriding the pack's own answer.
    ///
    /// Not in the current MSC revision, which has `usage` on the pack only.
    /// Earlier ones had it here as well and the clients that implement packs
    /// read it, so a pack carrying both is the ordinary case rather than a
    /// malformed one.
    usage: Option<Vec<String>>,
}

/// What a pack with nothing of its own to say falls back to, which for a pack
/// in a room is that room.
#[derive(Clone, Debug, Default)]
struct Fallbacks {
    name: Option<String>,
    avatar: Option<String>,
}

/// One pack's content as something to draw, or `None` when it holds no emoji.
///
/// Empty and sticker-only packs both answer `None`, which is the same answer
/// to the only question being asked: is there anything here to put in front of
/// somebody choosing an emoji.
fn pack_of(id: String, content: PackContent, fallbacks: &Fallbacks) -> Option<ImagePack> {
    let pack_usage = content.pack.usage.as_deref();

    let images: Vec<PackImage> = content
        .images
        .into_iter()
        .filter(|(_, image)| is_emoticon(image.usage.as_deref().or(pack_usage)))
        .filter_map(|(shortcode, image)| {
            Some(PackImage {
                shortcode,
                url: mxc(image.url.as_deref()?)?,
                body: image.body,
            })
        })
        .collect();

    if images.is_empty() {
        return None;
    }

    Some(ImagePack {
        id,
        name: content
            .pack
            .display_name
            .filter(|name| !name.trim().is_empty())
            .or_else(|| fallbacks.name.clone()),
        avatar: content
            .pack
            .avatar_url
            .as_deref()
            .and_then(mxc)
            .or_else(|| fallbacks.avatar.clone()),
        attribution: content
            .pack
            .attribution
            .filter(|words| !words.trim().is_empty()),
        images,
    })
}

/// Whether `usage` allows an image to be used as an emoji.
///
/// Absent and empty both mean every usage, which is what MSC2545 says and what
/// most packs in the wild carry: a pack with no `usage` is an emoji pack to
/// every client that reads one.
fn is_emoticon(usage: Option<&[String]>) -> bool {
    match usage {
        None => true,
        Some(usage) => usage.is_empty() || usage.iter().any(|one| one == EMOTICON),
    }
}

/// `url` if it is an `mxc://`, and nothing otherwise. See the module note.
fn mxc(url: &str) -> Option<String> {
    url.starts_with("mxc://").then(|| url.to_owned())
}

/// Every pack somebody reading `room_id` can choose an emoji from.
///
/// Local: every read is of the state store, so this is safe to call whenever
/// a room is opened. Nothing here makes a request, which is the same rule
/// [`crate::rooms`] keeps and for the same reason.
///
/// In the order MSC2545 asks for, with the account's own pack in front: what
/// somebody put together for themselves before what a room handed them. A pack
/// reachable two ways, which is what switching on a pack in the room you are
/// reading does, is returned once.
pub async fn packs(client: &Client, room_id: &str) -> Result<Vec<ImagePack>> {
    let here = room_of(client, room_id)?;

    let mut found: Vec<ImagePack> = Vec::new();
    if let Some(mine) = user_pack(client).await {
        found.push(mine);
    }
    for (room, state_key) in enabled_elsewhere(client).await {
        found.extend(packs_in(client, &room, Some(&state_key)).await);
    }
    found.extend(packs_in(client, here.room_id().as_str(), None).await);

    // First wins, which keeps the order above: a pack switched on for every
    // room stays where that decision put it rather than moving when somebody
    // opens the room it lives in.
    let mut seen = std::collections::HashSet::new();
    found.retain(|pack| seen.insert(pack.id.clone()));

    Ok(found)
}

/// The account's own pack, if it has one.
async fn user_pack(client: &Client) -> Option<ImagePack> {
    let raw = client
        .account()
        .account_data_raw(GlobalAccountDataEventType::from(USER_PACK))
        .await
        .map_err(|error| {
            tracing::warn!(%error, "could not read the account's own emoji pack");
        })
        .ok()??;

    let content = raw
        .deserialize_as_unchecked::<PackContent>()
        .map_err(|error| {
            tracing::warn!(%error, "the account's own emoji pack is not a pack");
        })
        .ok()?;

    pack_of(
        USER_PACK_ID.to_owned(),
        content,
        &Fallbacks {
            // Named by the person whose it is, in the absence of anything in
            // the event. "Your emoji" rather than a blank tab.
            name: Some("Your emoji".to_owned()),
            avatar: None,
        },
    )
}

/// The packs somebody has switched on for every room, as room and state key.
///
/// A room this account is not in is kept here and dropped by [`packs_in`],
/// which is where there is a store to ask. MSC2545 says outright that the list
/// can name one.
async fn enabled_elsewhere(client: &Client) -> Vec<(String, String)> {
    /// The shape of the list, which is a room ID to its pack's state keys.
    #[derive(Deserialize)]
    struct Enabled {
        #[serde(default)]
        rooms: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    }

    for event_type in ENABLED_ROOMS {
        let read = client
            .account()
            .account_data_raw(GlobalAccountDataEventType::from(event_type))
            .await;
        let Ok(Some(raw)) = read else {
            continue;
        };
        let Ok(enabled) = raw.deserialize_as_unchecked::<Enabled>() else {
            tracing::warn!(event_type, "the list of enabled emoji packs is not one");
            continue;
        };

        return enabled
            .rooms
            .into_iter()
            .flat_map(|(room, keys)| {
                keys.into_keys()
                    .map(move |state_key| (room.clone(), state_key))
            })
            .collect();
    }

    Vec::new()
}

/// The packs in one room's state, or the one named by `state_key`.
///
/// Empty for a room this account is not in, which is both what a list of
/// switched-on packs can name and what leaving a room leaves behind.
async fn packs_in(client: &Client, room_id: &str, state_key: Option<&str>) -> Vec<ImagePack> {
    let Ok(room) = room_of(client, room_id) else {
        return Vec::new();
    };

    let fallbacks = Fallbacks {
        name: room.name().filter(|name| !name.trim().is_empty()),
        avatar: room.avatar_url().map(|uri| uri.to_string()),
    };

    let mut found = Vec::new();
    for event_type in ROOM_PACK {
        let read = room
            .get_state_events(StateEventType::from(event_type))
            .await;
        let Ok(events) = read else {
            // A room whose packs could not be read draws no custom emoji,
            // which is visibly less than it should be rather than an error
            // in front of somebody who opened a room.
            tracing::warn!(event_type, room_id, "could not read a room's emoji packs");
            continue;
        };

        for raw in events {
            // Joined rooms only, which is the stance `rooms::facts` takes for
            // the same reason: stripped state belongs to an invite, and a
            // room somebody has been invited to is not one they are reading.
            let RawAnySyncOrStrippedState::Sync(raw) = raw else {
                continue;
            };
            // Read by field rather than through a ruma content type, because
            // there is none: this is a custom event, and the state key is on
            // the event rather than in its content.
            let Ok(Some(key)) = raw.get_field::<String>("state_key") else {
                continue;
            };
            if state_key.is_some_and(|wanted| wanted != key) {
                continue;
            }
            let Ok(Some(content)) = raw.get_field::<PackContent>("content") else {
                continue;
            };

            found.extend(pack_of(format!("{room_id}/{key}"), content, &fallbacks));
        }

        if !found.is_empty() {
            // The unstable type is only read where the stable one said
            // nothing. A homeserver carrying both holds one pack written
            // twice, and drawing it twice would be this build's fault.
            break;
        }
    }

    found
}

fn room_of(client: &Client, room_id: &str) -> Result<matrix_sdk::Room> {
    let parsed = RoomId::parse(room_id).map_err(|_| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })?;
    client.get_room(&parsed).ok_or_else(|| Error::NoSuchRoom {
        room_id: room_id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One pack's content, from the JSON a homeserver would hold.
    fn content(json: serde_json::Value) -> PackContent {
        serde_json::from_value(json).expect("the fixture is a pack")
    }

    /// The rules applied to `json`, with no room to fall back to.
    fn read(json: serde_json::Value) -> Option<ImagePack> {
        pack_of("account".to_owned(), content(json), &Fallbacks::default())
    }

    /// The shortcodes in a pack, in the order they come back.
    fn codes(pack: &ImagePack) -> Vec<&str> {
        pack.images
            .iter()
            .map(|image| image.shortcode.as_str())
            .collect()
    }

    fn a_room() -> Fallbacks {
        Fallbacks {
            name: Some("Blobs".to_owned()),
            avatar: Some("mxc://example.org/room".to_owned()),
        }
    }

    #[test]
    fn a_pack_that_says_nothing_about_usage_is_an_emoji_pack() {
        // What most packs in the wild carry, and what MSC2545 says an absent
        // usage means: every usage.
        let pack = read(serde_json::json!({
            "images": { "blobcat": { "url": "mxc://example.org/cat" } },
        }))
        .expect("a pack with one image is a pack");

        assert_eq!(codes(&pack), ["blobcat"]);
        assert_eq!(pack.images[0].url, "mxc://example.org/cat");
    }

    #[test]
    fn an_empty_usage_array_also_means_every_usage() {
        let pack = read(serde_json::json!({
            "images": { "blobcat": { "url": "mxc://example.org/cat" } },
            "pack": { "usage": [] },
        }));

        assert!(pack.is_some());
    }

    #[test]
    fn a_sticker_pack_is_not_offered_as_emoji() {
        // There is no sticker picker in this build, and a 512 pixel picture in
        // the middle of a sentence is not what somebody choosing an emoji
        // asked for.
        let pack = read(serde_json::json!({
            "images": { "wave": { "url": "mxc://example.org/wave" } },
            "pack": { "usage": ["sticker"] },
        }));

        assert_eq!(pack, None);
    }

    #[test]
    fn one_sticker_inside_an_emoji_pack_is_left_behind() {
        let pack = read(serde_json::json!({
            "images": {
                "blobcat": { "url": "mxc://example.org/cat" },
                "wave": { "url": "mxc://example.org/wave", "usage": ["sticker"] },
            },
        }))
        .expect("the emoji in it make it a pack");

        assert_eq!(codes(&pack), ["blobcat"]);
    }

    #[test]
    fn an_image_that_says_it_is_an_emoji_is_kept_out_of_a_sticker_pack() {
        // The image's own usage replaces the pack's rather than adding to it,
        // which is how every client that reads one treats it.
        let pack = read(serde_json::json!({
            "images": {
                "wave": { "url": "mxc://example.org/wave" },
                "blobcat": { "url": "mxc://example.org/cat", "usage": ["emoticon"] },
            },
            "pack": { "usage": ["sticker"] },
        }))
        .expect("the one emoji in it makes it a pack");

        assert_eq!(codes(&pack), ["blobcat"]);
    }

    #[test]
    fn an_image_somewhere_other_than_the_homeserver_is_refused() {
        // The security rule, and the reason this is checked here as well as in
        // the page: a pack is state anybody with permission can write, and an
        // `img` pointed at a stranger's web server is a read receipt nobody
        // asked for with an IP address attached.
        let pack = read(serde_json::json!({
            "images": {
                "blobcat": { "url": "mxc://example.org/cat" },
                "tracker": { "url": "https://example.net/pixel.gif" },
            },
        }))
        .expect("the one real image makes it a pack");

        assert_eq!(codes(&pack), ["blobcat"]);
    }

    #[test]
    fn a_pack_of_nothing_but_refused_addresses_is_not_a_pack() {
        let pack = read(serde_json::json!({
            "images": { "tracker": { "url": "https://example.net/pixel.gif" } },
        }));

        assert_eq!(pack, None);
    }

    #[test]
    fn a_pack_with_no_images_at_all_is_not_a_pack() {
        assert_eq!(read(serde_json::json!({ "images": {} })), None);
        assert_eq!(read(serde_json::json!({})), None);
    }

    #[test]
    fn a_pack_with_no_name_of_its_own_takes_the_rooms() {
        // MSC2545's own default. A tab labelled with nothing is worse than one
        // labelled with where the emoji came from.
        let pack = pack_of(
            "!blobs:example.org/".to_owned(),
            content(serde_json::json!({
                "images": { "blobcat": { "url": "mxc://example.org/cat" } },
            })),
            &a_room(),
        )
        .expect("a pack");

        assert_eq!(pack.name.as_deref(), Some("Blobs"));
        assert_eq!(pack.avatar.as_deref(), Some("mxc://example.org/room"));
    }

    #[test]
    fn a_blank_name_is_no_name_at_all() {
        // Legal, and some bridges write one. The same rule `rooms::facts`
        // applies to an empty `m.room.name`.
        let pack = pack_of(
            "!blobs:example.org/".to_owned(),
            content(serde_json::json!({
                "images": { "blobcat": { "url": "mxc://example.org/cat" } },
                "pack": { "display_name": "   " },
            })),
            &a_room(),
        )
        .expect("a pack");

        assert_eq!(pack.name.as_deref(), Some("Blobs"));
    }

    #[test]
    fn a_packs_own_name_and_icon_win_over_the_rooms() {
        let pack = pack_of(
            "!blobs:example.org/".to_owned(),
            content(serde_json::json!({
                "images": { "blobcat": { "url": "mxc://example.org/cat" } },
                "pack": {
                    "display_name": "Blobcats",
                    "avatar_url": "mxc://example.org/icon",
                    "attribution": "drawn by somebody",
                },
            })),
            &a_room(),
        )
        .expect("a pack");

        assert_eq!(pack.name.as_deref(), Some("Blobcats"));
        assert_eq!(pack.avatar.as_deref(), Some("mxc://example.org/icon"));
        assert_eq!(pack.attribution.as_deref(), Some("drawn by somebody"));
    }

    #[test]
    fn a_pack_icon_somewhere_other_than_the_homeserver_falls_back_to_the_rooms() {
        // The same refusal the images get. An icon is an `img` too.
        let pack = pack_of(
            "!blobs:example.org/".to_owned(),
            content(serde_json::json!({
                "images": { "blobcat": { "url": "mxc://example.org/cat" } },
                "pack": { "avatar_url": "https://example.net/pixel.gif" },
            })),
            &a_room(),
        )
        .expect("a pack");

        assert_eq!(pack.avatar.as_deref(), Some("mxc://example.org/room"));
    }

    #[test]
    fn shortcodes_come_back_in_an_order_that_does_not_move() {
        // A JSON object has no order worth keeping, so the only answer that
        // does not rearrange a picker between two reads of the same pack is a
        // sorted one.
        let pack = read(serde_json::json!({
            "images": {
                "zebra": { "url": "mxc://example.org/z" },
                "blobcat": { "url": "mxc://example.org/c" },
                "apple": { "url": "mxc://example.org/a" },
            },
        }))
        .expect("a pack");

        assert_eq!(codes(&pack), ["apple", "blobcat", "zebra"]);
    }

    #[test]
    fn a_shortcode_that_breaks_the_grammar_is_drawn_anyway() {
        // MSC2545 is explicit: a client should show one so that whoever owns
        // the pack can see the mistake and fix it.
        let pack = read(serde_json::json!({
            "images": { "not a shortcode!" : { "url": "mxc://example.org/cat" } },
        }))
        .expect("a pack");

        assert_eq!(codes(&pack), ["not a shortcode!"]);
    }

    #[test]
    fn the_description_is_kept_and_is_not_the_shortcode() {
        // Two different jobs, which MSC2545 says outright: the shortcode names
        // the emoji and the body describes the picture.
        let pack = read(serde_json::json!({
            "images": {
                "blobcat": {
                    "url": "mxc://example.org/cat",
                    "body": "a lazy cat lays on the floor",
                },
            },
        }))
        .expect("a pack");

        assert_eq!(
            pack.images[0].body.as_deref(),
            Some("a lazy cat lays on the floor")
        );
        assert_eq!(pack.images[0].shortcode, "blobcat");
    }

    #[test]
    fn an_image_with_no_address_leaves_the_rest_of_the_pack_alone() {
        // Anybody can write a pack, so a malformed entry is a thing that
        // happens. Serde refuses the entry; the pack is still a pack.
        let content: PackContent = serde_json::from_value(serde_json::json!({
            "images": {
                "blobcat": { "url": "mxc://example.org/cat" },
                "broken": { "body": "no url at all" },
            },
        }))
        .expect("a pack with one bad entry still reads");

        let pack =
            pack_of("account".to_owned(), content, &Fallbacks::default()).expect("still a pack");

        assert_eq!(codes(&pack), ["blobcat"]);
    }
}
