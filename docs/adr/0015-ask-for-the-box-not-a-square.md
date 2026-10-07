# 15. Ask the SFU for the box, not a square of its long edge

Date: 2026-10-06

## Status

Accepted and acted on, for the first half. The second half, the ceilings in
[ADR-0013](0013-ask-for-a-picture-in-pixels.md), is a decision nobody has
taken yet: see the consequences.

## Context

[ADR-0013](0013-ask-for-a-picture-in-pixels.md) settled that a receiver asks
for a stream in pixels, as `MediaConstraints::detail` carrying
`VideoDetail::Dimensions`. [ADR-0014](0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md)
settled that the number is the box the picture is drawn into, named by the
window on every poll.

The first build of both asked for a square. `their_view(user, kind, bound)`
takes one number, so `request` sent `Dimensions { width: bound, height: bound }`,
with a comment saying the still is sampled to fit a square so a square is what
to ask for.

The still is not sampled to a square. `Picture::thumbnail(bound, bound)` fits
the frame *inside* a square and keeps its shape, which ADR-0014's own table
says: 320 gives 320 by 180, 960 gives 960 by 540. The box drawn is as wide as
`bound` and as tall as the frame's shape makes it.

That matters because of which edge an SFU reads. `UpdateTrackSettings` carries
a width and a height, documented in `livekit_rtc.proto` as the size "to
receive", and livekit-server picks the lowest simulcast layer that covers the
request by comparing **heights**. So a square asks for a picture as tall as the
box is wide. For 16 by 9, which every screen share and most cameras are, that
is 1.78 times the height anything draws, and it lands one or two layers up the
ladder.

Issue #182's comment is what that looks like from a chair: "auto is setting it
to high quality, but then when I change it to low, nothing happens." A share
drawn across the window asked for 1920 by 1920 on auto and 640 by 640 on low.
A 1080p publication's layers are 1080, 540 and 270 tall, and the only one of
those that is 640 tall is the one it was already sending. Both choices
resolved to the top layer, so the control moved and the picture did not.

## Decision

**Ask for the box, measured the same way the still is measured.**

`consort_call::Asked` is a width and a height, `Wanted` holds one per person
per kind, and a cap brings its long edge down to the ceiling while keeping its
shape. `TheirViews::drawn_at` answers with `Picture::shape_in`, which is the
box the window has at the shape the frame is, so what is asked of the SFU and
what is drawn are the same number wherever there is more picture than box.

**And the box is never the frame's own size.** The first build of this answered
with `Picture::thumbnail_size`, which stops at the frame rather than filling
the box, on the grounds that the two should be measured identically. They
should not, because the frame is the answer to the last ask: a box that shrank
to the layer it was sent made the ask a function of its own output, which can
only ever ratchet down. A `Low` cap pulled a 1080p share to the half layer,
`thumbnail_size` then reported a 960 by 540 box, and lifting the cap asked for
960 by 540 again. Whether that stayed stuck or walked back up a layer at a time
turned on a rounding rule inside livekit-server that this repository cannot see
and must not lean on. `shape_in` scales a small frame up, so the ask depends
only on the window and the aspect ratio.

A square is still the answer in one case: before the first frame, when there is
no shape to go on. It is the safe over-ask, and the poll after the first frame
corrects it.

## Consequences

**Every picture now asks for less than it did**, cap or no cap, which is the
bandwidth half of `docs/PLAN-receiving-quality.md` phase 1 actually landing.
A face tile asks for 320 by 180 rather than 320 by 320, and on a 720p camera
that is a layer down.

**ADR-0013's ceilings still do almost nothing, and that is the open decision.**
They are absolute numbers on the long edge, applied as the smaller of the
ceiling and the box, so a cap cannot ask for less than is being drawn. Set
against the four boxes ADR-0014 draws into:

| Where it is drawn | Box | Auto | High | Medium | Low |
| --- | --- | --- | --- | --- | --- |
| A face tile, or a waiting screen | 320 | 320 | 320 | 320 | 320 |
| The stage on the floating card | 480 | 480 | 480 | 480 | 480 |
| The stage on the expanded card | 960 | 960 | 960 | 960 | 640 |
| The stage filling the window | 1920 | 1920 | 1920 | 1280 | 640 |

Two things follow, and neither needs an SFU to see. `High` can never differ
from `Auto`, because its ceiling is 1920 and `theirview::MAX_BOUND` is also
1920, so the box is never above it. And on a tile and on the floating card,
which is where a picture is drawn unless somebody has gone looking, all four
choices are one choice.

**A screen share has two layers, not three.** livekit's
`compute_default_simulcast_presets` returns a single extra preset for a
screenshare source ("Only one additional layer for screenshares. (Prioritize
quality)"), so a share publishes its full resolution and half of it at 3 fps,
and nothing between. A 1080p share therefore offers 1080 and 540, which puts
`Medium`'s 1280 ceiling (720 tall) and `High`'s 1920 on the same layer as
`Auto`: only `Low` reaches the other one. Whatever replaces the ceilings has
two rungs to aim at for a share and three for a camera, and the lower rung of a
share costs 3 fps.

A control that offers four options and acts on one combination of the sixteen
is not a working control. The fix is not another ceiling: it is that a cap has
to be relative to the box rather than absolute, because the box is already the
best size worth asking for and "less" means less than that. Thirds or quarters
of the long edge would map onto a simulcast ladder, which is built by halving.
Which words a person should see, and what each should mean, is a product
decision rather than this one, so it is left open here rather than guessed.
