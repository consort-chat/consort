# 12. Keep custom sections in the settings file, not in Matrix account data

Date: 2026-10-05

## Status

Accepted for the first cut of #170. The terms on which it would be revisited
are at the end, and they are specific.

## Context

#170 asked for sections somebody can make, name, fill with rooms, and still
have after a restart. "After a restart" is the whole requirement, so the only
real question was where the grouping lives.

Matrix has two concepts that would put it on the account rather than on the
machine, and both were considered because both would make the grouping follow
somebody to another client:

`m.tag` is per-room account data. Element reads `m.favourite` and
`m.lowpriority` out of it and ignores the rest, and the `u.` prefix is reserved
for user-defined tags. It fits a name but not an order, and it carries no
record of a section that is currently empty, which is the only state a new
section has.

`m.space` is a real room. It would make a section a thing on the server with
its own membership, its own state and its own history, and it would mean
creating and leaving rooms as somebody renames and deletes sections. #170 put
spaces as a navigation concept out of scope, and this is why: a section and a
space are not the same object, even though they look alike in a sidebar.

What settled it is what `consort-matrix` actually exposes today, which is
neither. There is no account data read or write anywhere in the crate outside
secret storage, and no tag API. Either route is a new module with a sync
handler, a write path, conflict handling for two clients editing one list, and
tests against a homeserver. That is a larger PR than the feature it would
carry.

## Decision

**The settings file, beside the fold and the order.** `SidebarSettings`
already holds which sections are folded away (#168) and the order somebody
dragged them into (#169). Both are local, both are keyed by the same key, and a
custom section is the third field of the same kind. Putting the list anywhere
else would mean a section whose name syncs and whose position does not.

Three rules follow from that file being the only copy:

**A room is in one section at a time.** The sections partition a space rather
than overlapping it. A room in two sections is two rows that do the same thing
and two badges counting the same messages.

**Deleting a section never hides a room.** A section's only claim on a room is
the mention of it in `rooms`, so dropping the section drops the claim and the
room is drawn under Text or Voice again.

**Text and Voice stay special, and stay underneath.** They follow
`m.room.type` rather than a choice, so they have no name to change and nothing
to delete, and between them they hold whatever no custom section claims. They
fold and drag exactly like a custom section, because to #168 and #169 they are
all just keys.

## Consequences

The grouping is per machine. Somebody who signs in on a second computer gets
Text and Voice and makes their sections again. That is the cost, it is the same
cost the fold and the order already pay, and nobody has complained about those.

A section is scoped to one space, by room ID. Without that a section made in
one space is drawn empty in every other one.

Moving this to the account later is additive rather than a rewrite: the shape
stored here (a key, a name, a space, a list of rooms) is the shape an
`im.consort.sections` account data event would hold, and the file becomes the
fallback for an account that has none. Worth doing when `consort-matrix` grows
an account data path for some other reason, and not worth growing one for this
alone.
