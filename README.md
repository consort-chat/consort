# Consort

A desktop chat client for [Matrix](https://matrix.org), built in Rust and Tauri.

Consort is aiming at the shape of a voice-first team chat app: persistent voice
channels you drop into, a room list you live in all day, and a push-to-talk
alternative that actually works. The protocol underneath is Matrix, all the way
down. There is no proprietary backend, and there is no Consort server. Point it
at whatever homeserver you already run.

> **Status: early.** Right now Consort signs you in and remembers you. That is
> the whole feature set. Voice is next, and the README will stop saying this when
> it lands.

---

## What works today

- Password login against any homeserver, entered as a plain server name
  (`example.org`) with `.well-known` discovery handled for you.
- Sessions persist across restarts, so you log in once.
- Cross-signing is bootstrapped on first login, which the voice layer will
  require later (MSC4153 only accepts media keys from cross-signed devices).
- Signing out clears the session locally and on the server.

## What does not work yet

Everything else. No room list, no messages, no voice. See
[the roadmap](#roadmap).

---

## Building from source

### Prerequisites

**Rust.** The toolchain is pinned in `rust-toolchain.toml`, so
[rustup](https://rustup.rs) will fetch the right version on the first build.
Nothing to do beyond having rustup installed.

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
pnpm tauri build    # release bundle in app/src-tauri/target/release/bundle
```

The first build compiles the Matrix SDK from source and takes a while. Later
builds are incremental and fast.

### Rust only

The Matrix layer has no dependency on Tauri and builds on its own:

```sh
cargo test -p consort-matrix
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

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
crates/consort-matrix/    Matrix authentication and session persistence.
                          No Tauri, no UI. This is the testable half.
app/src-tauri/            The Tauri shell. Commands, state, wiring.
app/src/                  React + TypeScript frontend.
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
| Authentication and session persistence | done |
| Room list and voice channel discovery | next |
| Join a voice channel over MatrixRTC and LiveKit | planned |
| RNNoise voice activity detection with hysteresis gating | prototyped separately |
| Text messaging | planned |
| Signed and notarised builds for Windows and macOS | someday |

## Known limitations

- **The access token is stored in a file, not the OS keyring.** It lives in the
  per-user application data directory with `0600` permissions on Unix, which
  matches what the SDK's own SQLite stores get. The keyring is the better
  answer and it is on the list. It is not the default yet because the Linux
  keyring backends are not reliably running, and a first launch that dies on a
  missing keyring daemon is a worse failure than this one. See
  `crates/consort-matrix/src/session.rs`.
- **Builds are unsigned.** Windows SmartScreen and macOS Gatekeeper will warn
  about them. Linux packages are unaffected.
- **Password login only.** No SSO or OIDC yet.

## Licence

[GNU Affero General Public License, version 3](LICENSE), and only version 3.

This is not a preference, it is inherited. Consort links
[`matrix-rust-rtc`](https://github.com/BillCarsonFr/matrix-rust-rtc), which is
AGPL-3.0-only, so any work derived from it carries the same terms. If you run a
modified Consort as a network service, the AGPL requires you to offer that
modified source to its users.

Copyright the Consort contributors.
