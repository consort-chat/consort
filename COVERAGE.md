# Test coverage

Target is 90% or better, and the suite currently clears it on both sides.

| | Lines | Tests |
|---|---|---|
| Rust | 96.0% | 141 |
| Frontend | 100% | 55 |

Run them:

```sh
# Rust, with the same exclusions CI uses
cargo llvm-cov --workspace \
  --ignore-filename-regex '(keyring_store\.rs|src-tauri/src/(main|lib)\.rs)' \
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

Three files are outside the measurement. Each is excluded because a test could
only reach it by pretending, not because the code is uninteresting.

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
