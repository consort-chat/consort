# Plan: joining the voice channel

Status: **phase 0 done**, the rest not started. This is the second half of issue #6. The first half,
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

### Phase 0: the spike (done)

The only phase that can invalidate the rest, so it comes first and it produces
a throwaway binary rather than a design. What it found is recorded at the end,
under [What phase 0 found](#what-phase-0-found). The binary itself is on the
`spike/voice-call` branch and not on main, because it takes matrix-rtc-livekit
by path from the sibling checkout.

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

## What phase 0 found

Everything below was run, not reasoned about, unless it says otherwise. The
spike registered throwaway users against the sibling repo's `demo/backend`
stack, on a remapped Synapse port so the local `consort-test-synapse` kept
8008, and the stack was torn down afterwards.

### 1. The matrix-sdk rev unifies

Exactly one `matrix-sdk` in the lock, one `ruma`, one of each `matrix-sdk-*`,
all at `377330059c3a335ab36d190fc10b87de3427c6b3`. Proven twice over: by the
lock, and by an identity function that only compiles if `consort_matrix`'s
`Client` and this crate's `Client` are the same type. Then `Call::join` accepted
a `Room` obtained through it. The warning at the top of `Cargo.toml` was right
to demand this, and the demand is met.

### 2. The patch block does not have to be copied

Consort has no `[patch.crates-io]` and everything built. `async-compat` and
`tracing-appender` are not in the graph at all at this feature set, and
`const_panic`, `tracing`, `tracing-core` and `tracing-subscriber` resolved from
crates.io with matrix-sdk, matrix-sdk-crypto, matrix-sdk-ui and ruma all
compiling against them. The sibling's warning is about its own feature set.

### 3. Something worse than the patch block does have to be copied

The livekit and webrtc crates publish semver-compatible releases that do not
compile with each other. A fresh resolve of `livekit = "^0.7"` produced three
build failures in a row:

- `livekit-api` 0.5.6 **and** 0.5.3 against `livekit-protocol` 0.7.12:
  `ConnectWhatsAppCallRequest` gained a field.
- `livekit` 0.7.48 against `libwebrtc` 0.3.46: `RtcConfiguration` became
  `#[non_exhaustive]`.

Eight crates are now pinned to the versions the sibling's own lock carries:
livekit 0.7.48, livekit-api 0.5.3, livekit-datatrack 0.1.9, livekit-protocol
0.7.9, libwebrtc 0.3.38, webrtc-sys 0.3.35, webrtc-sys-build 0.3.18, and
livekit-runtime 0.4.0 which already matched.

This is a standing cost rather than a one-off. A bare `cargo update` breaks the
build the moment any of those publishes, and `Cargo.lock` is the only thing
holding it. So the lock stays committed, the pins get a comment saying why, and
a future bump means rebuilding against the sibling's lock rather than resolving
freshly. Upstream could fix this by pinning `livekit` exactly in
matrix-rtc-livekit; until it does, this is ours to carry.

### 4. `Call::join` does not need `SyncService` in the state dialect

Run with an ordinary `Client::sync` and nothing else. The bridge ticked on
schedule (`tick: 0 sticky + 1 pre-sticky state membership(s)`), then again
exactly thirty seconds later, which is `STATE_MEMBERSHIP_POLL`.
`run_sticky_bridge` subscribes to `room.subscribe_to_updates()` in state mode
precisely because a call in that dialect "produces no sticky traffic
whatsoever", and those updates are what Consort's own sync already delivers.

The answer is dialect-specific and flips with the dialect: `Off` mode does
depend on sticky events, which need the MSC4354 sync extension that
`SyncService` turns on. So this is one more thing that changes when the
deployment moves, and it belongs on that list.

### 5. The state dialect needs no open slot

No `open_slot`, and the join succeeded with the core saying "no slot state
supplied yet; the open-slot condition stays unenforced". `feed_room_state`
skips the slot fetch deliberately in state mode, because a pre-sticky Element
Call room has no `m.rtc.slot` at all and reporting "no slots" would resolve
every session closed and drop every member, us included. Their own interop test
skips `open_slot` in the same mode for the same stated reason.

So: no `m.rtc.slot`, no power level for it, no room-creator privilege. The risk
is struck.

### 6. The token endpoint is `/sfu/get`, and it still works

`requesting an SFU token from .../sfu/get`, then `SFU token granted for
ws://localhost:7880`. Against lk-jwt-service **0.4.4**, which is worth stating
plainly: the legacy endpoint survives in current builds, so the deployed 0.3.0
is not a problem for this dialect and the bump stays a non-prerequisite.

### 7. consort-audio's frames are accepted verbatim

480 samples, mono, 48 kHz, `i16`, straight into `AudioFrame` with no
conversion. `StreamStarted { kind: Microphone }`, then `ActiveSpeakers`
carrying the SFU's own measurement of the tone rising from 0.125 to 0.25.

One correction to phase 2 falls out of this. `capture_audio`'s backpressure
paced a tight publish loop for ninety seconds with no drift and no drops, so
the queue in that phase is not there to pace the publish. It is there to keep
the capture thread from ever awaiting, which is a narrower job than the plan
described.

### 8. Cross-signing is a hard prerequisite, and the failure is louder than feared

Two freshly registered devices, neither cross-signed. Neither could send its
media key at all:

> could not send key index 0 to any of 1 recipient(s): failed to send event:
> encryption failed due to an error collecting the recipient devices:
> Encryption failed because cross-signing is not set up on your account

That refusal is the SDK's own, and it lands before MSC4153 gets a say: an
account with no cross-signing identity cannot encrypt the to-device message in
the first place. Each side then reported the other as
`FrameEncryptionState { state: MissingKey, diagnostic: NoKeyInstalled }`.

Three things follow, and they pull in different directions.

**The call otherwise worked perfectly.** Both memberships, both rosters,
`is_local` correct on each side, `StreamStarted` for both microphones, RTP
flowing. Only the audio was undecryptable. That is exactly the shape the plan
was afraid of.

**But it is not silent.** `MissingKey` with `NoKeyInstalled` arrives as a
`CallEvent`, and the sending side names the members it could not reach. Consort
can therefore say why somebody cannot be heard rather than drawing a call that
looks like it is working. Phase 3 gains that job.

**And the fix is already in Consort.** `verification/recovery.rs` says of a
successful recovery that "the cross-signing private keys are in this device's
store, the device has signed itself". So recovering with the account's recovery
key satisfies this, which means **the stalled emoji verification is not on the
critical path for calls**. Whether any work is needed at all depends on whether
the real Consort device is already cross-signed, which is worth checking before
assuming.

Two failures worth keeping apart, only the first of which was demonstrated:

| Account state | What happens |
| --- | --- |
| No cross-signing identity at all | We cannot send keys. Proven above. |
| Identity exists, this device unsigned | We can send; peers discard under MSC4153 as `KeyDiscarded { reason: NotCrossSigned }`. |

A real account that has ever set up recovery is the second shape if it is
broken at all.

### 9. Weight

`target` grew by 8.0 GB. The debug spike binary is 1065 MB against the current
app's 986 MB. Compiling the added graph took roughly four minutes on this
machine, most of it libwebrtc. Release with `strip` is the number that matters
for shipping and has not been measured yet.

### 10. Noise to expect

`ERROR livekit::rtc_engine::rtc_session: publisher data channel '_data_track'
closed unexpectedly` fires on every single connect and nothing is wrong.
libwebrtc also writes `VAAPI is supported.` to stdout as a bare print rather
than a log line, so no log filter will suppress it.

### What this changes about the phases

- **Phase 1** is unchanged.
- **Phase 2** is smaller. The queue protects the capture thread; it does not
  pace the publish, because the transport already does.
- **Phase 3** gains a job: surface `FrameEncryptionState` and `KeyDiscarded`,
  because "you cannot hear this person, and here is why" is available and
  silence is not an acceptable substitute.
- **Phase 4** is nearly free. `CallEvent::ActiveSpeakers` carries each speaker
  and their level, so remote speaking indication needs no metering of our own.
- **New phase 0.5**, small and first: confirm the real Consort device is
  cross-signed, and if it is not, that recovery fixes it.
