# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## What this is

Consort is a desktop Matrix chat client in Rust and Tauri, aimed at voice-first
team chat. Today it does authentication, session verification (emoji and
recovery key), room key backup, the room list, voice over MatrixRTC and
LiveKit, reading and sending text in a room, attachments both sent and drawn,
threads, replies both sent and drawn, reactions, mentions, a typing indicator,
read receipts with the unread marks and the where-you-stopped line that go with
them, desktop notifications that honour the account's push rules, and
`matrix.to` links that go where they point, and editing a message, both sent
and drawn. Deleting your own message is not built, and neither is upload
progress or a thumbnail for a clip somebody sends.

## Layout

```
crates/consort-matrix/   Matrix auth, session persistence, sync, rooms, timeline.
crates/consort-audio/    Sound cards, the voice gate, the mixer. No Matrix.
crates/consort-call/     Being in a MatrixRTC call. No sound backend.
app/src-tauri/           Tauri v2 shell: commands, state, events, wiring.
app/src/                 React 19 + TypeScript frontend, Vite.
testing/synapse/         A throwaway homeserver for the tests a mock cannot cover.
```

## Commands

```sh
# Rust
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

# Frontend (from app/)
pnpm install
pnpm build          # tsc --noEmit && vite build
pnpm test           # vitest
pnpm test:coverage  # vitest with the thresholds enforced
pnpm tauri build    # release bundle

# The dev build.
pnpm tauri dev
```

Use **pnpm**, not npm or yarn. The lockfile is pnpm's and `pnpm-workspace.yaml`
carries settings the build needs.

## Things that will waste your time if you do not know them

### A blank window on NVIDIA is the DMABUF renderer, and Consort now handles it

For a long time this section said the opposite: that
`WEBKIT_DISABLE_DMABUF_RENDERER=1` had to be typed in front of every
`pnpm tauri dev`, and that it was the single most repeated mistake in this
repository's history. It was. `app/src-tauri/src/renderer.rs` now sets it in
`run()` when `/sys/module/nvidia` is present, so neither the dev build nor an
installed one needs it on the command line.

Worth knowing anyway, because the failure is silent. WebKitGTK composites
through a DMABUF buffer it allocates with GBM, NVIDIA's driver refuses the
allocation, and the result is a window with a title bar and nothing inside it.
The process stays up, the frontend is served, and the Rust side logs a textbook
boot: session restored, channels listed, key backup state read. Every check
somebody would think to run says the application is fine. The only tell is
`Failed to create GBM buffer` on stderr, among GTK theme warnings that are
harmless and always present.

The old reasoning against baking the workaround in was that it would cost every
user the fast rendering path to paper over one machine. That was wrong on the
facts. The machine was never the special part, the driver is, and it is still
broken on webkit2gtk 2.52.6. So the workaround is applied on exactly that
condition and nowhere else, and an explicit `WEBKIT_DISABLE_DMABUF_RENDERER`
in the environment still wins in either direction.

When stopping the dev build, kill the Vite process too, not just the window.
Vite holds port 1420 with `strictPort`, so a leftover one makes the next
`pnpm tauri dev` abort with `Port 1420 is already in use` before it ever
compiles. `ss -ltnp | grep 1420` names the process still holding it.

### The matrix-sdk pin is load-bearing

`Cargo.toml` pins `matrix-sdk` to git rev `3773300` of
`BillCarsonFr/matrix-rust-sdk`, not a crates.io release.

That rev must stay identical to the one `matrix-rust-rtc` pins. When the voice
layer lands it takes a `matrix_sdk::Client`, and if the revs differ cargo
resolves two copies of the SDK and produces `expected Client, found Client`
errors that look like nonsense. Never bump this pin alone.

The upstream repo is at `../matrix-rust-rtc` on this machine. Check
`crates/matrix-rtc-livekit/Cargo.toml` there before touching the pin.
[docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) has what moves each pin, how to
see how far behind the fork has fallen, and what to do about an advisory.

### Call::join cannot live in a Tauri command

`Call::join` drives `!Send` futures via `spawn_local` and panics outside a
`tokio::task::LocalSet`. Tauri v2 commands run on a multi-threaded runtime and
require `Send` futures. The two cannot meet.

The design is a call actor: a dedicated thread owning a current-thread runtime
plus a `LocalSet`, holding the `Call`, reached over a command channel with an
event stream coming back. Only the channel handle goes in `AppState`. This is
written down in `app/src-tauri/src/state.rs`.

Do not try to solve this with `block_on` inside a command. It deadlocks.

