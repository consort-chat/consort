# 9. A shared screen takes the stage, and everything else goes under it

Date: 2026-10-02

## Status

Accepted. Takes up the layout
[ADR-0008](0008-one-square-for-everything-in-a-call.md) deferred. Other
people's cameras and screens are still open: see `docs/PLAN-webcam.md` phase 2.

## Context

ADR-0008 made every tile the avatar's box and said so outright: filling the
window is a third size of the same card, nothing is rearranged, and the layout
past that is deliberately not decided yet.

Left there, a call about somebody's screen drew that screen 48 pixels across in
the top left of the card and left the rest of the panel empty. The review of
#140 marked up the empty half and asked for the screen to fill it, with
everything else, other shared screens and the people in the call, below.

## Decision

**One stage, and a strip under it.** The screen the call is watching runs the
full width of whatever the card is. Every other screen and every person goes
underneath, in the two lists that were already there, at the size they already
were. A person and a screen nobody is watching are still the same kind of
thing, which is ADR-0008's point and is why the squares did not change.

**Ours takes the stage, and a click overrides it.** The default is the first
screen the card lists, which is this session's own while it is sharing. It is
the only share that can draw a picture: nothing carries a remote frame into
this window, so staging anybody else's would put a monitor glyph where a live
desktop could be. Clicking a tile stages it and keeps it until that share
stops, at which point the default takes over again.

Most recently started was the other candidate. It needs the order shares
arrived in, which the roster does not carry, so it needs state of its own
synchronised against a list derived every render, and it buys nothing while
only one of the screens on the card can show anything.

**16:9 while the card floats, and the leftover height once it fills the
window.** A stage with no shape of its own takes it from a picture that arrives
twelve and a half times a second, which would move every face under it each
time. A 16:9 box as wide as the window, though, is 9/16 of that again in
height, which a shorter window has nowhere to put. Measured in Chromium at
1280x800, the picture came out 49 pixels taller than the stage clipping it,
which is a cropped desktop and the one thing this must not do. So the
window-filling card is a column, the stage is handed what the header and the
squares leave, and the picture letterboxes into whatever shape that is.

Measured at a 1920x1080 source and a 1024x768 one, by the markup the component
renders laid out in headless Chromium:

| Card | Stage box | 16:9 source drawn | 4:3 source drawn |
|---|---|---|---|
| Floating | 222 x 125.8 | 220 x 123.8 | 165 x 123.8 |
| Expanded | 394 x 222.5 | 392 x 220.5 | 294 x 220.5 |
| Filling 1280x800 | 1248 x 493.5 | 873.8 x 491.5 | 602 x 451.5 |
| Filling 1920x1080 | 1888 x 773.5 | 1371.6 x 771.5 | |

Every drawn box is 1.778 or 1.333 to three places, which is the source's own
ratio. Nothing is stretched and nothing is cropped at any of them, and no width
down to a 320 pixel viewport overflows.

## Consequences

**The 320 pixel bound is now the limit people will notice.** ADR-0008 set it
and argued that a square was at most 160 pixels, so 320 was already more than
the largest. That is no longer true. The stage draws the same still at:

| Card | Upscale |
|---|---|
| Floating | 0.69x, still downscaled |
| Expanded | 1.23x |
| Filling 1280x800 | 2.73x |
| Filling 1920x1080 | 4.29x |

ADR-0008 said reading what is written on somebody's desktop belonged with the
layout rather than with the pixels. The layout has landed, so the pixels are
next, and `selfview::BOUND` is where it starts. Nothing here changes it.

**A click on a tile means something new.** It used to fill the window, which is
now what a click on the stage does. The tile's accessible name says which, and
the stage keeps one name with `aria-pressed` for its state, the way the card's
other controls do it.

**A second person sharing shrinks the stage in the window-filling view.** The
squares are 160 pixels there and the strip is above nothing, so a call with two
shared screens gives the stage 279 pixels of height at 1280x800 rather than
493. That is the honest cost of keeping every square the size ADR-0008 set, and
it is the trade the strip exists to make.
