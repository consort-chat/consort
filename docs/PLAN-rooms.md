# Plan: the room list and the app shell

Status: **Phases 1 to 3 done, Phase 4 next.** Every API named here was checked
against the pinned SDK rev rather than recalled, and the file and function
names are the ones to create. The numbers under "What the account actually
looks like" were read out of the local state store of a signed-in Consort, not
assumed.

This is issue #5, and it is the milestone between verification and voice. Issue
#6 wants a voice channel that connects the moment it is clicked, which means
something has to draw a voice channel first. Everything here exists to put that
thing on screen.

It is also the point where Consort stops being a login screen with a status
card attached and starts being shaped like a chat client. The signed-in view
today is one centred panel showing who you are and whether the sync loop is
alive. By the end of this it is a three-column shell: a rail of spaces, a list
of that space's channels, and a main pane that is empty until text messaging
lands.

## What has to be true by the end

1. Signing in shows a left rail with one icon per joined space, plus a Home
   button for rooms that belong to no joined space.
2. Selecting a rail entry lists that space's channels, split into text and
   voice, in a stable order that does not shuffle between launches.
3. A voice channel is visibly a voice channel before anyone clicks it. Issue #6
   needs a target that is already identified as one.
4. Spaces and channels show their avatars, and fall back to initials rather
   than to a blank square when they have none.
5. The account panel, the connection state, the verification banner and sign
   out all survive the move into the new layout. None of them may be quietly
   dropped on the way.
6. The list follows the account. Joining a room, leaving one, or renaming one
   updates the shell without a restart.

## What the account actually looks like

Read out of `matrix-sdk-state.sqlite3` on a signed-in Consort, because guessing
at the shape of real data is how a room list ends up correct only on the
developer's own account.

Twenty-six joined rooms. Of those:

- **One space.** It has an avatar and it lists twenty children.
- **Eighteen of those twenty children are joined.** Two are listed by the space
  and have never been joined, so nothing local knows their names.
- **Three of the joined children are `org.matrix.msc3417.call` rooms.** Those
  are the voice channels. The rest are ordinary rooms.
- **Seven rooms are in no space at all**, and all seven are direct messages.
  They have no `m.room.name` and the SDK has already calculated a display name
  for each from the other member.
- **Not one child carries an explicit `order`.** Every one of them falls
  through to the ordering fallback, so the fallback is the ordering, not an
  edge case.
- Most rooms have an avatar. Four do not.
- Two different rooms share the name "Private Room", and one of them is a call
  room and the other is not. Names are not identifiers here, not even within
  one space.

Two things fall out of that. Home is a direct-message list on this account,
which is exactly what Home is in Discord, so the analogy holds rather than
being stretched. And the ordering fallback is on the critical path: get it
wrong and every channel on the account is in the wrong place.

## Risks, largest first

**The layout change is larger than the feature.** `SignedIn.tsx` is 445 lines
and is currently the whole signed-in application. The room list is roughly
three new files; rehousing what is already there is the rest of the work, and
it touches the verification banner, which is the most safety-critical thing on
screen. A banner that says nothing because it was moved into a container that
does not render is worse than no banner.

**Two children are listed but not joined.** The space knows their room IDs and
nothing else. Rendering a channel called `!AbCdEf...` is not acceptable and
silently hiding two of twenty channels is a list that disagrees with Element.
This needs a decision rather than a default, and it is taken below.

**Voice channel detection rests on an unstable room type.**
`org.matrix.msc3417.call` is an MSC prefix, and `RoomType::Call` in ruma sits
behind a `unstable-msc3417` feature this workspace does not enable. Matching on
the string is the honest approach and it has to accept the stable `m.call`
spelling too, so that the day the MSC lands the channel does not turn into a
text channel.

**Frontend coverage thresholds are 90%.** Not a risk to correctness, a risk to
the estimate. Four new components with selection state and an async avatar
fetch is a real amount of test code, and `pnpm test:coverage` fails the build
below the line.

**The snapshot recomputes on every sync.** Twenty-six rooms is nothing, but the
work is proportional to the number of joined rooms and it runs every thirty
seconds forever. It must stay cheap and local. Any accidental network call
inside the snapshot, and the most tempting one is resolving an unjoined child's
name, turns an idle client into a client that polls.

## Design decisions taken up front

**The whole tree goes over the wire on every change.** The obvious alternative
is incremental updates: this room was added, that one renamed. It is wrong
here. Twenty-six rooms serialise to a few kilobytes, the diffing that
incremental updates need is exactly the bug surface a room list is famous for,
and the existing `LatestSink` already gives a late subscriber the current state
for free only because state is a value rather than a stream of edits. One
`Rooms` value, emitted when it differs from the last one.

**Home is a room ID that cannot collide.** Every Matrix room ID begins with
`!`, so the literal string `home` is available as a rail entry ID and can never
be a real room. This beats an `Option<String>` ID that the frontend has to
special-case at every key and every comparison.