### Quitting has to leave the call, because nothing here is dropped

Ctrl+Q is a `keydown` handler on the window in `app/src/App.tsx`. It invokes the
`quit` command, which calls `AppHandle::exit`, and that is where the interesting
part is: tao's event loop calls `process::exit` from inside its own `run`, so
`run` never returns, nothing Tauri manages is ever dropped, and the `Drop` on
`CallBridge` that a sign-out relies on never runs. The window's close button
ends up in the same place. So a quit while connected used to publish no leave at
all. The process vanished and left the membership standing, which is issue #110:
a phantom of the machine that quit, sitting in the voice channel for everybody
else to look at.

How long they look at it is a number, and it is not ours. Consort joins with
`ElementCallCompat::StateEvents` (`consort_call::Dialect::State`, still the only
dialect this build can read a membership back out of), so the membership is an
`org.matrix.msc3401.call.member` **room state** event, and `Call::join` arms an
MSC4140 delayed leave against it: a delayed *state* event with `{}` content,
which genuinely empties the membership when it fires. The timings are upstream's
defaults, because `livekit.rs` passes `..CallOptions::default()`, and they are a
30 second delay restarted by a heartbeat every 15 seconds. A client that dies is
therefore cleared 15 to 30 seconds later, by the homeserver, firing the switch
that client armed on its way in. Somebody watching a clone of themselves vanish
"after a few seconds" is watching exactly that.

Which answers the question that came with the report, because it was the first
guess: this is not a setting on anybody's homeserver, and nobody configured a
cleanup. The only server-side variable is whether MSC4140 delayed events are
available at all. Where they are refused (Synapse without `msc4140`, or
matrix.org, which has them switched off and says so) there is no dead man's
switch, and the bound becomes the deadline written into the content instead:
`created_ts + expires`, which the core degrades to five minutes for precisely
this reason. The ghost then lasts up to five minutes rather than thirty seconds.

Worth knowing before the dialect moves: this is the *good* case. In the sticky
dialects a delayed leave is a plain delayed event, which clears nothing from the
homeserver's sticky map, so a client that dies lingers for the whole sticky
duration, an hour by default. matrix-rust-rtc's `crates/matrix-rtc-bridge/src/sdk.rs`
carries the note and the reason. A move to `Dialect::Sticky` or `Dialect::Current`
makes a quit that fails to leave cost an hour rather than half a minute.

What Consort does now is leave, in the one place every exit converges:
`RunEvent::Exit` in `app/src-tauri/src/lib.rs` hands off to `on_exit`, which
hides the windows and then calls `AppState::leave_call_on_quit`. Not in the
`quit` command, because the close button and a window manager closing the window
never go through it. The wait is
bounded twice over: `consort_call::SHUTDOWN_LEAVE_TIMEOUT` bounds the request,
`LEAVE_ON_QUIT` bounds the wait for the thread making it, and the outer one is
deliberately the longer of the two so it is a backstop rather than a competitor.
A homeserver that has stopped answering costs a few seconds of an invisible
process and nothing else. It cannot leave somebody unable to close the
application, which would be a worse bug than the one this fixes. The windows go
first for the same reason: Ctrl+Q reaches `RunEvent::Exit` with the window still
on screen, and a window that sits there not drawing is indistinguishable from
one that has hung.

What is still not covered, and cannot be from here: `SIGKILL`, `SIGTERM`, a
panic, a machine losing power, and a quit during a slow join, where the call
thread cannot read its shutdown message until the join it is inside has finished
and the wait is abandoned at the bound. Every one of those is what the dead
man's switch is for, and the numbers above are what they cost.

### rustls needs an explicit crypto provider

`consort_matrix::install_crypto_provider()` runs once in `run()` before any TLS.
Once livekit joins the graph, both the `ring` and `aws-lc-rs` backends are
compiled in, rustls refuses to pick, and the first HTTPS request panics. Leave
that call where it is.

### The frontend cannot reach anything directly

The Tauri capability set grants `core:default` only. No filesystem, no shell, no
HTTP from the webview. Everything privileged goes through a Rust command. If a
frontend change seems to need a new capability, that is a signal the logic
belongs in Rust.

### A paste carries no picture, so the clipboard is read in Rust

`clipboardData.files` and `clipboardData.items` are both empty in WebKitGTK
when what is on the clipboard is an image. A composer that waits for a `File`
off a `paste` event waits forever, and it fails silently: the event fires, the
handler runs, and there is simply nothing in it.

