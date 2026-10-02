# ADR-0005: Capture the camera in Rust over V4L2, not in the webview

**Date**: 2026-10-01
**Status**: accepted
**Deciders**: tominal, with Claude Code

## Context

Issue #69 asks for a camera somebody can turn on. The signalling half already
exists: `roster::camera_live` reads `MediaStreamKind::Camera` off a membership
and the roster carries a camera flag per person. What does not exist is a frame.

Two places could produce one. The webview has `getUserMedia`, which is how
every Element client does this. Rust has V4L2, which is the symmetry with
`consort-audio`: cpal owns the sound card and the call crate is handed samples.

The pinned `matrix-rtc-media` settles half the question before it is asked. It
already carries the whole Rust-side video path: `PublishOptions::camera`,
`LocalTrackHandle::capture_video`, `RemoteTrackHandle::video_frames`, and a
`VideoFrame` built on a planar `I420Buffer`. Neither pin has to move for either
option, so this is not a dependency question about MatrixRTC. It is a question
about which side of the IPC boundary the pixels live on.

### The receive side already forces one crossing

Remote video is decoded in Rust, because that is where the SFU connection is,
and it has to be drawn in the webview, because that is where the interface is.
That crossing is not optional and it is not avoidable by choosing differently
here.

So capturing in the webview does not remove a crossing, it adds a second one in
the opposite direction. At 1280x720, one I420 frame is 1.38 MB. Thirty of those
a second is 41 MB/s, and webview capture pays it twice.

### The capability set says where privileged work goes

`app/src-tauri` grants `core:default` and nothing else. CLAUDE.md states the
rule that follows from it: a frontend change that appears to need a new
capability is a signal the logic belongs in Rust.

### This repository has already been bitten by a WebKitGTK media API

`clipboardData` carries no image under webkit2gtk, which cost this project a
section of CLAUDE.md, a Rust clipboard read, and five years of upstream bug
218519 to point at. `getUserMedia` under wry is another webkit2gtk media API to
bet a feature on, with its own permission plumbing and no way to test it in CI.

## Decision

Capture camera frames in Rust from V4L2, convert them to I420 there, and push
them into the existing `capture_video` path. The webview never sees a camera
device and gains no capability.

Use `moq-v4l` for the V4L2 bindings. Put the capture in a new `consort-video`
crate, with the device talking confined to `v4l_host.rs` and excluded from
coverage, which is what `consort-audio` does with `cpal_host.rs`.

## Alternatives Considered

### Alternative 1: `getUserMedia` in the webview, frames over the IPC

- **Pros**: The browser owns device enumeration, permissions, format
  negotiation and MJPEG decoding. No new Rust dependency. It is what every
  other Matrix client does, because every other Matrix client is a web page.
- **Cons**: Puts 41 MB/s on the IPC boundary in the uplink direction on top of
  the downlink the receive side already needs. Needs a webview capability this
  project deliberately withholds. Untestable in CI. Rests on a webkit2gtk media
  API in a repository that already keeps a scar from one.
- **Why not**: It does not replace a crossing, it adds one, and the thing it
  would add it to is the boundary this architecture is organised around
  avoiding.

### Alternative 2: The `v4l` crate rather than `moq-v4l`

- **Pros**: The established crate, 0.14.0, the one `nokhwa` itself uses on
  Linux. `moq-v4l` is a fork of it.
- **Cons**: Its default `v4l2` feature pulls `v4l2-sys-mit`, whose `build.rs`
  runs bindgen 0.65. That makes libclang and kernel headers a build-time
  requirement, which has to be added in three places: the CI container, the AUR
  `PKGBUILD` makedepends, and the deb workflow's `apt-get`.
- **Why not**: `moq-v4l` is that same crate with the kernel bindings checked in
  and no `build.rs`, and its only two dependencies, `bitflags 2` and `libc`,
  are already in `Cargo.lock` at 2.13.1 and 0.2.189. It is therefore the only
  third-party entry this change adds to the lock. Paying a bindgen toolchain in
  three packaging files to avoid a fork was the worse trade.

### Alternative 3: `nokhwa`

- **Pros**: Cross-platform, which this capture is not. The most used webcam
  crate in Rust.
- **Cons**: On Linux it reaches V4L2 through the `v4l` crate, so it inherits
  the bindgen problem above and adds its own tree on top, including an MJPEG
  decoder this workspace can get from a crate it already has.
- **Why not**: It buys macOS and Windows capture, and Consort ships a .deb and
  an Arch package. Paying for two platforms that are not built is the
  speculative generality CLAUDE.md asks not to add.

### Alternative 4: Hand-rolled V4L2 ioctls over `libc`

- **Pros**: No new dependency at all. `libc` is already in the graph.
- **Cons**: V4L2's `ioctl` surface is a set of C structs whose layout has to be
  right, in unsafe code, on the path that reads a camera.
- **Why not**: A struct-layout mistake there is memory corruption rather than a
  wrong pixel, and the project's own guidance is to prefer a proven
  implementation over writing new code.

## Consequences

### Positive

- No system library at build time or at run time, and no change to either
  packaging manifest. Checked rather than assumed: `readelf -d` on the binary
  built from this branch names eighteen libraries and every one of them is
  already covered by the `PKGBUILD` depends list, which that same command is
  how it was written. V4L2 is kernel ioctls on `/dev/videoN`, so there is
  nothing to link.
- One third-party crate added to `Cargo.lock`. Two more, `zune-jpeg` and
  `zune-core`, start being compiled: they were already resolved in the lock as
  optional dependencies of the `image` crate `app/src-tauri` uses for PNG, so
  they cost build time and no new resolution.
- Video on the IPC boundary in one direction only.
- The decision seam is the one this repository already uses twice. Everything
  that decides anything takes frames and device lists as data, so format
  conversion and device resolution are tested without a camera, and
  `v4l_host.rs` joins `cpal_host.rs` and `livekit.rs` outside the measurement.
- A camera never reaches the webview, so the answer to "can the page see my
  camera" stays no.

### Negative

- Linux only. A macOS or Windows build gains voice and not video until a second
  host implementation exists behind the same `CameraDevices` trait.
- `moq-v4l` is at 0.1.0 with one release. That is the real risk here and it is
  accepted on three grounds: it is a fork of a mature crate rather than new
  code, its two dependencies are already present so the surface added is one
  crate's own source, and the path it serves is confined to one file that is
  already outside coverage. It comes out if it goes unmaintained and `v4l`
  drops bindgen, or if this project ever needs a second platform badly enough
  to pay for `nokhwa`.
- Consort owns pixel format conversion. YUYV to I420 is arithmetic and is
  tested as data. MJPEG needs a decoder, and that is `zune-jpeg` directly
  rather than through `image`: `image` would pull the same decoder plus its own
  glue, and nothing here wants the rest of `image`.
