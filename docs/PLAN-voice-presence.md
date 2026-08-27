# Plan: who is in the voice channel

Status: **done.** Phases 1 to 5 are built, tested and proved against a real
homeserver. What changed from the plan below is recorded at the end, under
[What actually happened](#what-actually-happened).

This is the first half of issue #6, carved off deliberately. The issue asks for
two things: a voice channel that connects the moment it is clicked, and a view
of who is already connected. The second one is worth building first, on its own,
because it needs none of the machinery the first one needs and it is the half
that can be tested by one person with a browser.

Nothing here joins a call. No LiveKit, no libwebrtc, no SFU token, no
`LocalSet`, no MSC4153 gate. Those all belong to the connect half and they are
all still ahead. What this builds is presence: Consort watches the room state
that Element Call already writes, and draws the people it finds there under the
voice channel, the way Discord does.

## What has to be true by the end

1. A voice channel with people in it shows them underneath it in the channel
   list, without anybody clicking the channel.
2. Each of them shows an avatar and the name that room knows them by. Not a
   user ID, unless there is genuinely nothing else.
3. Connecting from another client puts that person in Consort's list within one
   sync. Disconnecting takes them out again.
4. The same human connected on two devices appears once.
5. A membership that expires with no event to announce it disappears anyway.
6. It costs no extra network traffic. Not one request per sync, not one per
   voice channel.
7. A voice channel with nobody in it looks exactly as it does today.

## What the account actually looks like

Read out of `matrix-sdk-state.sqlite3` on this signed-in Consort, the same way
the room list numbers were, because the whole question here is which dialect of
MatrixRTC membership this deployment actually writes. Two exist and they are not
compatible.

- **27 `org.matrix.msc3401.call.member` state events**, spread over **exactly 3
  rooms**, and those 3 rooms are byte-for-byte the same set as the 3 rooms typed
  `org.matrix.msc3417.call`. The voice channels the room list already draws are
  the rooms the membership lands in. There is nothing to correlate.
- **Zero `m.rtc.member` sticky events.** This deployment's Element Call is on
  the pre-MSC4354 generation, which keeps membership in room state. That is the
  dialect this plan reads. See the risk section for what happens when it moves.
- **All 27 use the underscore state key** (`_@user:example.org_DEVICEID`), so
  these are the per-device session shape rather than the older array-of-
  memberships shape. Both parse; it is worth knowing which is in front of us.
- **25 of the 27 have `"content": {}`**, which is how that generation says
  someone left. Only 2 carry a live membership, and both are hours old.
- Both live ones read `application: "m.call"`, `scope: "m.room"`, `call_id: ""`,
  `expires: 14400000`. Four hours.
- The two contents differ slightly: one carries `membershipID`, the other
  carries `m.call.intent`. Two Element Call builds wrote them. Whatever we read
  has to tolerate that, and the typed path below does.

One more reading, and it is the one that matters most:

- **`base_info.rtc_member_events` is empty in all 26 persisted rooms.** That is
  the correct answer, not a broken path. The SDK drops a membership the moment
  it holds no live entries, and all 27 events here are either leaves (25) or
  expired (2). The field is `#[serde(skip_serializing_if = "BTreeMap::is_empty")]`,
  so its absence from the stored JSON means empty rather than missing.

The consequence: everything below is built against a store that currently says
"nobody is in any call", which is true. Proving it fills correctly needs a live
membership, and the only way to get one is to connect from Element Call. That is
phase 5, and it is the phase that actually decides whether this works.

## Where the data comes from

`matrix_sdk::Room` derefs to `matrix_sdk_base::Room`, which has this, unfeatured
and already in Consort's build:

```rust
pub fn has_active_room_call(&self) -> bool
pub fn active_room_call_participants(&self) -> Vec<OwnedUserId>
```

It is worth being precise about what that call does, because it is doing more
work than its signature suggests.

- It reads `RoomInfo.base_info.rtc_member_events`, which is **in memory**.
  Synchronous, no `await`, no request, nothing to fail.
- It filters on `is_room_call()`, which means `application: "m.call"` and
  `scope: "m.room"`. Both of the live events on this account match. A future
  non-room-scoped call would not, correctly.
- It filters expired memberships **at read time**, against
  `MilliSecondsSinceUnixEpoch::now()`. The data does not go stale; only our
  decision about when to re-read it can.
- It handles both content shapes, the legacy memberships array and the
  per-device session shape, through ruma's `active_memberships`.
- It returns **oldest membership first**, which is a usable ordering to draw and
  a stable one across renders.
- It returns **one entry per device**, so the same user twice is expected and
  deduplicating is our job.

Deliberately the typed accessor rather than reading
`org.matrix.msc3401.call.member` state ourselves. The gating on ruma's
`unstable-msc3401` lives inside matrix-sdk-base, where it is already switched
on at the matrix-rust-sdk workspace level, so Consort never has to name a
feature it does not own. This is the same problem `RoomType::Call` posed for the
room list, solved better: there, matching on `as_str()` was the only way out.
Here somebody has already wrapped it.

Names and avatars come from `Room::get_member_no_sync(user_id)`, which is also a
store read. `RoomMember::display_name()` gives the per-room name with
disambiguation already applied, and `RoomMember::avatar(MediaFormat)` gives the
image bytes. Per-room rather than global on purpose: a person can be "Tom" in
one room and something else in another, and the room they are in is the room we
are drawing.

## Why not matrix-rust-rtc yet

The sibling repo has a fuller answer to this, and it is the wrong tool for this
phase. `matrix_rtc_bridge::sdk` has `element_call_state_snapshot`, which reads
the same state events and translates them through `element_call_state`, and
`matrix_rtc_core::RtcSessionManager` will hand out
`watch::Receiver<Vec<JoinedMembership>>` per session. It is more correct than
what this plan builds, in ways that matter once media is involved: it resolves
which SFU a member is on, ranks the device that sent an event by whether it was
authenticated, and reconciles the two dialects when a room has both.

None of that is presence. Drawing a name under a channel needs the user ID and
nothing else, and every one of those functions is private to the bridge
(`element_call_state_snapshot` is `async fn`, not `pub`). Reaching them means
either taking `run_sticky_bridge`, which wants an `RtcSessionManager` and a
command sender that can write to the room, or upstreaming a new public entry
point. Both are the right move when Consort actually joins calls. Neither is
worth doing to render a list.

Worth recording for the connect half, since it was checked: `matrix-rtc-core`
depends on serde, tokio, log, and rand. `matrix-rtc-bridge` adds matrix-sdk and
nothing else. **libwebrtc enters through `matrix-rtc-livekit` alone.** The
signalling half of voice is cheap to build; only the media half is heavy.

## Phases

### Phase 1: read the participants into the snapshot

`crates/consort-matrix/src/rooms/dto.rs`
- `pub struct Participant { id: String, name: String }`. `name` is a plain
  `String`, not an `Option`, because unlike a channel a member always resolves
  to something: the display name, or the user ID, which is at least a name a
  human can recognise.
- `Channel` gains `participants: Vec<Participant>`, `#[serde(default)]`, empty
  for every text channel and for every empty voice channel.

`crates/consort-matrix/src/rooms/facts.rs`
- `RoomFacts` gains `participants: Vec<Participant>`.
- `extract` fills it only when `classify` said `RoomKind::Voice`. A text room
  pays nothing.
- A new `async fn participants_of(room: &Room) -> Vec<Participant>`:
  `active_room_call_participants()`, dedupe by user ID preserving first-seen
  order, then `get_member_no_sync` each one for a display name, falling back to
  the user ID when the member is not in the store.
- The dedupe is why this is not a one-liner. Oldest-first order has to survive
  it, so it is a seen-set over the vec rather than a collect into a set.

`crates/consort-matrix/src/rooms/snapshot.rs`
- Carry `participants` through `assemble` onto the `Channel`. Unjoined channels
  get an empty vec: we cannot see the state of a room we are not in, and
  pretending otherwise would be a lie the interface would draw.

Tests, in `against_a_mock_homeserver.rs` under a new `mod voice_presence`, using
the existing `state_event` and `sync_with` helpers:
- a live membership in a call room becomes one participant
- `"content": {}` is a leave and yields nobody
- an expired `expires` yields nobody
- two devices for one user yield one participant
- a membership in a text room is ignored, because `is_room_call` is not the only
  thing that should be true before we draw it
- oldest membership is first
- a member with no display name falls back to the user ID

### Phase 2: draw them

This is the phase that makes the browser test possible, so it comes before
avatars.

`app/src/lib/api.ts`: `Participant` type, `participants` on `Channel`.

`app/src/components/ChannelList.tsx`: under a voice `ChannelRow` whose
`participants` is non-empty, a `<ul>` of participants, each an avatar slot plus
a name. Omitted entirely when empty, the same way `Group` is omitted when empty,
so a quiet voice channel keeps exactly the shape it has today.

`app/src/components/ChannelList.css`: indented under the row, smaller type,
avatar at around 20px. The list must not become a scroll trap or push the
channel list into an awkward height, so it wants a real look at what 8 people in
one channel does to the 240px column.

Initials only at this point, through the existing `initialsOf`. Avatars are the
next phase and this one should be testable without them.

Vitest: a voice channel with participants renders them, one without renders no
list, a text channel with participants somehow set renders no list.

### Phase 3: avatars

`crates/consort-matrix/src/rooms/avatar.rs`: `pub async fn member_avatar(client,
room_id, user_id) -> Option<String>`, alongside the existing room `avatar`.
Takes the room ID as well as the user ID because the avatar is the per-room one.
Reuses `image_type` and the same 96px crop thumbnail, so a member avatar and a
room avatar are fetched and sniffed identically.

`app/src-tauri/src/commands.rs`: `member_avatar_for` plus the `#[tauri::command]
member_avatar` delegate, registered in `lib.rs`.

`app/src/lib/avatars.ts`: generalise. Today it is two maps keyed by room ID and
a hardcoded call to `roomAvatar`. It becomes keyed by an arbitrary string with
the fetch supplied, so `${roomId}/${userId}` is a key like any other and the
one-request-per-id and shared-in-flight guarantees still hold. `resetAvatarCache`
keeps working for tests.

`app/src/components/RoomAvatar.tsx` already takes a size custom property and
already falls back to an initial, so the participant row uses it as-is rather
than growing a second avatar component.

### Phase 4: notice an expiry

Joins and leaves both write a state event, so the existing room watcher already
catches them: a call member state event is a room update, `RoomUpdates::is_empty()`
is false, and the snapshot re-derives. That is phases 1 to 3 done.

An expiry writes nothing. A client that is killed rather than closed leaves a
membership behind that says `expires: 14400000`, and Consort would draw that
person in the channel for up to four hours after their laptop shut.

`crates/consort-matrix/src/rooms/mod.rs`: add a timer to the wait in `watch`, so
the loop wakes on a room update **or** a tick. The tick only arms when at least
one voice channel currently has participants, which means an account with nobody
in any call arms no timer and an idle Consort still does nothing. Since the read
is in-memory, the tick costs a snapshot and no network, and the existing
`Changes` guard means an unchanged snapshot emits no event to the webview.

Thirty seconds is the interval matrix-rust-rtc's bridge chose for the same
problem in the same dialect, and there is no reason to disagree with it.

Tests: a mock test that a membership expiring with no further sync eventually
drops out of the emitted snapshot, and one that an account with no occupied
voice channel does not spin.

### Phase 5: prove it against the real account

The only phase that can actually fail in an interesting way, because everything
before it is tested against sync JSON that this plan wrote.

Run the dev build signed into the real account. Join one of the three voice
channels from Element Call in a browser. Consort should show that user under
that channel within a sync. Leave; they should go. Join from a second device;
still one entry.

Add a real-homeserver ignored test in `against_a_real_homeserver.rs` alongside
the room list ones, driving both sides from Rust so it is repeatable without a
browser: a second account writes an `org.matrix.msc3401.call.member` state event
into a call room, and the first account's snapshot grows a participant.

Then update the README roadmap row and this file's status.

## Risks

**The dialect moves.** The single real risk. When this deployment's Element Call
updates past MSC4354, membership stops being room state and becomes sticky
events, `rtc_member_events` stays empty forever, and every voice channel silently
shows nobody. Silent is the bad part: an empty list is indistinguishable from an
empty call. Mitigation, cheap and worth doing in phase 1: a `debug!` when a call
room has `org.matrix.msc3401.call.member` state events in the store but
`active_room_call_participants()` returns empty, which is the exact shape of the
failure. The fix when it happens is `unstable-msc4354` on Consort's matrix-sdk
and `Room::active_rtc_member_stickies()`, which returns the same thing from the
other dialect. That is a feature flag and a second read, not a redesign.

**Members not in the store.** `get_member_no_sync` reads what sync has
delivered. There are 124 `m.room.member` events across 26 rooms here, so the
people in these rooms are known, but a participant who joined the call before
Consort ever saw them in the room would fall back to a user ID. Acceptable, and
correct: showing a raw ID is honest, and the next sync fixes it.

**The list gets long.** Discord's channel list handles this by just being tall.
Eight people under one channel in a 240px column is worth looking at with real
data before deciding whether it needs a cap.

**Ghost after a crash.** Phase 4 bounds it to thirty seconds plus the four hour
`expires`. Nothing can do better without the SFU telling us, which is the
connect half.

## What this deliberately does not do

- Join a call. That is the rest of #6.
- Show who is speaking, muted, or deafened. Membership state carries none of it.
  `m.call.intent` says audio or video was intended, not what is happening now.
  Live state needs the SFU connection.
- Read the MSC4354 sticky dialect. See the risks.
- Touch verification, MSC4153, or `lk-jwt-service`. Those gate joining, and
  nothing here joins.

## What actually happened

All five phases landed as written. Six things are worth recording because they
are not what the plan above says.

**The state key trap.** The mock fixtures derive an event ID from the event's
own type and state key. A call membership state key is `_@user:server_DEVICE`,
so the derived ID carried a colon followed by an underscore, ruma read
everything after the colon as a server name, rejected it, and dropped the whole
event. The membership simply never arrived, with nothing logged. `state_event`
now strips the derived ID down to letters and digits, which fixes it for every
fixture rather than just this one.

**Names are disambiguated here, not by the SDK.** The plan assumed
`display_name()` already qualified two people who chose the same name. It does
not: it returns the raw display name, and `name_ambiguous()` is a separate
question. So `name_of_member` asks it and appends the user ID when the answer is
yes. Without that, one of them is impersonating the other in the only place the
channel names either.

**The diagnostic is not gated on the log level.** The first attempt put the
store query behind `tracing::enabled!(Level::DEBUG)`, so it cost nothing at the
default filter. That turned out to be untestable in a way worth writing down:
tracing caches, per callsite, whether anybody could ever be interested, and
another test in the same binary reaches that line first with no subscriber
installed. A later test that raises the level, even with
`rebuild_interest_cache`, does not undo the cached answer, so the test passed
while the diagnostic never ran. Proving that took running the test alone and
seeing the line appear, then running the binary and seeing it not.

So the gate is gone. The read happens whenever a voice channel is empty: one
indexed store query, the same local cost `children_of` already pays for every
space on every snapshot, and now exercised by every test that ends with an
empty channel rather than by none of them.

**`RoomAvatar` grew a `userId` rather than a sibling.** The plan said reuse it
as-is, which was almost right: it needed one optional prop and an `avatar` that
may be absent, because the room list carries an `mxc://` hint for a room and
nothing at all for a person. So a room with no avatar is still never asked
about, and a person always is.

**Phase 4 is tested through `wait` rather than through the clock.** A
membership expires against the wall clock, which no test can move, and the poll
is thirty seconds, which no test should wait for. Splitting the decision
(`poll_after`) from the wait (`wait`) makes both directly testable: that an
account with nobody in a call arms no timer, that an occupied one does, that the
timer ends the wait when no sync does, and that a sync still wins while it is
armed.

**Phase 5 is repeatable without a browser.** Two ignored tests against a real
Synapse, both passing: a second account joins a public call room and writes the
membership, and this account's channel grows them; then the same account writes
an empty content, and the channel empties without losing the channel. The call
room is created with `org.matrix.msc3401.call.member` at power level zero,
which is not a test convenience: Element Call sets exactly that, because
otherwise an ordinary member cannot announce that they have connected.

