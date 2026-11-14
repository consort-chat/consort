# 14. Ask for a remote picture at the size it is drawn

Date: 2026-10-05

## Status

Accepted. Issues #69 and #70: the receiving half of a camera and of a shared
screen. The sending half of both already worked, so two people could each put a
camera and a desktop into a call and each see nothing of the other.

## Context

Nothing in this repository ever asked for a remote video track.
`Call::remote_track` had two call sites and both passed
`MediaStreamKind::Microphone`, so no video frame stream was ever opened. The
frames were arriving regardless: `auto_subscribe` defaults to true and nothing
here overrides it, so every remote camera and screen share was subscribed,
decoded and dropped.

So the question is not how to get frames. It is how a frame reaches the webview,
and [ADR-0007](0007-draw-the-self-view-from-a-still.md) answered that for one
local camera: the capture thread samples each frame down to 320 pixels and keeps
the newest, a command encodes it to a `data:` URL, and the card draws it in an
`<img>` on a timer. It also said what it had not settled, and predicted against
itself: several remote tiles "would want a key per participant, and probably the
URI scheme rather than a `data:` URL per tile per tick, because the bytes stop
being incidental once they are multiplied by a roster".

Two open issues bear on the size rather than the transport. #165 says the
picture is bad when a share fills the window. #167 asks whether a receiver can
choose the quality of a stream it is receiving, and
[ADR-0013](0013-ask-for-a-picture-in-pixels.md) already settled that the control
is `MediaConstraints::detail` as `Dimensions`, which is the size of the box the
stream is drawn into: the one number the webview has and Rust does not. So
whatever is built here has to carry a readable desktop and has to be able to
change that size at runtime, or #167 cannot be built on it.

## Decision

**The same polled still, keyed per membership and per kind, with the size as a
parameter of the ask.** Four parts, and the third is the new one:

1. One pump per remote stream, started and stopped from the roster, exactly as
   [`hearing`](../../crates/consort-call/src/hearing.rs) does for audio. It
   keeps the newest frame as the transport handed it over, strides and all, so a
   frame costs a move rather than a copy.
2. Nothing is sampled and nothing is encoded until something draws. A poll with
   no new frame behind it redraws the frame that is there, so a card asking
   faster than a camera sends does not blink.
3. **The caller names the pixels.** `their_view(userId, kind, bound)` samples
   the newest frame to fit a square of `bound` and encodes that. A face tile
   asks for 320 and the stage asks for up to 1920.
4. Rust never upsamples and never exceeds `MAX_BOUND`, so a frontend cannot ask
   for a picture larger than the frame or larger than the ceiling.

| Where it is drawn | Pixels asked for |
| --- | --- |
| A face tile, or a screen waiting under the stage | 320 |
| The stage on the floating card | 480 |
| The stage on the expanded card | 960 |
| The stage filling the window | 1920 |

The first three are about twice the width the box is drawn at, so a display at
two device pixels per CSS pixel gets a true pixel for each one. The fourth is
the ceiling instead, because twice a window is past it: 1920 is where the
encode reaches a third of a core, and it is also ADR-0013's `High`.

## Why not the scheme ADR-0007 predicted

The roster multiplier is not what it looked like, because the sizes are not
equal. There is one stage and it is the only large picture; everything else on
the card is a square. Measured against a frame of pure noise, which is the
worst case a JPEG can be handed and not what a desktop or a face is:

| Pixels asked for | Sampled size | Sample and encode | `data:` URL |
| --- | --- | --- | --- |
| 160 | 160x90 | 0.26 ms | 10 KB |
| 320 | 320x180 | 1.0 ms | 39 KB |
| 480 | 480x270 | 2.1 ms | 89 KB |
| 960 | 960x540 | 8.2 ms | 366 KB |
| 1280 | 1280x720 | 14.2 ms | 632 KB |
| 1920 | 1920x1080 | 32.0 ms | 1.4 MB |

The real cost is the encode, and both transports pay it identically: a URI
scheme would save the base64, which is a third of the bytes and none of the
milliseconds. Eight faces at 320 cost 10 KB each and a tenth of a millisecond
of encoding per draw. Against that, a scheme of its own needs an entry in the
content security policy in both the Linux and the Windows origin forms, which
is what broke every attachment in v0.6.0, and a cache-busting URL per frame so
the webview re-fetches at all.

What would change this is wanting more than twelve and a half pictures a second
on the stage. The table above is the budget for that decision when somebody
takes it.

## Consequences

**#167's lever is one number in one place.** The bound travels from the call
site that draws the box, through the command, to the sample. #167 adds a per
stream cap over it and sends the same number to `set_constraints` as
`Dimensions`. Nothing else has to move.

**Until it does, the SFU is still sending more than is drawn.** A 1080p share
pulled to draw a 320 pixel square is the waste phase 1 of
`docs/PLAN-receiving-quality.md` describes. This change does not add it and does
not fix it.

**One frame is held per live stream.** As received, so a 4K share is about 12 MB
and a 720p camera about 1.4 MB. Bounded by the roster, dropped when the stream
goes or the call ends.

**A camera going off drops the frame rather than leaving it.** The pump is
started and stopped from `roster::camera_live` and `roster::screen_live`, so
muting ends it and forgets the picture. The cost is that unmuting draws nothing
until the next frame, about a thirtieth of a second, which is the right side to
err on for a camera somebody just covered.

**Our own still is untouched, so #165 is only half explained by this.** The
self view is sampled to 320 pixels on the capture thread, which is ADR-0007's
bound and the thing #165 is actually about: the stage now draws somebody else's
share at up to 1920 and our own at 320 upscaled. Moving that bound is a
different change, in `app/src-tauri/src/selfview.rs`, with a different cost
profile: it is paid per captured frame rather than per drawn picture. Reported
as #194 and done in
[ADR-0018](0018-sample-the-self-view-at-the-size-it-is-drawn.md), which moves
the sampling to the ask and measures the capture thread cheaper for it rather
than dearer.

**ADR-0009's reason for staging our own screen by default has expired.** It
staged ours "because it is the only share that can draw a picture". That is no
longer true, and the rule is now arbitrary rather than wrong: the first screen
listed still wins and ours is still listed first. Changing it needs the arrival
order ADR-0009 says the roster does not carry, so it stays as it is.

**Nobody has watched this work.** The X display on the machine it was written on
captures as solid black and synthetic keystrokes do not reach the webview, so
every claim above about what appears on a card is an argument rather than an
observation. Two accounts and a person looking at both is the only test that
settles it.
