# ADR-0001: Treat a contested membership as unclaimed, rather than patching the fork

**Date**: 2026-09-01
**Status**: accepted
**Deciders**: tominal, with Claude Code

## Context

Deafen and away travel between Consort clients as LiveKit data messages on
`consort.self_audio`, because neither has a representation in MatrixRTC or in
LiveKit. See `crates/consort-call/src/notices.rs` for why that channel and not
a participant attribute or a room event.

A notice names its own sender, in a `member_id` field. It has to: the LiveKit
participant identity a message arrives with and the `m.rtc.member` membership
id are derived from each other differently in each MatrixRTC generation, and
re-deriving one from the other in this crate would mean it quietly stopping
working in whichever dialect nobody tested.

Nothing checked that claim. Any participant in a call could send a notice
carrying another person's membership id, and the roster would draw a headphone
or an away icon beside somebody who was listening and present. The reverse is
also available: claim somebody's membership and say they are fine, to keep an
icon off. It is a spoof of a status indicator rather than of audio or of
identity, so nothing is overheard and nothing is impersonated in a room, but
"can that person hear me" is a question this interface is supposed to answer
truthfully.

The complete fix is to check the sender. `matrix_rtc_media::Participant`
carries no LiveKit identity field, so it cannot be done from here: it needs a
change to `tominal/matrix-rust-rtc`, and therefore a second fork commit to keep
rebased and a coordinated rev bump of the two pinned crates, which the
workspace manifest warns about at length.

## Decision

Drop any membership id that more than one participant claims, from both flag
lists, and draw that person as ordinary. Do not patch the fork yet.

This works because everybody re-announces on every roster change, for the
unrelated reason that deafening is per participant and a new arrival would
otherwise be audible. A forged claim about a Consort user therefore sits beside
that user's own claim about themselves, and the two are visible as one
membership arriving from two identities.

## Alternatives Considered

### Alternative 1: Add `identity` to `matrix_rtc_media::Participant` in the fork

- **Pros**: The actual fix. Checks the sender against the roster rather than
  inferring a conflict, so it holds against a single forged claim with nothing
  opposing it, and against a claim made about somebody running Element Call.
- **Cons**: A second commit on `fix/no-e2ee-in-unencrypted-rooms`'s fork to
  keep rebased, and a rev bump of `matrix-rtc-livekit`, `matrix-rtc-media` and
  `matrix-rtc-core` together, all of which have to stay pinned to the same
  matrix-sdk rev as this workspace.
- **Why not**: Not rejected. Deferred, because the conflict rule is fifteen
  lines with no dependency changes and closes the case that matters most, a
  Consort user being lied about in a room of Consort users, on the day it lands.

### Alternative 2: Believe the first claimant and refuse later ones

- **Pros**: Keeps the icon working for whoever announced first.
- **Cons**: Turns the attack into a race. An attacker who joins and claims a
  membership before its owner does wins permanently, and the owner's own
  truthful notices are then the ones refused.
- **Why not**: It converts an unauthenticated claim into an unauthenticated
  claim with a lock on it, which is worse than not deciding.

### Alternative 3: Derive the membership id from the participant identity here

- **Pros**: No forged field to trust, and no fork change.
- **Cons**: The derivation differs per MatrixRTC generation. This is the exact
  reason `member_id` is a field in the first place.
- **Why not**: It would work in whichever dialect it was written against and
  silently stop working in the others, which is a worse failure than the one
  being fixed.

## Consequences

### Positive

- A forged status claim about anybody running Consort no longer reaches the
  interface. Both sides announce, so the lie is always accompanied by the truth.
- Fails to the neutral state. The wrong answer is a missing icon, never an icon
  beside somebody it does not describe.
- No dependency change, so the matrix-sdk and matrix-rtc pins stay where they
  are.

### Negative

- Denial is still available. Somebody who wants an icon gone can contest the
  membership and get that, rather than getting a false icon.
- A reconnection can contest a membership honestly while the SFU still reports
  the old participant. The icon flickers off and comes back.

### Risks

- Somebody running Element Call sends no notice, so a claim about them meets no
  opposition and is drawn. Alternative 1 is what closes this, and this ADR is
  the record of it being outstanding.
- A forgery is answered when the roster next changes, not immediately. In a
  quiet call that can be a while.
