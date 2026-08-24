# CLAUDE.md

Guidance for Claude Code (claude.ai/code) working in this repository.

## What this is

Consort is a desktop Matrix chat client in Rust and Tauri, aimed at voice-first
team chat. Today it does authentication and nothing else. Voice over MatrixRTC
and LiveKit is the next milestone.

## Layout

```
crates/consort-matrix/   Matrix auth and session persistence. No Tauri, no UI.
app/src-tauri/           Tauri v2 shell: commands, state, wiring.
app/src/                 React 19 + TypeScript frontend, Vite.
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
pnpm tauri dev      # run the app with frontend hot reload
pnpm tauri build    # release bundle
```

Use **pnpm**, not npm or yarn. The lockfile is pnpm's and `pnpm-workspace.yaml`
carries settings the build needs.

## Things that will waste your time if you do not know them

### The matrix-sdk pin is load-bearing

`Cargo.toml` pins `matrix-sdk` to git rev `3773300` of
`BillCarsonFr/matrix-rust-sdk`, not a crates.io release.

That rev must stay identical to the one `matrix-rust-rtc` pins. When the voice
layer lands it takes a `matrix_sdk::Client`, and if the revs differ cargo
resolves two copies of the SDK and produces `expected Client, found Client`
errors that look like nonsense. Never bump this pin alone.

The upstream repo is at `../matrix-rust-rtc` on this machine. Check
`crates/matrix-rtc-livekit/Cargo.toml` there before touching the pin.

### Call::join cannot live in a Tauri command

`Call::join` drives `!Send` futures via `spawn_local` and panics outside a
`tokio::task::LocalSet`. Tauri v2 commands run on a multi-threaded runtime and
require `Send` futures. The two cannot meet.

The design is a call actor: a dedicated thread owning a current-thread runtime
plus a `LocalSet`, holding the `Call`, reached over a command channel with an
event stream coming back. Only the channel handle goes in `AppState`. This is
written down in `app/src-tauri/src/state.rs`.

Do not try to solve this with `block_on` inside a command. It deadlocks.

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

`cargo test --workspace` must stay green and must not need a network. Tests
requiring a live homeserver are `#[ignore]` with a comment saying what they
need.

New behaviour needs a test. Test behaviour, not implementation.

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
