# Plan: a screen somebody can share

Issue #70. Stage 1 of the three that issue describes. Where the frames come
from, and why not from the desktop portal:
[ADR-0006](adr/0006-share-a-screen-over-x11.md).

## What is true at the end of stage 1

A control in the voice strip opens a card with two tabs, Applications and
Screens, listing this machine's real windows and monitors. Choosing one
publishes it as a MatrixRTC screen share, so Element Call draws it large. An
indicator in the sidebar names what is going out for as long as it is going
out, and stops it in one press. No audio.

| Piece | Where |
| --- | --- |
| The source list, the ordering, the filtering | `consort_video::screens` |
| BGRA to I420 | `consort_video::pixels::from_bgra` |
| X11 property reads and `GetImage` | `consort_video::x11_host` |
| The publication and its lifecycle | `consort-call`: `thread.rs`, `transport.rs` |
| Capture ownership, and the order of the two acts | `app/src-tauri/src/screen.rs` |
| The picker, the control, the indicator | `SharePicker.tsx`, `CallPanel.tsx` |
| The square on the call card, and the picture in it | `ScreenTile.tsx`, `SelfPicture.tsx` |

## Decisions worth knowing before reading the diff

**The picker is an X11 feature, not a Linux feature.** The Wayland portal draws
its own dialog and never hands an application a window list, by design. So the
card #70 asked for is achievable here and not there. ADR-0006 has the argument.

**Opening the picker captures nothing.** The list is property reads and the
thumbnails are drawn glyphs. A real thumbnail would mean capturing every source
to populate the card, which is the one thing a picker must not do: looking at
the list is not consent.

**The ordering is decided in Rust and not re-sorted in the interface.** Most
recently raised first, from `_NET_CLIENT_LIST_STACKING`, with any fullscreen
window ahead of that. Not by CPU or GPU: #70 records why that is neither cheap
nor portable, and what it would cost to find out.

**The title comes from the capture, not from the click.** A window can be
resized or renamed while the picker is open, and the indicator has to name what
is actually going out. So `ScreenBridge::start` reports the title it read when
it opened the source, and `SelfScreen` carries it rather than a boolean.

**A camera and a share can be up at once.** Two `Showing` slots and two frame
queues, because somebody presenting usually wants their face in the call as
well as their slides. `set_camera` and `set_screen` share one `publication`
helper for the lifecycle and keep their own state reporting.

**Three things end a share, and all three are driven by the call channel.** A
channel switch, a call ending, and a quit. None of them is a click, which is
why the capture is released by whatever hears the call thread rather than by the
button. `leave_call_on_quit` stops the capture before the network leave: the
leave is a round trip on a budget, and this is local and immediate.

**A shared screen is a square on the call card, and so is everybody.** The
review of #140 asked for it and
[ADR-0008](adr/0008-one-square-for-everything-in-a-call.md) records the shape: a
second newest-wins slot feeding a second command, polled by the same hook the
camera uses, drawn in the same square a face sits in. Somebody else sharing gets
a square with their name and no picture, because nothing carries a remote frame
into this window yet.

**Dropping a capture joins its thread.** Not just signals it. A grab takes long
enough that a stop lands inside one, so without the join a frame captured before
the stop is delivered after it. The capture loop re-reads the flag before
handing a frame over, and the drop waits. Bounded by one frame interval.

## Stage 2: system audio

Not built. #70 carries the research: the ScreenCast portal has no audio at all,
per-application capture needs the PipeWire graph directly, and excluding
Consort's own output is a pid comparison against our own process which is
mechanically possible and not yet verified. It needs the `pipewire` crate and an
ADR of its own, and one measurement settled first, which #70 names.

The toggle does not exist until this lands. Capturing the default sink's monitor
instead, which is the easy thing, puts Consort's own output in the mix and makes
the call hear itself.

## Stage 3: Wayland

A second `ScreenCapture` behind the same trait, over the portal and PipeWire,
with the picker reduced to the single button the portal's own dialog leaves
room for. ADR-0006 is the argument for doing it second rather than for not
doing it: Wayland is where the desktop is going, and a session there currently
gets a refusal that names the reason.

## Out of scope in every stage

Remote control, annotation, and sharing a region rather than a window or a
screen.