Do not reach for `clipboardData` again. Ctrl+V is a `keydown` on the window,
and `attachment_paste` reads the clipboard through
`tauri-plugin-clipboard-manager`, where the picture is available and where
nothing depends on what has focus. `attaching::Clipboard` is the trait that
keeps the rule testable without a desktop.

The rule itself: **text on the clipboard means there is no screenshot to
stage.** A copy out of a spreadsheet carries both the words and a rendering of
them, and staging the picture would lose what somebody meant to paste, so Rust
reads text first and answers with nothing when it finds any. The keystroke is
never `preventDefault`ed, which is what lets the words reach the box on their
own. Reading text first is also what stops an ordinary paste pulling a
full-screen RGBA buffer nobody asked for.

Two things worth not rediscovering. The command is `async` so the read happens
off the main thread: an X11 selection belongs to a process, and pasting a link
copied out of Consort itself would otherwise have the window waiting on an
answer only the window can give. And the picture never crosses the IPC. It is
held in `AppState` and sent by asking for what is held, because a temp file has
no honest owner to delete it and a plaintext screenshot left on disk by an
end-to-end encrypted client is not a trade worth making.

The bug behind all of this is WebKit 218519, five years of `Pasteboard::read()`
not reading clipboard image buffers. It was fixed upstream in October 2025 and
released in webkitgtk 2.51.1, but 2.52.6 still hands the page nothing here, so
do not assume a version bump has made this section obsolete without pasting a
screenshot into a dev build and watching it land.

### The IPC runs both ways now

Commands are request and response; anything push-driven is an event.
`app/src-tauri/src/events.rs` owns every channel name, and `AppEvent` is the
only thing that names one. Do not write an event name as a string literal at a
call site: Tauri validates names at emit time, so a typo is a listener that
silently never fires rather than anything that fails to build.

On the frontend, `onConnection`, `onVerification`, `onVerificationFlow` and any
sibling in `app/src/lib/api.ts` return their unlisten function. Call it from the effect
cleanup. A leaked listener survives a sign out, and every later event then
arrives once per leak.

### Every channel carries state, so a late subscriber has to ask

The background tasks start with the session, which on a restored session is
inside Tauri's `setup`, before the webview has run any JavaScript. Whatever
they published before the listeners existed went to nobody, and because these
are state channels rather than streams of incidents, missing one is not a
missed message: it leaves the interface on its initial guess until something
happens to change, which on a healthy session may be never.

`LatestSink` in `events.rs` keeps the last event per channel, and the
`resend_state` command replays them. A component subscribes to everything it
needs, then calls `resendState()` once, and gets the current state through the
same handler as every later change. Any new channel gets this for free; do not
add a getter command per channel alongside it.

Not every channel is a state channel, and `AppEvent::is_worth_keeping` is where
that is decided. A verification flow is state while it is running and history
once it is over, so an ending clears what the channel was holding rather than
replacing it. Without that, a remount resurrects "the emoji did not match" for
a flow that finished twenty minutes ago.

### The sync loop reports its own health

`consort_matrix::sync` uses `sync_with_result_callback` rather than `sync`,
because `sync` swallows failed iterations and retries invisibly, which makes a
client that has been offline for an hour indistinguishable from one that is
connected and idle. Relatedly, `base_builder` sets
`RequestConfig::short_retry()`: matrix-sdk otherwise retries a 5xx for fifteen
minutes on its own, inside a single `sync_once`, with nothing able to observe
it.

### Two types are called VerificationState

`matrix_sdk::encryption::VerificationState` describes our own device and has
three plain variants: `Unknown`, `Verified`, `Unverified`. That is the one
`consort_matrix::verification` watches and maps onto `SessionVerification`.

`matrix_sdk_common::deserialized_responses::VerificationState` describes who
sent a message and its `Unverified` carries a `VerificationLevel`. Reaching for
that one when the question is "is this session verified" gets you a level that
does not apply. `Unknown` is a real state and must never be rendered as either
answer.

### Verification flows are addressed, not held

Nothing keeps a `SasVerification` in a field. The SDK has a registry keyed by
`(user_id, flow_id)`, every action in `verification::flow` re-resolves through
it, and the frontend passes back the pair the event carried. A
`Mutex<Option<SasVerification>>` in `AppState` would be a second lifetime to
manage and would make two concurrent requests unrepresentable, which they are
not: a request goes to every device on the account and two can answer.

What is owned is one task per flow, all of them inside the `JoinSet` that
`supervise` holds. Each flow task holds the `Client` and watches a stream
belonging to that same client, so nothing about signing out can make one end on
its own. Aborting the supervisor drops the set, which aborts the lot. Detaching
them with a bare `tokio::spawn` leaks a client per abandoned verification.

