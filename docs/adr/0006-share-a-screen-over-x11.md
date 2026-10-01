# ADR-0006: Share a screen over X11, not through the desktop portal

**Date**: 2026-10-01
**Status**: accepted
**Deciders**: tominal, with Claude Code

## Context

Issue #70 asks for screen sharing, and asks for a particular interface: a card
with two tabs, Applications and Screens, with the heaviest windows offered
first and a fullscreen game isolated from the rest.

On Linux there are two ways to read the screen, and the choice between them
decides whether that interface can exist at all.

### The portal owns the picker, by design

Under Wayland the only route is `org.freedesktop.portal.ScreenCast` over
PipeWire. The spec shipped with xdg-desktop-portal 1.22.1, at version 6 of the
interface, says of `Start`: "This will typically result the portal presenting a
dialog letting the user do the selection". An application passes `SelectSources`
a source-type bitmask and a `multiple` flag, and gets back PipeWire node ids. It
never sees a window list.

That is the portal's purpose rather than a gap in it. Unsandboxed enumeration of
every window on the session is the thing it exists to prevent. So on the portal
route #70's card cannot be drawn, the ordering cannot be chosen, and the whole
of this feature is one button that opens somebody else's dialog.

### On an X11 desktop the portal is absent, not degraded

Measured on the desktop this project is developed on (Arch, XFCE, X11):
`org.freedesktop.portal.Desktop` exports Camera, Account, FileChooser, Settings,
Secret and sixteen other interfaces, and does not export
`org.freedesktop.portal.ScreenCast` at all.

The reason is in `/usr/share/xdg-desktop-portal/xfce-portals.conf`, which
prefers `wlr` then `gtk` for that interface. `xdg-desktop-portal-wlr` is
wlroots-only and is not installed on an X11 session;
`xdg-desktop-portal-gtk` does not implement the interface, and its `.portal`
file says so by listing eleven interfaces, none of them ScreenCast.

A portal-first implementation would therefore ship a feature that does nothing
at all on the machine it was written on.

### X11 answers every question #70 asks

Measured on the same machine: `_NET_CLIENT_LIST_STACKING` gives the windows in
stacking order, `_NET_WM_PID` the process behind each, `WM_CLASS` and
`_NET_WM_NAME` the names, `_NET_WM_STATE` the fullscreen and skip-taskbar
flags, and RandR the monitors. Twelve sources in 1.5ms.

That is the two tabs, the ordering, the game, and the exclusion of Consort's own
windows, all from property reads.

### The platforms Consort actually ships to

`.deb` and `.rpm` from `tauri.conf.json`, Arch from `packaging/arch/PKGBUILD`,
and a Windows NSIS installer from the `windows` job in `release.yml`. ADR-0005
already settled what to do about the last of those for the camera: a `Host`
alias that is V4L2 on Linux and finds nothing elsewhere, so the Windows build
compiles and offers an empty picker.

## Decision

Capture the screen in Rust from X11, behind a `ScreenCapture` trait, with the
display talking confined to `x11_host.rs` and excluded from coverage. `Screens`
is `X11Host` on Linux and `NoScreens` everywhere else, exactly as `Host` is
`V4lHost` or `NoCameras`.

Wayland refuses with a sentence that names what is missing rather than failing
obscurely. A portal backend is a second implementation of the same trait, and
is stage 3 of #70's plan.

Use `x11rb` with the `randr` and `composite` features. It is already in
`Cargo.lock` at 0.13.2 by way of `arboard`, which `tauri-plugin-clipboard-manager`
pulls in, and enabling those features resolves no new crate: verified by diffing
the lock before and after.

## Alternatives Considered

### Alternative 1: the desktop portal first, X11 never

- **Pros**: The only path that works on Wayland, which is where the desktop is
  going. One implementation for both session types on a modern GNOME or KDE,
  because their portals proxy X11 clients too. The consent dialog is the
  compositor's, which is a stronger security story than anything Consort could
  build: the application never learns what windows exist.
- **Cons**: #70's interface becomes impossible, including both tabs and the
  whole of the ordering question. It is absent on XFCE, LXQt and any X11
  session without a portal backend installed, which is where this is developed
  and tested. On `xdg-desktop-portal-wlr` the Applications tab has nothing to
  put in it, because that backend advertises MONITOR only. It needs the
  `pipewire` crate, a new entry in the dependency graph and in both packaging
  manifests, to receive the frames.
