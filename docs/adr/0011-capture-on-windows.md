# ADR-0011: Capture on Windows through Media Foundation and Windows.Graphics.Capture

**Date**: 2026-10-04
**Status**: accepted
**Deciders**: tominal, with Claude Code

## Context

Issue #163. Consort 0.9 ships a Windows installer, and on Windows it drew a
camera button and a share button that could never do anything. The camera
said "there is no camera on this machine" beside a C920 that worked in the
Windows Camera app at the same moment, and the share picker opened with
nothing in it and no reason given.

Neither was an accident. ADR-0005 and ADR-0006 both chose a host that finds
nothing on any platform but Linux, so the Windows build would compile and
start. What that cost was never said anywhere a user would see it, and the
tests that pinned the empty host passed on every platform, so nothing could
have caught it.

The traits those ADRs defined are the seam this needs. `CameraDevices` and
`VideoCapture` for a camera, `ScreenCapture` for a screen, and every decision
above them already taken as data: which camera format to ask for
(`choose_offer`), which windows to offer and in what order (`shareable`), and
how to turn BGRA, YUYV or a JPEG into I420. What Windows lacked was the two
files that talk to a device.

## Decision

Two Windows hosts behind the existing traits, chosen by the same `cfg` pair in
`lib.rs` that chooses V4L2 and X11:

- **Camera: Media Foundation**, through `IMFSourceReader`, in `mf_host.rs`.
  Devices are listed with `MFEnumDeviceSources`, the symbolic link is the id a
  saved choice holds, and the native media types are offered to
  `choose_offer` exactly as V4L2's formats are. NV12 joins YUYV and MJPEG in
  `pixels.rs`, because many built-in laptop cameras offer nothing else.
- **Screens and windows: Windows.Graphics.Capture**, in `wgc_host.rs`. Monitors
  from `EnumDisplayMonitors`, windows from `EnumWindows`, frames through a
  Direct3D 11 frame pool copied to a staging texture and handed to
  `from_bgra`.

Everything either host would otherwise interpret goes in `win32.rs`, which
has no Windows API in it, compiles on every platform, and is tested on the
Linux CI that never builds the hosts: subtype FourCCs, packed frame sizes and
rates, which `HRESULT`s mean busy and which one is the camera privacy switch,
the taskbar's rule for which windows count, and the reversal of
`EnumWindows`' topmost-first order into the bottom-first order `shareable`
takes.

Both use the `windows` crate at 0.62, which `cpal` already puts in
`Cargo.lock`. Naming it adds one dependency edge and no package.

A build with no host now says so. `NoCameras` refuses with `Unsupported`
rather than `NoCamera`, and `NoScreens` refuses to list rather than listing
nothing, so a platform still without a backend gets a sentence about the
build instead of one about the machine.

## Alternatives Considered

### Alternative 1: `nokhwa` for the camera, `scap` or `xcap` for the screen

- **Pros**: Cross-platform, and macOS would come along with Windows.
- **Cons**: Each brings a dependency tree this workspace does not have, where
  the `windows` crate is already in the lock. `scap` and `xcap` own their own
  window enumeration, which would move #70's filtering and ordering into a
  dependency and out of the tested `shareable`.
- **Why not**: ADR-0005 and ADR-0006 refused the same trade, and the reasons
  still hold. The direct calls are two thin files.

### Alternative 2: DXGI Desktop Duplication for screens

- **Pros**: Older and widely used, works on Windows 8, and needs no WinRT.
- **Cons**: Monitors only. The Applications tab would need a second API for
  windows, and `BitBlt` or `PrintWindow` read an occluded or GPU-drawn window
  as black or as whatever covers it.
- **Why not**: Windows.Graphics.Capture serves both tabs from one API and
  captures a window as it is drawn whatever covers it, which is the X11
  host's known weakness (ADR-0006) not repeated. It needs Windows 10 1903,
  from 2019, which every Windows release still in support is past; on one
  that is not, the picker says the build cannot share a screen.

### Alternative 3: `getUserMedia` and `getDisplayMedia` in WebView2

- **Pros**: WebView2 has both, which webkit2gtk did not, and the browser owns
  permission and picker.
- **Cons**: Every argument in ADR-0005 about the IPC boundary applies, and the
  frames would arrive by a different route on each platform, so the call
  crate would carry two capture paths.
- **Why not**: It would split the architecture by platform to save two files.

## Consequences

### Positive

- The Windows build has a camera and screen sharing, with the same picker,
  the same tabs and the same ordering as X11.
- Checked on Windows 11 with a C920: listed, opened at 720p, 60 frames with
  rising timestamps, released on drop. All six hand-run display tests pass,
  including that a dropped share sends nothing more.
- The camera privacy switch, the commonest way a working Windows camera
  refuses an application, has a sentence that names the switch.
- `win32.rs` puts every Windows decision in front of the Linux CI.

### Negative

- **Two more files outside coverage**, `mf_host.rs` and `wgc_host.rs`, plus the
  COM guard they share. They are not compiled where coverage is measured, and
  CI has no camera or desktop on Windows either. The `windows` job in
  `ci.yml` builds them, lints them and runs their unit tests; capture itself
  is only proved by hand.
- **Windows draws a yellow border around what is shared.** That is left on
  deliberately: it is the one indicator on screen of what is leaving the
  machine.
- **Opening and closing a camera take about 0.3 and 0.45 seconds** on the
  machine measured, most of it Windows starting and releasing the device.
  `open` waits for the first frame because a busy camera and the privacy
  switch fail there rather than earlier. `set_camera` is a synchronous
  command, so the window waits with it.
- **A window closed mid-share is noticed by polling.** The capture item's
  `Closed` event did not fire for a window that went with its process, so
  the share asks every frame whether its target still exists.
- macOS still has no backend, and says so.
