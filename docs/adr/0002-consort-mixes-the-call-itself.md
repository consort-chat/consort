# ADR-0002: Mix the call in Consort, because natively nothing else will

**Date**: 2026-09-01
**Status**: accepted
**Deciders**: tominal, with Claude Code

Recorded on 2026-09-24, from the comments in `mixing.rs` and `playback.rs`. The
decision itself was made while those were written.

## Context

In a browser, a LiveKit room plays itself. The SDK attaches a subscribed track
to an `<audio>` element and the page makes noise without anybody asking for it.
Consort was built expecting the same shape, and `playback.rs` said so in a doc
comment: call audio would arrive already mixed from the media layer.

It does not. Natively a subscribed track is a decoder producing PCM into a
stream, and if nothing pulls that stream the frames are decoded and dropped.
This is the expensive part: every layer reports success the whole way down. The
subscription succeeds, the decoder runs, the logs are clean, and the call is
silent. There is no error anywhere to lead somebody to the cause.

So mixing several decoded streams and finding a sound card for the result is
this crate's job after all. That makes `mixing.rs` the other half of
`capture.rs`, and the shape is the capture path in reverse: many producers, one
device, and two clocks that do not agree.

## Decision

`consort_audio::mixing::Voices` owns a queue per participant, keyed by whatever
string the caller uses to tell participants apart. Nothing in this crate looks
inside the key, so the call layer's `member_id` goes in without this crate
having to know MatrixRTC exists.

### The two clocks

Frames arrive from the network, one per participant, paced by whenever the
packets carrying them turned up. They leave through a sound card that asks for a
buffer on its own schedule and will not wait. Neither side may block the other:
a producer stalled on the device would stall the whole call thread, and a device
callback stalled on the network is a click in somebody's headphones.

So a queue absorbs the difference, and both overflow and underflow are handled
rather than waited on.

- **Underflow plays silence.** A participant whose packets are late is one
  person briefly dropping out. Stalling the device for them would take everybody
  else out with them.
- **Overflow drops from the front**, past `JITTER_FRAMES` (12 frames, 120 ms).
  What is waiting is audio that would be heard late, and late audio in a
  conversation is worse than missing audio, because everything queued behind it
  is late too and stays late.

This is the same bargain `consort_call::Microphone` makes going the other way.

### The sound queue is the exception, and inverts the rule

Chimes and spoken notifications go in a queue of their own rather than under a
reserved key in the participant map, because the cap that is right for speech is
wrong for them:

- Its cap is `SOUND_SAMPLES`, six seconds rather than 120 ms. A chime plus a
  spoken sentence is over two seconds on its own, so anything shorter cuts the
  sentence off at the end.
- It drops from the **end**, not the front. A truncated chime is still
  recognisably the chime; a chime missing its beginning is a click.

### Levels

Levels are stored as integer percentages, not as the multipliers they become.
A percentage is what a slider draws, and an integer is what somebody can mean
when they hand-edit a settings file: `70` rather than `0.7071`.

`gain()` squares the percentage rather than using it proportionally. A slider
that is linear in amplitude is not linear in anything a person hears: half
amplitude is about six decibels down, which the ear takes as roughly two thirds
as loud, so a proportional slider spends its bottom half on changes nobody can
hear much of and its top half on almost nothing. Squaring puts the middle of the
slider near the middle of the range somebody is listening for.

Three levels, with two different ceilings:

| Level | Ceiling | Why |
|---|---|---|
| Master output | `FULL_VOLUME` (100) | Everything is already summed by then, so there is nothing left that could be raised on its own. Past full scale is distortion, not volume. |
| Notifications | `FULL_VOLUME` (100) | The same, and it is multiplied *under* the master so turning a call down turns these down with it. A notification level beside the master would get louder every time somebody turned the call down. |
| One person | `MAX_PERSON_VOLUME` (250) | A single stream among several. Somebody on a laptop microphone three feet away arrives quiet, and the repair is to bring that one voice up rather than bring the rest of the room down to meet it. |

The squaring applies above a hundred too, so 250 is a little over six times
amplitude rather than two and a half, and the control stays one continuous curve
instead of changing character where somebody crosses full volume.

Per-person levels are keyed by membership and replaced wholesale rather than
edited one at a time, because a membership is fresh on every join. A map that
was only ever added to would grow for the lifetime of the process and hold
levels against people who left an hour ago. They are also held separately from
the queues, because `forget` drops a queue the moment somebody's stream stops
and a level stored alongside it would be lost every time a person muted.

### Headroom

The mix accumulates into `i32` and is clamped once at the end. Summing four
people who are each at three quarters of full scale overflows `i16`, and a
wrapped sum is loud noise in the opposite direction, which is the worst possible
thing to put into somebody's headphones.

The final clamp clips rather than scaling the whole mix down. A limiter that
ducked the call whenever two people overlapped would be audible constantly.
Clipping is audible only when the sum genuinely runs out of room, which with
real speech is rare and brief. Boosting one person above unity can reach that
point, which is the cost of the 250 ceiling and is the right shape of cost: it
lands on the moments the boosted person is actually loud, rather than on every
call all the time.

## Alternatives Considered

### Alternative 1: Wait for the media layer to mix

- **Pros**: No mixer to own, no queues, no level curve. Matches what the browser
  SDK does and what the original design assumed.
- **Cons**: There is nothing to wait for. The native LiveKit API hands over one
  decoded stream per participant by design; mixing is the application's job on
  every native platform.
- **Why not**: Not a trade-off, a mistake in the original reading of the API.

### Alternative 2: One queue for everybody, mixed on arrival

- **Pros**: One cap to reason about, no per-person map, no wholesale level
  replacement.
- **Cons**: Per-person volume becomes impossible after the fact, and one late
  participant drags the shared queue past its cap, dropping audio from people
  whose packets arrived on time.
- **Why not**: Per-person volume is the feature that needs the separation, and
  the failure mode punishes the wrong people.

### Alternative 3: A limiter instead of a hard clamp

- **Pros**: No clipping distortion, ever.
- **Cons**: Audible ducking on every overlap, which in a call of four is most of
  the time. Trades a rare, brief artifact for a constant one.
- **Why not**: Deferred rather than rejected. If per-person boost turns out to
  clip in ordinary use, a limiter with a high threshold is where to go, and
  `clamp` in `mixing.rs` is the single place it would land.

## Consequences

- `consort-audio` owns a realtime mix path, so the rules that go with one apply:
  no allocation in a device callback, no lock held across one, and a poisoned
  mutex is recovered from rather than panicked on, because a panic on the audio
  thread takes the sound card with it.
- A silent call is now a diagnosable failure rather than a clean log. The events
  in `AudioEvent::CallAudioFailed` exist so that a call nobody can hear does not
  look like a call nobody is speaking in.
- The level curve is shared by the master, the notifications and each person, so
  a change to `gain()` moves all three together. That is deliberate: three
  curves would mean a slider that behaved differently depending on which one it
  was attached to.
