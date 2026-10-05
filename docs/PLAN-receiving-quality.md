# Plan: the quality of what you receive

Issue #167. The issue asks whether a receiver can change the quality of
somebody else's screen share, whether a bad connection can be detected and
answered automatically, and says the first job is finding out if Matrix can do
this at all.

It can. The control surface exists in the pinned SDK, Consort already drives it
for something else, and the one thing missing is the thing #167 is really
waiting on: nothing in this window draws a remote picture, so there is no
picture whose quality anybody could change.

## The short answer

| Question | Answer |
| --- | --- |
| Can a receiver cap the quality of a stream? | Yes. `MediaConstraints::detail`, per participant, per stream kind. |
| Does it reach the SFU? | Yes. `UpdateTrackSettings`, the same message deafen already sends. |
| Is the publisher involved? | No. Receive-side only, which is why #167 is separable from publish-side quality. |
| Can a bad connection be detected? | Partly. `Call::receive_stats` gives cumulative RTP counters to sample and compare. There is no connection-quality verdict to read. |
| Can it ship now? | No. There is no remote video to apply it to. |

## What the pin can do

`matrix-rtc-media` at rev `0427fc00` carries the whole receive-side model in
`crates/matrix-rtc-media/src/constraints.rs`:

- `MediaConstraints::enabled`, a hard switch meant to become a full
  unsubscribe.
- `MediaConstraints::visible`, a server-side pause that keeps the
  subscription, so resuming is immediate.
- `MediaConstraints::detail`, a `VideoDetail` of `Auto`,
  `Dimensions { width, height }` or `Quality(Low | Medium | High)`.
- `MediaConstraints::low_bandwidth`, which pauses every video stream of one
  participant and leaves their audio running.

`Call::set_constraints(member_id, kind, constraints)` is the whole API. It is
debounced, it is keyed by membership so it dies when that membership does, and
the engine re-applies it whenever the stream reappears. Consort already calls
it: `LiveKitSession::set_deafened` sets `visible: false` on every remote
microphone, which is how deafen is built.

So the plumbing for #167 is one more call to a function Consort already calls,
with a different kind and a different field.

## Dimensions or Quality

`Dimensions` is the one to use. Three reasons, in descending order of how much
they bind:

**`Quality` is deprecated on the wire.** The module docs on `constraints.rs`
say LiveKit has deprecated the quality field in its own protocol in favour of
dimensions, and name `Dimensions` as the preferred control.

**`Quality` is not safe to combine with a pause.** `transport_impl.rs` carries
a caution worth reading before writing either path: every LiveKit setter sends
a full-replacement `UpdateTrackSettings`, and `set_video_quality` sends only
the quality field, with zeroed dimensions and `disabled: false`. Following a
pause with it would un-pause the stream. The transport guards against that by
applying `Quality` only while the demand is `Active`, which means a quality cap
asked for while a tile is paused is silently dropped and the dimensions path
has no such hole.

**A renderer knows pixels, not ladders.** The size of the box a stream is drawn
into is a fact the window has. Which simulcast layers the publisher is sending
is not.

The cost of `Dimensions` is that a person picking "low" is not picking a layer,
they are picking a size, and the ceiling has to be chosen for them. That is
[ADR-0013](adr/0013-ask-for-a-picture-in-pixels.md).

## What neither of them can do

**One layer cannot be chosen between.** `PublishOptions::camera` and
`PublishOptions::screen_share` both set `simulcast: true`, but the LiveKit
transport then gates it: `simulcast && max(width, height) >= 480`. Below a 480
pixel long edge LiveKit computes a single encoding, which upstream found the
SFU delivered no frames for, so the transport publishes a plain one instead. A
share of a small window is therefore a single-layer track, and no receive-side
request of any shape can pick a smaller one out of it. A receiver asking for
less from such a track gets the same stream it already had.

**The detail path has no end-to-end verification upstream.**
`crates/matrix-rtc-livekit/tests/e2e_call/main.rs` drives
`verify_constraints_toggle` against a real LiveKit server and proves pause,
resume, off and on. It never sets `detail`. The `Dimensions` and `Quality`
branches have unit coverage in `matrix-rtc-media`'s engine against a fake
transport, which proves the value reaches the transport and nothing about what
the SFU does with it. Whoever builds phase 2 below should expect to be the
first person to watch a layer switch.

## Detecting a bad connection

Reachable, but it has to be built rather than read.

