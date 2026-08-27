// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the SDK knows about one room, in a form that can be built by hand.
//!
//! This layer exists for one reason: `matrix_sdk::Room` cannot be constructed
//! in a unit test. It is a handle into a live client's store, so any function
//! taking one is a function that can only be exercised against a homeserver.
//! Pulling the handful of facts out first leaves the grouping, the ordering
//! and the orphan detection as a pure function over plain data, which is where
//! the logic actually is and where the tests are worth writing.
//!
//! Everything here is local. [`extract`] runs once per joined room on every
//! sync that changed anything, so a network call in this file would turn an
//! idle client into a client that polls.

use std::collections::HashSet;

use matrix_sdk::Room;
use matrix_sdk::room::RoomMember;
use matrix_sdk::ruma::UserId;
use matrix_sdk::ruma::events::StateEventType;
use matrix_sdk::ruma::events::space::child::SpaceChildEventContent;
use matrix_sdk::ruma::room::RoomType;

use super::dto::Participant;

/// The MSC3417 room type marking a room as a call.
///
/// Matched as a string rather than against `RoomType::Call`, which sits behind
/// ruma's `unstable-msc3417` feature. This workspace does not enable it, and
/// enabling it would mean carrying a feature flag for a one-line comparison.
const CALL_ROOM_TYPE_UNSTABLE: &str = "org.matrix.msc3417.call";

/// What the MSC3417 room type is expected to become once it stabilises.
///
/// Accepted now so that the day a homeserver starts sending it, voice channels
/// do not silently turn into text channels.
const CALL_ROOM_TYPE_STABLE: &str = "m.call";

/// The room type marking a room as a space.
const SPACE_ROOM_TYPE: &str = "m.space";

/// The MSC3401 state event Element Call writes to say somebody is connected.
///
/// Never parsed here: the typed accessor does the reading, and this is only
/// used to tell an empty voice channel apart from a voice channel whose
/// membership arrived in a dialect this build does not understand. See
/// [`explain_an_empty_call`].
const CALL_MEMBER_STATE_UNSTABLE: &str = "org.matrix.msc3401.call.member";

/// What a room is, as far as the shell is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoomKind {
    /// A rail entry. Holds children rather than messages.
    Space,
    /// An ordinary room.
    Text,
    /// A MatrixRTC call room. What issue #6 connects to.
    Voice,
}

/// One joined room, reduced to what the shell needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoomFacts {
    pub(crate) id: String,
    /// Always known, because a joined room always resolves to something: its
    /// own name, a calculated one, or in the worst case its ID.
    pub(crate) name: String,
    pub(crate) avatar: Option<String>,
    pub(crate) kind: RoomKind,
    /// Empty unless this is a space.
    pub(crate) children: Vec<ChildFacts>,
    /// Who is connected to the call. Empty unless this is a voice channel.
    pub(crate) participants: Vec<Participant>,
}

/// One `m.space.child` event, reduced to what the ordering needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChildFacts {
    /// The state key: the room ID of the child. Not necessarily a room this
    /// account has joined, or has ever heard of.
    pub(crate) id: String,
    /// The `order` field, already validated by ruma, which drops it when it is
    /// not a string, is longer than fifty characters, or contains anything
    /// outside the printable ASCII range.
    pub(crate) order: Option<String>,
    /// When the `m.space.child` event was sent, in milliseconds. `None` for a
    /// stripped event, which cannot happen for a joined space and is carried
    /// as an absence rather than as a zero so the ordering can put it last.
    pub(crate) timestamp: Option<u64>,
}

/// Decide what a room is from its `m.room.create` room type.
///
/// Pure, and separated from [`extract`] because it is the one piece of voice
/// detection that can go wrong, and it is worth being able to test both
/// spellings and the absent case without a homeserver.
pub(crate) fn classify(room_type: Option<&str>) -> RoomKind {
    match room_type {
        Some(SPACE_ROOM_TYPE) => RoomKind::Space,
        Some(CALL_ROOM_TYPE_UNSTABLE | CALL_ROOM_TYPE_STABLE) => RoomKind::Voice,
        // No room type at all is the ordinary case: the overwhelming majority
        // of rooms in Matrix do not set one. An unrecognised type is a room
        // some other client understands and we do not, and showing it as an
        // ordinary room is better than hiding it.
        _ => RoomKind::Text,
    }
}

/// Everything the shell needs about one joined room.
pub(crate) async fn extract(room: &Room) -> RoomFacts {
    let kind = classify(room.room_type().as_ref().map(RoomType::as_str));

    RoomFacts {
        id: room.room_id().to_string(),
        name: name_of(room).await,
        avatar: room.avatar_url().map(|uri| uri.to_string()),
        kind,
        children: match kind {
            RoomKind::Space => children_of(room).await,
            RoomKind::Text | RoomKind::Voice => Vec::new(),
        },
        // A text room pays nothing for this. Both arms of the match are
        // reached on every sync that touched anything, so the cheap one has
        // to stay cheap.
        participants: match kind {
            RoomKind::Voice => participants_of(room).await,
            RoomKind::Space | RoomKind::Text => Vec::new(),
        },
    }
}

