// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The wire types for the room list.
//!
//! One value describes the whole tree: every rail entry, and every channel
//! under each of them. The alternative, telling the frontend that a room was
//! added or renamed and letting it patch its own copy, is where room lists go
//! wrong. Twenty-six rooms serialise to a few kilobytes, and a value that is
//! always complete is a value a late subscriber can be handed as-is.
//!
//! None of the SDK's own types appear here, for the same reason they do not
//! appear in the verification DTOs: the wire format is a contract with
//! `app/src/lib/api.ts`, and pinning it to an upstream type means an SDK bump
//! can silently change what the webview receives.

use serde::{Deserialize, Serialize};

/// The rail entry holding rooms that belong to no joined space.
///
/// A real room ID always begins with `!`, so this string cannot collide with
/// one. That is the whole reason it is a plain `String` rather than an
/// `Option<String>` the frontend has to unwrap at every key and comparison.
pub const HOME_ID: &str = "home";

/// What the Home entry is called.
const HOME_NAME: &str = "Home";

/// Everything the shell draws.
///
/// Home is always present and always first, even when it is empty: the button
/// is part of the furniture, and a rail whose first entry moves depending on
/// whether the account has direct messages is a rail that jumps around.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rooms {
    pub spaces: Vec<Space>,
}

/// One entry in the left rail, and the channels underneath it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    /// The room ID, or [`HOME_ID`].
    pub id: String,
    pub name: String,
    /// An `mxc://` URI, not image bytes. Fetching the bytes is a separate
    /// command, because putting them here would multiply a small payload by
    /// the number of rooms and re-send all of it every time one is renamed.
    pub avatar: Option<String>,
    /// Sorted. See `snapshot::assemble` for the order and why it is that one.
    pub channels: Vec<Channel>,
}

impl Space {
    /// The Home entry, holding the rooms that belong to no joined space.
    pub(crate) fn home(channels: Vec<Channel>) -> Self {
        Self {
            id: HOME_ID.to_owned(),
            name: HOME_NAME.to_owned(),
            avatar: None,
            channels,
        }
    }
}

/// One room under a rail entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    /// `None` only when the room is listed by a space and has never been
    /// joined, so nothing local knows what it is called. A joined room always
    /// has a name, because the SDK calculates one when the room does not carry
    /// its own.
    ///
    /// Modelled as an absence rather than as the room ID standing in for a
    /// name, so that the interface cannot show somebody `!AbCdEf...` by
    /// forgetting a check.
    pub name: Option<String>,
    /// The room's `m.room.topic`, when it has one worth drawing.
    ///
    /// Absent rather than null when there is none, and absent for a room a
    /// space lists that this account has not joined, whose state it cannot
    /// read. A blank topic is treated as no topic, for the reason a blank name
    /// is: some bridges set one, and a subtitle that is an empty line is worse
    /// than no subtitle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub kind: ChannelKind,
    pub avatar: Option<String>,
    /// False for a room a space lists that this account has not joined. Those
    /// are shown, and shown as unavailable, because hiding them makes Consort
    /// disagree with every other client about how many channels a space has.
    pub joined: bool,
    /// Who is connected to this voice channel right now, oldest membership
    /// first.
    ///
    /// Always empty for a text channel, for a voice channel nobody is in, and
    /// for a channel this account has not joined: the state of a room we are
    /// not in is a room we cannot see, and guessing at it would be a lie the
    /// interface would draw.
    ///
    /// Defaulted so that a snapshot serialised before this field existed still
    /// reads back, which is what the round-trip test relies on.
    #[serde(default)]
    pub participants: Vec<Participant>,
}

