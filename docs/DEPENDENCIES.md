# Pinned dependencies

Four things in this workspace are pinned to an exact revision rather than to a
version range, and none of them can be updated by `cargo update`. This is the
written half of that: what each pin is, who owns it, what would move it, and
how to find out how far behind it has fallen.

The reasoning for each pin lives in the comment beside it in `Cargo.toml`. This
file is the part that does not fit there: the process.

## The pins

### matrix-sdk, someone else's fork

`BillCarsonFr/matrix-rust-sdk` at `377330059c3a335ab36d190fc10b87de3427c6b3`,
naming `matrix-sdk` and `matrix-sdk-base`.

Not ours. It is the fork `matrix-rust-rtc` develops against, and the two revs
have to be the same string or cargo resolves two copies of the SDK and the
`Client` this workspace builds stops being the type the call API accepts.

**Moves when** the MatrixRTC pin below moves, and never on its own.

**Falls behind** upstream `matrix-org/matrix-rust-sdk`, which is where security
fixes to the crypto stack land first. This is the pin that matters most for
that reason and the one this project has least control over. The end state is
being back on a crates.io release, which happens when the MSC4143 work merges
upstream.

### matrix-rtc-livekit, matrix-rtc-media, matrix-rtc-core, our fork

`tominal/matrix-rust-rtc` at `0427fc00038bb1dce07b8ff28910b6629964a3ad`.

A fork of `BillCarsonFr/matrix-rust-rtc`, rebased onto upstream
`68e5bca` on 4 September 2026 and carrying eight commits on top of it plus one
test, on `feature/plain-rooms-and-arrival-mute-rebased`. The oldest and most
important is the one that stops media frames being encrypted in an unencrypted
room, which MSC4143 forbids and which leaves a conforming peer decoding
ciphertext as audio.

The rebase is what brought in upstream `bec01a7`, which is the fix for a device
that leaves a call and rejoins being unable to hear anyone who stayed. Key
distribution compared participations by `member_id` alone, which is sound under
MSC4143 where that id is fresh per join, and wrong in the pre-sticky Element
Call dialect this build always speaks, where it is `{user}:{device}` and comes
back reused. The comparison is now `(member_id, membership_ts)`.

The previous rev, `934cdbad34bda1a8a170faa022dcdd5436f4072a`, sat on
`7d9944fd6b02cbd09fc9cceff843c55ae2a8d4d8` and predates that fix. The branch it
was on is still there, unrebased, as a record.

**Moves when** one of those commits lands upstream, when upstream carries a fix
this project needs, or when a change here needs a new field on one of the fork's
types (ADR-0001 describes one that is outstanding).

**Ours to keep rebased.** A stale fork is a silent way to miss upstream fixes,
which is the risk this pin has and the matrix-sdk pin above does not.

**How far behind:**

```sh
cd ../matrix-rust-rtc
git fetch origin
git log --oneline $(git merge-base HEAD origin/main)..origin/main | wc -l   # upstream commits we do not have
git log --oneline $(git merge-base HEAD origin/main)..HEAD                  # ours they do not have
```

Rebasing means moving the rev in this workspace's `Cargo.toml` and the SDK rev
with it, since the fork's own matrix-sdk pin may have moved too. Check
`crates/matrix-rtc-livekit/Cargo.toml` in the fork before touching either. On
the September 2026 rebase it had not moved, and the `[patch.crates-io]` set
below was already identical, so only the three MatrixRTC revs changed.

What that rebase cost on this side, as a guide to the next one: upstream had
moved the media key bookkeeping out of `matrix-rtc-livekit` into a new
`matrix-rtc-media::keys`, so four of the eight commits had to be re-landed
against the new home rather than merged. In this workspace it was two test
fixtures and one match arm, all of them named by the compiler.

### Eight livekit and webrtc crates, pinned in Cargo.lock

`livekit`, `livekit-api`, `livekit-datatrack`, `livekit-protocol`,
`livekit-runtime`, `libwebrtc`, `webrtc-sys`, `webrtc-sys-build`.

These publish semver-compatible releases that do not compile with each other,
so they are held at the versions `matrix-rust-rtc`'s own lockfile carries. A
bare `cargo update` breaks the build with a type error inside a dependency
rather than anything naming a version.

**Moves when** the MatrixRTC pin moves and its lockfile has moved with it. Pin
parents before children: `livekit` before `livekit-api`, `livekit-datatrack`
before `livekit-protocol`.

## Advisories

CI runs `cargo audit` and `pnpm audit` on every push and pull request, and both
fail the build on a vulnerability. On a dependency set this frozen that is the
point: an advisory nobody can fix by moving a version is exactly the kind of
thing that gets scrolled past when it is only a warning.

