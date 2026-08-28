# Plan: make an encrypted call audible

Status: **Phases 0 to 2 and 4 done. Phase 3 is written and waiting on a push.
Phase 5 is the live half and is next.** Every API named here was read at the
pinned rev rather than recalled.

This is Phase 6 of [PLAN-verification.md](PLAN-verification.md) and it turned
out to be larger than the sentence there. That phase says "refuse to join an
encrypted call from an unverified session, and say why", which is one of the
five things wrong.

It blocks the Windows build in [PLAN-release.md](PLAN-release.md). Sending
somebody a build whose headline feature has never been confirmed working in
both directions means the first bug report is one we could have written
ourselves.

## What is wrong

### A. Nothing prevents the join

`calls::readiness` runs once at startup, logs a line, and is discarded.
`app/src-tauri/src/state.rs:511` is its only caller and it is a
`tracing::info!`. `call_connect_for` at `commands.rs:435` never asks.

### B. Nothing detects the failure when it happens anyway

This is the one that was not expected, and it matters more than the gate.
`crates/consort-call/src/trouble.rs` was written for exactly this failure and
cannot see it.

`Fault::NothingSent` is produced from `FrameEncryptionState::EncryptionFailed`,
which is the **local frame cryptor** failing. In the cross-signing case the
local cryptor works perfectly: it has its own key and encrypts happily. What
fails is the to-device **distribution** of that key to everybody else.

That failure is a `log::warn!` at `matrix-rtc-core/src/session.rs:467` and a
`log::error!` at `matrix-rtc-core/src/encryption/mod.rs:757`. It never becomes
an event, so nothing downstream can hear it.

So `trouble.rs` can detect four ways a call goes quiet and not the fifth, which
is the one Phase 0 of [PLAN-voice-call.md](PLAN-voice-call.md) actually
reproduced.

### C. Readiness is a snapshot of something that changes

Computed at startup, before anybody verifies. Cached, and somebody who verifies
mid-session stays locked out. Uncached, and every click costs a crypto-store
read, which is fine, except that the channel list needs the answer before the
click rather than at it.

### D. The gate is only correct for encrypted rooms

MSC4143 forbids RTC encryption in an unencrypted room, and
`connect_with_optional_e2ee` says so in as many words. No media keys are
distributed there, so cross-signing is irrelevant and refusing would break a
call that works.

`matrix-rtc-livekit/src/call.rs:433` already computes this from
`room.latest_encryption_state()`, but it does so *inside* the join, past the
point where a refusal would happen.

### E. Verification itself does not finish

The emoji flow stalls, and the cause is now known rather than suspected. See
Phase 0.

## Phases

### Phase 0: fix the verification stall (done)

Hard prerequisite. Gating calls on a flow that never completes converts "calls
are silently broken" into "calls are loudly impossible", which is more honest
and not better.

**The bug is a lost update, and it is provable from the source.**

`SasVerification::changes()` and `VerificationRequest::changes()` both come from
eyeball's `SharedObservable::subscribe()`, whose own documentation says it
"only resolves once the inner value has been updated again **after** the call to
`subscribe`". There is a `subscribe_reset` that replays and the SDK does not use
it. So anything that moves the state between the last read and the subscription
is never delivered.

Both drivers in `verification/flow.rs` act first and subscribe second.

In `drive`, at lines 300 to 307:

```rust
let initial = request.state();                  // Ready
if matches!(initial, Ready { .. }) {
    start_the_comparison(&request).await;       // request becomes Transitioned here
}
report.state(state_of(&initial));               // reports Ready
let mut changes = request.changes();            // subscribes past Transitioned
```

`matrix-sdk`'s `VerificationRequest::start_sas` at `requests.rs:200` transitions
the crypto-level request **before** it sends anything:
`self.inner.start_sas().await?` sets the observable, and
`send_verification_request` is the line after. So on the path where the request
is already `Ready` when `drive` first looks, the `Transitioned` that would move
this flow onto the SAS stream has already happened by the time anybody is
listening. The loop then waits for the next change to the *request*, and there
will not be one, because every remaining transition belongs to the *Sas*.

The flow strands at `Ready`. Pressing the "show me the emoji" button calls
`start_sas` on a request that has already transitioned, which does nothing.

