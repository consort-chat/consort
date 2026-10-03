# Test coverage

Target is 90% or better, and the suite currently clears it on both sides.

| | Lines | Tests |
|---|---|---|
| Rust | 93.4% | 1715 |
| Frontend | 98.1% | 1463 |

Both rows are read off CI rather than measured on somebody's machine, so they
are the numbers the thresholds were actually checked against. The Tests column
counts what a plain run runs, which leaves out the 32 `#[ignore]`d Rust tests
the sections below are about.

The Frontend job prints its summary and the figure can be read straight out of
the log. The Rust job cannot: its coverage step writes lcov instead of a table,
and `--fail-under-lines` says nothing when it passes. The number comes from the
`rust-coverage` artifact that step uploads, which is that same measurement:

```sh
gh run download <run id> -n rust-coverage
awk -F: '/^LF:/{f+=$2} /^LH:/{h+=$2} END{printf "%.1f%%\n", 100*h/f}' lcov.info
```

Run them:

```sh
# Rust, with the same exclusions CI uses
cargo llvm-cov --workspace \
  --ignore-filename-regex '(keyring_store\.rs|cpal_host\.rs|v4l_host\.rs|consort-call/src/livekit\.rs|src-tauri/src/(main|lib)\.rs)' \
  --summary-only

# Frontend, thresholds enforced from vitest.config.ts
cd app && pnpm test:coverage
```

`cargo llvm-cov` needs `llvm-tools-preview`. On a distro toolchain without
rustup, point it at the system LLVM instead, matching the version rustc reports
under `rustc -vV`:

```sh
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo llvm-cov ...
```

`cargo llvm-cov` builds into `llvm-cov-target`, a second target directory with
instrumentation in it, and never cleans up between runs. It reached 19 GB
before anybody noticed. `pnpm tauri` now drops it once a week has passed
without a coverage run, so an occasional run costs a rebuild and a permanent
one costs nothing.

## What is excluded, and why

Six files are outside the measurement. Each is excluded because a test could
only reach it by pretending, not because the code is uninteresting.

**`crates/consort-audio/src/cpal_host.rs`.** Every line talks to a sound card,
and a CI runner has none. Covering it would mean a fake audio backend, which is
what `AudioDevices` already is on the testable side of the boundary, so mocking
cpal as well would only test the mock twice.

The exclusion is honest only while this file stays thin. Deciding which device
to use, filtering ALSA's plugin wrappers out of the list and turning a
backend's buffers into frames are all decisions, and all three live in
`devices.rs` and `frames.rs` at 100%. Anything in `cpal_host.rs` that starts
deciding something belongs to move.

It carries an `#[ignore]` test that asks a real machine what it has:

```sh
cargo test -p consort-audio --lib -- --ignored --nocapture list_the_real_devices
```

**`crates/consort-video/src/v4l_host.rs`.** The same shape as `cpal_host.rs`
with a camera instead of a sound card, and a CI runner has neither. Faking V4L2
would mean faking the kernel's `ioctl` interface, which is the whole of what
this file is.

Thin on the same terms, and this one has more to keep out than the audio
backend does. Which format to ask a camera for is a real decision, and it is
`choose_offer` in `capture.rs`; reading the bytes back is the other, and it is
`pixels.rs`. Both are at 100% and both are tested as data.

Two `#[ignore]` tests ask a real machine what it has and then read from it:

```sh
cargo test -p consort-video --lib -- --ignored --nocapture list_the_real_cameras
cargo test -p consort-video --lib -- --ignored --nocapture read_the_real_camera
```

The second one reports the frame rate it actually sustained, which is the only
way to tell a conversion that cannot keep up from a camera that has decided to
run at fifteen frames a second because the room is dark.

**`crates/consort-matrix/src/secrets/keyring_store.rs`.** Every line is a call
into the platform credential store. Covering it would mean either a mock, which
tests the mock, or a live Secret Service, which no container has. It is a
separate file precisely so the exclusion can be this narrow: the backend
selection, the file fallback and the session logic that uses them are all
measured normally. The file does carry real tests, marked `#[ignore]`, that run
against an actual keyring on a developer machine:

```sh
cargo test -p consort-matrix -- --ignored keyring
```

**`crates/consort-call/src/livekit.rs`.** The same shape as `cpal_host.rs`, one
layer out: every line talks to a LiveKit SFU, and a CI runner has none. There is
no useful fake either, because what would be faked is `Call::join`, which is the
entire dependency.

It is thin on purpose and has to stay that way. Which MatrixRTC generation to
speak (`dialect.rs`), what a failure means (`failure.rs`) and everything the call
thread does with either (`thread.rs`) are decisions, they are all measured, and
the thread is driven through a fake `CallTransport` so none of it needs an SFU.
What is left in `livekit.rs` is one `Call::join` and one `Call::leave`. Anything
in it that starts deciding something belongs to move.

**`app/src-tauri/src/main.rs`.** Three lines calling `run()`.

**`app/src-tauri/src/lib.rs`.** `run()` constructs a Tauri application and
blocks until the window closes. What could be pulled out of it has been:
`default_log_filter` and `init_tracing` are tested. What remains is builder
wiring and `resolve_data_dir`, both of which need a real `AppHandle`.

Everything else counts, including the two Tauri command modules. `State<'_,
AppState>` cannot be built outside a running app, so each `#[tauri::command]`
is a one-line delegate to a plain function taking `&AppState`, and the plain
function is what the tests drive.

## How the homeserver half is tested

`login`, `restore`, `logout` and `persist_token_refreshes` all need something
answering like Synapse. They are covered against `MatrixMockServer`, from
matrix-sdk's `testing` feature, in
`crates/consort-matrix/tests/against_a_mock_homeserver.rs` and in the
`against_a_mock_homeserver` module of `app/src-tauri/src/commands.rs`.

The feature is a dev-dependency only. With resolver 3 that keeps it out of
`cargo build`, at the cost of matrix-sdk compiling twice, once per feature set.
Both artifacts stay cached, so it is disk rather than repeated time.

Endpoints those tests do not mount answer 404, which is deliberate. A login has
to succeed on a homeserver with key backup switched off, and that is what an
unmounted endpoint looks like.

## The one file that measures low, and why

`crates/consort-matrix/src/verification/flow.rs` sits around 80%, well below
everything else. It is not excluded, because most of it is measured and the
rest is worth seeing as a gap rather than hiding.

What is missing is the two functions that drive a live handshake, `drive` and
`follow_sas`, plus the five action wrappers and the two event handlers. All of
them need an olm exchange between two real devices, and `MatrixMockServer`
cannot produce one: the crypto machine will not even build a request object
from an injected to-device event, because it looks the sender's device up in
its own store first and a mocked `/keys/query` has no devices in it.

They are covered, in `crates/consort-matrix/tests/against_a_real_homeserver.rs`,
against the throwaway Synapse in `testing/synapse/`. Those tests are `#[ignore]`d
so a plain `cargo test` and CI both skip them, which is what leaves the number
here low. Run them deliberately:

```sh
testing/synapse/up.sh
CONSORT_TEST_HOMESERVER=http://localhost:8008 cargo test --workspace -- --ignored
testing/synapse/down.sh
```

Everything in that file that is ordinary logic has been pulled out so that it
is measured normally: the state mappings live in `dto.rs`, the dedup and
identity handling in `Report` take plain strings rather than an SDK request
object, and the task ownership in `Flows` and `run` is generic over what a flow
is. What is left uncovered is genuinely the cryptography.

## What a covered line does not tell you

Two places on the frontend where a line counts as covered and no assertion can
say what it did. Both are measured rather than assumed, and #98 has the
working.

**A rejection from a `vi.fn()` mock is never an unhandled rejection.** Vitest
attaches its own handler to the promise a mock returns so it can record the
settled result, and that marks the rejection handled. So a `.catch` whose whole
body is swallowing the error cannot be pinned through
`process.on("unhandledRejection")`: the line is covered, the catch did run, and
nothing can assert it caught anything.

