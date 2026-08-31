# Test coverage

Target is 90% or better, and the suite currently clears it on both sides.

| | Lines | Tests |
|---|---|---|
| Rust | 95.3% | 1068 |
| Frontend | 98.1% | 431 |

Run them:

```sh
# Rust, with the same exclusions CI uses
cargo llvm-cov --workspace \
  --ignore-filename-regex '(keyring_store\.rs|cpal_host\.rs|consort-call/src/livekit\.rs|src-tauri/src/(main|lib)\.rs)' \
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

## What is excluded, and why

Five files are outside the measurement. Each is excluded because a test could
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