- **Why not**: It would ship a feature that does nothing on the development
  desktop, in exchange for an interface the issue did not ask for. It is the
  right second implementation and the wrong first one.

### Alternative 2: both, chosen at runtime

- **Pros**: Correct everywhere on day one.
- **Cons**: Two capture backends, two frame paths, and two pickers, because the
  portal's and Consort's cannot be the same interface. The PipeWire dependency
  and the packaging changes arrive anyway.
- **Why not**: The seam that makes it cheap later is the thing this change
  defines. Building the second host before the first one has a caller is
  inventing an abstraction for a case that does not exist, which CLAUDE.md asks
  not to do. It is stage 3 of the issue's plan for that reason and not because
  Wayland does not matter.

### Alternative 3: `getDisplayMedia` in the webview

- **Pros**: The browser owns the picker, the permission and the encoding. It is
  what every other Matrix client does.
- **Cons**: Every argument in ADR-0005 applies unchanged, and two of them get
  worse. A 2560x1440 frame is 14.7 MB of BGRA, against a webcam's 1.38 MB, so
  the IPC cost of capturing in the webview is an order of magnitude larger. And
  under webkit2gtk `getDisplayMedia` needs the same portal that is absent here,
  so it does not even avoid the problem.
- **Why not**: It is the camera decision again, with the numbers pointing the
  same way harder.

### Alternative 4: `scap` or `xcap`, the cross-platform capture crates

- **Pros**: macOS and Windows capture for free, and `scap` wraps the portal on
  Wayland, so Alternative 2 arrives without writing it.
- **Cons**: Both pull a tree this workspace does not otherwise have, and
  neither is in `Cargo.lock`. `xcap` is screenshot-shaped rather than
  stream-shaped. Both own their own enumeration, which would mean #70's
  ordering and filtering happening inside a dependency rather than in a tested
  function here.
- **Why not**: The same trade ADR-0005 refused for `nokhwa`: paying a
  dependency tree for platforms that are not built. Worth revisiting when the
  portal path is written, where the wrapping is most of the work.

## Consequences

### Positive

- Nothing added to the dependency graph: `x11rb` is already in the lock and the
  two features it gains resolve no new crate. Neither packaging manifest
  changes, because X11 is a socket protocol and `x11rb` speaks it in Rust with
  nothing to link.
- #70's interface is buildable exactly as asked, including the fullscreen game,
  and the ordering is a tested function over a window list rather than a guess.
- The seam is the one this repository already uses three times. Everything that
  decides anything takes a window list as data, so the ordering, the filtering
  and the pixel conversion are tested without a display, and `x11_host.rs`
  joins `cpal_host.rs`, `v4l_host.rs` and `livekit.rs` outside the measurement.
- Consort's own windows are excluded by pid rather than by name, which is exact.
- No frame of anybody's screen is read to populate the picker. The list is
  property reads and the thumbnails are drawings, so opening the picker captures
  nothing.

### Negative

- **Wayland gets no screen sharing at all.** This is the real cost and it is a
  growing one: Fedora, Ubuntu and both major desktops default to Wayland now.
  What a session there gets is a refusal that names the reason. Stage 3 of #70
  is the answer and this ADR is the argument for doing it second rather than
  for not doing it.
- Windows ships with the feature absent, as it already does for the camera.
- **An occluded window may capture the wrong pixels.** `GetImage` against a
  window drawable returns what is on screen in that rectangle, so a window with
  something on top of it captures the thing on top of it unless a compositing
  manager is redirecting windows. XFCE's compositor is a checkbox. The
  `composite` feature is enabled against this, and reading through
  `XCompositeNameWindowPixmap` is the fix; it is not wired up in stage 1 and is
  the first thing to look at when somebody reports sharing the wrong content.
- Consort owns the pixel conversion. BGRA to I420 is arithmetic and is tested as
  data, and it costs 66ms a frame for a 2560x1440 monitor in a release build,
  measured. That keeps the 15fps capture target with headroom. In a debug build
  it is 517ms, which is why the hand-run display tests poll rather than sleep.
