# Contributing to Consort

Thanks for looking. Consort is early, which means the surface area is small and
a well-scoped change can land quickly.

## Before you start

For anything larger than a bug fix, open an issue first and say what you intend
to do. This is not bureaucracy, it is to stop you spending a weekend on
something that conflicts with work already in progress or with a design decision
that has a reason behind it.

For a typo, a broken link, or an obviously wrong line, skip the issue and open
the pull request.

## Setting up

See [Building from source](README.md#building-from-source) in the README.

The short version:

```sh
cd app
pnpm install
pnpm tauri dev
```

## Before you open a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && pnpm test && pnpm build
```

All of them have to pass. Clippy is denied at warning level on purpose, so a
lint you disagree with needs an `#[allow]` with a comment explaining why, not a
silent pass.

Nothing in that list needs the network or a homeserver. The tests that do are
`#[ignore]`d, so CI skips them and so does everybody who has not asked for
them. If you are touching encryption or verification, ask for them:

```sh
testing/synapse/up.sh        # a throwaway Synapse with two accounts
export CONSORT_TEST_HOMESERVER=http://localhost:8008
cargo test --workspace -- --ignored
testing/synapse/down.sh      # deletes every trace of it
```

It needs Docker and it downloads a Synapse image the first time. A SAS
handshake is real cryptography between two real devices, so there is no mock
that can stand in for this.

House style bans the em dash and the en dash, in code, comments, commit
messages and documentation alike. A plain hyphen in a compound word or a list
bullet is fine. CI checks this, so it is worth catching first.

## What CI does, and when it runs

CI lives in `.github/workflows/ci.yml` and runs four jobs: frontend (typecheck,
tests with coverage thresholds, build), Rust (fmt, clippy, tests, coverage),
advisories (`cargo audit` against the lockfile) and hygiene (the dash rule and a
couple of other greps).

If this is your first pull request, **the run will not start until a maintainer
approves it.** That is GitHub holding workflows from authors the repository does
not yet trust, and it is deliberate rather than something going wrong. Once
somebody approves one, your later pull requests run without waiting. Nothing is
required from you but patience on the first one.

## What makes a change easy to accept

**One thing per pull request.** A fix plus an unrelated refactor is two pull
requests wearing one coat, and it takes longer to review than both would
separately.

**Explain the why, not the what.** The diff already says what changed. The
description should say why the change is correct, and what you considered and
rejected. If there is a subtlety, a comment in the code is better than a comment
on the pull request, because the code outlives the thread.

**Put logic where it can be tested.** Anything that does not need a webview
belongs in `crates/consort-matrix`, not in a Tauri command. Commands should be
thin enough to read in one pass.

**Match the surrounding code.** Comment density, naming, and structure vary
between files for reasons. Read the file you are editing before adding to it.

## Comments

Comments should say something the code cannot.

```rust
// Bad: restates the line below.
// Set the timeout to 30 seconds.
let timeout = Duration::from_secs(30);

// Good: explains a choice a reader would otherwise question.
// Synapse's default federation timeout is 20s, so anything shorter here turns
// a slow-but-working remote server into a login failure.
let timeout = Duration::from_secs(30);
```

If a comment would only ever be read as "yes, I can see that," delete it.

## Tests

New behaviour needs a test. Test the behaviour, not the implementation: a test
that asserts a particular function was called will break on every refactor and
catch no bugs.

Coverage has to stay above 90% on both halves, and CI fails below that. It is
currently 96% on the Rust side and 100% on the frontend, so there is room, but
not much. [COVERAGE.md](COVERAGE.md) explains what is excluded and why.

Code that talks to a homeserver is still testable. `MatrixMockServer` answers
like Synapse, and `crates/consort-matrix/tests/against_a_mock_homeserver.rs`
shows the pattern for login, restore, logout and token rotation. Reach for
`#[ignore]` only when a test genuinely needs something a container cannot have,
such as a live platform keyring, and say so in the ignore reason.

Anything that only a running Tauri app can produce, `State<'_, AppState>` above
all, belongs behind a one-line delegate. Put the logic in a plain function
taking `&AppState` and test that.

## AI assistance

Read the [AI-assisted development](README.md#ai-assisted-development) section of
the README. The summary is that AI is welcome and unread output is not, and the
line between them is whether you can explain and defend every line you are
submitting.

If a maintainer asks why you did something and the honest answer is that you did
not choose it, say so. That is a fine answer once, on a small change, while you
are learning the codebase. It is not a fine answer on a large one.

## Commit messages

Conventional commits:

```
feat: restore the session on startup
fix: clear the local session when server-side logout fails
docs: explain the matrix-sdk pin
refactor: move profile fetching out of the command layer
test: cover username normalisation
chore: bump the pinned toolchain
```

Subject in the imperative, under about 72 characters. Body for the why, wrapped,
if the subject does not cover it.

They are also the changelog. `CHANGELOG.md` is generated from them with
[git-cliff](https://git-cliff.org), which is why the type and the scope matter
beyond tidiness: a commit that is not a conventional one is left out of the
release notes entirely.

## Releasing

Four files carry the version and none of them reads another: `Cargo.toml`,
`app/package.json`, `app/src-tauri/tauri.conf.json`, and the placeholder in
`packaging/aur/PKGBUILD` that makepkg overwrites. Set all four, then:

```sh
git cliff --tag v0.1.3 --output CHANGELOG.md
git commit -am "chore(release): 0.1.3"
git tag -a v0.1.3 -m "0.1.3"
```

`--tag` is needed because the tag does not exist yet: without it everything
since the last release lands under an "Unreleased" heading. `cliff.toml` says
which commit types are listed and which are kept out.

## Licence

Consort is AGPL-3.0-only, inherited from `matrix-rust-rtc`. By contributing you
agree your work is licensed under the same terms. There is no CLA.

## Code of conduct

Be decent. Assume the other person is trying. Disagree about the code without
making it about the person. Maintainers will act on harassment.
