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
> replies, reactions, attachments and edits, sending included. It marks what
> you have not read, remembers where you stopped, and tells you when something
> arrives while you are looking at something else. You can leave a room, and
> ask somebody into one. Deleting your own message is not built.

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
- **Sending attachments.** Pick a file, drag one onto the window, or paste a
  screenshot. Whatever it is is decided by the bytes rather than by the name,
  so a picture arrives as a picture however it was last renamed, and pictures
  are measured before they go so nobody's room jumps as they load. Whatever is
  in the box goes with it as a caption, and a picture can answer a message.
- **Threads and replies.** Threads open beside the room and can be started from
  any message. Any message can also be answered in the room itself: the box
  above the composer says what is being answered, the reply is drawn as a
  reply, and pressing it jumps to what it answers.

  [Screenshot of the thread panel open beside a room Here]
- **Links into Matrix.** A `matrix.to` link to a room or to a message is drawn
  as a badge naming where it goes, and pressing it goes there rather than
  opening a browser. Every message carries a Copy link control that puts its
  own address on the clipboard.
- **Reactions.** Twelve keys to pick from, and any key anybody else sends draws
  correctly. Custom emoji from other clients are shown.
- **What you have not read.** A channel with something waiting in it is drawn
  in white, a channel where somebody said your name carries a count, and a room
  you come back to opens where you left off with a line across it. Read
  receipts go out publicly by default, the way every other Matrix client sends
  them; Settings has a switch that keeps them to your own account instead.
- **Who has read what.** Small faces under the last message in a group, showing
  who has read up to it, and a count when more have read it than the row can
  hold. Only people who share read receipts can appear there, so the row says
  so: somebody who has turned the switch above off is invisible to everybody,
  and fewer faces than there are people in the room is that setting working
  rather than this feature failing.
- **Leaving a room, and asking somebody into one.** Both live under the room's
  own details, beside its name and topic. Leaving asks first, because it does
  not come back: an invite-only room left by mistake needs somebody still in it
  to ask you back. Inviting takes a Matrix user ID, and says which of five
  things went wrong when one does, because "that did not work" is useless when
  the reason is that somebody here banned them.
- **Notifications.** A desktop notification when Consort is not the window you
  are looking at, or when it is and you are reading a different channel.
  Clicking one brings the window forward and opens the channel it was about.
  What counts as worth telling you about is your account's own Matrix push
  rules, so a room you muted in another client is muted here; Settings adds
  only what is true of this machine, and nothing is drawn about what happened
  while Consort was closed.
- **An icon in the system tray.** With Show Consort and Quit Consort in its
  menu, so a client left running is still one click away on a desktop that
  hides minimised windows. Closing the window still closes Consort; the tray is
  for getting back to a window that is out of sight, not yet for keeping one
  alive behind a close button. On Linux the tray needs
  `libayatana-appindicator`, which the packages ask for; a machine without it
  runs Consort with no tray icon and says so in the log.
- **Where it opens.** With no channel selected, the pane offers the rooms you
  were last in and any voice channel somebody is sitting in right now, across
  every space rather than the one on screen. What was opened last is written
  down locally, per account, so it survives a restart and costs the homeserver
  nothing.

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
sudo dnf install ./target/release/bundle/rpm/Consort-0.7.0-1.x86_64.rpm
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
| Sending attachments, by picker, drag or paste | working |
| Read receipts sent and drawn, unread channels, where reading stopped | working |
| Desktop notifications, honouring your push rules | working |
| Leaving a room, and inviting somebody to one | working |
| Editing, upload progress, video thumbnails | planned |
| Signed and notarised builds for Windows and macOS | someday |

"Working" means doing real work in that row, not that the row is finished.

## Known limitations

- **Builds are unsigned.** SmartScreen and Gatekeeper will say so.
- **Password login only.** No SSO or OIDC yet.
- **No local echo.** A message you send appears when the sync brings it back.
- **A reply to something older than what is loaded** says so rather than
  fetching it, and a thread longer than fifty replies shows its recent end.
- **A link to a message older than what is loaded** opens the room and stops
  there. Going to the message itself needs history fetched around it, which is
  the same gap the reply above has.
- **Playing a clip needs codecs this application does not ship.** Consort
  renders through WebKitGTK, which decodes through GStreamer, so an mp4 needs
  H.264 and AAC decoders installed on the machine. Where they are missing the
  clip says so and offers to save itself. On Arch that is `gst-libav`,
  `gst-plugins-ugly` and `gst-plugins-bad`.
- **Notifications have only been run on Linux.** The Linux path is DBus to
  whatever notification daemon the desktop runs. The Windows one is a toast
  attributed to an AppUserModelID that has to match the shortcut the installer
  writes, and nobody has checked that it does; if toasts do not appear on
  Windows, that is the first thing to look at.
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
