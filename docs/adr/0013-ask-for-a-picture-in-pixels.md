# 13. Ask for a received picture in pixels, not in a quality level

Date: 2026-10-05

## Status

Accepted and acted on. Issue #167 asked whether a receiver can choose the
quality of somebody else's screen share. It can, and this is which of the two
available controls it should use. `consort_call::detail` holds the ceilings
below and the drawn size they sit over, and the control is on the card about
one person. The ceilings are still a guess: see the consequences.

## Context

`MediaConstraints::detail` in the pinned `matrix-rtc-media` is a `VideoDetail`,
and its three variants are not three points on one scale. `Auto` is no
preference. `Dimensions` names the size of the box the stream is drawn into.
`Quality` names a layer, `Low`, `Medium` or `High`. The two non-`Auto` variants
are mutually exclusive by construction, so a caller picks a kind of control
rather than a value.

`Quality` is the one that reads like what #167 asks for. A menu offering Auto,
Low, Medium and High maps onto it one for one, and onto `Dimensions` not at
all: a person choosing "low" has not chosen 640 by 360.

## Decision

**Use `Dimensions`, and keep `Quality` out of Consort entirely.**

Three things decide it, and the first two are not preferences:

**LiveKit has deprecated the quality field in its own wire protocol.** The
module docs on `matrix-rtc-media`'s `constraints.rs` say so and name
`Dimensions` as the preferred control. Building on the deprecated half of a
pinned SDK is borrowing against the next pin bump.

**`Quality` cannot be asked for while a stream is paused.** Every LiveKit
setter sends a full-replacement `UpdateTrackSettings`, and
`set_video_quality` sends only the quality field: zeroed dimensions and
`disabled: false`. Sending it after a pause would un-pause the stream. The
transport guards the hole by applying `Quality` only while the demand is
`Active`, which means a cap asked for on a tile that is currently paused, which
is every tile that is off screen, is dropped on the floor. `Dimensions` sends
`{disabled, width, height}` and stays consistent with the pause either way.
Consort pauses streams already: that is how deafen is built.

**A window knows pixels.** The size of the box a stream is drawn into is a
number the webview has. The publisher's simulcast ladder is not, and a client
that guessed at it would be guessing about a machine it cannot see.

**A named level becomes a pixel ceiling, chosen here rather than per caller.**
A person still picks a word. Auto asks for the drawn size, and a cap asks for
the smaller of the drawn size and the ceiling, so capping a tile that is
already small changes nothing and costs no round trip.

| Choice | Ceiling on the long edge |
| --- | --- |
| Auto | none: the drawn size |
| High | 1920 |
| Medium | 1280 |
| Low | 640 |

## Consequences

**A single-layer track ignores all of it, and that is not a bug to chase.**
`PublishOptions::screen_share` asks for simulcast, but the LiveKit transport
only honours it when the long edge is 480 pixels or more, because below that
LiveKit computes a degenerate single encoding the SFU delivered no frames for.
Somebody sharing a small window publishes one layer, and a receiver asking for
less gets exactly what it was already getting. A control that says it did
something and did not is worse than one that is absent, so a cap on such a
stream should read as unavailable rather than as applied.

**The control cannot be built before the tile.** `Dimensions` is the size of a
box, so a caller with no box has nothing to send. This is the reason #167 is
blocked on drawing a remote picture rather than merely nicer with one.

**The ceilings above are a guess until somebody watches a layer switch.**
Upstream's end-to-end test against a real LiveKit server drives pause, resume,
off and on, and never sets `detail` at all. The `Dimensions` branch is covered
only against a fake transport, which proves the value arrives and nothing about
what the SFU picks. Whoever builds this is the first person to see it work.

**Automatic downgrade is a separate decision.** Deciding the units says nothing
about who sets them. `Call::receive_stats` can feed a cap, and that needs its
own measurement, its own hysteresis, and an answer to the fact that the SFU is
already adapting downwards on its own. `docs/PLAN-receiving-quality.md` phase 3.
