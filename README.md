# Consort

A desktop chat client for [Matrix](https://matrix.org), built in Rust and Tauri.

Consort is aiming at the shape of a voice-first team chat app: persistent voice
channels you drop into, a room list you live in all day, and a push-to-talk
alternative that actually works. The protocol underneath is Matrix, all the way
down. There is no proprietary backend, and there is no Consort server. Point it
at whatever homeserver you already run.

> **Status: early.** Consort signs you in, remembers you, and can verify itself
> by emoji in either direction: ask from here, or answer a request from another
> client. No rooms, no messages, no voice yet. Recovery-key verification and key
> backup are next, then voice, and the README will stop saying this when they
> land.

---

## What works today

- Password login against any homeserver, entered as a plain server name
  (`example.org`) with `.well-known` discovery handled for you.
- Sessions persist across restarts, so you log in once. The access token goes
  in your system keyring.
- Cross-signing is bootstrapped on first login, which the voice layer will
  require later (MSC4153 only accepts media keys from cross-signed devices).
- The signed-in screen says whether the sync loop is connected and whether this
  session is verified, and says "checking" rather than guessing while it does
  not know.
- Emoji verification, in both directions. Press "Verify this session" here, or
  start one from Element or another Consort on the same account. Either way you
  compare the seven pictures and this session becomes verified, which is what
  MSC4153 calls will require of it. Declining, a mismatch and an expiry are
  each reported as themselves rather than as one generic failure. With no other
  session signed in the button is not offered, because it could only time out.
- Verifying with a recovery key, for when the other device is in the next room
  or does not exist. A passphrase works too if the account has one. The four
  ways it can fail get four different answers, because "that did not work" is a
  bad reply to a key that is fine and simply belongs to another account.
- Signing out clears the session locally and on the server.

## What does not work yet

Restoring key backup, so history that predates this session still will not
decrypt even once the session is verified. No room list, no messages, no voice.
See [the roadmap](#roadmap).

---

## Installing

Nothing is released yet. There are no signed binaries and no package
repository, so every route below builds from source. See
[Known limitations](#known-limitations) before you install this on a machine
you care about.

### Arch Linux

A PKGBUILD lives in [`packaging/aur/`](packaging/aur/). It is not on the AUR
yet, so build it in place:

```sh
git clone https://github.com/consort-chat/consort.git
cd consort/packaging/aur
makepkg -si
```

That installs a `consort-git` package: the `consort` binary in `/usr/bin`, a
desktop entry, and icons in the hicolor theme. When the package is published,
`yay -S consort-git` will do the same thing.

If `makepkg` stops with a missing `pnpm` dependency even though `pnpm --version`
works, your pnpm comes from corepack rather than the `pnpm` package. makepkg
checks installed packages, not `$PATH`. Either `pacman -S pnpm`, or pass `-d` to
skip the check.

Arch is the distro this is developed on, so it is the one most likely to work.

### Debian, Ubuntu, Fedora

`pnpm tauri build` writes a `.deb` and an `.rpm` to `target/release/bundle/`:

```sh
sudo apt install ./target/release/bundle/deb/Consort_0.1.0_amd64.deb
sudo dnf install ./target/release/bundle/rpm/Consort-0.1.0-1.x86_64.rpm
```

Both are built and installed locally. Neither is signed, and neither is served
from anywhere.

### AppImage

There isn't one, on purpose. Two separate reasons:

- linuxdeploy carries its own ancient binutils `strip`, which cannot parse the
  `.relr.dyn` relocation section current Arch libraries use. The bundler dies
  on `libzstd` before it reaches anything of ours.
- Even with that worked around, an AppImage built here still links the host
  glibc, so it would refuse to start on an older distro. That is the exact
  problem AppImage is supposed to solve, so shipping a broken one is worse
  than shipping none.

It has to be built in CI on an old base image. Until that exists, use the
`.deb`, the `.rpm`, or the PKGBUILD.

## Building from source

### Prerequisites

**Rust.** The toolchain is pinned in `rust-toolchain.toml`, so
[rustup](https://rustup.rs) will fetch the right version on the first build.
Nothing to do beyond having rustup installed.

A distro rust package works too, as long as it is 1.85 or newer. The pin only
exists to guarantee an edition 2024 compiler. Note that `rust-toolchain.toml`
is a rustup mechanism: a distro toolchain ignores it, and the AUR PKGBUILD
deletes it during `prepare()` so that a builder who does have rustup is not
sent to the network mid-build.

**Node and pnpm.** Node 20 or newer, and pnpm:

```sh
corepack enable pnpm
```

**System libraries.** Tauri needs a webview. On Linux:

```sh
# Arch
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl \
               appmenu-gtk-module libappindicator-gtk3 librsvg

# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
                 libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

macOS needs Xcode command line tools. Windows needs the MSVC C++ build tools and
WebView2, which ships with Windows 11.

### Build and run

```sh
git clone https://github.com/consort-chat/consort.git
cd consort/app
pnpm install
pnpm tauri dev      # development, with hot reload on the frontend
pnpm tauri build    # .deb and .rpm under target/release/bundle
```

`target/` is at the workspace root, not under `app/src-tauri`, because the
Rust crates and the Tauri app share one cargo workspace.

The first build compiles the Matrix SDK from source and takes a while. Later
builds are incremental and fast.

### Rust only

The Matrix layer has no dependency on Tauri and builds on its own:

```sh
cargo test -p consort-matrix
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

### Tests

Both halves have to stay above 90% line coverage, and CI enforces it.

```sh
cargo test --workspace          # 281 tests
cd app && pnpm test             # 117 tests
cd app && pnpm test:coverage    # thresholds enforced from vitest.config.ts
```

Twelve Rust tests are marked `#[ignore]` because they need something a CI
container does not have. Four want a live platform keyring:

```sh
cargo test -p consort-matrix -- --ignored keyring
```

The other ten want a homeserver, because a SAS verification handshake is real
cryptography between two real devices and no mock produces one. They drive both
sides of the emoji exchange unattended, in both directions, so asking,
accepting, confirming, declining and reporting a mismatch are all exercised end
to end. There is a throwaway Synapse for that:

```sh
testing/synapse/up.sh
export CONSORT_TEST_HOMESERVER=http://localhost:8008
cargo test --workspace -- --ignored
testing/synapse/down.sh
```

See [testing/synapse/README.md](testing/synapse/README.md) for what it is and,
more importantly, what it is not.

[COVERAGE.md](COVERAGE.md) explains what is measured, what is excluded, and
why.

### If the window comes up blank on Linux

WebKitGTK's DMA-BUF renderer does not get along with every driver, and when it
fails the window paints nothing while the process keeps running happily. The
giveaway is `Failed to create GBM buffer` on stderr.

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 pnpm tauri dev
```

If that fixes it, put it in your shell profile. It is a driver problem, not a
Consort one, and there is nothing to fix on this side.

---

## How it is put together

```
crates/consort-matrix/    Matrix authentication, session persistence, sync,
                          verification state. No Tauri, no UI. This is the
                          testable half.
app/src-tauri/            The Tauri shell. Commands, state, events, wiring.
app/src/                  React + TypeScript frontend.
testing/synapse/          A homeserver to throw away, for the tests that
                          cannot use a mock.
```

The split is deliberate and worth keeping. Anything that can live in
`consort-matrix` should, because that is the code you can exercise with
`cargo test` instead of by clicking through a window.

### Two things that will bite you

**The Matrix SDK pin.** `Cargo.toml` pins `matrix-sdk` to a specific git rev of
a fork, not a crates.io release. That rev has to stay in lockstep with the one
[`matrix-rust-rtc`](https://github.com/BillCarsonFr/matrix-rust-rtc) pins,
because the voice layer takes a `matrix_sdk::Client` and cargo will happily
resolve two incompatible copies of the SDK if the revs drift. The symptom is a
page of `expected Client, found Client`. Bump both together or neither.

**The `LocalSet` constraint.** When voice lands, `Call::join` will not be
callable from a Tauri command. It drives `!Send` futures through `spawn_local`
and panics outside a `tokio::task::LocalSet`, while Tauri commands run on a
multi-threaded runtime and require `Send`. The call has to live on its own
thread with a current-thread runtime, reached over a channel. This is written
down in `app/src-tauri/src/state.rs` next to the state it constrains.

---

## AI-assisted development

**Use AI. Read what it wrote before you send it.**

Consort is built with AI assistance and contributions written with AI
assistance are welcome. There is no disclosure ritual and no scarlet letter for
using a model. Moving fast is the point.

What is not welcome is unread output.

The cost of generated code does not disappear, it moves. When you skip the part
where you read, test, and cut your own patch down, you have not saved effort,
you have transferred it to whoever reviews it. Review is the scarce resource on
this project. A 900-line diff that a human has not read is not a contribution,
it is a request for someone else to do the hard part.

**Before you open a pull request, you should be able to say yes to all of these:**

- I have read every line I am submitting and can explain why it is there.
- I ran it. It actually does the thing.
- I removed the parts that were generated but not needed, including invented
  abstractions, defensive branches for conditions that cannot occur, and
  comments restating what the line above already says.
- The tests test behaviour, not the implementation echoed back.
- If a reviewer asks "why this approach," I have an answer that is not
  "that is what it gave me."

**What gets a change sent back:**

- Sweeping refactors bundled into an unrelated fix.
- Comments narrating the obvious (`// increment the counter`).
- Error handling that catches everything and does nothing.
- Tests asserting that a mock was called.
- Docs describing behaviour the code does not have.
- Any pull request the author cannot discuss.

**If it keeps happening,** we will ask you to submit smaller changes, and if it
still keeps happening, we will stop accepting pull requests from you. That is
not a threat about using AI. It is the same standard we would apply to someone
pasting code from anywhere else without reading it. The tool is fine. Not
reading is the problem.

Small, focused, explained pull requests get reviewed quickly. That is true here
whether a model wrote them or you did.

---

## Roadmap

| Milestone | State |
|---|---|
| Password login and session persistence | working |
| Session verification by emoji, in either direction | working |
| Verifying with a recovery key | working |
| Key backup, so history older than this session decrypts | in progress, [planned here](docs/PLAN-verification.md) |
| Room list and voice channel discovery | planned |
| Join a voice channel over MatrixRTC and LiveKit | planned |
| RNNoise voice activity detection with hysteresis gating | prototyped separately |
| Text messaging | planned |
| Signed and notarised builds for Windows and macOS | someday |

"Working" is doing some work in that first row, and it is not a synonym for
done. Signing in gets you an authenticated session, and that session is
unverified: it cannot decrypt encrypted history, and no encrypted call will
accept it. Authentication is not finished until the row under it is, because a
session you cannot verify is an account rather than a usable client.

## Known limitations

- **On a machine with no keyring, the access token falls back to a file.** The
  default is the platform credential store: Secret Service on Linux and the
  BSDs, the Credential Manager on Windows, Keychain on macOS. Secret Service is
  a DBus service rather than a kernel feature, though, and a bare window
  manager, a container or an SSH session may not have one running. Refusing to
  start there would be worse, so Consort falls back to a file in the per-user
  application data directory with `0600` permissions, and says so on the
  signed-in screen rather than letting you assume otherwise. See
  `crates/consort-matrix/src/secrets/`.
- **One Consort at a time.** A second launch focuses the first window and
  exits. Two processes would share one SQLite crypto store, and racing on that
  is how device keys get lost.
- **Builds are unsigned.** Windows SmartScreen and macOS Gatekeeper will warn
  about them. Linux packages are unaffected.
- **Signing out leaves the encryption store on disk.** The session and the
  access token go, but the per-account SQLite store holding this device's room
  keys stays until the next sign-in on that account removes it. Those keys
  belong to a device the server has already destroyed, so nothing can be done
  with them, but they are decrypted room keys sitting in your data directory
  after you asked to be signed out. Removing them at sign-out is the right end
  state and is not done yet, because the removal has to happen after the client
  is dropped and that is shutdown-ordering work.
- **Password login only.** No SSO or OIDC yet.
- **A verified session still cannot read history that predates it.** Both
  routes to verification work, and neither restores the server-side key backup
  yet, so messages sent before you signed in here stay unreadable. That is the
  [next step of that milestone](docs/PLAN-verification.md).

## Licence

[GNU Affero General Public License, version 3](LICENSE), and only version 3.

This is not a preference, it is inherited. Consort links
[`matrix-rust-rtc`](https://github.com/BillCarsonFr/matrix-rust-rtc), which is
AGPL-3.0-only, so any work derived from it carries the same terms. If you run a
modified Consort as a network service, the AGPL requires you to offer that
modified source to its users.

Copyright the Consort contributors.
