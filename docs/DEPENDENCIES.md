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

`tominal/matrix-rust-rtc` at `934cdbad34bda1a8a170faa022dcdd5436f4072a`.

A fork of `BillCarsonFr/matrix-rust-rtc`, branched at
`7d9944fd6b02cbd09fc9cceff843c55ae2a8d4d8` and carrying eight commits on top of
it, on `feature/plain-rooms-and-arrival-mute`. The oldest and most important is
the one that stops media frames being encrypted in an unencrypted room, which
MSC4143 forbids and which leaves a conforming peer decoding ciphertext as
audio.

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
`crates/matrix-rtc-livekit/Cargo.toml` in the fork before touching either.

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

Both are clean as of the day this was written, and no advisory is being ignored.

`cargo audit` also reports crates that are unmaintained, unsound or yanked, and
those do not fail the build. Twenty-two of them do not: almost all are the GTK3
bindings Tauri depends on, which this project cannot move and which say nothing
about a vulnerability. Turning them into failures would mean a long ignore list
on the first day and would teach everybody to add to it, which is the habit
this is trying to avoid.

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
