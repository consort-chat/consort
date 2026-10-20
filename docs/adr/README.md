# Architecture Decision Records

Decisions that shaped this codebase, and the reasoning that is not visible in
the diff. A decision belongs here when a future reader would otherwise be left
asking why the obvious thing was not done.

Most of the reasoning in this repository lives in the comments beside the code
it explains, which is the right place for it. These are the ones too large for
that: a trade-off spanning several modules, or a decision to accept something
imperfect for now and the terms on which it would be revisited.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-notice-conflicts-over-a-fork-patch.md) | Treat a contested membership as unclaimed, rather than patching the fork | accepted |
| [0002](0002-consort-mixes-the-call-itself.md) | Mix the call in Consort, because natively nothing else will | accepted |
| [0003](0003-measure-who-is-talking-locally.md) | Measure who is talking from the samples, not from the SFU | accepted |
| [0004](0004-trust-no-device-list.md) | Probe every device, and prefer the host's default over a saved name | accepted |
| [0005](0005-capture-the-camera-in-rust.md) | Capture the camera in Rust over V4L2, not in the webview | accepted |
| [0006](0006-share-a-screen-over-x11.md) | Share a screen over X11, not through the desktop portal | accepted |
| [0007](0007-draw-the-self-view-from-a-still.md) | Draw the self view from a still the card asks for | accepted |
| [0008](0008-one-square-for-everything-in-a-call.md) | One square for everything in a call, and a second still to fill one | accepted |
| [0009](0009-a-shared-screen-takes-the-stage.md) | A shared screen takes the stage, and everything else goes under it | accepted |
| [0010](0010-a-timeline-on-the-base-sdk.md) | Read the timeline off the base SDK, not matrix-sdk-ui | accepted |
| [0011](0011-capture-on-windows.md) | Capture on Windows through Media Foundation and Windows.Graphics.Capture | accepted |
| [0012](0012-sections-in-the-settings-file.md) | Keep custom sections in the settings file, not in Matrix account data | accepted |
| [0013](0013-ask-for-a-picture-in-pixels.md) | Ask for a received picture in pixels, not in a quality level | accepted |
| [0014](0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md) | Ask for a remote picture at the size it is drawn | accepted |
| [0015](0015-ask-for-the-box-not-a-square.md) | Ask the SFU for the box, not a square of its long edge | accepted |
| [0016](0016-only-a-person-may-ask-a-share-for-less.md) | Only a person may ask a share for less, because the rung below its best is 3 fps | accepted |