/// One person connected to a voice channel.
///
/// A membership is per device, so the same human on a laptop and a phone is
/// one of these and not two. See `facts::participants_of`, which does that
/// deduplication, for why it is done there rather than here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    /// The user ID. Also half of the key their avatar is fetched by, the other
    /// half being the room, because a person can have a different avatar in
    /// every room they are in.
    pub id: String,
    /// What to call them.
    ///
    /// A plain `String` rather than an `Option`, unlike [`Channel::name`],
    /// because there is always an answer: a display name, or failing that the
    /// user ID, which is still something a person recognises. There is no case
    /// where the interface would have nothing to draw.
    pub name: String,
    /// Whether they have muted themselves.
    ///
    /// Only ever true for somebody in the call this session is also in. It
    /// comes from the media layer, which learns it from the SFU, and room state
    /// carries nothing like it: a person listed from `m.rtc.member` alone is
    /// reported unmuted because nothing there could say otherwise, not because
    /// anything checked.
    ///
    /// `#[serde(default)]` so the field can be left out. Every construction
    /// site that has no answer says so by omission rather than by writing down
    /// a `false` that looks like a finding.
    #[serde(default)]
    pub muted: bool,
    /// Whether they have stopped listening to the call.
    ///
    /// Only ever true of another Consort client, and not because of a
    /// limitation worth apologising for: deafening is built out of one
    /// session's own subscriptions and nothing in MatrixRTC or LiveKit has a
    /// name for it, so Consort clients tell each other over the call's data
    /// channel and nobody else is listening. See `consort_call::notices`.
    ///
    /// Implies [`muted`](Self::muted), because deafening mutes. Kept separate
    /// so an interface can say which of the two somebody chose.
    #[serde(default)]
    pub deafened: bool,
    /// Whether they have said they are not at their computer.
    ///
    /// Carried the same way `deafened` is, over the call's data channel and
    /// between Consort clients only, and true under the same rule: every one
    /// of their memberships said so. Somebody away on a laptop who is at their
    /// phone is at their computer.
    ///
    /// Implies the microphone is off, and deliberately implies nothing about
    /// [`deafened`](Self::deafened). Still hearing the call is the entire
    /// difference between walking away and leaving.
    #[serde(default)]
    pub away: bool,
    /// Whether a camera of theirs is live.
    ///
    /// True only for somebody publishing a camera that is not muted, which on
    /// the wire is the same test the microphone gets, applied to the other
    /// stream. Somebody on two devices is on camera if either of them is: the
    /// opposite of the [`muted`](Self::muted) rule, and for the same reason
    /// behind it, which is that the answer should describe what the call can
    /// actually see and hear rather than what one device happens to be doing.
    ///
    /// False where nothing knows, exactly like `muted`, and with the same
    /// caveat: somebody listed from room state alone is reported without a
    /// camera because room state carries nothing that could say otherwise.
    #[serde(default)]
    pub camera: bool,
    /// When they joined the call, in milliseconds since the Unix epoch.
    ///
    /// The SFU's own record rather than the moment this session noticed them,
    /// so it is still right for people who were already in the call when we
    /// arrived. `None` for somebody listed from room state alone, for somebody
    /// whose media has not appeared yet, and against a server too old to
    /// report it.
    ///
    /// Ourselves excepted, and only ourselves. An arrival is something the SFU
    /// watches other people do, so it has nothing to say about the one client
    /// that was already here, and the call fills that row in from the moment
    /// its own join returned.
    ///
    /// Somebody on two devices joined when the first of them did.
    ///
    /// Deliberately not "when they joined the room". That is answerable from
    /// their membership event, but the event carries the timestamp of their
    /// *last* profile change, so it means "when they last picked a new
    /// avatar", which is not a fact worth putting under somebody's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
}

impl Participant {
    /// Somebody nothing is known about beyond who they are.
    ///
    /// Which is every source but one: room state says who is in a channel and
    /// nothing else about them, and only the live roster of the call this
    /// session is sitting in can say whether somebody has muted themselves.
    pub fn named(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            muted: false,
            deafened: false,
            away: false,
            camera: false,
            since: None,
        }
    }

    /// The same person, with what the media layer says about them.
    pub fn with_muted(self, muted: bool) -> Self {
        Self { muted, ..self }
    }

    /// The same person, with what their own client said about them.
    pub fn with_deafened(self, deafened: bool) -> Self {
        Self { deafened, ..self }
    }

    /// The same person, with whether their own client says they are there.
    pub fn with_away(self, away: bool) -> Self {
        Self { away, ..self }
    }

    /// The same person, with whether the call can see them.
    pub fn with_camera(self, camera: bool) -> Self {
        Self { camera, ..self }
    }

    /// The same person, with when they joined the call.
    pub fn with_since(self, since: Option<u64>) -> Self {
        Self { since, ..self }
    }
}

