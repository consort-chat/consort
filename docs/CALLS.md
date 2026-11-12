# Being in a call

How `consort-call` is put together, and the two constraints that shape it. The
module documentation points here rather than carrying this.

## The seam between the call thread and the SFU

**Why it exists.** The only real implementation talks to an SFU through
`matrix-rtc-livekit`, and the jobs that measure coverage have no SFU, so every
line that talks to one is a line those tests cannot reach. A call thread nobody
tests is a call thread whose reconnect logic is discovered by a person clicking
a channel twice. So `CallTransport`, `CallSession` and `Roster` are the seam:
everything that decides anything sits above them and is tested against a fake,
and `crate::livekit` is kept thin and excluded from coverage.

**Why the traits are not object-safe.** `CallTransport::join` is an `async fn`
in a trait, which cannot go behind `dyn`. That is deliberate. Unlike the audio
backends, which are chosen at runtime because a test and the application need
different ones in the same binary, there is exactly one call transport per
build. `crate::CallThread` is generic over it and the choice is made once at the
call site.

**Why `CallSession` is not `Send`.** This is the whole reason the call thread
exists. The real one is built by `LiveKitCall::join`, which uses `spawn_local`
for its heartbeat and its key distribution, so it can only exist inside a
`LocalSet` on the thread that made it. It cannot be held in shared application
state and cannot be moved across an await in a Tauri command.

**Why `Roster` is two methods rather than a stream.** `now` resolves display
names, which is a read per person against the room's member store, and should
happen when there is something to report rather than on a schedule. The roster
comes from MatrixRTC signalling rather than from `m.room.state`, so it is right
in every dialect, including the ones where room state shows nothing at all.

**Why publishing is split per source rather than parameterised.** Peers read a
camera and a screen as different streams. `MediaStreamKind::ScreenShare` is what
a client draws large and a camera is what it draws in a tile, and a session may
have both up at once.

**Why retraction lives on the session rather than on the publication.**
Retracting LiveKit's track handle took the camera off the SFU and left it on the
engine's roster, which is where this session's own icon is read from, so the
camera stayed lit after it was switched off. Only the session can reach the
engine.

**What would revisit it.** Upstream's `LiveKitCall::join` now spawns through
`matrix_rtc_core::executor`, which requires `Send`, so the thread below the seam
could become an ordinary task. The seam is also what keeps the call off the
thread that draws, so it does not go with the thread. See
[PLAN-off-the-pinned-forks.md](PLAN-off-the-pinned-forks.md), stage 5.

## Only the room-state dialect can be read back

**The failure this exists to prevent.** MatrixRTC has been rewritten twice and
deployments have not caught up together. A join in the wrong generation is the
worst kind of failure to diagnose, because it succeeds. Membership publishes,
the SFU accepts the connection, the log says connected, and the people already
in the channel see nobody arrive, because they are reading a different event
type. So `Dialect` is a named choice with three values rather than a default
nobody looks at.

**The chain behind `Dialect::readable`.** It was read rather than assumed, and
is written down to be re-checked rather than rediscovered:

1. A call's peers are read from the room's sticky events.
2. Those are filled by exactly one call site in matrix-sdk,
   `sticky_manager.dispatch` in `matrix-sdk-base`'s `sliding_sync`.
3. Consort syncs with `sync_with_result_callback`, which is sync v2 and never
   reaches that call site. Turning on `unstable-msc4354` adds the ruma types,
   not the ingestion.

A fourth link has already gone: the sticky bridge used to take room updates as a
second wake source in the room-state mode alone, and element-hq's room feed now
subscribes to room updates and sticky events unconditionally for every format.
The first three stand, so the conclusion does not move. A call in the other two
dialects connects, publishes membership, carries RTP and shows an empty room to
the person who joined it, which is why the LiveKit transport refuses such a join
rather than making it.

**What `detect` can and cannot see.** One signal, and the only one available
without a matrix-sdk feature this workspace does not enable:
`live_state_memberships`, the number of unexpired pre-MSC4354 room-state
memberships the room holds, which is
`Room::active_room_call_participants().len()`. Anybody in there through room
state speaks `Dialect::State`, and joining any other way makes this session
invisible to them.

The other two cannot be told apart here at all. Nothing can see MSC4354 sticky
membership, so an empty answer means either an empty channel or a channel full
of people this build cannot see. Even once it can, `Current` and `Sticky` would
still not be distinguishable by counting: a sticky-dialect join is deliberately
additive and stays valid to a current peer, so both generations write an event
that reads the same from outside. The difference is in the field names inside it
and in which to-device type carries the media keys.

So detection can only push towards `State`, never away from it, and `fallback`
is what an empty room gets. That asymmetry is the right way round: joining an
empty channel in the wrong dialect is a call nobody has arrived at yet, while
joining an occupied one in the wrong dialect is a call that looks connected and
is not.

**The wire mapping.** There are two `MembershipFormat` modes for three peer
generations, so one pair collapses. Element Call's 2025 sticky dialect is not a
third format, because upstream reads it inside the room-state format: Element
Call wrote sticky memberships beside its state events through the transition. So
`Sticky` and `State` map to the same mode, and the collapse is unobservable in
practice because `readable` refuses a `Sticky` join before the mapping is ever
consulted.

`Sticky` is kept rather than deleted because the variant names are a settings
file's contract, in a stronger sense than a rename: the whole file is one
`serde_json::from_slice`, so an unknown dialect makes every other setting in it
revert to its default.

**What would revisit it.** This is a build limitation with an end date. When
Consort moves to sliding sync, `readable` and every refusal that consults it
come out together, and the tests in `dialect.rs` are what say so.
