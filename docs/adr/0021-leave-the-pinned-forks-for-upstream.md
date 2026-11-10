# ADR-0021: Leave the pinned forks for upstream, and pay for it in one change

**Date**: 2026-11-10
**Status**: proposed
**Deciders**: tominal, with Claude Code

Recorded as [PLAN-off-the-pinned-forks.md](../PLAN-off-the-pinned-forks.md)'s
stage 0 gate, which asked for an ADR and said of itself that it was not one.
`proposed` rather than `accepted` because the move is done and measured on
[#217](https://github.com/consort-chat/consort/pull/217), which is a draft that
is deliberately not for merge: the acceptance gate is a call held between two
real clients, and nothing in CI can tell a call that connects from a call
somebody can hear.

## Context

Two git pins carry this project's Matrix and MatrixRTC stack, and they cannot
move separately: `LiveKitCall::join` takes a `&matrix_sdk::Room`, so the rev the
rtc crates pin and the rev this workspace pins must be the same string or cargo
resolves two copies of the SDK. `Cargo.toml` and
[DEPENDENCIES.md](../DEPENDENCIES.md) have the procedure.

Until this change both were personal forks. `BillCarsonFr/matrix-rust-sdk` at
`3773300`, a fork commit that is not an ancestor of upstream main, and
`tominal/matrix-rust-rtc` at `0427fc0`, nine commits on top of a repository
that was archived read-only on 20 September 2026 and whose README now points at
`element-hq/matrix-rust-rtc`. Three things followed from that.

**A reachable advisory could not be fixed.** `RUSTSEC-2026-0451` sits in
`vodozemac` on the path every Olm session takes. `matrix-sdk` 0.18.0 asks for
`vodozemac ^0.10.0`, and the fix is 0.11.2, so cargo refused the bump. The
entry in `.cargo/audit.toml` was an accepted risk nobody had judged acceptable
on its merits.

**The ignore list had become the shape of the problem.** Seven entries, six of
them saying they come out only when one of these pins moves.

**Nobody could be asked for help.** An archived upstream takes no issues and no
pull requests, so every MatrixRTC fault was Consort's alone and the rebase
`DEPENDENCIES.md` describes had nothing left to rebase onto. The fork was not
nine commits ahead of a moving target. It was nine commits ahead of a stopped
one, 257 commits behind where the work actually went.

## Decision

Move both pins together, in one change, to
`matrix-org/matrix-rust-sdk@56bc591` (tagged `matrix-sdk-ffi/20261020`) and
`element-hq/matrix-rust-rtc@4b3cee5`, and accept losing two call-quality
signals rather than carry a fork of element-hq to keep them.

## What was weighed instead

**Rebase the fork.** The first option `DEPENDENCIES.md` names, and the one that
has worked every other time. It cannot work here: the upstream it would rebase
onto has stopped, so a rebase buys nothing but the same nine commits on an
older base.

**Fork element-hq instead.** Keeps the two lost events and keeps a fork to
rebase forever, against a tree moving fast enough to have added 257 commits in
two months. Rejected because it spends the thing the move is for.

**Stay.** Keeps the reachable advisory, keeps the list of six entries waiting
on a pin, and widens the gap every week. Rejected on the first of those alone.

## Consequences

**The advisory clears, and the list shrinks.** Measured by running `cargo audit`
against both lockfiles with the ignore file set aside: eleven advisory rows
before, five after. `RUSTSEC-2026-0451` and `RUSTSEC-2026-0292` come out of the
file outright, taking it from seven entries to five.

**Two call-quality signals are lost.** `CallEvent::KeyDistributionFailed` and
`CallEvent::KeyDistributionRecovered` were the owner's, and they are the only
thing Consort could say about the one fault in an encrypted call that the
person causing it cannot see: their own media looks fine locally while nobody
can decrypt it. They are not deleted. `Fault::NotDistributed` and both its
sentences are kept, the decision moved into `trouble::what_a_rollout_says`, and
nothing calls it, because `LiveKitCall` exposes no way to reach the list
upstream already keeps.
[docs/upstream](../upstream/0001-expose-the-members-a-media-key-rollout-did-not-reach.patch) holds the patch that would fix that,
which is two delegating accessors and no new types.

**`matrix-sdk-ui` arrives in the graph.** Transitively, because
`matrix-rtc-call-sdk`'s `matrix-sdk` feature needs it for `unstable-msc4354`.
Nothing in Consort uses it: the timeline is deliberately on the base SDK, which
is [ADR-0010](0010-a-timeline-on-the-base-sdk.md). What it costs is measured
below.

**A compiler floor arrives with it.** The lockfile pins `webrtc-sys` 0.3.51,
whose hermetic libc++ requires clang 21, and Debian bookworm ships 14. Both CI
jobs that build Rust in that container needed an `apt.llvm.org` source: see the
plan document's section on it.

**The `LocalSet` constraint goes away.** element-hq's `LiveKitCall::join`
spawns through `matrix_rtc_core::executor`, which requires `Send`, so the call
actor thread in `app/src-tauri/src/state.rs` could become an ordinary tokio
task. Deliberately not in this change: the thread is also what keeps the call
off the thread that draws, so it is its own change with its own tests, and
CLAUDE.md's section on it is to be rewritten rather than deleted.

**Nothing was lost in the roster.** The archived fork's `5a0fe1b` built the
roster from publications rather than subscriptions, and the plan could not
settle whether that behaviour was upstream. It is not, and it does not need to
be: element-hq derives the roster from the core's membership snapshots and lets
transports attach media to entries that already exist, so a peer whose track we
never subscribe to is on the roster regardless. One adjacent gap is real and is
upstream's, not ours: a remote stream is added with `muted: false` whatever the
publication says, so a peer who was already muted when we subscribed reads as
live until they next toggle.

## The two decisions taken inside the move

**`Dialect` keeps all three variants.** Upstream went from three wire modes to
two, and `Sticky` and `State` now both map to `MembershipFormat::RoomState`.
Deleting `Sticky` was the tidier change and would have been a mistake:
`settings.json` is read with one `serde_json::from_slice`, so a file naming a
dialect this build no longer knows does not lose that one setting, it reverts
every other setting in the file to its default. The collapse is unobservable
anyway, because `Dialect::readable` refuses a `Sticky` join before the mapping
is consulted.

**`livekit_transport` replaces `livekit_service_url_fallback`.** The field
Consort fed from `.well-known` discovery is gone by design: element-hq's
`token.rs` argues that there is no safe fallback between `/sfu/get` and the
homeserver's `get_token`, because the endpoint mints the identity peers map us
by and a wrong guess connects fully and decrypts nothing. The replacement is an
override rather than a fallback, which is the right way round for the only
dialect this build joins, and it is upstream's own idiom in both its examples.

## What the new dependency costs

Measured on this branch, on sixteen cores, in the debug profile unless said
otherwise.

**Nine crates and forty seconds of build.** `matrix-sdk-ui` enters the graph
through `matrix-rtc-call-sdk` and nothing else. Diffing the resolve graph from
`cargo metadata` against the same graph with its node deleted, nine packages
are reachable only through it: itself, `async-rx`, `async_cell`, `emojis`,
`eyeball-im-util`, `fuzzy-matcher`, `generator`, `loom` and `scoped-tls`.
Cleaning all nine and rebuilding takes 39.8s wall, against 4.4s for
`matrix-sdk-ui` on its own, so most of the cost is the eight it brings with it.
Its own rlib is 20.6 MB.

**None of it reaches the binary.** Not one of the 992,370 symbols in the
unstripped debug binary names it, which is the linker finding nothing in that
rlib that anything references. Measured there rather than on the release
binary, which is 120.6 MB and `strip = true` for libwebrtc's sake, so it can
say nothing about its own symbols.

**And it cannot be cut.** The feature that pulls it is
`matrix-rtc-call-sdk`'s `matrix-sdk`, which is the `LiveKitCall` facade this
project joins calls through, and that feature asks for
`matrix-sdk-ui/unstable-msc4354` because `SyncService` requesting sticky events
over sliding sync is what MSC4354 support is made of. Giving the crate up means
giving the facade up.

**Neither `[patch.crates-io]` entry is needed.** element-hq states that a
downstream workspace must replicate them or the SDK fails to build, and with
Consort's features neither is used: cargo warns about both on every invocation
and `Cargo.lock` records both under `[[patch.unused]]`. Removing `async-compat`,
`const_panic`, and both at once, each passes `cargo check --workspace
--all-targets` and `cargo check -p consort-app --all-targets --features
self-update`, which is the feature only the Windows release job turns on and so
a graph nothing else resolves. The only lockfile movement in any of the three is
the deletion of the `[[patch.unused]]` record itself. They are left in place
here so that this change stays one thing; taking them out is a two line
follow-up with the measurement above behind it.

## When this would be revisited

The gate is a call. Two real clients in the state dialect, heard by two people,
because every other check this repository has passes already and none of them
can tell a call that connects from a call somebody can hear. Until that
happens, this stays `proposed` and #217 stays a draft.

After that, the open question is the two lost events. The choice is the
[upstream patch](../upstream/0001-expose-the-members-a-media-key-rollout-did-not-reach.patch), a fork
of element-hq, or a written decision to drop the capability. The first is narrow enough to be worth asking
for, which is the one thing the archived fork made impossible.
