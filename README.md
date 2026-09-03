# Consort

A desktop chat client for [Matrix](https://matrix.org), built in Rust and Tauri.

Consort is aiming at the shape of a voice-first team chat app: persistent voice
channels you drop into, a room list you live in all day, and a push-to-talk
alternative that actually works. The protocol underneath is Matrix, all the way
down. There is no proprietary backend and there is no Consort server. Point it
at whatever homeserver you already run.

[Screenshot of the main window in a text room Here]

> **Status: early.** It signs you in and keeps you signed in, verifies itself,
> draws your rooms, joins voice channels, and reads and writes text, threads,
> replies, reactions and attachments. Sending an attachment and editing a
> message are not built.

---

## What works today

- **Signing in.** Password login against any homeserver, typed as a plain
  server name. `.well-known` discovery is handled for you, the session survives
  a restart, and the access token goes in your system keyring.
- **Verification.** Emoji verification in both directions, or a recovery key.
  Key backup means history older than this session decrypts.

  [Screenshot of emoji verification mid-flow Here]
- **Voice.** Persistent channels over MatrixRTC and LiveKit. You can see who is
  already in one before joining, who is speaking, and who has gone deaf.

  [Screenshot of a voice channel with somebody speaking Here]
- **Sound.** Device pickers, a level meter, an output test, and RNNoise voice
  activity detection with hysteresis gating, so the gate does not chatter on
  the first syllable of every word.

  [Screenshot of the audio settings with the level meter Here]
- **Text.** Reading and sending messages, with names and avatars. Older
  messages load as you scroll to them. Attachments are drawn and can be saved
  anywhere. Mentions of you are marked.
- **Threads and replies.** Threads open beside the room and can be started from
  any message. A reply is drawn as a reply, and pressing it jumps to what it
  answers.

  [Screenshot of the thread panel open beside a room Here]
- **Reactions.** Twelve keys to pick from, and any key anybody else sends draws
  correctly. Custom emoji from other clients are shown.

---

## Installing

Every tag carries three builds on its
[release page](https://github.com/consort-chat/consort/releases), each made by
CI on a clean runner from the tagged commit. Nothing is signed and no
repository serves any of it, so each of these is a file you fetched yourself.
See [Known limitations](#known-limitations) before putting this on a machine
you care about.

### Windows

`Consort_<version>_x64-setup.exe`.

SmartScreen will say "Windows protected your PC" the first time it runs, which
means "More info" and then "Run anyway". That warning is about the absence of a
signature, not about anything the installer does, and the only way to tell
those apart is to check the download against the SHA-256 GitHub prints beside
the asset.

### Arch Linux

```sh
sudo pacman -U ./consort-<version>-1-x86_64.pkg.tar.zst
```

pacman will say the package is unsigned and ask whether to install it anyway.
Nothing here has a key yet, so that is expected rather than a sign of trouble.

To build it yourself instead, run `makepkg -si` in
[`packaging/arch/`](packaging/arch/). To track `main` rather than the last
release, [`packaging/aur/`](packaging/aur/) builds the latest commit. Neither
is on the AUR yet. Arch is the distro this is developed on, so it is the one
most likely to work.

### Debian and Ubuntu

```sh
sudo apt install ./Consort_<version>_amd64.deb
```

Built on Debian 12, which is therefore the oldest thing it runs on, and that is
deliberate: a package built on the current Ubuntu links a newer glibc, installs
happily on Debian 12, and then refuses to start. One file covers Debian 12 and
13 and Ubuntu 24.04 onwards.

Ubuntu 22.04 is out of reach. It has glibc 2.35 and only webkit2gtk-4.0, and
Tauri 2 needs 4.1.

### Fedora

Not built by CI, because nobody has said where an `.rpm` would be tested.
`pnpm tauri build` writes one locally alongside the `.deb`:

```sh
sudo dnf install ./target/release/bundle/rpm/Consort-0.3.0-1.x86_64.rpm
```

There is no AppImage on purpose. linuxdeploy cannot parse the `.relr.dyn`
sections current Arch libraries use, and one built here would link the host
glibc anyway, which is the exact problem an AppImage is supposed to solve. Use
the `.deb` or the Arch package.

---

## Building from source

```sh
git clone https://github.com/consort-chat/consort.git
cd consort/app
pnpm install
pnpm tauri dev
```

[docs/BUILDING.md](docs/BUILDING.md) has the prerequisites, the tests, running
two accounts at once, and the Windows build, which needs more than one line's
worth and includes a prerequisite that reports a successful install while
installing nothing.

## How it is put together

```
crates/consort-matrix/    Authentication, session persistence, sync, rooms,
                          timeline, verification. No Tauri, no UI. The
                          testable half.
crates/consort-audio/     Devices, capture, playback, the voice gate, the
                          level meter. Knows nothing about Matrix.
crates/consort-call/      Being in a MatrixRTC call. Separate because it
                          brings libwebrtc, which should not be in the way of
                          every `cargo test -p consort-matrix`.
app/src-tauri/            The Tauri shell. Commands, state, events, wiring.
app/src/                  React and TypeScript frontend.
testing/synapse/          A homeserver to throw away, for the tests that
                          cannot use a mock.
```

The split is worth keeping. Anything that can live in `consort-matrix` should,
because that is the code you can exercise with `cargo test` rather than by
clicking through a window. [CLAUDE.md](CLAUDE.md) is the working notes for the
parts that will waste your time otherwise.

## AI-assisted development

**Use AI. Read what it wrote before you send it.**

Contributions written with AI assistance are welcome and there is no disclosure
ritual. What is not welcome is unread output: the cost of generated code does
not disappear, it moves to whoever reviews it, and review is the scarce
resource here. The full standard, and what gets a change sent back, is in
[CONTRIBUTING.md](CONTRIBUTING.md#ai-assistance).

## Roadmap

| Milestone | State |
|---|---|
| Login, session persistence, verification, key backup | working |
| Room list and voice channel discovery | working |
| Voice over MatrixRTC and LiveKit, with device settings | working |
| Text, attachments, threads, replies, reactions, mentions | working |
| Sending attachments, editing, read receipts | planned |
| Signed and notarised builds for Windows and macOS | someday |

"Working" means doing real work in that row, not that the row is finished.

## Known limitations

- **Builds are unsigned.** SmartScreen and Gatekeeper will say so.
- **Password login only.** No SSO or OIDC yet.
- **No local echo.** A message you send appears when the sync brings it back.
- **A reply to something older than what is loaded** says so rather than
  fetching it, and a thread longer than fifty replies shows its recent end.
- **Playing a clip needs codecs this application does not ship.** Consort
  renders through WebKitGTK, which decodes through GStreamer, so an mp4 needs
  H.264 and AAC decoders installed on the machine. Where they are missing the
  clip says so and offers to save itself. On Arch that is `gst-libav`,
  `gst-plugins-ugly` and `gst-plugins-bad`.
- **On a machine with no keyring, the access token falls back to a file** with
  `0600` permissions, and the signed-in screen says so rather than letting you
  assume otherwise. Secret Service is a DBus service rather than a kernel
  feature, and a bare window manager, a container or an SSH session may not
  have one. See `crates/consort-matrix/src/secrets/`.
- **Signing out leaves the encryption store on disk.** The session and the
  token go; the SQLite store holding this device's room keys stays until the
  next sign-in on that account removes it. The keys belong to a device the
  server has already destroyed, but they are decrypted room keys sitting in
  your data directory after you asked to be signed out.
- **One Consort at a time.** A second launch focuses the first window. Two
  processes would race on one crypto store, which is how device keys get lost.
  [docs/BUILDING.md](docs/BUILDING.md#two-accounts-at-once) has the way round
  it for testing.

## Licence

[GNU Affero General Public License, version 3](LICENSE), and only version 3.

Inherited rather than chosen: Consort links
[`matrix-rust-rtc`](https://github.com/BillCarsonFr/matrix-rust-rtc), which is
AGPL-3.0-only. If you run a modified Consort as a network service, the AGPL
requires you to offer that modified source to its users.

Copyright the Consort contributors.
