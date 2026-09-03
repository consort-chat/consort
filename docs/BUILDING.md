# Building Consort

Everything needed to compile it, and everything that goes wrong while doing
so. The [README](../README.md) covers installing a build somebody else made,
which is what most people want.

## Prerequisites

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
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl alsa-lib \
               appmenu-gtk-module libappindicator-gtk3 librsvg

# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
                 libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
                 libasound2-dev
```

macOS needs Xcode command line tools. Windows needs more than one line's worth,
including one prerequisite that reports a successful install while installing
nothing, so it has [a section of its own](#windows).

## Build and run

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


## Building the Arch package by hand

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


## Windows

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

### Getting a log out of it

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

## Two accounts at once

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

## Rust only

The Matrix layer has no dependency on Tauri and builds on its own:

```sh
cargo test -p consort-matrix
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Tests

Both halves have to stay above 90% line coverage, and CI enforces it.

```sh
cargo test --workspace          # 1311 tests
cd app && pnpm test             # 665 tests
cd app && pnpm test:coverage    # thresholds enforced from vitest.config.ts
```

Thirty-one Rust tests are marked `#[ignore]` because they need something a CI
container does not have. Four want a live platform keyring:

```sh
cargo test -p consort-matrix -- --ignored keyring
```

Three want a real sound card, and are how the cpal layer is checked against
hardware rather than against a fake:

```sh
cargo test -p consort-audio -- --ignored
```

The other twenty-four want a homeserver, because a SAS verification handshake is
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

See [testing/synapse/README.md](../testing/synapse/README.md) for what it is and,
more importantly, what it is not.

[COVERAGE.md](../COVERAGE.md) explains what is measured, what is excluded, and
why.

## If the window comes up blank on Linux

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

