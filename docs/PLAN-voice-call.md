# Plan: joining the voice channel

Status: **not started.** This is the second half of issue #6. The first half,
drawing who is already in a channel, is done and described in
[PLAN-voice-presence.md](PLAN-voice-presence.md). This half is the one that
carries audio.

Everything below was written after reading the sibling `matrix-rust-rtc`
checkout rather than guessing at it, because the single most important fact
about this work is how much of it already exists somewhere else.

## The finding that shapes the plan

`matrix-rtc-livekit` ships `Call::join(&room, CallOptions)`, and it is not a
thin wrapper. One call publishes this device's membership, arms a dead man's
switch and heartbeats it, starts distributing and ingesting media keys over
Olm-encrypted to-device messages, discovers the LiveKit transport from the
homeserver, and connects to the SFU with per-participant frame encryption.
`Call::leave` unwinds all of it in the right order.

The other end fits just as well. `Call::publish` hands back a
`LocalTrackHandle` whose `capture_audio` takes:

```rust
pub struct AudioFrame {
    pub data: Vec<i16>,
    pub sample_rate: u32,
    pub num_channels: u32,
    pub samples_per_channel: u32,
}
```

`consort-audio` already produces exactly that and currently throws it away.
`state.gated` in `thread.rs` is a `Vec<i16>`, `FRAME_SAMPLES` (480) long, mono,
48 kHz, refilled every 10 ms and read by nobody. `AudioSourceConfig::default()`
is 48 kHz mono. The two ends were built to the same shape without either
knowing about the other, which is a good sign and also a thing to verify rather
than trust.

So this plan is mostly not about MatrixRTC. It is about a dependency, a
runtime, a queue, and an interface. The protocol work is done and it is in the
sibling repo.

## What has to be true by the end

1. Clicking a voice channel connects to it, and the interface says so while it
   is happening rather than after.
2. Somebody in Element Call in a browser can hear this machine's microphone,
   and this machine can hear them.
3. The voice activity gate decides what goes out. Silence is silence to the
   peer, not a stalled sender.
4. Disconnecting is one click and it actually leaves, rather than leaving a
   ghost until the dead man's switch fires.
5. The channel being sat in shows its roster from the call itself. Every other
   channel keeps the room-state path that already works.
6. A failure to join says which part failed, in a sentence a person can act on.
7. Nothing regresses when no call is running. An idle Consort does no more work
   than it does today.

## The dialect problem, stated first because it decides everything

`CallOptions::element_call_compat` has three settings and they are not
compatible with each other:

- `Off` is spec-current MSC4143 plus MSC4354. Sticky events, MSC4195
  pseudonymous SFU identities.
- `StickyEvents` is Element Call as of 2025. Joins stay spec-valid, leaves and
  media keys do not.
- `StateEvents` is Element Call before MSC4354: membership as
  `org.matrix.msc3401.call.member` **room state**, plain `{user}:{device}` SFU
  identity, and the pre-MSC4195 `/sfu/get` token endpoint.

The presence work already measured which one this deployment speaks, and the
answer is not ambiguous: 27 `org.matrix.msc3401.call.member` state events
across the three call rooms, and **zero** `m.rtc.member` sticky events. This
deployment is on the state-events generation, so Consort has to join with
`ElementCallCompat::StateEvents` or it will be in a call by itself.

Two consequences, both worth stating plainly because both reverse something
believed earlier:

**The `lk-jwt-service` bump is not a prerequisite. It is the opposite.**
`StateEvents` mode takes its token from the pre-MSC4195 `/sfu/get` endpoint,
which is what the deployed 0.3.0 serves. Bumping to 0.4.4 becomes necessary
only when this deployment's Element Call moves past MSC4354 and Consort follows
it to `Off`. Until then, the running version is the one that matches.

