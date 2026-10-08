# Updating Consort from inside Consort

What #162 asked for, what it turned out to be, and the one thing still waiting on
a person.

Short version: the Windows installer updates itself, Linux packages do not and
must not, the hosting costs nothing, and none of it is live until the signing
keypair exists.

## The mechanism

`tauri-plugin-updater`, compiled in behind a Cargo feature. Three pieces:

1. **The artifact.** With `bundle.createUpdaterArtifacts` on, Tauri 2 reuses the
   NSIS installer as the update bundle and writes a detached signature beside
   it: `Consort_<version>_x64-setup.exe` and `.exe.sig`. The installer was
   already on the release page; the signature is the only new upload.
2. **The manifest**, at
   `https://consort-chat.github.io/consort/updater/latest.json`, published by the
   release run. `scripts/check-manifest.sh` is the shape it has to be.
3. **The signature over the artifact**, checked before a byte of it is executed.

The frontend never talks to the plugin. `@tauri-apps/plugin-updater`, which the
issue guessed at, is not a dependency and no capability is granted: the webview
has `core:default` and everything privileged goes through a Rust command, so the
checking and installing is `UpdaterExt` on the Rust side and the page only ever
sees an `update` event and two commands.

## Why Pages, and not the release page

The natural URL is `releases/latest/download/latest.json`. It 404s here, and that
was measured rather than assumed:

```
$ curl -o /dev/null -w '%{http_code}' \
    https://github.com/consort-chat/consort/releases/latest/download/latest.json
404
```

GitHub's `latest` is the newest release that is **not** a prerelease, and
`release.yml` publishes every 0.x release `--prerelease` on purpose. Pages gives
one URL that no release flag can move, which also means a backfill or a release
published out of order cannot break the update path.

A manifest committed to the repository and served from `raw.githubusercontent.com`
fails on ordering rather than on hosting: the manifest carries the artifact's
signature, so it cannot exist before the Windows build, and `release.yml`'s
version commit happens before it. It would need a second push to `main` after
the tag, leaving `main` carrying a file the tag does not.

## What it costs

Nothing, and the arithmetic matters more than the word.

| What | Where | Metered |
|---|---|---|
| The installer, about 15 MB, once per update | GitHub Releases | No. "There is no limit on the total size of a release, nor bandwidth usage." |
| The manifest, under 1 KB, once per poll | GitHub Pages | Soft 100 GB a month |
| Building and deploying | Actions, public repo | No |

The manifest is the only thing on a metered path. At roughly 700 bytes, 100 GB a
month is of the order of 100 million fetches. One install polls about 120 times a
month, so the soft limit sits somewhere around a million concurrent installs.
Pages' ten-builds-an-hour limit does not apply to a site published by an Actions
workflow, which is how this one is published.

If release-asset hosting ever changed, the manifest's `url` field is the only
thing that moves. That is the point of having an indirection.

## Signing

The private key is a credential. It is generated on Thomas's machine, it lives in
a password manager, and it is handed to CI as a secret and nowhere else.

```sh
cd app && pnpm tauri signer generate -w ~/.consort/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY          < ~/.consort/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD   # paste at the prompt
cat ~/.consort/updater.key.pub                     # goes in tauri.conf.json
```

Those two environment names are not a choice: `tauri build` reads exactly those,
and Tauri documents that a `.env` file is not read for them. The public half goes
in `tauri.conf.json` at `plugins.updater.pubkey`, replacing
`PUBLIC_KEY_NOT_SET`.

**There is no recovery.** The public key is baked into every shipped binary, so a
lost private key means no existing install can ever be updated again and
everybody reinstalls by hand, which is the failure this whole thing exists to
prevent.

Until both halves are in place the Windows job builds a plain installer and warns
which half is missing, and the manifest job does not run. A half-configured
signing setup therefore produces no manifest rather than one every client
refuses.

### requireSignedVersion

On, from the first release. The plugin's own documentation describes what it
stops: the manifest is fetched over TLS but is not itself signed, and the
signature covers only the artifact, so anybody able to serve a crafted manifest
can pair an inflated `version` with the `url` and `signature` of a genuine older
release and force a downgrade to a build with a known hole in it. With the flag
on, the version is read from the signature's trusted comment, which the signature
covers, and a mismatch is refused.

The documented cost of enabling it is that releases signed before the Tauri CLI
recorded the version are rejected. Consort has signed none, so that cost is zero.

It does put a floor under the CLI: **2.11.5** is the first release that writes
`version:` into the trusted comment, and `app/package.json` says so. The lockfile
slipping below that would not fail a build; it would publish a release every
client refuses after downloading it. `scripts/check-manifest.sh` is what catches
it, in the release run, before the manifest is served.

`allowDowngrades` stays off, so a release is offered only when its version is
strictly greater.