`Call::receive_stats(member_id, kind)` answers a `ReceiveStats`:
`packets_received`, `packets_lost`, `bytes_received`, `jitter`,
`frames_decoded`, `frames_dropped`, and the audio concealment counters. Every
field is a cumulative total, so a verdict needs two samples and the time
between them. `None` is a real answer, both for a stream that is not subscribed
and for one whose first RTCP report has not landed.

There is no shortcut. The pin exposes no connection-quality enum, no bandwidth
estimate, and no `ConnectionQualityChanged` event to the application, so an
automatic downgrade is a sampler plus a threshold plus hysteresis, all of it
Consort's.

Consort does not call `receive_stats` anywhere today. `consort_call::trouble`
is the module that already answers "why can nobody hear me", and it answers it
from frame-encryption diagnostics rather than from counters, so a sampler has
no existing loop to join.

Two things worth settling before writing one:

- **A downgrade that fights the SFU is worse than none.** LiveKit already
  adapts a subscription downwards under congestion on its own, and the
  constraint model says so outright: whatever is asked for is a cap, not a
  floor. An automatic cap that drops on loss, on top of an SFU already
  dropping, is two controllers on one loop.
- **Loss on a screen share is not the same signal as loss on audio.** The
  thresholds cannot be shared, and one of the two has nowhere to be shown yet.

## Why none of it ships yet

Nothing carries a remote frame into this window.
`docs/PLAN-webcam.md` phase 2 is where that lands, and ADR-0008 and ADR-0009
record the layout waiting for it: somebody else's camera or screen is a square
with their name in it. `consort-call` reads exactly one remote track kind,
`MediaStreamKind::Microphone`, in `LiveKitSession::listen`.

So every part of #167 that a person could see depends on phase 2 of the webcam
plan:

- A quality control on a tile needs a tile with a picture in it.
- `Dimensions` needs the size of the box the picture is drawn into, which is
  the one number the webview has and Rust does not.
- An automatic downgrade needs somewhere to say it happened, or it is a setting
  that changes itself and never admits it.

Building the Rust half now would be a seam, a thread command and a Tauri
command with no caller, and a `Dimensions` value invented from a box size
nobody has measured. The honest order is phase 2 first.

## Phase 1: stop paying for video nobody draws

Not part of #167, and worth its own issue. Recorded here because the research
turned it up and it is the same one function.

`LiveKitConnection` connects with `auto_subscribe: true` and the transport sets
no adaptive-stream option, so a subscribed track is a track the SFU is
forwarding. Consort subscribes to every remote camera and every remote screen
share, reads frames from neither, and draws neither. A Consort client in a call
with somebody sharing a 1080p desktop is paying for that desktop in order to
discard it.

`MediaConstraints { visible: false }` on `Camera` and `ScreenShare` for every
remote membership would stop it, in the loop `set_deafened` already walks.
`dynacast: true` is set, so if every receiver in the call pauses, the publisher
stops encoding the layer too. Nothing Consort draws reads subscription state:
the camera and screen indicators come from MatrixRTC membership signalling, in
`consort_call::roster`.

The catch is that it has to be undone in the same change that draws a remote
picture, or phase 2 starts by debugging a stream that is paused on purpose.

## Phase 2: a control on a tile

Only once a remote picture draws.

| Piece | Where |
| --- | --- |
| The chosen cap, and what it is in pixels | `consort-call`, a module of its own |
| The call to `set_constraints` | `consort-call`: `livekit.rs`, `transport.rs` |
| The command and the state | `thread.rs`, `app/src-tauri/src/state.rs` |
| The control | wherever phase 2 of the webcam plan puts a tile |

What is true at the end: a stream drawn in a tile is asked for at the size of
that tile, a person can cap it below that, and the cap survives the stream
going away and coming back because the engine re-applies it.

## Phase 3: an automatic cap

Only once phase 2 exists, and with a measurement first rather than a threshold
picked from the issue text. Needs:

- a sampler, on the call thread, polling `receive_stats` per drawn stream
- a verdict from two samples and the interval, with hysteresis, so one bad
  second does not drop a share to 360p for the rest of a meeting
- somewhere to say that it happened, and a person's own choice overriding it
  until they clear it, which is what #167 asks for in its second sentence

## Out of scope

Publish-side quality, which is the other half of a bad connection and is
`PublishOptions` rather than `MediaConstraints`. Issue #165, which this
research does explain: the picture that gets worse when a share fills the
window is this session's own, drawn from the 320 pixel still that
`app/src-tauri/src/selfview.rs` bounds, upscaled up to 4.29x by the table in
ADR-0009. Nothing on the receive side is involved in it.