**Picking wrong is silent.** A call joined in the wrong dialect connects, holds
a membership, publishes audio, and is heard by nobody, which looks exactly like
a working call with nobody in it. This is the same silent-failure shape the
presence plan called its single real risk, and it deserves the same treatment:
the mode is derived from what the room actually contains rather than hardcoded,
and the derivation is logged.

## Phases

### Phase 0: the spike

The only phase that can invalidate the rest, so it comes first and it produces
a throwaway binary rather than a design.

Add `matrix-rtc-livekit` with `features = ["matrix-sdk"]`, join one of the
three real call rooms from a scratch example, and hear a tone. Nothing about
Consort's architecture changes in this phase.

Four questions have to be answered here, because every one of them changes the
plan if it goes the wrong way:

**Does the matrix-sdk rev unify?** Both workspaces pin
`377330059c3a335ab36d190fc10b87de3427c6b3` from `BillCarsonFr/matrix-rust-sdk`,
which is what the comment at the top of Consort's `Cargo.toml` predicted and
demanded. If cargo resolves two copies anyway, `Call::join` will refuse the
`Room` this workspace builds, and the error is a wall of "expected Room, found
Room" rather than anything mentioning versions.

Note that matrix-rust-rtc asks for matrix-sdk features Consort does not
(`unstable-msc4354`, `experimental-send-custom-to-device`) and pulls in
`matrix-sdk-ui`. Features unify additively so this should be fine, but it means
Consort's `Client` gains capabilities it did not ask for, and the build gets
bigger.

**Does the `[patch.crates-io]` block have to be copied?** matrix-rust-rtc's own
manifest says yes, in as many words: cargo does not propagate a dependency's
patches to the consuming workspace, so the five pins (async-compat, const_panic
and three tracing crates) have to be replicated verbatim or the SDK does not
build. Consort has no patch block today. Adding one is a workspace-wide change
that affects every crate here, including the audio crate that has nothing to do
with any of it.

**Does `Call::join` need `matrix_sdk_ui::sync_service::SyncService`?** The
preconditions name it, and the reason given is that under `unstable-msc4354` it
auto-enables the sticky-events sync extension the membership bridge relies on.
In `StateEvents` mode membership is room state, which Consort's own sync loop
already delivers, so it may not be needed. If it is needed, this is by far the
largest risk in the plan: Consort's sync is its own, it is tested, and replacing
it is not a phase, it is a rewrite.

**Does `StateEvents` mode need an open slot?** `Call::join`'s preconditions
require an `m.rtc.slot` state event, and `open_slot` needs the power level for
it, which by default only the room creator has. The pre-MSC4354 generation has
no slots at all, so this requirement probably does not apply in that mode, but
"probably" is not good enough for something that fails at join time in rooms
this account may not administer.

Also measured here, since it is cheap to measure and expensive to discover
later: build time, target directory growth, and release binary size. libwebrtc
enters through this crate and through nothing else. The release profile in
`Cargo.toml` already strips symbols in anticipation.

### Phase 1: the call thread

`Call::join` drives `!Send` futures and panics outside a
`tokio::task::LocalSet`. Tauri's runtime is multi-threaded. So the call needs a
dedicated thread owning a current-thread runtime and a `LocalSet`, fed by a
command channel and emitting events over another.

That is precisely the shape `AudioThread` already has, and for precisely the
same reason: a resource that cannot move between threads. Building `CallThread`
as a deliberate mirror of it means one pattern in this codebase rather than two,
and it means the tests can be the same shape too.

The phase builds the thread, its message enum and its event enum against a
`MatrixCall` trait with `join`, `leave`, `publish` and `set_muted`, plus a fake.
No LiveKit is linked into anything tested here, exactly as `AudioCapture` and
`AudioPlayback` keep cpal out of every test but three.

The real implementation lands behind that trait and joins `cpal_host.rs` in the
coverage ignore regex, for the same reason: code whose behaviour is a device's
behaviour cannot be asserted without the device.

### Phase 2: the audio goes somewhere

