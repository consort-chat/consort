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
