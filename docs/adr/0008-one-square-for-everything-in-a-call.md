# 8. One square for everything in a call, and a second still to fill one

Date: 2026-10-01

## Status

Accepted. Closes the local half of what
[ADR-0007](0007-draw-the-self-view-from-a-still.md) left open. Other people's
cameras and screens are still open: see `docs/PLAN-webcam.md` phase 2.

## Context

The review of #140 asked for four things at once, and they are one design
rather than four: a shared screen previewed the way the camera is, the avatar
on the floating card becoming a square the camera fills, a square of its own
per shared screen with two people sharing meaning two squares, and clicking a
shared screen filling the window with the card.

The simplification underneath them is the point. A face, a camera and a shared
screen were about to become three special cases on one card. They are one kind
of thing: something the call is carrying, drawn in a square.

ADR-0007 settled how one local still reaches the webview and deliberately left
the next question open, saying that several tiles at once would want a key per
participant and probably a URI scheme, because the bytes stop being incidental
once they are multiplied by a roster.

The question this change actually faces is narrower than that. There are two
local captures, a camera and a screen, both already in this process. There is
no roster of frames, because nothing carries a remote frame into this window
at all.

Measured against this machine's own display, by
`selfview::tests::measure_a_real_screen`:

| | Bytes |
|---|---|
| One captured 2560x1440 screen frame | 5,529,600 |
| The same frame as a tile picture, base64 in a `data:` URL | 3,007 |

## Decision

**A second slot, not a keyed map.** `SelfView` is instantiated twice, the
screen gets its own `screen_view` command, and one hook polls either by name.
A map keyed by participant would have exactly two keys, both known while this
is being written, and would be an abstraction built for a case that does not
exist yet. When remote frames arrive they bring their own question, and
ADR-0007's answer to it is still the one to start from.

**The bound stays 320 pixels for both.** A square is 48 pixels on the card, 72
expanded and 160 filling the window, so 320 is already more than the largest of
them. Reading what is written on somebody's desktop is a different problem, and
it belongs with the layout the review deferred rather than with the pixels.

**Every tile is the avatar's box.** A camera fills the square its owner's face
was in; a shared screen gets a square the same size. The ring that marks
somebody talking moved from the avatar to that box, so a camera is ringed
exactly as a face is, and the box squares its corners when a picture fills it.

**Somebody else sharing gets a square with no picture in it.** The roster now
carries `screen`, read from `MediaStreamKind::ScreenShare` exactly as `camera`
is read from `Camera`, so the card knows who is presenting. Drawing what they
are presenting is phase 2 and is not pretended at: the square holds a monitor
glyph and their name.

**Filling the window is a third size of the same card, not a second view.** The
review deferred the layout, so nothing is rearranged and nothing new appears:
the squares grow, as they already do when the card is expanded. Escape, the
card's own control, another click on the screen, and a double-click all come
back from it.

## Consequences

One more encode per drawn picture while somebody is both sharing and watching
the card: 9.1 ms to convert and encode, 93 microseconds to sample a frame down
on the capture thread. At twelve and a half pictures a second that is about an
eighth of a core for the screen, and a quarter with a camera beside it, on a
dev build with `consort-video` at `opt-level = 2`.

The picture is 3 KB, which is the camera's order of magnitude rather than the
roster-multiplied one ADR-0007 was worried about. A desktop sampled to 320
pixels across compresses like a photograph because the text in it has stopped
being text. That is the honest limit of this preview: it says which window
layout is going out, not what it says.

A frame arriving redraws one leaf. The picture is a component rather than a
string the card holds, which is what keeps #142's trap shut now that there are
two of them: a picture held one level up is every face in the call redrawn
twelve and a half times a second.

The caption under a square truncates at the smallest card size, where a tile is
48 pixels wide. The call panel's indicator names what is going out in full, and
expanding the card widens the caption.