/// Who is connected to a voice channel, in the order they joined.
///
/// The read itself is [`Room::active_room_call_participants`], which is worth
/// being precise about because it does more than its signature suggests. It
/// reads `RoomInfo.base_info.rtc_member_events` out of memory, so there is no
/// request and nothing to fail. It keeps only memberships with application
/// `m.call` and scope `m.room`, which is what makes a MatrixRTC session a room
/// call rather than something else riding the same events. It drops expired
/// memberships at the moment of reading, against the clock rather than against
/// a cached decision, so the answer is never stale. And it returns oldest
/// first, which is both a sensible order to draw and a stable one, so the list
/// does not reshuffle under the pointer between renders.
///
/// What it does not do is deduplicate. A membership is per device, so somebody
/// on a laptop and a phone comes back twice, and that is this function's job.
/// It has to be done without disturbing the ordering, which is why it is a
/// seen set over the vector rather than a collect into one.
async fn participants_of(room: &Room) -> Vec<Participant> {
    let connected = room.active_room_call_participants();

    if connected.is_empty() {
        explain_an_empty_call(room).await;
        return Vec::new();
    }

    name_all(room, connected.iter().map(|user_id| user_id.as_str())).await
}

/// Name the people in `user_ids`, in the order given, once each.
///
/// Shared by the room-state path above and by the live roster a joined call
/// reports, which arrive from entirely different places and have to draw the
/// same. Both are per device before they get here: somebody on a laptop and a
/// phone appears twice in either source, and the deduplication has to happen
/// without disturbing the order, which is why it is a seen set over the
/// iterator rather than a collect into one.
pub(crate) async fn name_all<'a>(
    room: &Room,
    user_ids: impl Iterator<Item = &'a str>,
) -> Vec<Participant> {
    let mut seen = HashSet::new();
    let mut participants = Vec::new();

    for user_id in user_ids {
        if !seen.insert(user_id.to_owned()) {
            continue;
        }

        let Ok(parsed) = UserId::parse(user_id) else {
            // Not a user ID at all. Nothing local can be looked up under it,
            // and putting it on screen as somebody's name would be worse than
            // leaving it out.
            tracing::warn!(%user_id, room_id = %room.room_id(), "a call roster carried something that is not a user id");
            continue;
        };

        // Local. `get_member_no_sync` reads the store and, unlike its
        // requesting sibling, will not go and fetch the room's whole member
        // list because somebody joined a call.
        let name = match room.get_member_no_sync(&parsed).await {
            Ok(Some(member)) => name_of_member(&member),
            // In the call but not in the room, as far as this account has been
            // told. It happens when the membership arrives before the
            // `m.room.member` that explains it, and the next sync fixes it.
            Ok(None) => user_id.to_owned(),
            Err(error) => {
                tracing::warn!(%error, %user_id, room_id = %room.room_id(), "could not read a call participant");
                user_id.to_owned()
            }
        };

        participants.push(Participant {
            id: user_id.to_owned(),
            name,
        });
    }

    participants
}

/// What to call somebody in a voice channel.
///
/// The user ID when there is no display name at all. Unhelpful, and honest,
/// and better than the SDK's own fallback of the localpart, which drops the
/// server and so can show two different people the same way.
///
/// Two people who have chosen the same display name get theirs qualified,
/// which is the same thing every other Matrix client does and the reason
/// `name_ambiguous` exists. Without it, one of them is impersonating the
/// other in the only place the interface names them.
fn name_of_member(member: &RoomMember) -> String {
    let name = member
        .display_name()
        .map(str::trim)
        .filter(|name| !name.is_empty());

    match name {
        Some(name) if member.name_ambiguous() => format!("{name} ({})", member.user_id()),
        Some(name) => name.to_owned(),
        None => member.user_id().to_string(),
    }
}