## Why Linux packages carry none of this

Not "not yet". The in-app updater must never touch a `.deb` or an Arch package,
and the reasons are concrete.

`tauri-plugin-updater` does handle both: `install_inner` dispatches on the bundle
type and `install_deb` shells out to `pkexec dpkg -i`. A `.deb` that `apt` is
tracking, overwritten from inside a chat client, is exactly what
[#47](https://github.com/consort-chat/consort/issues/47) exists to replace with a
repository. The Arch case is worse: `packaging/arch/PKGBUILD` builds with plain
`cargo build`, never through `tauri-bundler`, so the bundle-type marker is never
stamped, detection returns nothing, and the dispatch falls through to the
AppImage arm on a binary that is not an AppImage. There is no AppImage either,
and `README.md` says why.

So the discriminator is a **Cargo feature, not a runtime check.** Runtime
detection returns nothing on Arch, which is indistinguishable from a build made
from source, and a security decision resting on a magic string the bundler may or
may not have patched in is the wrong kind of load-bearing.

- `self-update` is off by default and enables the plugin as an optional
  dependency.
- Only the Windows release job passes `--features self-update`.
- `packaging/arch/PKGBUILD` and the `.deb` job are untouched, so neither can
  acquire it by accident, and nor can a contributor running `pnpm tauri build`.

A build without the feature has no updater code in it. Three small commands
remain, because `tauri::generate_handler!` takes one list: `updates_itself`
answers false, and the other two do nothing. The frontend asks that question
first and draws nothing at all when the answer is false. Not a disabled control
and not an "updates unavailable" notice, both of which invite a bug report about
something working as intended.

`createUpdaterArtifacts` is deliberately **not** in `tauri.conf.json`, because it
applies to every bundle and would make the `.deb` job and every local build
demand a signing key. It is an overlay, `tauri.updater.conf.json`, on the one
command line that needs it.

The manifest reinforces all of it by carrying `windows-x86_64` and nothing else,
and `scripts/check-manifest.sh` fails if a Linux or macOS key ever appears in one.

## What somebody sees

A one-line bar across the top of the window, above all three views, so it reaches
somebody stuck on a sign-in screen too. It names the version, offers Update and
restart, and offers Not now. Nothing is downloaded or installed without a press.
Dismissing hides that version for the session; a different version brings it
back.

### A call in progress

Consort is a voice client, so an update that restarts it mid-sentence is a defect.
The install is refused while a call is up and the bar says why rather than going
grey with no explanation.

The refusal is checked **twice**, and the second one is the one that matters. A
download takes seconds, somebody can join a channel inside them, and the Windows
installer exits Consort as soon as it is launched without going through the
shutdown that leaves a call. So the question is asked again with the bytes already
in hand, which is the last moment it can be asked at all.

That second check is why `updating::put_in_place` takes a closure instead of the
bytes. Reaching it needs a signed artifact and the key that signed it, so a test
cannot get there any other way, and an untested guard on this particular line
would be the worst one in the change to get wrong.

### When it looks

Thirty seconds after startup, then every six hours. The delay keeps a check from
competing with the first sync; six hours rather than minutes because a release
here is a weekly event and a tighter interval only costs somebody else's CDN.

## Failure modes

| Failure | What happens | Covered by |
|---|---|---|
| No network | Reported on the channel, bar stays undrawn, next poll tries again | Test, against a dead endpoint |
| Manifest does not parse | Same path, reported as unreadable rather than unreachable | Test, against a server answering with junk |
| No entry for this platform | Nothing offered | Test |
| Signature does not verify | Rejected inside the plugin before anything is written | Reasoned about, from the plugin's ordering. The crypto is not ours to test |
| Partial download | The signature is checked over the complete bytes, so a truncated download is a signature failure. There is no window in which half an installer runs | Reasoned about |
| Downgrade | Two independent guards: `allowDowngrades` off, and `requireSignedVersion` on | Tests for both, including a manifest that asks for an older release |
| A manifest that lies about the version | Refused: the signed version is compared against the announced one | `scripts/check-manifest.sh`, exercised against a signature made for a different version |
| Install refused mid-call | Nothing is handed to the installer | Test, and it asserts the installer was never reached |
| Packaged build | No updater code, and `updates_itself` is false | Test |
| A release broken for everybody | Not solvable in the client. The bar asks before installing, so a bad release reaches only people who pressed a button, and `release.yml` already gates on CI being green on the released commit. Withdrawing one means deleting its assets, which makes the manifest's `url` 404 and leaves every install on the version it has | Not tested. Stated |

## What is not proven yet

The end to end path. It needs a published, signed release, and nothing in this
change published one. The first release after the secrets exist offers nothing to
anybody, because nothing in the wild carries the feature. The second one is the
first real test.
