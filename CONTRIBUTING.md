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

See [docs/BUILDING.md](docs/BUILDING.md) for the prerequisites and for
Windows, which needs a page of its own.

The short version:

```sh
cd app
pnpm install
pnpm tauri dev
```

`pnpm tauri` sweeps the Cargo target directory before it builds, because cargo
orphans artifacts it never collects and the directory does not stop growing on
its own. Install cargo-sweep so the size cap has teeth:

```sh
cargo install cargo-sweep
```

[docs/BUILDING.md](docs/BUILDING.md#the-target-directory-and-keeping-it-bounded)
has what it removes and how to tune it.

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

`.github/workflows/release.yml` is separate and runs on a pushed `v*` tag: it
writes that release's notes with git-cliff and attaches a Windows installer
built from the tagged commit. It also takes a tag by hand, so a release made
before it existed can be given both without moving anything.

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

```sh
scripts/release.sh
```

That is the whole of it. It refuses to run on a dirty tree or off `main`, and
then does the five things nobody should be doing by hand.

**Nobody picks the version.** git-cliff reads the commits since the last tag
and answers with the next one: a `feat` moves the minor, a `fix` moves the
patch, a `!` or a `BREAKING CHANGE:` footer moves the major. `git cliff
--bumped-version` is what the script asks, and you can ask it yourself at any
time to see where the next release would land. The consequence is worth being
awake to: a `feat` that should have been a `fix` moves the minor number and
there is no taking it back once the tag is pushed.

**Six files carry the version and none of them reads another:** `Cargo.toml`,
`app/package.json`, `app/src-tauri/tauri.conf.json`, the placeholder in
`packaging/aur/PKGBUILD` that makepkg overwrites, `pkgver` in
`packaging/arch/PKGBUILD`, which is not a placeholder but the name of the
commit that package builds, and the `.rpm` filename in the README. The script
writes all six and refreshes `Cargo.lock` so its four `consort-*` entries
follow.

**The changelog and the tag message both come from the commits.**
`git cliff --tag` is what puts the new commits under the version about to
exist rather than under an "Unreleased" heading, and `cliff.toml` says which
commit types are listed and which are kept out. The same notes go into the
annotated tag, so `git show v0.2.0` says what changed.

**Nothing is pushed.** The script prints the two commands and stops. Pushing
the tag to `origin` is what runs `.github/workflows/release.yml`, which writes
the release page from the same notes and hangs three builds off it: a Windows
installer, a `.deb` built on Debian 12 so it runs on more than the newest
Ubuntu, and an Arch package built by `makepkg` from `packaging/arch/PKGBUILD`.
The two Linux ones are workflows of their own, so either can be rebuilt for a
tag on its own from the Actions tab.

## Licence

Consort is AGPL-3.0-only, inherited from `matrix-rust-rtc`. By contributing you
agree your work is licensed under the same terms. There is no CLA.

## Code of conduct

Be decent. Assume the other person is trying. Disagree about the code without
making it about the person. Maintainers will act on harassment.