/// Say why a voice channel is empty.
///
/// The one failure this whole feature has that is otherwise silent. Element
/// Call is moving MatrixRTC membership out of room state and into MSC4354
/// sticky events, and on the day this deployment follows,
/// [`Room::active_room_call_participants`] starts returning nothing at all. An
/// empty list is indistinguishable from an empty call, so the symptom would be
/// voice channels that quietly never show anybody again, with nothing on
/// screen or in the log to say why.
///
/// So: if the store holds the state events and the accessor still found
/// nobody, it is either every membership having expired, which is ordinary, or
/// the dialect having moved, which is not. The count is what lets whoever is
/// looking tell those apart. The fix, when it comes, is `unstable-msc4354` on
/// matrix-sdk and a second read through `active_rtc_member_stickies`.
///
/// The read is done whether or not anybody is listening, rather than behind a
/// level check. It is one indexed store query for a voice channel nobody is
/// in, which is the same local cost [`children_of`] already pays for every
/// space on every snapshot, and it is the only version of this that the tests
/// actually exercise: a level check is cached per callsite for the whole
/// process, so a test that turns the level up after another test has already
/// been through here changes nothing and quietly proves nothing.
async fn explain_an_empty_call(room: &Room) {
    let events = match room
        .get_state_events(StateEventType::from(CALL_MEMBER_STATE_UNSTABLE))
        .await
    {
        Ok(events) => events.len(),
        // The store failed, which is not worth a word of its own here: the
        // caller has already decided the channel is empty, and every other
        // read in this file logs its own failures.
        Err(_) => return,
    };

    if events > 0 {
        tracing::debug!(
            room_id = %room.room_id(),
            events,
            "a voice channel has call membership state but nobody active: either all of it has expired, or the membership dialect has moved on"
        );
    }
}

/// What to call a room.
///
/// Three local answers before the one that is not. `name` is the room's own
/// `m.room.name`, which most rooms in a space have and no direct message does.
/// `cached_display_name` is the SDK's calculated name, refilled on every
/// successful sync, which is what gives a direct message the other person's
/// name. Only if both are missing is the calculation forced, and in practice
/// that never happens: the cache is warm by the time anything asks.
///
/// An empty `m.room.name` is treated as no name at all. It is legal, some
/// bridges set it, and rendering a room with a blank label is worse than
/// falling through to a calculated one.
async fn name_of(room: &Room) -> String {
    if let Some(name) = room.name().filter(|name| !name.trim().is_empty()) {
        return name;
    }

    if let Some(name) = room.cached_display_name() {
        return name.to_string();
    }

    match room.display_name().await {
        Ok(name) => name.to_string(),
        // The store failed, which is not a reason to leave the room out of the
        // list. The ID is unhelpful and it is honest, and this is the only
        // branch that can produce it.
        Err(error) => {
            tracing::warn!(%error, room_id = %room.room_id(), "could not work out a room name");
            room.room_id().to_string()
        }
    }
}

/// The rooms a space says belong to it.
///
/// Two filters, both from the spec. A redacted `m.space.child` is how a child
/// is removed, so an event with no original content is not a child. And a
/// child with no `via` is explicitly not part of the space: without a server
/// to join through the entry is unusable, and the spec says to ignore it.
async fn children_of(room: &Room) -> Vec<ChildFacts> {
    let events = match room
        .get_state_events_static::<SpaceChildEventContent>()
        .await
    {
        Ok(events) => events,
        // A space whose children could not be read renders as a space with no
        // channels, which is visibly wrong in a way that a caller can report,
        // rather than as a missing space.
        Err(error) => {
            tracing::warn!(%error, room_id = %room.room_id(), "could not read a space's children");
            return Vec::new();
        }
    };

    events
        .iter()
        .filter_map(|raw| {
            let event = raw.deserialize().ok()?;
            // Stripped state belongs to invited rooms, and nothing here looks
            // at a room it has not joined, so this is the joined case only.
            let event = event.as_sync()?.as_original()?;

            if event.content.via.is_empty() {
                return None;
            }

            Some(ChildFacts {
                id: event.state_key.to_string(),
                order: event
                    .content
                    .order
                    .as_ref()
                    .map(|order| order.as_str().to_owned()),
                timestamp: Some(u64::from(event.origin_server_ts.0)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_is_a_space() {
        assert_eq!(classify(Some("m.space")), RoomKind::Space);
    }

    #[test]
    fn both_spellings_of_the_call_type_are_voice_channels() {
        // The unstable one is what the account being developed against
        // actually sends today. The stable one is what it becomes when
        // MSC3417 lands, and accepting it now is the difference between that
        // day being a non-event and voice channels silently becoming text
        // channels.
        assert_eq!(classify(Some("org.matrix.msc3417.call")), RoomKind::Voice);
        assert_eq!(classify(Some("m.call")), RoomKind::Voice);
    }

    #[test]
    fn a_room_with_no_type_is_an_ordinary_room() {
        assert_eq!(classify(None), RoomKind::Text);
    }

    #[test]
    fn a_room_type_nobody_here_understands_is_shown_rather_than_hidden() {
        assert_eq!(classify(Some("org.example.something")), RoomKind::Text);
    }

    #[test]
    fn a_type_that_merely_contains_call_is_not_a_voice_channel() {
        // Guards against anyone replacing the comparison with a `contains`,
        // which would make every room in a namespace ending in `.call` a
        // voice channel.
        assert_eq!(classify(Some("org.example.call.settings")), RoomKind::Text);
        assert_eq!(classify(Some("m.callisto")), RoomKind::Text);
    }
}