**The pure part takes facts, not `Room`s.** `matrix_sdk::Room` cannot be
constructed in a unit test, so a snapshot function that takes `Vec<Room>` is a
snapshot function that is only testable against a live homeserver. An
intermediate `RoomFacts` is extracted from each `Room` in an async pass, and a
pure `assemble(Vec<RoomFacts>) -> Rooms` does the grouping, the ordering and
the orphan detection. That pure function is the most valuable unit test in this
milestone, the same way the DTO mapping was in the last one.

**Orphans are found from the space side, never from the room side.**
`Room::parent_spaces()` exists and is the wrong tool: it is async, it is
per-room, and it validates parents against state we may not have. Collecting
every `m.space.child` state key across the joined spaces gives the set of
claimed rooms in one local pass, and Home is every joined non-space room
outside that set. It is also the more correct question, because a room whose
only parent is a space we have not joined has no rail icon to live under and
belongs in Home regardless of what it claims.

**Unjoined children are shown, greyed, and named by the space.** Hiding them
makes Consort disagree with Element about how many channels a space has. The
`m.space.child` event does not carry a name, but the space's own
`/hierarchy` response does, and it is one request per space, cached, refreshed
only when the child set changes. This is the one place a network call is
allowed, and it is deliberately outside the per-sync snapshot path. If the
request fails, those entries are omitted rather than rendered as room IDs.

**Ordering follows MSC1772 exactly, then groups by kind.** Sort by `order` when
present, then `origin_server_ts`, then room ID, which is what the spec says and
what Element does. Then split the sorted list into text and voice for display.
Grouping after sorting rather than before is what keeps the two columns stable
when a room moves between them.

**Voice detection matches the string, both spellings.**
`org.matrix.msc3417.call` and `m.call`. `RoomType` is a string enum with
`as_str()`, so this needs no feature flag and no SDK bump.

**Avatars are fetched by a command, one room at a time.** The snapshot carries
the `mxc://` URI and no bytes. A `room_avatar` command returns a thumbnail as a
data URL, and the SDK's media store, which is already on disk, makes the second
call free. Putting image bytes in the snapshot would multiply a 5 KB payload by
a hundred and re-send all of it every time a room is renamed.

**The verification banner stays in the main pane.** Discord's user panel is
sixty pixels tall and the banner is the one piece of UI that tells someone
their messages cannot be decrypted. It does not get compressed into a corner
because the mockup has a corner. The user panel takes the name, the avatar, the
connection dot and sign out.

**Rooms live in `consort-matrix`, not in the Tauri layer.** Same split as
`verification` and `backup`. Commands stay one-line delegates.

## Phases

### Phase 1: the snapshot (done)

`crates/consort-matrix/src/rooms/` as a directory, matching `verification/`:

- `dto.rs`: `Rooms`, `Space`, `Channel`, `ChannelKind`, and `HOME_ID`. A
  channel's `name` is an `Option<String>` rather than the room ID standing in
  for a name, so the interface cannot show somebody `!AbCdEf...` by forgetting
  a check.
- `facts.rs`: `RoomFacts`, `ChildFacts`, `classify`, and the async extraction
  from `matrix_sdk::Room`. Name resolution is `name()`, then
  `cached_display_name()`, then `display_name().await`, then the room ID. The
  first three are local; the fourth never happens in practice and exists so
  the type has no hole. A child with no `via`, and a redacted `m.space.child`,
  are both not children, which is what the spec says and what leaving a room
  actually looks like on the wire.
- `snapshot.rs`: the pure `assemble`. Grouping, MSC1772 ordering, orphan
  detection. Channels come out as one sorted list carrying a `kind` rather
  than as two lists, because the two columns are a rendering decision and
  sorting once is what keeps a channel in the same place relative to its
  neighbours when it changes type.
- `mod.rs`: `snapshot(client)`, and `watch(client, on_change) -> JoinHandle<()>`
  driven by `Client::subscribe_to_all_room_updates()`, which the SDK sends once
  per sync whether or not anything happened. An update touching no room is
  skipped without reading the store, a `Lagged` receive means recompute now,
  and it emits once at startup, because the first sync may already have
  happened by the time the task is spawned.

Done: `cargo test -p consort-matrix` covers, against hand-built facts, a room
in no space landing in Home, a room in a joined space not landing in Home, a
child listed by a space we have not joined landing in Home anyway, both call
type spellings becoming voice channels, an `order` beating a timestamp, a
timestamp beating a room ID, a subspace getting a rail entry rather than
becoming a channel of its parent, and two rooms with the same name staying two
rooms. `MatrixMockServer` covers a room arriving in a sync reaching the list,
and an idle sync not being reported twice. A real Synapse covers a real space
holding a real `org.matrix.msc3417.call` room, which is the one assertion no
mock makes convincing.

### Phase 2: over the wire (done)

- `AppEvent::Rooms` on the existing enum in `events.rs`, channel name `rooms`,
  and `is_worth_keeping` returning true so `resend_state` replays it. It is the
  starkest case for that: the thing that changes a room list next may be days
  away, so a webview that missed the last one would sit on an empty shell until
  somebody joined or left something.