Two more things worth not rediscovering. As the responder you must call
`SasVerification::accept()` after `Transitioned` or the exchange stops dead;
`follow_sas` does it automatically, because it settles hash and MAC algorithms
rather than asking the user anything. And expiry needs no timer of ours: the
crypto machine garbage-collects timed-out flows on every sync response and the
`Cancelled(Timeout)` arrives on `changes()` by itself.

## Conventions

**Put logic in `consort-matrix`.** Tauri commands are adapters: translate
arguments, call in, translate the result. If a command grows a branch worth
testing, that branch is in the wrong crate.

**Comments explain why, never what.** The repository holds a fairly high bar
here. Read a neighbouring file before writing new ones. A comment that restates
its line gets deleted in review.

**Errors carry two messages.** `consort_matrix::Error::user_message()` is written
for a person and is what the UI renders. `Display` is for logs. Do not leak raw
homeserver error codes into the interface.

**No em dashes** in code, comments, docs, or commit messages. Use a comma, a
colon, parentheses, or two sentences.

**Design tokens, not literals.** Colours, spacing, type scale, and durations
live in `app/src/styles/tokens.css`. The mint accent is reserved for voice and
presence; using it decoratively breaks the one signal the voice work needs.

## Testing

`cargo test --workspace` must stay green and must not reach the network.
Coverage must stay above 90% on both halves; CI fails below it. See
[COVERAGE.md](COVERAGE.md).

New behaviour needs a test. Test behaviour, not implementation.

Three patterns this codebase relies on, worth following rather than
rediscovering:

- **Homeserver code is testable.** `MatrixMockServer`, from matrix-sdk's
  `testing` feature, is a dev-dependency of both crates.
  `crates/consort-matrix/tests/against_a_mock_homeserver.rs` covers login,
  restore, logout, token rotation and the sync loop. Its `SyncResponseBuilder`
  has `add_to_device_event`, so a to-device event reaching a handler is
  testable without a homeserver. A verification *flow* is not: the crypto
  machine looks the sender's device up in its own store before building a
  request object, and a mocked `/keys/query` has no devices in it. `#[ignore]`
  is for things a container genuinely cannot have: a live platform keyring, and
  a homeserver for the eight tests in
  `crates/consort-matrix/tests/against_a_real_homeserver.rs`, which
  `testing/synapse/up.sh` provides. Those drive both sides of a real emoji
  handshake, so they are the coverage for `verification/flow.rs`, and each one
  registers an account of its own: two logins to a reused account produce two
  devices where neither holds the cross-signing private keys, so nothing can
  sign anything and the test passes only the first time.
- **An upload needs the media config endpoint mocked.** `Room::send_attachment`
  asks the homeserver how large an upload it takes before it sends anything, so
  a `MatrixMockServer` test of the send path that mounts only `mock_upload` and
  `mock_room_send` fails on a 404 that names neither. Mount
  `mock_authenticated_media_config` and `mock_media_config` both: which one the
  SDK reaches for depends on the versions the server advertises.
- **Tauri commands are one-line delegates.** `State<'_, AppState>` only exists
  inside a running app, so logic written directly in a `#[tauri::command]` is
  logic no test can reach. Every command calls a plain `*_for(&AppState, ..)`
  function, and that is what the tests drive.
- **Secrets go through a trait.** `secrets::Backend` has a `MemoryBackend`
  implementation, and `SessionStore::with_backend` takes one. No test should
  ever touch the developer's real keyring.

## AI-assisted contribution policy

This repository accepts AI-assisted work and says so in the README. It also
rejects unread output. When working here, that applies to you:

- Do not add abstractions for cases that do not exist.
- Do not add defensive branches for conditions that cannot occur.
- Do not write comments narrating the code.
- Do not expand scope past what was asked. If you notice something else worth
  fixing, say so rather than fixing it in the same change.
- Prefer a small diff that is obviously correct to a large one that is
  impressively general.

## Skills

`.claude/skills/` carries conventions for this stack. Consult the relevant one
before writing code, and pass its conventions into any subagent prompt:

| Working on | Skill |
|---|---|
| `crates/**/*.rs`, `app/src-tauri/**/*.rs` | `rust-patterns`, `rust-testing` |
| `app/src/**/*.tsx` | `react-patterns`, `frontend-patterns`, `react-testing` |
| `vite.config.ts`, build setup | `vite-patterns` |
| Anything security-adjacent | `security-review` |
| A decision worth recording | `architecture-decision-records` |
