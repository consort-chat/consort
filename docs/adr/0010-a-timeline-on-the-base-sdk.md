# ADR-0010: Read the timeline off the base SDK, not matrix-sdk-ui

**Date**: 2026-09-02
**Status**: accepted
**Deciders**: tominal, with Claude Code

Recorded on 2026-10-02, from the module header in `timeline/mod.rs`. The
decision itself was made in "read and send text in a room".

## Context

`matrix-sdk-ui` has a timeline that does far more than Consort's: gap-aware
storage, edits folded into the events they replace, reactions grouped, local
echo, read receipts. Using it would have saved most of
`crates/consort-matrix/src/timeline`.

It is a second crate to pin to the same git revision as `matrix-sdk` itself.
The SDK pin is already load-bearing and already shared with
`matrix-rust-rtc`: [DEPENDENCIES.md](../DEPENDENCIES.md) has what moves it and
what breaks when two revisions disagree. A third crate in that knot is a third
thing to hold still on every bump.

## Decision

Build on the three things the base SDK already gives: the events a sync
delivered, a page of history on request, and the window around one event.

`timeline::watch` keeps what is loaded in a `Loaded` value and republishes the
whole of it on every change, so a late subscriber can be handed a complete
value rather than a stream of deltas it has to replay in order.

## Consequences

Three absences, each of which `matrix-sdk-ui` would have filled. All three are
answered honestly in the interface rather than papered over.

**A `limited` sync appends across its own gap without saying so.** That is what
a client which has been offline for a while receives. The messages drawn are
all real and all in order, but some in the middle may be missing until the room
is reopened. Fixing it properly means a gap-aware store, which is the thing
`matrix-sdk-ui` exists to be.

**Going to an older message draws that part of the room instead of this one.**
With nowhere to put a piece of history that is not next to what is loaded, a
window replaces what was loaded and the interface says so. A store that knew
where its gaps were could hold both at once and would not have to choose.

**There is no local echo.** A message goes to the homeserver and appears when
the sync brings it back, which on a healthy connection is a moment and on a bad
one is visible. Echo means a second, provisional kind of message and a rule for
reconciling it.

## When this would be revisited

The gap is the one that will force it. A reader who regularly finds messages
missing until they reopen a room is being lied to by the room, and no amount of
honest wording in the window fixes that. The revisit is adopting
`matrix-sdk-ui`, or writing the gap-aware store, and the first is cheaper than
the second.
