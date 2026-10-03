# Performance

Measurements, and the profile settings they argue for. Everything here was
measured on this workspace rather than reasoned about from reading.

## A dev build could not keep up with a camera

A camera was reported as making Consort unusably laggy, with one process at
97.2% CPU. The question was whether that was the code or the debug build. It
was the debug build, and the gap is large enough to be worth writing down.

### What was measured

`consort-video`'s `read_the_real_camera` test, which opens the real device and
reports the rate frames arrive at the sink. It is the whole capture path and
nothing else: no SFU, no encoder, no webview, no Matrix sync in the process. A
Logitech C920 negotiating MJPEG at 1280x720, which is what `choose_offer` picks
for it, so every frame is a JPEG decode followed by the RGB to I420 conversion
in `pixels.rs`.

```sh
cargo test -p consort-video --lib -- --ignored --nocapture read_the_real_camera
```

CPU is the user time of the whole process over the run.

| Build | Frames per second | CPU | Per frame |
|---|---|---|---|
| dev, as it was | 5.5 | 9.69 s over 11.95 s | 159 ms |
| dev, `consort-video` at opt-level 2 | 7.3 | 6.32 s over 9.22 s | 104 ms |
| dev, both packages at opt-level 1 | 15.2 | 1.05 s over 4.67 s | 17 ms |
| dev, both packages at opt-level 2 | 15.2 | 0.33 s over 4.61 s | 5.4 ms |
| release | 15.0 | 0.28 s over 4.76 s | 4.7 ms |

So the shipped build was never the problem: release spends 4.7 ms of CPU on a
frame and about 6% of one core keeping up with the camera. A dev build spent
159 ms on the same frame, delivered 5.5 of the 30 frames a second the camera
offered, and saturated a core doing it. That saturated core is the 97.2%.

### Where the cost was

Optimizing `consort-video` alone moved 9.69 s to 6.32 s. Optimizing `zune-jpeg`
as well moved it to 0.33 s. So the JPEG decode was about 6.0 s of the 9.69 s and
our own conversion about 3.4 s: both mattered, and naming only our own crate
would have left two thirds of the cost in place.

`[profile.dev.package."*"]` does not help here. It sets `debug = false`, which
is debuginfo and not speed, so every dependency still compiled at opt-level 0.

opt-level 1 is already enough to keep up with the camera, at three times the CPU
of opt-level 2. 2 is the setting, because there is nothing to buy with the
difference: it is where a frame costs what the shipped build spends on it.

### What it costs

Measured against a cold build of `consort-video` and its dependencies:

| | dev, as it was | both at opt-level 2 |
|---|---|---|
| Cold build of the crate and its dependencies | 4.90 s | 7.02 s |
| Rebuild after editing `pixels.rs` | 0.87 s | 2.22 s |

An edit to `consort-video` costs about 1.4 s more to rebuild, and the
dependency is built once. Nothing else in the workspace is touched, so no other
crate's rebuild or the application's link time changes.

The real cost is that `consort-video` is now optimized in a dev build, so
stepping through it in a debugger is degraded. It is a thousand lines of
arithmetic over byte buffers and the thing most worth knowing about it is how
fast it runs, so that is the right trade here. Every other crate, the
application included, still builds at opt-level 0.

`the_dev_profile_keeps_up_with_a_camera.rs` holds the profile in place, because
the symptom of losing it is a frame rate rather than a failure.

### The frame rate is the camera's, not ours

15 fps rather than 30, in both builds that keep up, is the device. The C920 was
in a dim room with `auto_exposure` at aperture priority and
`exposure_time_absolute` at 666, which is 66.6 ms a frame and so exactly 15 a
second. At 6% of a core the conversion is not the limit; the light in the room
is. In better light the same build reads 30.

## A read receipt on its own channel, not a republished timeline

[`Readers`](../crates/consort-matrix/src/timeline/dto.rs) is its own value
rather than a field on `Timeline`, and the reason was measured rather than
assumed.

| What | On its own channel | Folded into the timeline |
|---|---|---|
| Putting one receipt on the wire | 0.9 us | 4.8 us |
| Redrawing what changed | 0.14 ms | 6.3 ms |

The wire cost is the smaller half. The larger half is that a republished
timeline is new objects all the way down, so every row in the room is a new
reference and the whole list redraws. A busy room produces about as many
receipts as messages, which makes this the difference between a conversation
that sits still while people read it and one that does not.

Both conversations, the room's and whatever thread is open, travel in one
value. The channel keeps only its latest for a late subscriber, so two values
on one channel would mean the second erasing the first.
