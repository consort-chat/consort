# 7. Draw the self view from a still the card asks for

Date: 2026-10-01

## Status

Accepted, for the self view only. Drawing other people's cameras is still open:
see `docs/PLAN-webcam.md` phase 2.

## Context

Somebody switched their camera on and the floating call card did not change.
Nothing was wrong with the camera: frames were being captured, converted and
published to the SFU. There was simply no path from a captured frame to the
webview, and no element to draw one in.

`docs/PLAN-webcam.md` left the shape of that path open on purpose, saying the
answer depends on measurement rather than taste, and naming three candidates: a
URI scheme like the one `app/src-tauri/src/media.rs` uses for attachments, a
push over an IPC channel, or a still encoded per visible tile and polled.

The numbers that decide it, measured by
`selfview::tests::measure_a_real_self_view` against a C920 at 720p:

| | Bytes |
|---|---|
| One captured I420 frame | 1,382,400 |
| The same frame as a self view, base64 in a `data:` URL | 3,115 |

The second number is the whole argument. A self view is a square a couple of
hundred pixels wide, so it does not need the frame: it needs about a four
hundredth of it. The card is 240 pixels wide and 420 expanded, and the picture
is sampled to fit inside 320 by 320.

Two constraints narrowed the field before any of that. A `<video>` element with
`getUserMedia` is not available: ADR-0005 put the camera in Rust, and V4L2 hands
a device to one process, so the webview cannot open the camera Consort already
has. And the content security policy in `tauri.conf.json` already allows `data:`
in `img-src`, where `blob:` is absent and a new scheme would need adding to both
the Linux and the Windows origin forms. Getting that pair wrong is what broke
every attachment on Windows in v0.6.0.

## Decision

The capture thread samples each frame down and keeps the newest one. A command
converts it and hands back a `data:` URL, and the card draws it in an `<img>`
refreshed every 80 milliseconds.

The split between the two halves is the point. Sampling down is all that happens
per captured frame, and it is cheap because it reads only the pixels it keeps.
The colour conversion, the JPEG encode and the base64 happen in the command, so
they happen once per picture drawn and not at all while nobody is drawing one.
The hook stops asking when the camera goes off and when the card is put away.

Polling rather than an event per frame, because the card's cadence should decide
how often a frame is converted. An event stream would have Rust encoding at the
camera's rate into a listener that may be drawing at a different one, or not at
all.

Chained timeouts rather than an interval, so an answer slower than the interval
delays the next ask instead of queueing one behind it.

## Consequences

The frame never crosses the IPC. What crosses is about 3 KB for a plain scene
and, at the pathological end, 93 KB for a frame of pure noise, which camera
footage is not.

The cost is one encode per drawn picture. Measured in a dev build, which is an
upper bound rather than what ships: 600 microseconds to sample a frame down and
7.6 milliseconds to convert and encode one. At twelve and a half pictures a
second that is about a tenth of a core in a dev build, and the sampling is a
further one percent at the camera's rate. Both halves are the kind of per-pixel
arithmetic that is an order of magnitude cheaper optimized, which
`docs/PERFORMANCE.md` measures for the capture path.

The self view is not live video. It is a still replaced twelve and a half times
a second, which reads as video for a face in a small square and would not for
anything fast. If that ever matters, the thing to change is the cadence, and the
ceiling on it is the encode above rather than anything structural.

What this does not settle is other people's cameras. There is one picture here
because there is one local camera, and the slot holding it is a single newest
wins slot for the same reason `consort_call::Camera` is. Several remote tiles at
once would want a key per participant, and probably the URI scheme rather than a
`data:` URL per tile per tick, because the bytes stop being incidental once they
are multiplied by a roster. That decision stays open.