/// Which column a channel belongs in.
///
/// Two variants and no `Unknown`. A room that does not announce itself as a
/// call is a text room, which is what every client already assumes and what
/// the spec implies by having no room type at all for the ordinary case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelKind {
    Text,
    Voice,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> Channel {
        Channel {
            id: "!a:example.org".to_owned(),
            name: Some("general".to_owned()),
            topic: None,
            kind: ChannelKind::Text,
            avatar: None,
            joined: true,
            participants: Vec::new(),
        }
    }

    #[test]
    fn home_cannot_collide_with_a_room_id() {
        // Every room ID in Matrix begins with a sigil. If that ever stopped
        // being true, Home would start shadowing somebody's room.
        assert!(!HOME_ID.starts_with('!'));
    }

    #[test]
    fn each_kind_is_tagged_the_way_the_frontend_reads_it() {
        let tag = |kind: ChannelKind| serde_json::to_string(&kind).unwrap();

        assert_eq!(tag(ChannelKind::Text), "\"text\"");
        assert_eq!(tag(ChannelKind::Voice), "\"voice\"");
    }

    #[test]
    fn a_channel_with_no_known_name_sends_null_rather_than_its_room_id() {
        let unknown = Channel {
            name: None,
            joined: false,
            ..channel()
        };

        let json = serde_json::to_value(&unknown).unwrap();

        assert!(json["name"].is_null());
        assert_eq!(json["joined"], false);
    }

    #[test]
    fn a_channel_with_nobody_in_it_still_sends_a_list() {
        // Not `null`, and not an absent key. The frontend maps over this
        // without a guard, and an empty array is the shape that lets it.
        let json = serde_json::to_value(channel()).unwrap();

        assert_eq!(json["participants"], serde_json::json!([]));
    }

    #[test]
    fn a_channel_with_no_topic_leaves_the_key_out() {
        // Absent rather than null, so the frontend's `topic?: string` is the
        // whole of what it has to check.
        let json = serde_json::to_value(channel()).unwrap();

        assert!(json.get("topic").is_none());
    }

    #[test]
    fn a_channel_with_a_topic_sends_it() {
        let json = serde_json::to_value(Channel {
            topic: Some("Where the good links go".to_owned()),
            ..channel()
        })
        .unwrap();

        assert_eq!(json["topic"], "Where the good links go");
    }

    #[test]
    fn a_channel_from_before_participants_existed_reads_back_empty() {
        // The field is defaulted rather than required, so a snapshot written
        // by an older build is still a snapshot.
        let json = serde_json::json!({
            "id": "!a:example.org",
            "name": "general",
            "kind": "text",
            "avatar": null,
            "joined": true,
        });

        let channel: Channel = serde_json::from_value(json).unwrap();

        assert!(channel.participants.is_empty());
    }

    #[test]
    fn the_whole_tree_survives_a_round_trip() {
        let rooms = Rooms {
            spaces: vec![
                Space::home(vec![channel()]),
                Space {
                    id: "!space:example.org".to_owned(),
                    name: "Kahu HQ".to_owned(),
                    avatar: Some("mxc://example.org/abc".to_owned()),
                    channels: vec![Channel {
                        kind: ChannelKind::Voice,
                        participants: vec![Participant::named("@bob:example.org", "Bob")],
                        ..channel()
                    }],
                },
            ],
        };

        let json = serde_json::to_string(&rooms).unwrap();
        let back: Rooms = serde_json::from_str(&json).unwrap();

        assert_eq!(back, rooms);
    }
}
