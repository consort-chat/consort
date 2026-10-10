# 16. Only a person may ask a share for less, because the rung below its best is 3 fps

Date: 2026-10-08

## Status

Accepted and acted on. Supersedes the open question at the end of
[ADR-0015](0015-ask-for-the-box-not-a-square.md), which proposed making a cap
relative to the box. It cannot be: for a shared screen the box is already
below the only other rung.

## Context

Issue #195: "When I changed the quality on another user's card, it did not do
anything with the resolution and instead the video stream appeared more choppy
than anything."

Both halves of that are true, and neither is a bug in the ask.

**What the control sets.** `PersonMenu`'s select calls `set_person_quality`,
which reaches `consort_call::Wanted::cap` on the call thread.
`Wanted::asked` folds it with the box the card is drawing into, and
`CallSession::request` sends it as `MediaConstraints::detail`. The LiveKit
transport turns `VideoDetail::Dimensions` into `update_video_dimensions`, which
is an `UpdateTrackSettings` carrying a width and a height. So this is
receive-side **simulcast rung selection**, not a bandwidth cap and not a
degradation preference: nothing in the path sets a bitrate or asks the
publisher for anything.

**How the SFU reads it.** `MediaTrackReceiver.GetQualityForDimension` sorts the
published layer heights, multiplies the requested height by
`layerSelectionTolerance` (0.9), and takes the first layer at or above it. The
quality it answers with is that layer's **index**, and `applySettings` uses the
dimensions whenever the width is above zero and the `Quality` field only when
it is not.

**What a Consort publisher publishes.** `TrackPublishOptions` is left at its
defaults bar `source` and `simulcast`, so livekit computes the ladder from the
source resolution:

| Published | Rungs |
| --- | --- |
| A 1080p screen share | 1920x1080 at 30 fps, 960x540 at **3 fps** |
| A 720p camera | 1280x720 at 30, 640x360 at 20, 320x180 at 15 |

A screenshare source gets one extra preset and no more
(`screenshare::compute_default_simulcast_preset`, `FPS = 3.0`, under the
comment "When simulcasting, we prefer to reduce the FPS"). A camera gets two.

Put the three together against the boxes [ADR-0014](0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md)
draws into, and a 1080p share resolves like this:

| Where it is drawn | Box | Height asked | Rung |
| --- | --- | --- | --- |
| A face tile | 320x180 | 180 | 540 at 3 fps |
| The stage on the floating card | 480x270 | 270 | 540 at 3 fps |
| The stage on the expanded card | 960x540 | 540 | 540 at 3 fps |
| The stage filling the window | 1920x1080 | 1080 | 1080 at 30 fps |

So a share was already at three frames a second everywhere but full window,
before anybody touched the control, and `Low` is what drops a full-window
share onto the same rung. That is the choppiness in the report. The resolution
did change, by half, and it is invisible because the still is drawn at the size
of its box either way: a smaller frame is scaled up to the same number of
pixels on screen, so a rung swap reads as blur and never as a smaller picture.
A share at 4K cannot reach its top rung at all, `theirview::MAX_BOUND` being
1920.

## Decision

**The box decides a camera. Only a person decides a share.**

`Wanted::asked` answers a `Ask::Size` for a camera, as before: the box it is
drawn in, brought under any ceiling. Every rung of a camera ladder carries 15
frames a second or more, which is above the rate the card redraws at, so
spending a rung to save bandwidth costs nothing anybody can see.

For a screen share it answers `Ask::Best` unless somebody has chosen less, and
a chosen cap is that cap's ceiling at the shape it is drawn, not the box under
the ceiling. A cap is a choice about the stream, so it asks for the same thing
wherever the card is floating.

**`High` caps nothing.** Its ADR-0013 ceiling of 1920 is 1080 tall, which picks
the lower rung of anything published above 1080p, and the most a person can ask
for should be the most there is. `Cap::High.ceiling()` is therefore `None`,
which leaves `Medium` and `Low` as the only two ceilings.

**`Ask::Best` goes on the wire as `Quality(High)`, not as `Auto`.**
`VideoDetail::Auto` is the honest word for it and the LiveKit transport
implements it as doing nothing, so a size already sent would stand: somebody who
tried `Low`, disliked it and went back to `Auto` would be stuck at 3 fps for the
life of that membership. `set_video_quality` sends a full-replacement
`UpdateTrackSettings` with zeroed dimensions, which is exactly "your best" to
the SFU. The field is deprecated on the wire and the transport applies it only
while a stream is `Active`; no video stream here is ever paused, so neither
bites.

## Consequences

**A share is at its publisher's frame rate again**, at every card size, which
is the user-visible half of this.

**It costs the bandwidth ADR-0015 saved on shares.** An uncapped share in a 320
pixel strip tile now pulls the full publication rather than half of it at 3 fps.
That is the trade: the saving was only ever available at a rung nobody can watch
move, and a person who wants it can pick `Low`.

**Three of the four choices still do the same thing on a share.** `Auto`, `High`
and `Medium` all land on the top rung of a 1080p publication (`Medium`'s 720
becomes 648 under the tolerance, which is above 540), and `Low` lands on the
other one, at 3 fps. A two-rung ladder cannot honour a four-position control.

**What is left is on the send side**, and is issue #196. A share's lower rung is
3 fps because the publisher built it that way, and no receive-side request of
any shape can ask for a rung that was never published. Two levers exist, both in
`TrackPublishOptions` and neither exposed by `matrix-rtc-media`'s
`PublishOptions`: `simulcast_layers`, which would replace the screenshare preset
with rungs that keep their frame rate, and `scalability_mode`, where SVC spatial
layers share one frame rate by construction. Until one of them is reachable,
"less resolution at the same frame rate" is not a thing this client can ask for,
and whether the control should keep four positions is a product decision that
waits on which rungs exist.
