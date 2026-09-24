# ADR-0004: Probe every device, and prefer the host's default over a saved name

**Date**: 2026-08-26
**Status**: accepted
**Deciders**: tominal, with Claude Code

Recorded on 2026-09-24, from the comments in `devices.rs` and `cpal_host.rs`.
The decision itself was made in "the voice gate and the device catalogue" and
refined after.

## Context

A device picker looks like a solved problem: ask the host what it has, show the
list, save what somebody chose, reopen it by name next time. Each of those four
steps is wrong in a way that only shows up on a real desktop.

### The host is guessing

Whether a device works in a given direction is something a host may only be
declaring. cpal's ALSA backend derives it from each PCM hint's `IOID` field, and
cpal's own source notes that a hint leaves that field NULL to mean "either". So
the answer is a declaration, not a measurement.

On a PipeWire desktop those declarations are wrong often enough to be
embarrassing. The observed result was a webcam offered as a place to play sound
out of, and a pair of speakers offered as a microphone.

### The list is mostly plumbing

A PipeWire desktop offers 21 input devices, of which 12 are ALSA's plugin
wrappers: rate converters, and the null device that discards everything. Nobody
has ever wanted to be recorded by a resampler, and a list that long where most
of it is plumbing is a list people scroll past instead of read.

### A saved name is a photograph

The host's default is a live answer; a saved name is a record of what the list
said once. Plug in a headset on Windows or macOS and the system default moves,
which is exactly what somebody who never opened this screen expects to happen. A
saved name cannot move.

Worse, cpal 0.18 removed `Device::name()` and offers only `Display`, so a name
is all the identity there is. Two identical capture cards are indistinguishable,
and re-resolving a saved name can land on the wrong twin.

### Asking is not free of consequences either

If probing a device is treated as pass/fail, a device somebody else has open
fails. The other process is usually Consort itself, holding the microphone for
the level meter on the very screen the list is being drawn on. Naive probing
therefore deletes the device in use from the picker offering it.

## Decision

Four rules, one per problem above.

**Probe, do not trust.** `cpal_host::collect` asks each candidate what it
supports and keeps only those offering something at `SAMPLE_RATE`. That is the
same requirement `AudioCapture::open` enforces a moment later, which is the
point: a picker should not offer a device that selecting it would fail on. It
costs about 300 ms for a whole ALSA namespace, and nothing measurable on WASAPI
or CoreAudio, where endpoints are enumerated rather than guessed at.

**Sort the reasons a probe can fail, rather than collapsing them.**
`devices::Answer` is that sort, and it is tested. The two mistakes available are
not the same size: listing a device that turns out not to work costs somebody one
confusing attempt, while hiding one that does work costs them the conclusion that
Consort cannot hear them, with nothing on screen to argue with. So a device is
dropped only on a definite `No`, `Absent` or `Forbidden`, and `Busy` and
`Unclear` stay listed. `Busy` in particular is proof the device works.

**Filter ALSA's wrappers by how ALSA names them**, not by any use of the words.
Dropping somebody's actual microphone would be far worse than leaving a
resampler in the list. Sound servers (PipeWire, PulseAudio, JACK) are
deliberately kept: on a modern Linux desktop they are the entries most worth
selecting.

**Ask for the default by asking for the default.** `Selection::name_to_open`
returns `None` when the resolved device is the one the host already calls its
default, so the backend is asked for its live answer rather than for a name.
This is a different question from `Selection::device`, which answers what to
draw as selected, and the two differ in exactly that one case.

The exception is a host that lists devices and flags none as default. There is
nothing to defer to, so the fallback is named: asking for the default there
fails with "there is no audio input device" on a machine that visibly has one.

## Alternatives Considered

### Alternative 1: Trust the host's direction flags

- **Pros**: No probe, so no 300 ms on ALSA, and much less code.
- **Cons**: Offers a webcam as an output on the most common Linux desktop
  configuration. Observed, not hypothetical.
- **Why not**: The list is the feature. A list that is wrong is worse than a
  slower one.

### Alternative 2: Treat any failed probe as a no

- **Pros**: One boolean, no `Answer` enum, no test matrix for host error kinds.
- **Cons**: Deletes the microphone Consort is itself holding from the picker
  that is at that moment drawing its level.
- **Why not**: The failure lands on the most common case rather than a rare one.

### Alternative 3: Always reopen the saved name, including when it is the default

- **Pros**: One code path, and what somebody picked is what they get.
- **Cons**: A newly plugged-in headset does not become the device in use, which
  is what every other application on the machine does. And with only a display
  name for identity, it can resolve to the wrong one of an identical pair.
- **Why not**: The person who never opened this screen is the one this hurts,
  and they are the majority.

## Consequences

- Two identical devices are still indistinguishable, and a saved choice between
  them can resolve to the wrong twin. Known and accepted: fixing it needs an
  identity cpal does not expose.
- Enumeration is slower on ALSA than a bare listing would be, paid once per time
  the settings screen asks.
- `devices.rs` decides everything and takes the device list as data;
  `cpal_host.rs` only talks to cpal. That is what lets all of the above be
  tested on a CI runner with no sound card, and it is why `cpal_host.rs` is
  excluded from the coverage numbers: the exclusion is only honest while that
  file stays thin.