`follow_sas` has the same shape at lines 385 to 390, with a narrower window:
`sas.state()` is read, then `sas.changes()` subscribes.

**The fix, both places: subscribe before acting.**

`Subscriber` records a version at subscription time and buffers by it, so
subscribing first and reading the state afterwards closes the window
completely. A transition landing in between is reported twice, once by the read
and once by the stream, and `Report::state` already dedupes through
`Changes::accept`, so that costs nothing.

Second half, in `start_the_comparison`: `start_sas()` returns the
`SasVerification` it created and the current code throws it away, then waits to
observe `Transitioned` to get the same object back. Use the returned one. It
removes the dependency on observing a transition that has already happened.

Done when the running application completes an emoji verification against
Element, which is the interop claim Phase 2 of the verification plan explicitly
deferred.

### Phase 1: make readiness live (done)

- `calls::watch_readiness(client, on_change) -> JoinHandle<()>`, shaped like
  `verification::watch`. The trigger already exists:
  `encryption().verification_state()` changes exactly when this answer would.
- A `call-readiness` channel carrying `CallReadiness`, replayed to late
  subscribers through the existing `LatestSink`. Phase 1 of the verification
  plan built that for precisely this race and it applies unchanged.
- A fifth task slot beside sync, refresh, verification and backup. Started and
  aborted in the same two places as the other four.

`classify` and its tests do not change. They are already right.

### Phase 2: the gate (done)

- `consort-matrix`: `calls::can_join(client, room_id) -> Result<JoinVerdict>`.
  Two questions in one place. Is this room encrypted, and if so is readiness
  `Ready`. Unencrypted is always allowed whatever the readiness. Unknown counts
  as encrypted, matching `call.rs:437`'s `unwrap_or(true)` and for the same
  reason: the direction to fail in is the private one.
- `consort-call`: `CallEvent::Refused { room_id, readiness }`. Not `Failed`,
  which carries a bare string, because this has to render a button.
  `consort-call` already depends on `consort-matrix` and its manifest says that
  edge points one way and always will, so naming `CallReadiness` there is legal.
- `call_connect_for` asks before `state.connect_call`, emits `Refused` on the
  `call` channel, and returns `Ok`. That matches the documented contract that a
  join's outcome arrives on the channel rather than as a command error.
- Asked again inside the join, immediately before key distribution, because the
  answer can change between the click and the connect. It is a crypto-store
  read, so it is nearly free.

### Phase 3: make the failure visible when it happens anyway (half done)

The gate is prevention. This is what makes the thing honest rather than merely
defensive, and it is the phase not to skip.

There is an exact precedent to copy. `KeyDiscarded` already travels core to
`MediaKeyBridge::set_key_discard_listener` to `engine.notify_key_discarded` to
`CallEvent::KeyDiscarded` to the host, wired at
`matrix-rtc-livekit/src/call.rs:492`. A distribution failure follows the same
road:

- Upstream in `matrix-rust-rtc`: `set_key_distribution_failure_listener` on
  `MediaKeyBridge`, called from the two places that currently only log, and a
  `CallEvent::KeyDistributionFailed { reason }` out of the engine.
- Downstream in `consort-call/src/trouble.rs`: `Fault::NotDistributed { reason }`,
  ranked above `NothingSent`, naming cross-signing when the reason does.
  `what_it_says` grows one arm.

### Phase 4: the interface (done)

- Voice channels render as unavailable when the room is encrypted and readiness
  is not `Ready`, with the reason on hover, rather than accepting a click that
  will be refused.
- The refusal renders in `CallPanel`, reusing `VerificationBanner`'s copy so
  there is one description of the problem and one route out of it. `NoIdentity`
  and `SessionUnverified` get different sentences and different actions, which
  is why `readiness.rs` kept them apart to begin with.

### Phase 5: prove it (next)

- **Unit:** `can_join`'s truth table. Encrypted times ready, encrypted times
  unverified, unencrypted times unverified, unknown times unverified. The
  unencrypted row is the one that catches an over-eager gate.
- **Mock:** a refusal emits on the `call` channel and starts no call thread.
  `commands.rs:815` already has the right assertion shape.
