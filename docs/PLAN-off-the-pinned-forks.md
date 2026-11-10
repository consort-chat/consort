# Plan: off the pinned forks

What it would take for Consort to stop depending on two personal git forks and
sit on officially maintained upstreams instead.

This is an investigation, not a migration. No pin was changed in the branch
that carries this file. The spike described below was run in a worktree and
reverted.

Short version: the SDK half is nearly free and the MatrixRTC half is a real
piece of work, the reachable crypto advisory does clear, the `LocalSet` the
call actor exists for goes away, and three of the nine commits the fork carries
are Consort's own features that the new upstream does not have.

## The short answer

| Question | Answer |
| --- | --- |
| Is there an officially maintained MatrixRTC to move to? | Yes. `element-hq/matrix-rust-rtc`, pushed to today. The upstream of our fork is archived and read-only since 20 September 2026. |
| Does it pin the official SDK? | Yes. `matrix-org/matrix-rust-sdk` at `56bc5911`, tagged `matrix-sdk-ffi/20261020`. |
| Does `consort-matrix` survive the SDK bump? | Yes, with four mechanical changes. Measured, not guessed. |
| Does `consort-call` survive the rtc move? | No. Twelve compile errors after the renames, all named below. |
| Does `RUSTSEC-2026-0451`, the reachable vodozemac flaw, clear? | **Yes.** Consort resolves `vodozemac` 0.11.2. See [What the audit list does](#what-the-audit-list-does). |
| Does the state dialect survive? | Yes, as `MembershipFormat::RoomState`, with the MSC4140 delayed *state* leave intact. |
| Does the `!Send` constraint survive? | No. It goes away, and the call actor thread could become an ordinary tokio task. |
| Can the two pins move separately? | No. They are one change. |
| Does the owner's MSC4143 patch need re-proposing? | No. Upstream handles it, in a stronger form. Three other fork commits do need re-proposing. |

## Where things stand

Two pins, described at length beside themselves in `Cargo.toml` and
procedurally in [DEPENDENCIES.md](DEPENDENCIES.md). What is new since those
were written:

**The upstream of our fork is archived.** `BillCarsonFr/matrix-rust-rtc` main
sits at `4cebb37`, 20 September 2026, and its README says the project is now
maintained at `element-hq/matrix-rust-rtc` and that this repository is read-only
with no issues, pull requests or releases. Our fork, `tominal/matrix-rust-rtc`
at `0427fc00`, is nine commits on top of `68e5bca` of 3 September. The archived
upstream added twelve commits we do not have and then stopped.

**element-hq is where the work went.** Its tree carries 257 commits past the
base our fork sits on, and its workspace version is `0.5.0-rc.10` against the
archived fork's `0.2.0-rc.1`. Its last push is today.

**The matrix-sdk pin is three months behind.** `377330059` is a fork commit by
Valere, "update latest ruma rtc event format", not an ancestor of upstream main.
element-hq's pinned `56bc5911` carries 952 commits that are not in ours.
element-hq builds the same stack against upstream, so whatever that fork commit
added has an upstream equivalent, which is the SDK's own `unstable-msc4354`
feature that element-hq turns on through `matrix-sdk-ui`.

**The audit list has become the measure of all this.** [PR
#215](https://github.com/consort-chat/consort/pull/215), open and green, adds
six ignore entries to `.cargo/audit.toml`, and six of the seven entries there
after it merges say they come out only when one of these pins moves.

## The spike, and what it proved

Run in a worktree and reverted. Both pins repointed at the same time:
`matrix-sdk` and `matrix-sdk-base` at `matrix-org/matrix-rust-sdk@56bc5911`,
and the rtc crates at `element-hq/matrix-rust-rtc@4b3cee5`, with element-hq's
`[patch.crates-io]` table replicated, `livekit` moved from `0.7` to `0.9.7`,
and the crude import renames the new crate layout needs.

It got as far as `consort-call`'s own source. That is further than expected.

**The graph resolves, and the whole dependency set compiles.** One `matrix-sdk`
in the lockfile, at 0.19.1. `livekit`, `libwebrtc`, `webrtc-sys`,
`matrix-rtc-call-sdk`, `matrix-rtc-transport`, `matrix-rtc-core` and
`matrix-sdk-ui` all build.

It took two rounds, and the second one is the hand-pinned lockfile problem from
`Cargo.toml` happening again, exactly as written there. A fresh resolve picked
`livekit-signaling` 0.1.9, `libwebrtc` 0.3.55 and `webrtc-sys` 0.3.53; livekit
0.9.7 does not compile against `livekit-signaling` 0.1.9, which has no
`use_legacy_data_streams` field on `SignalOptions`. Pinning all three back to
what element-hq's own lockfile holds (0.1.7, 0.3.54, 0.3.51) fixed it. So the
eight hand-pinned entries become ten or so, against a different lockfile, and
the procedure in DEPENDENCIES.md still applies unchanged.

**`consort-matrix` needs four changes.** That is the whole three months of SDK
drift, 952 commits of it, against the crate that holds nearly all of Consort's
Matrix logic:

1. `#![recursion_limit = "256"]` on the crate and on each integration test. The
   reqwest and tower retry future got deep enough that proving it `Send` at
   `tokio::spawn` overflows the default depth limit. A compiler limit, not a
   breakage.
2. `Room::load_event_receipts` now takes `&ReceiptThread` rather than
   `ReceiptThread`.
3. `Client::event_cache()` now returns `Option<&EventCache>`
   (`matrix-rust-sdk@c4396ed`), and `Room::event_cache` is gone.
4. `EventCache::subscribe` is gone. The cache is created and subscribed by the
   client, and disabled through `ClientBuilder` instead, so `receipts::count_unread`
   has nothing left to call. Worth a look rather than a delete: that function
   exists so an unread count is not silently one that will never have any, and
   the replacement check is whether the cache exists at all.

With those four, `cargo check -p consort-matrix --all-targets` is clean.

**`consort-call` needs twelve.** After renaming the crates and the three types
whose module moved, `cargo check -p consort-call` reports exactly twelve errors,
and every one is a named thing rather than a cascade:

| What breaks | What it becomes |
| --- | --- |
| `CallOptions::element_call_compat` | `LiveKitCallOptions::format`, a two-valued `MembershipFormat` |
| `CallOptions::livekit_service_url_fallback` | Gone by design. `livekit_transport: Option<LiveKitTransport>` instead |
| `PublishOptions` built by hand | Three new fields: `muted`, `video_codec`, `video_encoding` |
| `matrix_rtc_livekit::identity::pseudonymous_identity(user, device, member)` | `matrix_rtc_core::compat::room_state::participant_identity(user, device)`, two arguments, and format-specific |
| `CallEvent::KeyDistributionFailed` | Nothing. Ours. See [the owner's patch](#what-happens-to-the-owners-patch) |
| `CallEvent::KeyDistributionRecovered` | Nothing. Ours |
| `CallEvent::ParticipantLeft` | Gained `participant_count` |
| `CallEvent::Ended` | Gone. The call's end is on `LiveKitCall::subscribe_status` |
| `CallError` match | New `LiveKitCallError::Attach` variant |

`matrix-rtc-core` also has to stop being a dev-only dependency of
`consort-call`: `MembershipFormat` and `participant_identity` both live there.

What the spike did **not** do is make `consort-call` compile. Working through
those twelve is a migration, and the three that are not mechanical (the dialect
collapse, the transport override replacing the fallback URL, and the two lost
events) are decisions rather than edits.

## What the move buys

### What the audit list does

Measured, by running `cargo audit` against main's lockfile and then against the
spike's. Nineteen vulnerability rows before, fourteen after.

| Advisory | Crate, before and after | Verdict |
| --- | --- | --- |
| RUSTSEC-2026-0292 | `imbl` 6.1.0 → 7.0.2, `imbl-sized-chunks` 0.1.3 → 0.2.0 | **Clears.** The ignore entry becomes unnecessary |
| RUSTSEC-2026-0451 | `vodozemac` 0.10.0 → **0.11.2** | **Clears.** This is the reachable one |
| RUSTSEC-2026-0367/0368/0369/0429 | `zip` 0.6.6 and 4.6.1 → 4.6.1 only | **Halves.** `webrtc-sys-build` moves 0.3.18 → 0.3.21 and uses `zip` 9.5.0. The remaining copy is `tauri-plugin-updater`'s |
| RUSTSEC-2026-0459 | `libsqlite3-sys` 0.35.0 → 0.38.2 | **Survives.** The fix is 0.39.0, which first arrives with `rusqlite` 0.41; `56bc5911` asks for 0.40.2 |
| RUSTSEC-2026-0346/0347/0348/0349, 0381/0382/0383, 0414/0433 | `rustls`, `webpki`, `pulldown-cmark`: identical before and after | **Unaffected.** Not pin-bound, and #215 moves the versions |

The vodozemac result deserves spelling out, because reading element-hq's
manifest gets it wrong. That manifest asks for `vodozemac = "0.11.1"` and its
own lockfile holds 0.11.1, so element-hq is itself exposed. But `"0.11.1"` is a
caret requirement, Consort resolves its own lockfile, and 0.11.2 is
semver-compatible, so cargo picks the patched version. Today's pin cannot do
that: `matrix-sdk` 0.18.0 asks for `vodozemac ^0.10.0` and 0.11.2 is
semver-incompatible with it, which is why the entry exists. So what the entry
says, that it comes out when the matrix-sdk pin moves, is right, and this move
is enough to take it out.

### The LocalSet goes away

The constraint that shapes `app/src-tauri/src/state.rs`, and that `CLAUDE.md`
calls out as a thing that will waste somebody's time, is gone upstream.

The fork's `matrix-rtc-livekit/src/call.rs` says it in its module docs: the
core's command sender is `?Send`, the futures driving the session are `!Send`,
`Call::join` uses `spawn_local` internally and panics outside a
`tokio::task::LocalSet`. element-hq's equivalent says instead that
`LiveKitCall::join` must be called from within a tokio runtime, because the
library spawns onto the current one through `matrix_rtc_core::executor`. That
module re-exports `tokio::task::spawn`, which requires `Send`. The only native
`spawn_local` left in the tree is in an example tool that owns its own
`LocalSet`, and `matrix-rtc-call-sdk`'s own docs say everything in the crate is
`Send`, deliberately on the other side of a channel boundary from the core's
`?Send` futures.

So the dedicated thread with a current-thread runtime, the command channel and
the event stream coming back could all become one ordinary task, and
`Call::join` could live in a Tauri command after all. That is a real
simplification, and it is worth not taking it in the same change as the
migration: the actor is also what keeps the call off the thread that draws, and
collapsing it is its own decision with its own test surface.

### Being able to file a bug

Not a code change, and the most durable part. An archived repository takes no
issues and no pull requests, so right now every fault in the MatrixRTC layer
is Consort's to diagnose, patch and carry. element-hq's is open.

## What it does not buy

**The MSC dialect is still unreadable.** `Dialect::readable`'s chain is written
down in `crates/consort-call/src/dialect.rs`, and the move breaks one link of
four. Link 4, that the sticky bridge takes room updates as a wake source only in
the state dialect, is gone: element-hq's room feed subscribes to
`room.subscribe_to_updates()` and `room.sticky_events()` unconditionally, for
every format. But links 2 and 3 stand. Sticky membership still arrives only
through the sliding-sync path, element-hq says so twice (its `matrix-sdk`
feature turns on `matrix-sdk-ui/unstable-msc4354` "to make `SyncService` request
sticky events over sliding sync", and its preconditions name a running
`SyncService`), and Consort syncs with `sync_with_result_callback`. So joining in
`MembershipFormat::Msc` would still connect, publish membership, carry RTP and
show an empty room. Sliding sync is still the gate, and this move does not open
it.

**`libsqlite3-sys` stays on the ignore list,** and its reasoning does not even
change: one database, at a path Consort made itself, with nobody less trusted
able to write it or its `-wal`.

**Four of the zip entries stay,** narrowed to `tauri-plugin-updater` alone,
which is the second half the #215 comment already describes as a change of its
own behind the `self-update` feature.

**Three fork commits have no home.** Below.

## What happens to the owner's patch

The fork carries nine commits. They are not one patch, and they do not all need
the same thing.

**The MSC4143 safety patch is upstream, in a stronger form.** `89bd966` ("do not
encrypt media frames in an unencrypted room") and `8bad817` ("do not distribute
media keys in an unencrypted room") exist because the pre-MSC4354 Element Call
dialect has no `m.rtc.slot` event, so nothing consulted
`RoomEncryption::Unencrypted` and GCM frame encryption went on in a plain room,
which leaves a conforming peer decoding ciphertext as audio.

element-hq closes exactly that hole, at the point where the rule is decided
rather than at the two places it is applied. `SlotSession::negotiated_encryption`
in `matrix-rtc-core` returns `Some(false)` when there is no slot to read a
mechanism from and the room is unencrypted, and its comment names the case:
"the room-state format reads none: the room alone decides". That one answer
reaches both halves, `encryption_config.manage_media_keys = negotiated` for the
keys and `e2ee: call.encrypts_media()` in `attach.rs` for the frames. The room's
encryption state arrives through the feeder rather than needing the host to
report it, and there are tests on the unencrypted path, including spec 038 R37:
a join made in the clear leaves the moment the room's encryption turns on. So
the patch does not need re-proposing, and the rule is better placed than ours.

**Three more are already upstream too.** `4cbe282` (mute state on a track that
arrives already muted) is covered by `StreamState::muted`, `PublishOptions::muted`
and `CallEngine::set_local_muted`. `896f493` (each participant's join time on
the roster) is covered by `Participant::joined_at_ms`, with upstream's own note
that it is `None` for a native MSC4143 membership because that carries no join
time. `5a0fe1b` (build the roster from publications, not just subscriptions)
needs checking against element-hq's engine rather than against its type surface,
and is listed as an unknown below. `0427fc0` is a test for `896f493`.

**Three are ours and have nowhere to go.** `6c9ce65`, `d83686b` and `3598bee`
built `CallEvent::KeyDistributionFailed` and `CallEvent::KeyDistributionRecovered`:
this session's own media key failing to reach the call, reported from where that
is actually known, and the recovery. element-hq's `CallEvent` has no equivalent.
It reports the receiving side well, with `UnableToDecrypt`, `KeyDiscarded`,
`KeyImported` and `FrameEncryptionState` carrying a diagnostic, and it has a
variant ours does not (`UnknownParticipant`), but nothing says "nobody can hear
you because your key did not get out". Consort renders that, and it is the most
useful thing it has to tell somebody about a call nobody can hear.

So the choice the owner has is about those three and not about the MSC4143 one:
propose them to element-hq, which now accepts pull requests, or carry a fork of
element-hq instead of a fork of an archive. The first is the point of moving.
The second is the same position Consort is in today, with a live upstream
underneath it, which is still strictly better.

## Does the dialect story hold

Yes, with one shape change and one improvement.

**The state dialect survives** as `MembershipFormat::RoomState`, and
`LiveKitCallOptions::format` documents it in the same terms Consort's
`Dialect::State` does: membership moves to `org.matrix.msc3401.call.member` room
state, the SFU identity becomes the plain `{user}:{device}` string, the token
comes from the pre-MSC4195 `/sfu/get` endpoint, and nothing about such a call is
visible to a spec-current peer. `LiveKitCall::join` logs a warning saying so.

**The delayed leave still empties the membership.** This is the part
`CLAUDE.md` is careful about, because in the sticky dialects a delayed leave is
a plain event that clears nothing and a dead client lingers for the sticky
duration, an hour by default. In the room-state format element-hq's
`DialectBackend` routes the delayed leave through `send_delayed_state_event`
with `{}` content, which `SdkMatrixBackend` implements, and there is a test
pinning it. So the MSC4140 delayed *state* leave arms exactly as it does today,
and a quit that fails to leave still costs 15 to 30 seconds rather than an hour.

Worth knowing: on the SDK host the sticky side is now *worse* than Consort's
note implies. `SdkMatrixBackend::send_delayed_sticky_event` refuses outright,
because the SDK cannot attach a sticky duration to a delayed send
(matrix-rust-sdk#6973), and the core then joins with no dead man's switch at all
on a five minute lifetime. That is another reason the state dialect stays the
default.

**Three values become two.** `MembershipFormat` has `Msc` and `RoomState`.
Element Call's 2025 sticky dialect is not a third option: it is read inside the
room-state format, because Element Call writes sticky memberships beside its
state events during the transition. So Consort's `Dialect::Sticky` has no
counterpart, and `Dialect` either collapses to two values or keeps three with
`Sticky` mapping to `RoomState` and a comment saying why. `detect` and the
settings contract both depend on the answer, and the `name()` strings are a
settings file's contract that a rename silently breaks.

## The crate map

Where the types `consort-call` uses went. `matrix-rtc-media` and
`matrix-rtc-bridge` are both gone as crate names.

| Was | Is |
| --- | --- |
| `matrix-rtc-media` (transport-level) | `matrix-rtc-transport`: `AudioFrame`, `VideoFrame`, `I420Buffer`, `VideoRotation`, `MediaConstraints`, `VideoDetail`, `Dimensions`, `QualityLimit`, `MediaStreamKind`, `PublishOptions`, `AudioSourceConfig`, `VideoSourceConfig`, `TransportError`, `FrameEncryptionState` |
| `matrix-rtc-media` (call-level) | `matrix-rtc-call-sdk`: `CallEvent`, `Participant`, `StreamState`, `FrameEncryptionDiagnostic`, `UnableToDecrypt` |
| `matrix-rtc-media::{LocalTrackHandle, RemoteTrackHandle}` | Same names in `matrix-rtc-transport`, but **traits** now, held as `Arc<dyn _>` |
| `matrix-rtc-bridge` | `matrix-rtc-matrix-sdk`, exposing `SdkMatrixBackend`, re-exported from `matrix-rtc-call-sdk` |
| `matrix-rtc-livekit::{Call, CallError, CallOptions}` | `matrix-rtc-call-sdk::{LiveKitCall, LiveKitCallError, LiveKitCallOptions}`, behind its `matrix-sdk` feature |
| `matrix-rtc-livekit::compat::ElementCallCompat` | `matrix-rtc-core::compat::MembershipFormat` |
| `matrix-rtc-core::reactions::ReactionError` | `matrix-rtc-call::reactions::ReactionError` |
| `matrix-rtc-core::encryption::types::KeyRejection` | Unchanged |
| `matrix-rtc-livekit` (the LiveKit transport itself) | Still `matrix-rtc-livekit`, but a transport implementation rather than the call facade |

`LiveKitCall`'s surface is close enough to the fork's `Call` that `livekit.rs`
maps across: `join`, `publish`, `remote_track`, `receive_stats`,
`set_constraints`, `subscribe_call_events`, `participants`,
`subscribe_participants`, `membership_id`, `local_identity`, `session`, `events`,
`leave`. New alongside them: `status` and `subscribe_status`, `engine()` onto the
whole `CallEngine`, reactions and the raised hand.

## Unknowns I could not settle

Named rather than guessed at, because a plan that hides these is worse than one
that is shorter.

- **Whether `5a0fe1b` is upstream.** "Build the roster from publications, not
  just subscriptions" is behaviour inside the engine, not a type, so reading
  element-hq's `Participant` says nothing about it. It needs reading
  `matrix-rtc-call-sdk/src/engine.rs` against the fork's
  `matrix-rtc-media/src/engine.rs`, or a call with two peers.
- **What replaces `.well-known` discovery.** `livekit_service_url_fallback` is
  gone deliberately: element-hq's `token.rs` says there is no fallback between
  `/sfu/get` and the homeserver's `get_token` because the endpoint mints the
  identity peers map us by, and a wrong guess connects fully and decrypts
  nothing. `livekit_transport: Option<LiveKitTransport>` is the override, which
  looks like a better fit for `consort_call::discovery` than the fallback was,
  but it was not built in the spike and the shape of `LiveKitTransport` was not
  checked against what a `.well-known` focus entry gives.
- **Whether the three `[patch.crates-io]` entries are needed.** element-hq says
  a downstream workspace must replicate them or the SDK fails to build. With
  Consort's feature set all three (`async-compat`, `const_panic`, `zxcvbn`)
  resolve as unused, and cargo warns about each on every invocation. The SDK's
  own table has all three; element-hq replicates two. Keeping all three is
  harmless except for the noise, dropping them is untested, and which is right
  depends on feature combinations the spike did not sweep.
- **What `matrix-sdk-ui` in the graph costs.** It arrives transitively, because
  `matrix-rtc-call-sdk`'s `matrix-sdk` feature needs it for
  `unstable-msc4354`. Nothing in Consort uses it, the timeline is deliberately
  built on the base SDK instead, and the build time and binary size it adds were
  not measured.
- **Whether `consort-audio`, `consort-video` and the Tauri shell are clean.**
  The spike stopped at `consort-call`, so `app/src-tauri` was never checked, and
  it is where the call actor and every command live.
- **The frontend.** Not looked at. Any `CallEvent` shape that reaches the IPC
  changes with the two lost variants.

## A staged route

The two pins cannot move separately. `LiveKitCall::join` takes a
`&matrix_sdk::Room`, so the rev element-hq pins and the rev this workspace pins
have to be the same string or cargo resolves two copies of the SDK, which is the
whole reason the comment in `Cargo.toml` exists. Moving the SDK alone to
`matrix-org@56bc5911` while the rtc crates still point at a fork pinning
`BillCarsonFr@3773300` is precisely the forbidden state.

So there is no staged route through the manifest. There is one through the
branch.

**Stage 0: decide, and record it.** The decision is whether to move at all, and
it is not a technical one. Moving means accepting a larger one-off change than
Consort usually takes and three features to re-propose; not moving means what
the last section describes. Gate: an ADR, written once the decision is made.
This document is not one on purpose.

**Stage 1: the SDK changes, on their own, against the current pins.** Three of
`consort-matrix`'s four fixes are forward-compatible with today's pin:
`recursion_limit` is additive, `&thread` is not (the signature differs), and the
event cache changes are not. So only the recursion limits can land ahead. That
is small, but it is the honest first step and it is worth taking, because it is
the one piece of the migration that cannot break anything. Gate: the full set
from `CLAUDE.md`, unchanged pins, green CI.

**Stage 2: the migration, one change.** Both pins, the lockfile pins
re-established against element-hq's lockfile, the crate renames, the four
`consort-matrix` fixes, the twelve `consort-call` ones, `app/src-tauri`, and the
frontend if the IPC shape moves. The two decisions inside it (what `Dialect`
becomes, and what carries `.well-known` discovery) should be settled before the
first edit rather than discovered in the compiler. Gate: `cargo test
--workspace` green and off the network, clippy clean, coverage above 90%, the
eight `#[ignore]` tests run against `testing/synapse`, and a call held between
two real clients in the state dialect, because nothing in CI can tell a call
that connects from a call somebody can hear.

**Stage 3: the ignore list shrinks.** `RUSTSEC-2026-0292` and
`RUSTSEC-2026-0451` come out, the four zip entries have their comments narrowed
to `tauri-plugin-updater`, and `RUSTSEC-2026-0459` has its "comes out when the
matrix-sdk pin moves" line corrected, because it will not have. Gate: `cargo
audit` green with a shorter file than it had.

**Stage 4: the three features, re-proposed.** `KeyDistributionFailed` and
`KeyDistributionRecovered` to element-hq, which takes pull requests. Until they
land, Consort either carries a fork of element-hq or loses the one thing it can
say about a call nobody can hear. Gate: a pull request open upstream, or a
deliberate decision to drop the feature, written down.

**Stage 5, separately and later: the actor thread.** `Call::join` no longer
needs a `LocalSet`, so the thread in `state.rs` can go. Not in stage 2. Gate:
its own change, its own tests, and `CLAUDE.md`'s section on it rewritten rather
than deleted, since the reason it existed is worth keeping.

## What doing nothing costs

Not a slow decay. Three specific things.

**The crypto stack stays three months behind, and that is the pin least under
this project's control.** `RUSTSEC-2026-0451` is reachable, sits on the path
every Olm session takes, and is being accepted because cargo refuses the bump
rather than because anybody judged the risk acceptable in the abstract. It is
fixable today by moving these pins, as measured above. Every further advisory in
the crypto stack will arrive the same way: fixed upstream, unreachable from
here, and added to a list.

**The ignore list becomes the shape of the problem.** Seven entries after #215,
six of them waiting on a pin. The file is written so that each entry is a
decision rather than a way to quiet a build, which is the right design, and it
is exactly why a list of six entries all waiting on the same thing is a signal
rather than a maintenance task. The habit it was built to avoid is the habit it
will teach if the pin never moves.

**Nobody can be asked for help.** An archived upstream takes no issues and no
pull requests. Every MatrixRTC fault is Consort's alone, every fix is a patch on
a fork of a dead repository, and the rebase that `DEPENDENCIES.md` describes has
no upstream left to rebase onto. The fork is not nine commits ahead of a moving
target any more. It is nine commits ahead of a stopped one, 257 commits behind
where the work actually is, and that gap only widens.
