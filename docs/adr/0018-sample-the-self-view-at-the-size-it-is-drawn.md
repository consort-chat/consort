# 18. Sample the self view at the size it is drawn

Date: 2026-10-08

## Status

Accepted. Issue #194: the person sharing a screen saw their own share at 320
pixels however large the card was, while everybody else in the call saw it at
the size of the box it was drawn in.

## Context

[ADR-0007](0007-draw-the-self-view-from-a-still.md) sampled each captured frame
down to 320 by 320 on the capture thread, because a self view was a square 48
pixels across and the alternative was a megabyte of I420 per tick.
[ADR-0008](0008-one-square-for-everything-in-a-call.md) kept that bound for the
shared screen, saying outright that reading what is written on somebody's
desktop "is a different problem".

[ADR-0009](0009-a-shared-screen-takes-the-stage.md) made it the same problem. A
share is no longer a square: it takes the stage, which is 480 pixels on the
floating card, 960 expanded and the whole window when the stage is clicked.
[ADR-0014](0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md) fixed that
for everybody else's share by sending the box with the ask, and said what it
was leaving: "the stage now draws somebody else's share at up to 1920 and our
own at 320 upscaled. Moving that bound is a different change, in
`app/src-tauri/src/selfview.rs`, with a different cost profile: it is paid per
captured frame rather than per drawn picture."

That is #194. Nothing about simulcast is involved: the sender never subscribes
to its own track, so there is no rung to choose and nothing to unpin. The
picture was a local still fixed at the size a tile needed in 2026.

A window is the worst case, which is why the report came from somebody sharing
an application. The bound is square, so a portrait window loses on its long
edge: a 1000 by 1400 window sampled into 320 by 320 is 228 pixels across.

## Decision

The capture thread keeps the frame. `SelfView::latest` takes the long edge of
the box being drawn, samples the kept frame to it, and encodes. The ask travels
from `SelfPicture` through `usePicture` and `screen_view`, which is the route
`TheirPicture` already takes, and is clamped by `theirview::clamped` so both
sides share one ceiling.

## Consequences

**The capture thread got cheaper, not dearer.** ADR-0014 expected the opposite.
Per 1080p frame, measured by
`selfview::tests::measure_the_cost_of_sampling_when_it_is_drawn`:

| | Per captured frame |
|---|---|
| Sampling to 320, which is what it used to do | 86 us |
| Copying the frame, which is what it does now | 51 us |

**The encode is paid per drawn picture, at the size drawn.** 0.9 ms at 320,
1.8 ms at 480, 7.3 ms at 960 and 29 ms at 1920, so a full-window share costs
about a third of a core at twelve and a half asks a second. That is what
everybody else's full-window share already cost, and only while the stage is
actually filling the window. ADR-0014's table has the bytes for a real desktop
at each bound; the sizes this measurement prints are a flat test frame's.

**A self view holds a frame rather than a thumbnail.** One slot per thing being
sent, so sharing a 4K screen with a camera on is about 15 MB resident rather
than 170 KB. The same shape `TheirViews` already has, per remote stream.

**The string cache is gone.** It existed so a card asking faster than the
camera produced frames would not blink; keeping the frame does that without it,
and a cached string cannot answer two different bounds.
