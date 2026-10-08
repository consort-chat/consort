# 17. A sender may send less camera, and cannot yet send less share

Date: 2026-10-08

## Status

Accepted and acted on. Follows
[ADR-0016](0016-only-a-person-may-ask-a-share-for-less.md), which established
that the quality control is receive-side rung selection and that what is left
is the send side.

## Context

Issue #196: "Right now it's magic. The sender of a stream should have control
over what quality other users will receive by default. If they have internet
issues, they might want to send a lower quality stream."

A receiver can only choose among rungs the publisher built, so a sender-side
control is either a change to the ladder or a change to what the ladder is
built from. Three routes were available and only one of them is reachable from
this repository.

**A different ladder is not reachable.** `matrix_rtc_media::PublishOptions`
carries `kind`, `audio`, `video` and `simulcast`, and nothing else
(`crates/matrix-rtc-media/src/local.rs` at the pinned rev `0427fc0`). livekit's
own `TrackPublishOptions` has the three fields that would do this,
`simulcast_layers`, `scalability_mode` and `video_encoding`, and none of them
is exposed. Reaching any of them is a change to matrix-rtc-media, which is a
dependency decision rather than ours. See the Consequences.

**Declaring a smaller size without capturing one is a lie.** livekit reads
`req.width` and `req.height` off the source's declared resolution and computes
both the advertised layers and each encoding's `scale_resolution_down_by` from
it, while the frames pushed in are scaled by the sink's wants from their own
size. Declare 1280 by 720 and push 1080p and the SFU is told about a 720 tall
top layer that is actually 1080 tall, which makes
`GetQualityForDimension` answer the wrong layer and reduces nothing.

**Opening the capture smaller is reachable**, needs no new rung, and is a
genuine change to what every receiver gets. What it costs is the frame-rate cap
livekit derives from the same number, and that is where the camera and the
screen part company.

`compute_appropriate_encoding` walks a preset table until a preset is wider
than the declared size, and the table for a screenshare is not the table for a
camera. Measured through livekit's own `compute_video_encodings`, pinned in
`crates/consort-call/tests/the_ladder_a_declared_size_builds.rs`:

| Opened at | Camera rungs | Screenshare rungs |
| --- | --- | --- |
| 1920x1080 | 1920x1080@30, 640x360@20, 320x180@15 | 1920x1080@**30**, 960x540@3 |
| 1280x720 | 1280x720@30, 640x360@20, 320x180@15 | 1280x720@**15**, 640x360@3 |
| 960x540 | 960x540@**30**, 640x360@20, 320x180@15 | 960x540@**5**, 480x270@3 |
| 640x360 | 640x360@25, **640x360**@20 | 640x360@5, 320x180@3 |
| 320x180 | 320x180@**15**, one rung | 320x180@**3**, one rung |

A camera keeps 30 frames a second down to 960. A share loses them immediately:
every size below 1920 caps its top rung below 30, and 960 caps it at five. A
resolution control on a share would therefore deliver the exact complaint
ADR-0016 was written about, arriving by a different route.

One size is unusable for a camera. 640 by 360 is the upper entry of
`video::DEFAULT_SIMULCAST_PRESETS`, so a camera opened there publishes two
encodings of the same size and discards the 320 by 180 rung it would otherwise
have had.

## Decision

**A sender chooses what size its camera is opened at, and nothing else.**
`consort_video::Sending` is saved in `VideoSettings` and read by
`AppState::start_camera`, which hands it to `VideoBridge::start` as the
`Resolution` the device is asked for. `choose_offer` then picks the nearest
format the device actually offers, as it always has.

Three positions, each landing on a ladder of its own:

| Setting | Opened at | Rungs peers may choose from |
| --- | --- | --- |
| Full | 1280x720 | 1280x720@30, 640x360@20, 320x180@15 |
| Reduced | 960x540 | 960x540@30, 640x360@20, 320x180@15 |
| Minimal | 320x180 | 320x180@15, the only one |

960 and 320 rather than ADR-0013's receive-side 1280 and 640, because 640 is
the size a camera must not be opened at and 1280 is what Full already is.

**The frame rate asked of the device is untouched at every setting.** Dropping
frames is what a lower rung already does and is the half of a rung swap people
notice.

**A screen share is left alone.** Not an oversight and not deferred work that
a number could settle: there is no size below 1920 that reduces a share without
reducing its frame rate, so this route cannot deliver #196 for a share at all.

**It applies at the next switch-on.** A capture size is fixed when a
publication is created, so changing it under a live one means retracting and
republishing, which is a reconnect in everybody else's call in answer to
somebody browsing a settings screen. The camera picker above it has worked this
way since it was written.

## Consequences

**Somebody on a bad connection can send a third or a twentieth of the camera
pixels they were sending**, at the frame rate they were sending them.

**Their own self view is smaller too.** The device produces fewer pixels and
the card draws from the same capture. The card's picture is a couple of hundred
pixels wide, so Reduced is invisible there and Minimal is the only setting
anybody would notice.

**Minimal is capped at 15 frames a second**, because livekit's camera preset
table caps a 320 by 180 publication there. It is the rate the bottom rung of
every camera ladder already carries, so it is the floor rather than a
regression, but it is not 30 and the table above says so.

**The receive-side control still has at most two distinct outcomes on a
share**, which is ADR-0016's open question answered: a screenshare publication
is two rungs at every size, because
`screenshare::compute_default_simulcast_preset` returns exactly one extra
preset. Nothing here adds a rung to anything, so `Auto`, `High` and `Medium`
still land together and `Low` still lands on the 3 fps rung. A camera has
three rungs at Full and Reduced and one at Minimal, so the receive control has
three outcomes on a camera and one on a Minimal camera. Four positions are
honoured by nothing this client can publish.

**Sending less of a share at its own frame rate is still not reachable**, and
it is the remaining half of #196. It needs one of three fields on
matrix-rtc-media's `PublishOptions`:

| Field | What it would do |
| --- | --- |
| `video_encoding` | Set `max_framerate` and `max_bitrate` directly, so a share could be opened smaller and told to keep 30 fps |
| `simulcast_layers` | Replace the screenshare preset with rungs that keep their frame rate, which would also give the receive-side control more than two outcomes |
| `scalability_mode` | SVC, where spatial layers share one frame rate by construction |

Which of them, and whether to carry a fork of matrix-rtc-media to get it, is
not a decision this repository can make on its own.