`state.gated` gains its first reader.

One real design decision, and it is a decision rather than a wiring job.
`capture_audio` is `async` and applies backpressure: it "resolves when the frame
has been accepted". The audio thread must never await it, because the audio
thread is also servicing a cpal callback and a stalled capture loop is a
glitching microphone. So the two threads meet at a bounded queue that drops the
oldest frame when full, and the drop is counted and logged rather than silent.

That queue is its own type with its own tests: a full queue drops the oldest,
the level meter is unaffected by drops, and a call that goes away does not wedge
the capture thread.

The second decision is smaller and easier to get wrong. When the gate closes,
Consort must **mute the publication**, not merely stop pushing frames.
`LocalTrackHandle::set_muted` exists for this and its documentation says why:
simply not calling `capture_audio` is what a wedged client looks like to a peer.
Muting tells them it is deliberate so their interface can show it. Getting this
wrong produces a call where everyone appears frozen whenever they stop talking.

This is also where the voice activity switch from
[PLAN-voice-settings.md](PLAN-voice-settings.md) stops being a demonstration and
starts being a policy: with it off, the publication is never muted and every
frame goes out.

### Phase 3: join and leave from the interface

Clicking a voice channel connects. A connection panel above the user area, the
way Discord does it, showing the channel, the state, and a disconnect control.

The roster splits. The channel being sat in takes its participants from
`Call::subscribe_participants()`, which is a live `watch::Receiver` enriched
with actual media stream state, and is strictly better than reading room state.
Every other channel keeps the existing path. Both feed the same `Participant`
shape the channel list already draws, so this is a source change and not a
rendering change.

Failures get sentences. `CallError` distinguishes a Matrix client error, a
transport error, a media error and a signalling error, and those are four
genuinely different things to tell somebody.

### Phase 4: who is speaking

Consort's own speaking indication needs nothing new: the gate already produces a
voice probability per frame and the level meter already renders it.

Remote speaking comes from `CallEvent` and the `Participant` roster, and needs a
read of what those actually carry before it can be planned in more detail than
this sentence.

### Phase 5: prove it against Element Call

The three real voice channels. Consort in one, Element Call in a browser in the
other, in `StateEvents` mode, hearing each other in both directions. Then the
awkward cases: joining a channel somebody is already in, two clients on one
account, leaving by closing the window rather than clicking disconnect.

## Risks

**Sync.** Named above and repeated here because it is the one that could turn a
phase into a rewrite. If `Call::join` genuinely requires `SyncService`, phase 0
is where that is discovered, and the plan changes shape before anything is
built on top of it.

**The dialect moves.** When this deployment's Element Call updates past
MSC4354, `StateEvents` becomes the wrong answer and calls silently stop
interoperating. Deriving the mode from room contents rather than hardcoding it
is the mitigation, and the presence code already counts the events that decide
it.

**Cross-signing.** The default media-key policy requires key senders to be
cross-signed, per MSC4153. Consort's verification flow currently stalls after
`Ready`, which means this device may not be cross-signed at all. The failure
mode matters and is not yet known: joining might fail loudly, or might succeed
and produce a call where audio arrives and never decrypts. `EncryptionConfig`
can relax the policy, and the documentation is explicit that this is for test
setups only. Finding out which failure it is belongs in phase 0.

**libwebrtc.** The build gets heavy in a way nothing else here has been. This is
also the first dependency that makes `cargo build` a thing worth thinking about
on this machine.

**Two `!Send` threads.** Audio and call, both pinned, both talking to a
multi-threaded Tauri runtime and to each other. The queue in phase 2 is the only
place they touch, deliberately.

## What this deliberately does not do

- Video or screenshare. `PublishOptions` covers both and neither is in issue #6.
- Ringing, or calls in direct messages. Channels only.
- The spec-current dialect, until this deployment speaks it.
- Replace the room-state presence path. It stays, and it is what draws every
  channel not currently sat in.
