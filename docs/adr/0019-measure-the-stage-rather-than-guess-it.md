# 19. Measure the stage filling the window, rather than guess at it

Date: 2026-10-08

## Status

Accepted and acted on. Numbered 19 because 16, 17 and 18 are taken by the
three quality changes that are open but not yet on main.

## Context

[ADR-0014](0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md) made the
size a picture is asked for travel with the ask, and put the sizes in a table
in `CallCard`: 320 for a square, 480 for the stage on the floating card, 960
expanded, 1920 once a share fills the window. The first three are twice the
width CSS fixes those boxes at, so a display at two device pixels per CSS pixel
gets a true pixel for each one.

The fourth is not a box CSS fixes. `.call-card[data-size="full"] .call-stage`
takes the width of the window and whatever height the bar, the padding and the
row of faces leave it, and the picture inside is `contain`ed in that. So 1920
is a guess at a number only the window knows, and it is wrong in both
directions:

| Window, at one device pixel per CSS pixel | Stage picture, about | Asked | Result |
| --- | --- | --- | --- |
| 1366 x 768 | 1340 x 600 | 1920 | A 1920 still encoded for a 1340 box, and the SFU sent a layer to match |
| 1920 x 1080 | 1890 x 850 | 1920 | Right, which is the case it was chosen for |
| 2560 x 1440 | 1950 x 1100 | 1920 | Right by accident: the stage is `contain`ed, so a 16 by 9 share in a short box is not as wide as the window |
| 2560 x 1440 at two device pixels | 3900 x 2200 | 1920 | Half the detail the screen can draw |

The over-ask is paid twice, in the layer the SFU forwards and in the encode of
a still nothing draws at that size, which is the waste
`docs/PLAN-receiving-quality.md` phase 1 exists to stop. The under-ask is issue
#165's second sentence: "it automatically adjusts based on both the user's
network speed + size of the window."

## Decision

**Measure the box, and ask for it in device pixels.**

`app/src/lib/stageBound.ts` holds both halves. `stageBound(box, ratio)` is the
arithmetic: the long edge, in device pixels, rounded up to a step of 64 and
clamped to a ceiling of 1920. `useStageBound` is the measurement, from
`getBoundingClientRect` on the element the picture fills, taken again on a
window resize and on the card changing shape. `CallCard` uses it for the stage
while it fills the window and keeps the constants for the boxes CSS fixes.

Three numbers in that are choices rather than facts:

**The long edge, not the fitted picture.** A share is `contain`ed in the box,
so the picture is narrower than the box whenever the box is taller than the
share. Asking for the box over-asks by that letterbox and never under-asks,
and the shape the SFU is told is `Picture::shape_in`'s, which Rust already
computes from the frame rather than from this number.

**A step of 64.** The bound restarts the poll and is what tells the SFU which
layer to send, so measuring to the pixel would be a constraint per frame of a
window drag.

**A ceiling of 1920**, unchanged. It is `theirview::MAX_BOUND` and `selfview`'s
clamp, so asking past it would name a size nothing will make.

## Consequences

**A window smaller than the ceiling stops paying for pixels nobody sees.** That
is most windows: anything under about 1920 across at one device pixel per CSS
pixel.

**A display with more pixels than the ceiling still draws an upscale**, because
the ceiling did not move. Raising it is a cost decision rather than this one:
the encode is near-linear in pixels, and
`theirview::tests::measure_what_a_remote_picture_costs` puts a 1920 still at
about 29 ms of a core on worst-case content, against a poll every 80 ms. A
2560 ceiling is about 1.7 times that and a 4K one about four times, which is
past the poll interval. The two ways out are to accept the CPU or to pair a
higher ceiling with a slower poll for a large picture, since a desktop being
presented is mostly still. Neither is a bug fix and neither is taken here.

**Issue #165's reported symptom is not this.** The picture that was stuck when
a share filled the window was this session's own, drawn from the 320 pixel
still `selfview` bounded, which `docs/PLAN-receiving-quality.md` diagnosed and
which issue #194 fixed. This is the other half of the same sentence, and it is
the half nothing had built: the size of the window.

**The network-speed half is still not built.** Nothing in this path sets a
bitrate, and an automatic ladder is phase 3 of
`docs/PLAN-receiving-quality.md`: a sampler over `Call::receive_stats`, a
verdict with hysteresis, and a person's own choice overriding it.