Each runs in a job of its own, Advisories and Frontend advisories, so an open
advisory reports beside the type check, the tests and the build rather than
instead of them. Both read a lockfile and need nothing compiled.

`pnpm audit` ignores nothing, and has no ignore file to do it with. The gate is
`--audit-level=moderate`, so a low advisory is reported and passes while
anything above it fails. The frontend half of this dependency set is not frozen
the way the Rust half is, so the first option below is almost always open there:
eleven undici advisories (#136) went that way, on a jsdom bump inside the range
`app/package.json` already allowed.

Seven `cargo audit` advisories are ignored, in `.cargo/audit.toml`. The file
carries the argument for each; this is the index.

| Advisory | Crate | Why it is ignored | What takes it out |
|---|---|---|---|
| RUSTSEC-2026-0292 | `imbl-sized-chunks` 0.1.3 | A double free reachable only when an element's `Drop` panics. Nothing put in an imbl collection here has one: the timeline is an `ObservableVector<Arc<TimelineItem>>`, and what that bottoms out in is ruma event types, strings and counted pointers. | The matrix-sdk pin moving. Only imbl 7.0.2 asks for the patched `imbl-sized-chunks`, the pinned fork holds `imbl = "6.1.0"`, and upstream matrix-rust-sdk is already on 7.0.2. |
| RUSTSEC-2026-0459 | `libsqlite3-sys` 0.35.0 | Uninitialized memory out of the bundled C SQLite, for a process that opens a crafted database and `-wal` pair. Consort opens one database, the store it made itself under the app data directory, and takes a path for one from nowhere else. | The matrix-sdk pin moving. rusqlite 0.37.0 asks for `libsqlite3-sys ^0.35.0`, the patched bundle is 0.39.0, and upstream matrix-sdk-sqlite is already on rusqlite 0.41.1. |
| RUSTSEC-2026-0451 | `vodozemac` 0.10.0 | Reachable, and accepted on impact: the Olm ratchet leaves old root and chain keys in freed heap, which costs forward secrecy for earlier messages to anybody who can already read this process's memory and therefore already holds the live keys. | The matrix-sdk pin moving. matrix-sdk 0.18.0 asks for `vodozemac ^0.10.0`, the fix is 0.11.2, and upstream matrix-rust-sdk is already on it. |
| RUSTSEC-2026-0367, 0368, 0369, 0429 | `zip` 0.6.6 and 4.6.1 | Four extraction flaws, at two call sites that extract no archive anybody else chose: `webrtc-sys-build`, a build dependency unpacking LiveKit's own release asset, and `tauri-plugin-updater`, compiled only under `self-update` and extracting signature-verified bytes in `#[cfg(windows)]` code. | Two moves. The MatrixRTC pin reaching a lockfile whose webrtc-sys matches webrtc-sys-build 0.3.20, which carries a different `WEBRTC_TAG`, and tauri-plugin-updater 2.14.1 with the wry 0.55 to 0.57 bump under it, as its own change. |

Every one of those is the third option below, reached because the first two are
closed: cargo refuses the version bump outright, and the fix is in a crate one
of the pins holds rather than in either fork.

`cargo audit` also reports crates that are unmaintained, unsound or yanked, and
those do not fail the build. Eighteen of them do not: the unic and
proc-macro-error families, zbus's zvariant, two yanked crates, and unsoundness
in glib, tokio and bitmaps. None is a vulnerability and most sit under
dependencies this project cannot move. Turning them into failures would mean a
long ignore list on the first day and would teach everybody to add to it, which
is the habit this is trying to avoid.

When a real advisory does fire, in order of preference:

1. Move the version, if it is a crate this workspace is free to move.
2. Rebase the fork, if the fix is upstream.
3. Add it to `.cargo/audit.toml` under `[advisories] ignore`, with a comment
   saying why it does not reach this code or what has to happen before the
   entry can come out. Never add one without that comment.

An ignored advisory is a decision to accept a risk, so it should read like one.

## Adding a dependency

The bar is the usual one for this repository: it has to earn its place, and
the manifest comment has to say what it is for. Two extra questions for this
workspace in particular.

Does it pull in a second copy of something already here? `cargo tree -d` says.
Two rustls crates, two matrix-sdks or two tokios are all failures that surface
a long way from the manifest.

Does it want its own random number generator, TLS stack or crypto? Those are
process-global choices this workspace has already made, and `install_crypto_provider`
in `consort-matrix` is the reason it is not a runtime panic.
