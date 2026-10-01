# Plan: a camera somebody can turn on

Issue #69. The signalling half already existed: `roster::camera_live` reads
`MediaStreamKind::Camera` off a membership, the roster carries a camera flag
per person, and `CallFace` draws it. What did not exist was a frame.

Two phases, because they are two halves with one seam and each stands on its
own. Where the frames come from, and why not from the webview:
[ADR-0005](adr/0005-capture-the-camera-in-rust.md).

## Phase 1: publish a camera

What is true at the end: a camera is chosen in Voice & Video, a button in the
voice strip switches it on, and everybody else in the call sees it. Including
Element Call, because the publication is an ordinary MatrixRTC camera stream.
Consort itself still draws the existing icon rather than the picture.

That is a complete thing rather than half of one. The indicator for somebody
else's camera already shipped, so the only Consort-shaped gap left at the end
of phase 1 is seeing a stream rather than knowing about it.

| Piece | Where |
| --- | --- |
| Devices, pixel formats, V4L2 | `crates/consort-video` |
| The queue, the publication, the publish seam | `consort-call`: `camera.rs`, `showing.rs` |
| Device ownership and the order of the two acts | `app/src-tauri/src/video.rs` |
| The button and the picker | `CallPanel.tsx`, `VoiceVideoSection.tsx` |

### Decisions worth knowing before reading the diff

**A camera does not survive a channel switch.** Mute and deafen do, because a
person who muted themselves and moved has not asked to be heard. A camera is
the other way round: it is published into one call, and somebody who moves
channels has not asked to be filmed in the new one. So `SelfVideo` is reset on
every connect and the button follows. See `consort_call::SelfVideo`.

**The device opens before anything is published.** Opening is what fails in the
ordinary way, because on Linux exactly one process may hold a video node. A
publication with no frames behind it is a black rectangle in everybody else's
call, so the order is: is there a call, then open the device, then publish.

**Switching the camera off retracts the publication rather than muting it.**
Peers drop the stream and draw an avatar, which is what `camera_live` already
reads. A mute would leave every peer holding a decoder for a stream that will
never carry another frame. See the header of `consort_call::showing`.

**The frame queue is one slot, newest wins.** The opposite of `Microphone`, and
`consort_call::camera` says why: audio is a sequence and a picture is a state.

**Frame rate outranks resolution when choosing a format.** Measured on a C920:
YUYV at 720p is ten frames a second and MJPEG at 720p is thirty, because a
webcam's uncompressed modes are USB bandwidth limited. Ten is not a video call.
See `consort_video::capture::choose_offer`.

## Phase 2: draw the camera

What is true at the end: the picture appears where the icon is now, and the
floating card shows it.

The problem phase 2 solves is not signalling, it is transport. Decoded frames
arrive in Rust, where the SFU connection is, and have to be drawn in the
webview, where the interface is. That crossing is the one video crossing this
architecture accepts, and the shape for it already exists: `app/src-tauri/src/media.rs`
serves attachments over a URI scheme the webview can range-request, with the
CSP entry and the `mediaUrl` builder that go with it.

Still open, and to be settled in phase 2 rather than guessed at now:

- Whether a frame is served over that scheme, pushed over an IPC channel, or
  encoded once per visible tile and polled. The answer depends on measurement
  rather than taste.
- What the card shows when several people have a camera on. The issue asks for
  the active speaker's, and that question is already answered here:
  [ADR-0003](adr/0003-measure-who-is-talking-locally.md) rejected the SFU's
  dominant-speaker report because it answers the wrong question at the size
  Consort is built for, and `AppEvent::Speaking` already carries who is audible,
  measured locally. The card should read that rather than subscribe to
  `CallEvent::ActiveSpeakers`, which the pinned transport does offer.
- What `#71` asked for and did not get: double-click to expand the card, a
  streamer's content filling their own card, and double-click on a person for
  full screen. All three need a picture to act on, so none of them could land
  before this.

## Out of scope in both phases

Screen sharing, which is #70. Nothing here is built toward it, and the camera
publication deliberately does not generalise: `PublishOptions::screen_share`
exists at the pin and takes the same `VideoSourceConfig`, so when #70 arrives it
has a short path, and inventing the abstraction now would be inventing it for
one caller that does not exist.
