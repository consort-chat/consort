# ADR-0003: Measure who is talking from the samples, not from the SFU

**Date**: 2026-09-02
**Status**: accepted
**Deciders**: tominal, with Claude Code

Recorded on 2026-09-24, from the comments in `talking.rs`. The decision itself
was made in "light the rings from the audio we already handle".

## Context

LiveKit reports active speakers, and Consort drew its green rings from that
report to begin with. It answers a different question from the one the rings
ask.

Dominant-speaker detection is built to pick out the one or two people worth
putting on a video tile in a meeting of thirty. So it is smoothed, it is
thresholded against everybody else in the room, and it is deliberately slow to
change. Three separate judgements had to agree before a ring lit, and the last
of them was made on a machine across a network.

In a voice channel of three people that produced a ring you could only light by
leaning into the microphone, and somebody else's ring you might never see at
all. The feature was not subtly wrong, it was unusable at the size Consort is
built for.

Consort does not need to know who is dominant. It needs to know who is audible,
which is a question about samples, and every sample is already on this machine:
ours on the way out and everybody else's on the way in, because
[ADR-0002](0002-consort-mixes-the-call-itself.md) means this client decodes and
mixes the call itself.

## Decision

`consort_audio::talking` keeps the tally locally. A frame whose peak clears
`FLOOR` lights that person for `HOLD_FRAMES`, and the tally reports only when
the lit set changes.

### The same rule in both directions

Both directions are measured on the frames that actually travel. Ours are the
gate's output, so they are already zeroed while it is shut, and theirs have been
through the gate on the sending machine.

Nothing here consults the gate's verdict directly. That is what keeps the
picture honest in both configurations: with voice activity switched on a ring
follows the gate, and with it switched off a ring follows the room, because that
is what the other person is hearing in each case.

### Two numbers, and why neither is what it looks like

`FLOOR` is 0.005, deliberately near the bottom. The question is "is there
anything there", not "is this speech": the gate has already made the second
judgement by the time these samples exist, and making it twice is how the rings
got slow in the first place.

It is not zero, though, and cannot be. Opus reconstructs silence as comfort
noise rather than as zeroes, so an exact test for silence would light every ring
in the call for as long as anybody was connected.

`HOLD_FRAMES` is 20, which is 200 ms. The gap between two words is longer than
one frame and shorter than this, so a ring holds across a sentence instead of
stuttering once per syllable, and still goes dark within a fifth of a second of
somebody actually stopping.

### Counted, not clocked

Frames arrive every 10 ms by construction, so the hold is a frame count and
there is no `Instant` anywhere in the module. The same reasoning as
`consort_audio::meter`: it makes the tests exact, and it makes them fast.

The tick is this session's own capture, because that is the one clock in the
building that runs whether or not anybody is saying anything. The consequence is
that leaving a call stops the tick, which is why `Talking::quiet` exists: without
it the last people talking would stay lit until the next call.

## Alternatives Considered

### Alternative 1: Keep using LiveKit's active speaker events

- **Pros**: Nothing to write, nothing to tune, and the SFU sees the whole room.
- **Cons**: Answers "who is dominant", not "who is audible". Smoothed and
  thresholded against the room, so it is slow and it scales its answer to the
  meeting size. Unusable in a channel of three.
- **Why not**: Tried first, and reverted. This ADR exists so it is not tried
  again.

### Alternative 2: Ask the gate directly for our own ring

- **Pros**: No second measurement of this session's own audio.
- **Cons**: Our ring would then mean something different from everybody else's,
  and would go on meaning it after somebody switched voice activity off, when
  the gate's verdict no longer describes what leaves the machine.
- **Why not**: One rule for everybody is the property that makes a lit ring
  readable as "this is reaching people".

### Alternative 3: Key the tally by membership rather than by person

- **Pros**: No resolution step in the caller, and it matches how the mixer keys
  its queues.
- **Cons**: Somebody in a call on a laptop and a phone would be two lit rings.
- **Why not**: The rings are about people. The caller resolves a member ID to a
  Matrix user ID before it gets here, and that is the reason.

## Consequences

- The rings light within 10 ms of an audible frame, against the several hundred
  milliseconds the SFU's detector took.
- `advance` returns `None` while nothing changes, which is the ordinary answer:
  a settled call is somebody talking for several seconds, and a caller that
  emitted every frame would put a hundred messages a second across the IPC
  boundary to repeat itself.
- The measurement costs one peak scan per frame per participant, on audio that
  has already been decoded for the mixer.