| how the rejection is made | `unhandledRejection` sees it |
|---|---|
| `void Promise.reject(x)` | yes |
| `vi.fn().mockRejectedValue(x)` | no |
| `vi.fn().mockImplementation(() => Promise.reject(x))` | no |
| `vi.fn(() => Promise.reject(x))` | no |
| a plain `() => Promise.reject(x)`, no `vi.fn` | yes |

Most catches in `app/src` do something afterwards, log, set state, put a
sentence on screen, and each of those is tested through what it does. Eighteen
have an empty body, four of them holding only a comment: one in `AppShell.tsx`,
two in `MediaControls.tsx`, one in `PersonMenu.tsx`, ten in `RoomTimeline.tsx`
and four in `ThreadPanel.tsx`. Those are the ones nothing speaks for. Pinning
one means mocking that command with a plain function rather than a `vi.fn()`,
which gives up the call assertions on it, so it is worth doing where the
swallowing is the point and not otherwise.

**A state update after an unmount is silent.** React dropped the warning in 18
and 19 has not brought it back, so `if (!cancelled) setThing(x)` in a handler
whose whole body is a state update behaves identically present or absent. There
is nothing to observe and so nothing to test. `SignedIn.tsx` carried twelve of
them and they were two thirds of its branch count: 36 branches before, 12
after, 100% both times. The two `cancelled` checks left in that file do
something, stopping a listener that arrived late and not asking to be caught up
into a screen that has gone, and each fails a test when it is removed. That is
the line: a check earns its place when removing it turns a test red.

That line has since been held to all thirty-six of these checks, one mutation
at a time, each run against the whole suite. Three were pinned: the two in
`SignedIn.tsx` and the `resendState` one in `ThreadPanel.tsx`. Thirty-three
turned nothing red. What follows is not one thing, because the checks are not
one thing, and what decides it is the effect's dependency list rather than the
body.

**A mount-only effect is where the silence argument holds.** With `[]` the
cleanup runs at unmount and nowhere else, so the late answer reaches a
component that has gone and a check in front of a bare `setState` cannot be
told from its absence. Nine are this: `App` 1, `OpeningPane` 2,
`NotificationsSection` 2, `PrivacySection` 2, `ThreadPanel` 1, `RoomTimeline`
1. They are `SignedIn.tsx`'s twelve again and they would go the same way.

**A keyed effect is not.** With `[channel.id]` or `[userId]` the cleanup runs on
every change of the key while the component stays mounted, so the check guards
a write to live state, and removing it draws the previous room's or person's
answer under the name of the one that replaced it. Twenty are this:
`VerificationBanner` 4, `VoiceVideoSection` 4, `AccessibilitySection` 1,
`ThreadPanel` 1, `RoomAvatar` 1, `PersonMenu` 3, `PresenceDot` 1,
`RoomTimeline` 5. They earn their place and the suite was not asking.

Two now ask, one per shape. `RoomAvatar` covers a stale answer overwriting the
key that replaced it. `RoomTimeline` covers a stale channel listener, which
closed over the room it subscribed for, so the `roomId === channel.id` beside
the check cannot reject the answer and only `cancelled` can; what it costs to
take is every face in the open room, because a published value replaces the
store rather than merging into it. Each fails when its own check is removed and
nothing else does. The eighteen left are the same two shapes again.

**Four guard something other than a state update**, so the silence argument
never applied to them: `App` logs through `console.error` twice and writes the
document's text scale once, and `RoomTimeline` stages a dropped file.
Observable, mount-only, untested.

Two numbers worth keeping beside each other. Replacing one unfalsifiable test
with a falsifiable one and adding a second moved coverage not at all:
statements 95.83%, branches 92.47%, functions 95.98%, lines 98.07%, before and
after. Those branches were already counted. That is the whole of #98 in one
line.
