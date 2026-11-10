# Pinned dependencies

Four things in this workspace are pinned to an exact revision rather than to a
version range, and none of them can be updated by `cargo update`. This is the
written half of that: what each pin is, who owns it, what would move it, and
how to find out how far behind it has fallen.

Neither of the git pins is a personal fork any more. Both were, until the move
described in [PLAN-off-the-pinned-forks.md](PLAN-off-the-pinned-forks.md), and
what that cost and bought is written up there.

The reasoning for each pin lives in the comment beside it in `Cargo.toml`. This
file is the part that does not fit there: the process.

## The pins

### matrix-sdk, upstream at a rev

`matrix-org/matrix-rust-sdk` at `56bc59112822e7eb25943a95e02502855cdeef2d`,
naming `matrix-sdk` and `matrix-sdk-base`.

Upstream itself, pinned to a rev rather than forked: it is the rev
`matrix-rust-rtc` develops against, and the two have to be the same string or
cargo resolves two copies of the SDK and the `Client` this workspace builds
stops being the type the call API accepts.

The previous pin was `BillCarsonFr/matrix-rust-sdk` at `3773300`, a fork commit
that is not an ancestor of upstream main.

**Moves when** the MatrixRTC pin below moves, and never on its own.

**Falls behind** upstream's own main, which is where security fixes to the
crypto stack land first. Being on a rev of upstream rather than a fork of it is
most of what this pin's risk used to be: a bump is now a fast-forward rather
than a rebase. The end state is still a crates.io release, which happens when
`matrix-rust-rtc` asks for one.

**How far behind:**

```sh
git ls-remote https://github.com/matrix-org/matrix-rust-sdk main
```

### Five matrix-rtc crates, upstream at a rev

`element-hq/matrix-rust-rtc` at `4b3cee57adfd5868eb8ebb3731d9011dfbd6d891`,
naming `matrix-rtc-call-sdk`, `matrix-rtc-transport`, `matrix-rtc-core`,
`matrix-rtc-call` and `matrix-rtc-livekit`.

Upstream, and not a fork. The previous pin was `tominal/matrix-rust-rtc` at
`0427fc0`, nine commits on top of `BillCarsonFr/matrix-rust-rtc`, which was
archived read-only on 20 September 2026 with its work moved to element-hq. Six
of those nine commits are upstream in some form, the MSC4143 safety patch among
them, in a better-placed version than ours. Three are not: see
[PLAN-off-the-pinned-forks.md](PLAN-off-the-pinned-forks.md)'s stage 4, which is
now one accessor rather than three commits.

**Moves when** upstream carries a fix this project needs, or when a change here
needs something upstream does not expose yet. A bump is a fast-forward and the
repository takes issues and pull requests, which the archive did not.

**How far behind:**

```sh
git ls-remote https://github.com/element-hq/matrix-rust-rtc main
```

Moving the rev means moving the SDK rev with it, since upstream's own matrix-sdk
pin may have moved too. Check `Cargo.toml` in `element-hq/matrix-rust-rtc`
before touching either, along with its `[patch.crates-io]` table, which this
workspace replicates.

What the move off the forks cost on this side, as a guide to the next bump:
four mechanical changes in `consort-matrix` for three months of SDK drift, and
in `consort-call` the crate renames plus thirteen named compile errors. Every
one was named by the compiler except two, and those two are the ones to watch
for: the `tracing` filter in `app/src-tauri/src/lib.rs` names crates as strings,
so a rename there parses perfectly and logs nothing, and `Participant` lost a
field that only test fixtures read.

### Nine livekit and webrtc crates, pinned in Cargo.lock

`livekit`, `livekit-api`, `livekit-datatrack`, `livekit-protocol`,
`livekit-runtime`, `livekit-signaling`, `libwebrtc`, `webrtc-sys`,
`webrtc-sys-build`.

These publish semver-compatible releases that do not compile with each other,
so they are held at the versions `matrix-rust-rtc`'s own lockfile carries. A
bare `cargo update` breaks the build with a type error inside a dependency
rather than anything naming a version.

`livekit-signaling` joined the list on the move to element-hq, and is the
current example of the whole problem: a fresh resolve picks 0.1.9, and `livekit`
0.9.7 does not compile against it because its `SignalOptions` has no
`use_legacy_data_streams` field.

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

Five `cargo audit` advisories are ignored, in `.cargo/audit.toml`. The file
carries the argument for each; this is the index.

It was seven until the move off the pinned forks, and the two that came out are
the point of having made the move: `RUSTSEC-2026-0292` (`imbl-sized-chunks`)
and `RUSTSEC-2026-0451` (`vodozemac`), the second of which was the only
reachable advisory on the list. Measured rather than reasoned about: eleven
vulnerability rows against the old lockfile with the ignore file set aside,
five against this one.

| Advisory | Crate | Why it is ignored | What takes it out |
|---|---|---|---|
| RUSTSEC-2026-0459 | `libsqlite3-sys` 0.38.2 | Uninitialized memory out of the bundled C SQLite, for a process that opens a crafted database and `-wal` pair. Consort opens one database, the store it made itself under the app data directory, and takes a path for one from nowhere else. | A matrix-sdk pin whose matrix-sdk-sqlite is on rusqlite 0.41 or newer. The pin moving was not enough and this row used to say it would be: the move took the crate from 0.35.0 to 0.38.2 and the advisory came along. The patched bundle is 0.39.0, which arrives with rusqlite 0.41, and `56bc591` asks for 0.40.2. |
| RUSTSEC-2026-0367, 0368, 0369, 0429 | `zip` 4.6.1 | Four extraction flaws, at one call site that extracts no archive anybody else chose: `tauri-plugin-updater`, compiled only under `self-update` and extracting signature-verified bytes in `#[cfg(windows)]` code. `webrtc-sys-build` was the other call site until the move off the forks took it to 0.3.21, which uses the patched `zip` 9.5.0. | tauri-plugin-updater 2.14.1 with the wry 0.55 to 0.57 bump under it, as its own change. That is the second of the two moves this row used to describe; the first is done. |

Both of those are the third option below, reached because the first two are
closed: cargo refuses the version bump outright, and the fix is in a crate one
of the pins holds rather than in anything this workspace is free to move.

`cargo audit` also reports crates that are unmaintained, unsound or yanked, and
those do not fail the build. Fourteen of them do not: the unic and
proc-macro-error families, zbus's zvariant, two yanked crates, and unsoundness
in glib and tokio. None is a vulnerability and most sit under
dependencies this project cannot move. Turning them into failures would mean a
long ignore list on the first day and would teach everybody to add to it, which
is the habit this is trying to avoid.

When a real advisory does fire, in order of preference:

1. Move the version, if it is a crate this workspace is free to move.
2. Move the pin, if the fix is in a rev of `matrix-rust-sdk` or
   `matrix-rust-rtc` newer than the one pinned. Both together: see the pins
   above.
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