- **Live:** two sessions in one encrypted room, one verified and one not. The
  unverified one is refused, verifies, and is then allowed without a restart.
  That last clause is what Phase 1 exists to make true.
- **Live, the one that settles everything:** two verified sessions in an
  encrypted room, and each confirms hearing the other. Nothing in this repo has
  ever tested that claim.

## Risks

**The upstream change is in a fork that has to stay rebased.** Phase 3 patches
`matrix-rtc-core` and `matrix-rtc-media`. Worth carrying anyway: it is small,
purely additive, and arguably an upstream bug that a key distribution failure is
invisible to the host.

**Refusing is worse than nothing if verification is unreachable.**
`VerificationBanner` already handles the honest dead end (no other session, no
recovery key). The gate must not fire on somebody in that state without saying
the same thing, or it is a locked door with no key.

**Getting the encryption check wrong turns a working call into a refused one.**
Hence the unencrypted row in the unit test.

## Complexity

Medium, unevenly split. Phase 0 is a small diff now that the cause is known.
Phases 1, 2 and 4 each follow a pattern this codebase already runs four times
over. Phase 3 is small and crosses a repo boundary. Phase 5 is where the real
answer comes from.

## What actually happened

Six things the plan above did not have.

**The stall was a lost update, and it was provable from the source rather than
by reproducing it.** `changes()` is eyeball's `SharedObservable::subscribe`,
whose own documentation says it resolves "only once the inner value has been
updated again after the call to `subscribe`", and there is a `subscribe_reset`
that replays which the SDK does not use. `VerificationRequest::start_sas` sets
the observable to `Transitioned` *before* it sends anything, so sending the
start and then subscribing meant the transition that moves a flow onto the SAS
stream had already happened with nobody listening. Both drivers now subscribe
before acting, and `start_the_comparison` returns the `SasVerification` it
created rather than waiting to observe a transition it caused itself.

**A refusal cannot travel on the call channel.** The plan said
`CallEvent::Refused`, and that would have been a bug: the call channel carries
state, so somebody sitting in one voice channel who clicks a second one and is
refused would have had the call they are in evicted, drawing a client connected
to nothing while this process was still publishing a membership. It is
`AppEvent::CallRefused` on a channel of its own, and it is not replayed to a
webview that reloaded, because the standing answer is already on
`call-readiness` and this only adds which click failed.

**The SDK collapses "unknown" into "not encrypted".**
`EncryptionState::is_encrypted` returns false for `Unknown`, which is the wrong
direction for a gate. `encrypts` matches `NotEncrypted` positively instead, so
a room whose state could not be fetched, or that this account is not in, is
gated. `matrix-rtc-livekit/src/call.rs:437` has the same shape with the same
comment and the opposite behaviour: its `unwrap_or(true)` covers only the `Err`
case, so its stated rule that "unknown counts as encrypted" is not what its
code does. Worth fixing upstream separately.

**A gate that cannot reach an answer lets the join through.** Not in the plan
and it should have been. The error is about not being able to ask rather than
about the answer, `CallReadiness` has no "not known" variant on purpose, and
there is no honest refusal to draw from a request that timed out. The network
that stopped the question will stop the join a moment later and say so in the
vocabulary of the thing that actually failed.

**Phase 3 crosses a repo boundary and is only half landable from here.** The
upstream work is done and committed in the `matrix-rust-rtc` checkout on
`feature/report-key-distribution-failure`: an `on_key_distribution_failed`
handler method beside `on_key_discarded`, a `MediaKeyBridge` listener beside
the discard one, and `CallEvent::KeyDistributionFailed` out of the engine,
wired in `Call::join` and tested at both ends. `DistributionFailure` carries
`at_join`, because the join's own distribution failing means nobody has ever
held this session's key while a later one failing leaves everybody already
there with the last one that worked.

Consort pins that fork by rev, so its half (`Fault::NotDistributed` in
`trouble.rs`) cannot compile until the branch is pushed and the rev in the
workspace manifest moves. That is the only thing standing between this and a
call that says out loud when nobody can hear you.

**Phase 5 is the only phase that can still fail interestingly.** Everything
above is tested against fakes and mocks that this plan wrote. The claim nothing
in this repository has ever tested is that two verified sessions in an
encrypted room can hear each other.
