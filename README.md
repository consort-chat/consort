# Consort

A desktop chat client for [Matrix](https://matrix.org), built in Rust and Tauri.

Consort is aiming at the shape of a voice-first team chat app: persistent voice
channels you drop into, a room list you live in all day, and a push-to-talk
alternative that actually works. The protocol underneath is Matrix, all the way
down. There is no proprietary backend, and there is no Consort server. Point it
at whatever homeserver you already run.

> **Status: early.** Consort signs you in, remembers you, verifies itself,
> draws your rooms, joins voice channels, reads and sends text in a room,
> reads attachments, and reads and writes threads. Edits, reactions and
> sending an attachment are not built, and the README will stop saying this
> when they land.

---

## What works today

- Password login against any homeserver, entered as a plain server name
  (`example.org`) with `.well-known` discovery handled for you.
- Sessions persist across restarts, so you log in once. The access token goes
  in your system keyring.
- Cross-signing is bootstrapped on first login, which the voice layer will
  require later (MSC4153 only accepts media keys from cross-signed devices).
- The signed-in screen says whether the sync loop is connected, and says so
  about verification only while there is something to say: a session that is
  not verified, or one nothing has looked at yet, gets a banner, and a verified
  one gets no words at all.
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
- Key backup. The first session on an account creates one, both routes to
  verification unlock it, and a message sent before this session existed can be
  read on it afterwards. When an account has no backup at all the screen says
  so, because that is the case where losing this machine loses the messages.
- The room list: a rail entry per joined space, a Home entry for everything
  belonging to no joined space, and the channels under each of them split into
  text and voice. A room a space lists and this account never joined is shown
  as unavailable rather than hidden, and named by asking the space, because
  disagreeing with every other client about how many channels there are is
  worse than one request.
- Who is already in a voice channel, drawn underneath it, without joining the
  call or clicking the channel. Somebody connecting from Element Call in a
  browser appears within a sync and disappears when they hang up, and the same
  person on two devices appears once.
- Reading and sending text in a room. Consecutive messages from one person are
  drawn as one, emotes and notices are drawn as themselves, and a message this
  session has no key for is drawn as one still waiting for its key rather than
  left out: a gap that says nothing about itself cannot be told apart from a
  quiet room. Scrolling back asks the homeserver for more.
- Markdown, both ways. What you type is read as markdown and sent with the
  formatting beside the plain text, so it arrives formatted in Element too, and
  what arrives formatted is drawn that way here. The HTML is never handed to
  the browser's parser: it is rebuilt from an allow-list of elements, so a
  message cannot bring markup of its own into the window. Links are drawn but
  do not open yet.
- A card about whoever said something, from their name or their face in the
  timeline, and the same card a name in a voice channel opens. A dot on the
  face says whether they are here, when the homeserver will say; most have
  presence switched off, and no dot is drawn rather than a grey one.
- Attachments. A picture is drawn in the room as soon as the room is and opens
  full size when you press it; a clip waits behind its own thumbnail, because
  scrolling back through a room of them should not download every one. Both
  come from a scheme served by Rust that answers range requests, so a clip can
  be seeked and neither is ever held in the window as a string. A file or a
  voice note is a card that opens your desktop's Save As window. Whatever words
  were sent with any of them are drawn underneath. Encrypted rooms work the
  same way: the file is decrypted here.
- Threads. A message somebody has replied to says how many replies it has, and
  pressing the message opens them in a panel down the side of the window,
  where you can read the thread and add to it. The count is the homeserver's
  own tally and rides on the message, so a room knows which of its messages are
  threads while it is being drawn rather than by asking about each one. Thread
  replies stay out of the room's own timeline, which is where every other
  client puts them and why a room does not read as two conversations at once.
- Replies, drawn as replies. A message that answers another says whose message
  and one line of what it said, and pressing that jumps to it and lights it up.
  The quote the sender writes for clients that draw no reply of their own is
  taken off, so nothing is shown twice.
- Mentions. Somebody's name in a message is drawn as a name rather than as a
  link that goes nowhere, with the `@` the sender left off put back, and a
  message that names you gets a gold band down its side.
- Text in a voice channel, because a voice channel is an ordinary Matrix room
  that happens to carry a call. The same timeline is there, beside the call.
- Signing out clears the session locally and on the server.

## What does not work yet

Edits, reactions, redactions, sending an attachment, playing a voice note, read
receipts and typing notifications. A reply to a message older than what is
loaded says so rather than fetching it. A thread longer
than fifty replies shows its recent end and says so rather than loading the
rest. Links
in a message are drawn but go nowhere, because opening one outside the window
needs a Tauri plugin this build does not grant. A message sent from here
appears when the sync brings it back rather than immediately, because there is
no local echo yet.

Playing a clip needs codecs this application does not ship. Consort renders
through WebKitGTK, which decodes through GStreamer, so an mp4 needs an H.264
and an AAC decoder to be installed on the machine. Where they are missing the
clip says so and offers to save itself instead of showing you a garbled
picture. On Arch that is `gst-libav`, `gst-plugins-ugly` and `gst-plugins-bad`;
on Debian and Fedora the equivalent `gstreamer1.0-libav` packages.
See [the roadmap](#roadmap).

---

## Installing

Every tag carries three builds on its
[release page](https://github.com/consort-chat/consort/releases), each made by
CI on a clean runner from the tagged commit: a Windows installer, a `.deb`, and
an Arch package. Fedora still builds from source. Nothing is signed and no
repository serves any of it, so every install below is a file you fetched
yourself. See [Known limitations](#known-limitations) before you put this on a
machine you care about.

### Windows

`Consort_<version>_x64-setup.exe`.

It is not signed, because a code-signing certificate costs money this project
does not have. SmartScreen will therefore say "Windows protected your PC" the
first time it runs, and getting past it means pressing "More info" and then
"Run anyway". That warning is about the absence of a signature and not about
anything the installer does, and the only way to tell those apart is to check
the download against the SHA-256 GitHub prints beside the asset.

### Arch Linux

`consort-<version>-1-x86_64.pkg.tar.zst`, built by `makepkg` from
[`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD) at the tag:

```sh
sudo pacman -U ./consort-<version>-1-x86_64.pkg.tar.zst
```

pacman will say the package is not signed and ask whether to install it
anyway. Nothing here has a key yet, so that is the expected answer rather than
a sign something went wrong.

To build the same package yourself instead, run `makepkg -si` in
`packaging/arch`. To track `main` rather than the last release, a second
PKGBUILD in [`packaging/aur/`](packaging/aur/) builds the latest commit.
Neither is on the AUR yet, so build in place:

```sh
git clone https://github.com/consort-chat/consort.git
cd consort/packaging/aur
makepkg -si
```

Either way you get the `consort` binary in `/usr/bin`, a desktop entry, and
icons in the hicolor theme. The two packages declare each other as conflicts,
so pacman will not let both be installed.

If `makepkg` stops with a missing `pnpm` dependency even though `pnpm --version`
works, your pnpm comes from corepack rather than the `pnpm` package. makepkg
checks installed packages, not `$PATH`. Either `pacman -S pnpm`, or pass `-d` to
skip the check.

If `pacman -S pnpm` then stops with `/usr/bin/pnpm exists in filesystem`, those
are corepack's symlinks and no package owns them, so pacman will not write over
them. Remove them first:

```sh
sudo rm /usr/bin/pnpm /usr/bin/pnpx
sudo pacman -S pnpm
```

Passing `--overwrite` to that command does not help on its own, because
`makepkg -si` runs its own `pacman -S` for the build dependencies without your
flags.

Arch is the distro this is developed on, so it is the one most likely to work.

### Debian and Ubuntu

`Consort_<version>_amd64.deb`.

```sh
sudo apt install ./Consort_<version>_amd64.deb
```

It is built on Debian 12, which is the oldest thing it will run on, and that is
deliberate: a package built on the current Ubuntu links a newer glibc, installs
happily on Debian 12, and then refuses to start. So this one covers Debian 12
and 13 and Ubuntu 24.04 onwards from one file.

Ubuntu 22.04 is out of reach. It has glibc 2.35 and only webkit2gtk-4.0, and
Tauri 2 needs 4.1.

### Fedora

Not built by CI, because nobody has said where an `.rpm` would be tested.
`pnpm tauri build` writes one locally alongside the `.deb`:

```sh
sudo dnf install ./target/release/bundle/rpm/Consort-0.2.0-1.x86_64.rpm
```

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
`.deb`, which is built that way for the same reason, or the Arch package.

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

macOS needs Xcode command line tools. Windows needs more than one line's worth,
including one prerequisite that reports a successful install while installing
nothing, so it has [a section of its own](#windows).

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

### Windows

Windows builds and the result runs. It needs more than Linux does, one of the
prerequisites reports a successful install while installing nothing, and a
release build has no console to tell you about any of it. Everything below is
PowerShell.

**The toolchains.**

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup
winget install --id OpenJS.NodeJS.LTS
winget install --id pnpm.pnpm
```

rustup targets `x86_64-pc-windows-msvc` here, so the MSVC build tools are not
optional: without them the first link fails with `linker 'link.exe' not found`.
WebView2 needs nothing, because Windows 11 ships it. pnpm is installed directly
rather than through `corepack enable`, because newer Node lines have dropped
corepack; on the LTS either route works.

Check that each one answers before starting a long build, because on this
platform a success message is not evidence:

```powershell
rustup show          # the host triple should read x86_64-pc-windows-msvc
cargo --version
node --version
pnpm --version
```

**NASM, from the zip rather than from winget.** matrix-sdk's crypto backend here
is `rustls-aws-lc-rs`, and `aws-lc-sys` assembles its x86-64 code with NASM.

`winget install --id NASM.NASM` is not the route. It reports a successful
install and leaves nothing on `PATH`, nothing in either Program Files, and
nothing `winget list` will admit to afterwards. NASM ships as a plain zip with
`nasm.exe` in it, so skip the installer:

```powershell
$ProgressPreference = 'SilentlyContinue'
$ver = '2.16.03'
Invoke-WebRequest -Uri "https://www.nasm.us/pub/nasm/releasebuilds/$ver/win64/nasm-$ver-win64.zip" -OutFile "$env:TEMP\nasm.zip"
Expand-Archive -Path "$env:TEMP\nasm.zip" -DestinationPath 'C:\Tools' -Force
$nasm = "C:\Tools\nasm-$ver"
[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ";$nasm", 'User')
$env:Path += ";$nasm"
nasm -v
```

If that URL 404s, the release listing is at
<https://www.nasm.us/pub/nasm/releasebuilds/>: take the newest version's `win64`
zip and adjust `$ver`. Use `SetEnvironmentVariable` rather than `setx PATH`.
`setx` truncates the value at 1024 characters and will quietly eat entries you
wanted to keep.

There is a supported way out if NASM keeps fighting you, because aws-lc-sys
ships 26 prebuilt object files for exactly this case:

```powershell
[Environment]::SetEnvironmentVariable('AWS_LC_SYS_PREBUILT_NASM', '1', 'User')
```

It is a fallback rather than an override. The gate in that build script fires
only on Windows x86-64, with assembly enabled, and only when `nasm` is absent
from `PATH`, so setting it alongside a working NASM changes nothing.

**CMake is not needed,** which is worth writing down because everything about
this dependency suggests it should be and the Linux CI container does install
one. aws-lc-sys ships pregenerated bindings for `x86_64-pc-windows-msvc`, so it
takes its `cc` builder rather than its CMake builder and never invokes CMake on
this target. Install one anyway if you like; its 4.x policy changes will not
reach you here.

**Disk is the real constraint.** A release-only `target` runs to around 20 GB,
most of it LiveKit's prebuilt libwebrtc, and a debug profile is a second full
copy of the same thing. A 64 GB machine with Windows already on it fits one of
those and not both, so stay off `pnpm tauri dev` on that checkout unless you
have the room. Cargo does not collect the old copies either: libwebrtc unpacks
under `target\release\build\scratch-<hash>\out`, every change to rustflags
rehashes that directory, and what was there before stays there. Deleting
`target` outright is the recovery, at the cost of fetching the libwebrtc zip
again.

**Build outward,** rather than straight at the app, so that a failure names the
layer it came from:

```powershell
cargo build -p consort-matrix
cargo build -p consort-audio
cargo build -p consort-call
```

`consort-matrix` is the aws-lc-sys and SQLite test, `consort-audio` is cpal
against WASAPI, and `consort-call` is the long one because it pulls libwebrtc.
Finding out about a libwebrtc problem after forty minutes of compiling
matrix-sdk is avoidable.

Then the app:

```powershell
cd app
pnpm install
pnpm tauri build --no-bundle
```

`--no-bundle` is not optional. `bundle.targets` in `tauri.conf.json` is `["deb",
"rpm"]`, and neither of those exists on Windows. If you want an installer,
`pnpm tauri build --bundles nsis` produces one without touching the committed
config, and the Tauri CLI fetches NSIS itself.

The binary is `target\release\consort.exe`: at the workspace root rather than
under `app\src-tauri`, and named `consort` rather than `consort-app`.

#### Getting a log out of it

A release build has no console, so a startup problem reads as silent refusal
rather than as an error. `main.rs` sets `windows_subsystem = "windows"` for
anything that is not a debug build, and every log line is then written to a
handle that does not exist. Redirecting at process creation hands the child real
ones:

```powershell
$exe = Resolve-Path .\target\release\consort.exe
Start-Process -FilePath $exe `
  -RedirectStandardOutput "$HOME\consort-out.log" `
  -RedirectStandardError "$HOME\consort-err.log"
```

Run that from the workspace root, and give `Start-Process` absolute paths as
above. It does not inherit PowerShell's idea of the current directory, so a
relative one is resolved against somewhere else and the redirect either lands
where you are not looking or fails outright.

The log lines land in the stdout file. Watch it live from a second pane with
`Get-Content -Wait "$HOME\consort-out.log"`.

`RUST_LOG` set in the shell beforehand reaches the child. If you set one, start
it with a bare level:

```powershell
$env:RUST_LOG = "info,matrix_sdk=warn"
```

A filter that names only targets turns every crate it forgot to `OFF` rather
than leaving it where it was, which is how a log taken to find out why a call
would not connect ends up containing nothing about the call. The default filter
this build ships opens with a bare level for that reason.

Its data directory, `settings.json` included, is
`$env:APPDATA\chat.consort.desktop`.

### Two accounts at once

There is normally one Consort per machine: two copies would share one SQLite
crypto store and one session file, and racing on a crypto store is how device
keys get lost. Testing a call needs two accounts, though, so a named profile
gets its own data directory and is allowed to run beside the ordinary one.

```sh
CONSORT_PROFILE=second pnpm tauri dev
```

Its data lives under `profiles/second/` inside the usual application data
directory, and it signs in separately. Give each one a different name. Two
processes on the *same* profile would fight over one store, which is the thing
the single-instance guard exists to prevent.

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
cargo test --workspace          # 918 tests
cd app && pnpm test             # 365 tests
cd app && pnpm test:coverage    # thresholds enforced from vitest.config.ts
```

Twenty-seven Rust tests are marked `#[ignore]` because they need something a CI
container does not have. Four want a live platform keyring:

```sh
cargo test -p consort-matrix -- --ignored keyring
```

Three want a real sound card, and are how the cpal layer is checked against
hardware rather than against a fake:

```sh
cargo test -p consort-audio -- --ignored
```

The other twenty want a homeserver, because a SAS verification handshake is
real cryptography between two real devices and no mock produces one. They drive both
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

WebKitGTK composites through a DMA-BUF buffer it allocates with GBM, and
NVIDIA's driver refuses the allocation, so the window paints nothing while the
process keeps running happily. The giveaway is `Failed to create GBM buffer` on
stderr.

Consort detects that case at startup and takes the older rendering path, so
there should be nothing to do. If some other driver has the same problem:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 consort
```

Setting it explicitly always wins, in either direction, so
`WEBKIT_DISABLE_DMABUF_RENDERER=0` is how to keep the fast path on a machine
where it works. Please open an issue either way, so the detection can learn
about it.

---

## How it is put together

```
crates/consort-matrix/    Matrix authentication, session persistence, sync,
                          verification state. No Tauri, no UI. This is the
                          testable half.
crates/consort-audio/     Devices, capture, playback, the voice gate and the
                          level meter. Knows nothing about Matrix.
crates/consort-call/      Being in a MatrixRTC call. Separate from
                          consort-matrix because it brings libwebrtc, and that
                          should not be in the way of every `cargo test -p
                          consort-matrix`.
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

<img width="1365" height="832" alt="image" src="https://github.com/user-attachments/assets/e809d5d2-6dc6-43bf-9b93-f054351215fa" />

| Milestone | State |
|---|---|
| Password login and session persistence | working |
| Session verification by emoji, in either direction | working |
| Verifying with a recovery key | working |
| Key backup, so history older than this session decrypts | working |
| Room list and voice channel discovery | working |
| Seeing who is already in a voice channel | working |
| Join a voice channel over MatrixRTC and LiveKit | working |
| Audio device pickers, a level meter and an output test | working |
| RNNoise voice activity detection with hysteresis gating | working |
| Reading and sending text in a room | working |
| Reading attachments, and saving them anywhere | working |
| Reading and replying in threads | working |
| Edits, reactions and sending attachments | planned |
| Signed and notarised builds for Windows and macOS | someday |

"Working" is doing some work in that first row, and it is not a synonym for
done. Signing in gets you an authenticated session, and that session is
unverified: it cannot decrypt encrypted history, and no encrypted call will
accept it. Authentication is not finished until the row under it is, because a
session you cannot verify is an account rather than a usable client.

It is doing some work in the text row too. A room's messages are drawn, in
order, with names and avatars, and typing into the box sends one. What is not
there is everything a conversation grows once it is busy: threads, edits,
reactions, sending an attachment, and a reply drawn as a reply rather than as
quoted text.

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
- **Nothing has asked the key backup for a key on its own yet.** The setting
  that does it fetches the one key a message needs at the moment that message
  fails to decrypt, and there is no timeline for a message to fail in. The keys
  are provably readable, the trigger is the SDK's, and neither has been watched
  working together. The room list is where that gets checked.

## Licence

[GNU Affero General Public License, version 3](LICENSE), and only version 3.

This is not a preference, it is inherited. Consort links
[`matrix-rust-rtc`](https://github.com/BillCarsonFr/matrix-rust-rtc), which is
AGPL-3.0-only, so any work derived from it carries the same terms. If you run a
modified Consort as a network service, the AGPL requires you to offer that
modified source to its users.

Copyright the Consort contributors.