- `rooms::watch` spawned in `state.rs` next to `sync::start`,
  `verification::watch` and `backup::watch`, in the same kind of `TaskSlot` and
  torn down the same way on sign-out. Unlike the verification channels it gets
  a parting word, an empty `Rooms`, because the retained value names somebody's
  rooms and signing in as a second account would otherwise show the first
  account's spaces until the new watcher reported.
- `room_avatar(room_id)` returning `Option<String>`, a data URL. It asks for a
  96 pixel cropped thumbnail, which is one of Synapse's default sizes, and
  sniffs the image type from the magic number because the SDK's media API hands
  back bytes and no content type. Every failure is `None` and a log line,
  because the fallback is initials and a dialog about an avatar would be worse
  than the initials.
- The mirrored types, `onRooms` and `roomAvatar` in `app/src/lib/api.ts`. A
  channel's `name` is `string | null` there too, so a caller has to decide what
  an unjoined child looks like rather than accidentally rendering a room ID.

Done: a mock homeserver covers a whole space arriving in a sync response, with
a call room, an ordinary room, a child that was never joined and a child with
no `via`, plus the avatar path end to end including the bytes that are not an
image. Vitest covers the channel name, the payload shape and the argument
names. The signed-out half is confirmed in the dev build: it starts, reports no
rooms, and says so.

Not yet confirmed: the counts against the real account, because there is no
session on this machine to restore. The same code path is covered against a
real Synapse in `against_a_real_homeserver.rs`, and the account check is worth
doing once there is a shell drawing it.

### Phase 3: the shell (done)

`AppShell.tsx` replaces what `SignedIn.tsx` renders: a three-column grid, and
the existing verification banner and connection state moved rather than
rewritten. `SignedIn.tsx` keeps every subscription it owned and hands their
values down, which is the split worth having: the listeners are the part with a
lifetime to get wrong, and the layout is the part that changes every time the
design does.

Three files came out of the old 445-line component. `VerificationBanner.tsx`
moved wholesale. `UserPanel.tsx` is new and holds the avatar, the name, the
connection dot and sign out. `AppShell.tsx` is the layout and the main pane.

One thing genuinely changed rather than moving. The account name was this
screen's `h1`, which was true when the screen was a centred card and is not
true of a thirty-two pixel strip in a corner. The heading moved to the main
pane, where it currently says there is nothing there, and will say the name of
the selected channel once there is one. The account strip became a labelled
group instead, which announces itself to somebody arriving by keyboard and
gives the tests a stable anchor that is not a heading it is not.

Done: all 162 frontend tests pass at 100% statements, branches, functions and
lines, and every piece of the old signed-in view is still reachable. What is
not yet done is looking at it: there is no session on this machine to restore,
so the shell has not been on screen.

### Phase 4: the rail and the channel list (next)

- `SpaceRail.tsx`: Home first, then spaces. Selected state, hover state, the
  Discord pill on the active entry.
- `ChannelList.tsx`: the space name as a header, then TEXT and VOICE groups.
  Unjoined children greyed and not selectable.
- `RoomAvatar.tsx`: the mxc-to-data-URL fetch, an in-memory cache keyed by room
  ID, and initials as the fallback. Used by both of the above.
- `UserPanel.tsx`: avatar, name, connection dot, sign out.

Done when the account's real space, its eighteen joined channels split three
voice to fifteen text, and its seven direct messages under Home all render, and
`pnpm test:coverage` passes the 90% thresholds.

### Phase 5: the hierarchy fetch

The one network call. Per space, on the child set changing, cached. Fills in
the names of children that are listed but not joined.

Kept last on purpose: everything before it works without it, and it is the only
part that can fail in a way the user sees. If it slips, Phase 4 ships with two
channels missing on this account rather than with two channels named after
their room IDs.

## Testing

Unit tests on `assemble` carry the milestone, for the same reason the DTO
mapping did last time: it is pure, every branch is reachable, and it is where
the logic actually is.

`MatrixMockServer` covers `watch` emitting a snapshot when a sync response
contains rooms, and emitting nothing when a sync changes nothing relevant.

`against_a_real_homeserver.rs` gains one test that creates a space, creates a
child, and asserts both appear with the right shape. Space creation is the part
no mock makes convincing.

Vitest covers the rail rendering Home plus spaces, selection changing which
channel list is shown, text and voice grouping, an unjoined child being
unselectable, and the avatar falling back to initials when the fetch returns
nothing.

## Complexity

Medium, and the weight is not where it looks. The Rust side is one module with
a pure core and one subscription, and it is the smaller half. The frontend is
four new components, a layout that replaces the existing signed-in view, and
90% coverage on all of it.

## Deliberately out of scope

No messages. The main pane is empty and says so.

No room creation, no joining by alias, no leaving. The rail shows what the
account already has.

No drag ordering. Matrix stores that in `m.space.child`'s `order` field and
writing it is a different feature from reading it.

No nested spaces. A space inside a space renders as a rail entry of its own
rather than as a folder. This account has none, so the alternative would be
untested code.

No unread badges, no notification counts. `room_info` carries them and the
snapshot could, but a badge that is wrong is worse than no badge and getting it
right needs read receipts.
