# Security audit, October 2026

What this asks: does installing Consort on somebody's machine expose them to
anything stupid. Not a compliance pass. Everything below is either a path from
an input somebody else controls to an impact, or it is recorded as a thing that
was checked and found clean.

- **Audited at** `efadb53`, the tip of `main` on 2 October 2026, version 0.8.0.
- **Method**: static reading of this tree, the pinned `matrix-sdk` and
  `matrix-rust-rtc` sources in the cargo store, plus `cargo audit` and
  `pnpm audit` run locally. Nothing was scanned, probed or attacked. No
  homeserver, SFU or machine other than this one was contacted, beyond the
  advisory databases the two audit tools read.
- **Not covered**: the camera and screen sharing paths. They are not on `main`
  (see [What the brief got wrong](#what-the-brief-got-wrong)), and auditing an
  open branch would be auditing something that is still moving.

## Summary

| | Finding | Severity |
|---|---|---|
| E1 | Room keys go to devices nobody vouched for, so a homeserver that adds a device reads the room | High |
| E2 | A message body reaches the notification daemon's markup parser unescaped | Low to medium |
| H1 | `timeline_attach_file` reads any path the page names, which turns one XSS into file exfiltration | Hardening |
| H2 | An SFU advertised over `http://` is accepted, and the identity token goes with it | Hardening |
| H3 | The Arch recipes build from a mutable git tag with no checksum and no signature | Hardening |
| H4 | The release workflow holds `contents: write` beside third party actions pinned by tag | Hardening |
| H5 | `unsafe impl Send` on the audio streams rests on call site discipline, not on a type | Hardening |
| H6 | No signed artifacts and no update mechanism, so a fix reaches only whoever notices | Hardening |
| H7 | A call participant can draw the deafened or away icon beside somebody else | Hardening |
| H8 | The SQLite store key is never zeroized | Hardening |

Three things were fixed here, under [Changed in this pull
request](#changed-in-this-pull-request). Everything else is written up and left
alone, because it needs a decision that is not an auditor's to take.

The short version of the good news: the IPC surface is 76 commands and all but
one of them are either parameterless or take an identifier that ruma parses. The
webview has `core:default` and nothing else. The content security policy is real
and restrictive. `formatted_body` is never handed back to an HTML parser. The
access token and the store key are in the platform keyring, the fallback is
`0600`, and the SQLite stores are encrypted. There is no telemetry, no panic
hook, no committed credential, and no deep link entry point. The detail is in
[Checked and found clean](#checked-and-found-clean), which is as much of the
answer as the findings are.

## What the brief got wrong

Three premises in the brief do not match the tree, and two of them change what
could be audited.

**PR #135 has not merged.** The brief says 135, 141 and 142 merged on 1 October.
141 and 142 did; 135 is still open, and so is 140. `main` has moved further than
the brief expected: #147 merged after 142 and is the tip.

**There is no `crates/consort-video`.** It arrives with #135. The brief asks for
the call path "in crates/consort-call, consort-audio and consort-video" and for
anything Linux-only to follow that crate's `cfg` gating. On `main` the crate does
not exist and the workspace does not list it (`Cargo.toml:3-7`), so the camera
and pixel format paths are not in scope and there was no gating pattern to
follow. Nothing added here needs one.

**CI does audit the frontend lockfile.** The brief asks whether the Advisories
job covers it. It does not, but the Frontend job does, at `ci.yml:77`:
`pnpm audit --audit-level=moderate`. The two halves are audited in two jobs
rather than one.

## Exploitable

### E1. Room keys go to devices nobody vouched for

**Severity: high.** The attacker has to be the homeserver, or hold the
credentials of somebody already in the room, because what they need is a device
on a member's account that the member never authorised. That is the attacker end
to end encryption exists to survive.

`auth.rs:328-378` builds every client, for both login and restore, and never
calls `ClientBuilder::with_room_key_recipient_strategy`. That method exists at
the pinned rev
(`matrix-sdk/src/client/builder/mod.rs:493`), and the field it sets defaults to
`CollectStrategy::AllDevices`
(`matrix-sdk-crypto/src/session_manager/group_sessions/share_strategy.rs:50-51`),
whose own documentation in that file reads "Not recommended, per the guidance of
MSC4153".

So when Consort sends a message in an encrypted room, the megolm session is
distributed to every device of every joined member, whether or not that device
is signed by its owner's cross-signing identity.

The path from input to impact:

1. A homeserver operator adds a device to a room member's account, which a
   homeserver can do unilaterally: `/keys/query` is the homeserver's answer about
   who owns which keys, and nothing but cross-signing contradicts it. The same
   step is available to anybody who learns a member's password.
2. That device is not signed by the member's cross-signing identity. No client
   that follows MSC4153 would send it a room key.
3. Consort sends it the room key anyway, with the next message it writes.
4. The holder decrypts every message Consort sends under that megolm session,
   and under every session after it, because a rotation redistributes to the same
   recipients. It ends when the device leaves the member's device list, not when
   the session changes.

The sharp part is that Consort already holds its **voice** path to the standard
it does not hold text to. Media keys are distributed with
`CollectStrategy::IdentityBasedStrategy`
(`matrix-rtc-bridge/src/sdk.rs:539` at the pinned rev), `calls::readiness` exists
to predict that strategy's verdict, and `calls::can_join` at `gate.rs:70` refuses
an encrypted room's call outright when this session could not pass it. A client
that will not let you into a voice channel unverified, and will hand your
messages to an unverified device, is inconsistent in the direction that matters.

**Recommendation, not applied.** Pass
`CollectStrategy::IdentityBasedStrategy` in `base_builder`. That file is the
right place: its own doc comment says settings must not drift between login and
restore, and this is such a setting.

**Why it is not applied here.** It trades functionality for safety, which this
audit was told to leave alone, and the trade is visible: anybody in a shared room
whose own client is not cross-signed stops being able to read new messages, with
the failure landing on them rather than on whoever changed the setting. Element
ships the same choice as "exclude insecure devices" and makes it a setting.
Worth knowing before deciding: switching protects nothing already sent, the key
backup is a separate copy with separate reach, and the direction of the mistake
is asymmetric, since being too strict costs somebody a verification prompt and
being too loose costs them the room.

### E2. A message body reaches the notification daemon's markup parser unescaped

**Severity: low to medium**, and the ceiling depends on the notification daemon
rather than on Consort.

`notifications.rs:70-75` states the rule out loud:

> The plain-text body, never the HTML. A notification is drawn by the desktop
> rather than by us, and handing a notification daemon markup it may or may not
> render is how a message arrives with tags in it.

The rule is right and the implementation does not achieve it. Choosing `body`
over `formatted_body` avoids repeating the HTML Consort itself parsed; it does
nothing about the body, because `body` is a string the sender writes and may
hold the same characters. `notifications.rs:197` copies it through,
`notify.rs:149` returns it unchanged, and `notify.rs:215` hands it to
`notify_rust::Notification::body`. Nothing escapes `&`, `<` or `>` anywhere on
that path. `notify.rs:214` has the same shape for the summary, which is built at
`notify.rs:144-148` out of the sender's display name and the room's name, both
also chosen by other people.

The path from input to impact:

1. Somebody in a room sends a message whose `body` is markup, for example
   `<b>Session expired.</b> <a href="https://attacker.example/">Sign in</a>`.
2. The push rules call it worth a notification and Consort is not the window in
   front, so `draw` runs (`notify.rs:205`).
3. The freedesktop notification specification allows a body to carry markup
   when the daemon advertises `body-markup`, and the daemon decides that, not
   Consort. Which daemons do was not checked here, because checking means asking
   a running one and this audit touched nothing outside the tree. On any daemon
   that does, the sender decides how the notification reads.

What that is worth: the certain part is that a sender controls the formatting
and can make a notification misrepresent what was said. The escalations are
daemon dependent and were **not** verified here, because verifying them means
driving a notification daemon and this audit was static: a daemon that makes
`<a href>` clickable gives a sender a link that bypasses `checked_link`, the
allowlist every other link in the application goes through, and a daemon that
implements the specification's body images gives them an outbound fetch from the
reader's machine, which is the exact thing `FormattedBody`'s `mxc://` only rule
and the content security policy exist to prevent. Both of those surfaces sit
outside the webview, so neither the policy nor the renderer is in the way.

**Recommendation, not applied.** Escape `&`, `<` and `>` in both halves before
the call, behind `#[cfg(unix)]`, and ideally only when
`notify_rust::get_capabilities()` reports `body-markup`.

**Why it is not applied here.** Which of the three available answers is right is
a decision rather than a fix. Escaping unconditionally shows `&lt;` to somebody
on a daemon that does not do markup. Escaping on Windows is wrong outright, since
that backend takes no markup and would display the entities. Querying
capabilities is correct and costs a D-Bus round trip per notification, which is
a judgement about a hot path. One line of the three is yours to pick.

## Hardening

No attack path today. Each of these is the code depending on something it
should not.

### H1. `timeline_attach_file` reads any path the page names

`commands.rs:1996` takes `path: String` from the webview and
`commands.rs:943` hands it to `attaching::read` (`attaching.rs:88`), which reads
the file and uploads it to a room. The path round trips through the page by
design: `attachment_pick` opens the picker in Rust and answers with
`attaching::Chosen`, whose `path` field (`attaching.rs:47`) is documented as
"this side's own string and the page does nothing with it but hand it back".

Nothing today can make the page hand back a different one. There is no HTML
injection sink in the frontend, the policy forbids inline and remote script, and
`FormattedBody` never returns a string to a parser. So this is not an exploit, it
is the blast radius of one: any future script execution in the webview becomes
"read any file this user can read and upload it to a room the attacker is in",
rather than "read what is on screen".

The module already contains the shape that closes it. A pasted screenshot is
held in `AppState` and addressed rather than handed over, and `attaching.rs:98-101`
says why in as many words. Doing the same for a picked file, keeping the path in
`AppState` and giving the page a token, removes the primitive. It changes the
command's signature, so it is written up rather than done.

### H2. An SFU advertised over `http://` is accepted

`discovery.rs:114` accepts `http://` as well as `https://` for the LiveKit
service URL read out of a server's discovery document, and the comment argues
that the document arrived over TLS so this is "the same trust as the homeserver,
not less". The first half is true and the conclusion does not follow. That URL is
where a Matrix OpenID token is presented in exchange for an SFU token
(`matrix-rtc-livekit/src/lib.rs:104-134` at the pinned rev). Over plain HTTP,
anybody on the path takes both: the OpenID token, which proves this Matrix
identity to a third party, and the LiveKit JWT, which is admission to the call.
With plaintext signalling, an active attacker is also between the DTLS
fingerprints, so in an unencrypted room the media is theirs as well. In an
encrypted room the per-frame GCM layer still holds.

Nobody attacker-controlled chooses this. The document is served over TLS from
the server named in the user's own ID, so the value is the operator's, and the
realistic way it goes wrong is a `http://localhost:8080` copied out of a
development setup into a production `.well-known`. That is why it is here rather
than above.

**Recommendation.** Require `https://` for anything that is not a loopback host.
The repository's own demo backend is the reason the door is open, and loopback
keeps it working. Written up rather than applied because it trades a working
configuration for safety.

### H3. The Arch recipes build from a mutable git tag

`packaging/arch/PKGBUILD:50-51` fetches
`git+$url.git#tag=v$pkgver` with `sha256sums=('SKIP')`, and the comment above it
reasons that "a tag is a name for one commit", so there is nothing honest to
checksum. A tag names one commit at a time. A lightweight tag can be moved, and
nothing in the recipe would notice: no `validpgpkeys`, no signature check, no
commit pin. Anybody who takes the GitHub repository can retarget `v0.8.0` and
every later `makepkg` builds what they point at.

`packaging/aur/PKGBUILD:93-94` is `pkgname=consort-git` and tracks `main` with
`SKIP`, which is what a `-git` package is supposed to do. It is not a finding.

**Recommendation.** Pin `#commit=<sha>` in the release recipe, which
`scripts/set-version.sh` already rewrites and could rewrite one more field in, or
sign the tags and add `validpgpkeys`.

### H4. The release workflow holds `contents: write` beside third party actions

`release.yml:81-84` grants `contents: write` for the whole workflow, and the job
that builds and uploads the Windows installer uses
`dtolnay/rust-toolchain@stable` (`release.yml:347`) and
`Swatinem/rust-cache@v2` (`release.yml:349`). Both are pinned by a mutable ref.
Whoever can move those refs runs code in a job that can write releases.

Worth saying what the workflow already does right, because it is unusual: git
cliff is fetched as a binary and verified against a pinned SHA512
(`release.yml:134-147`) rather than taken as an action, no workflow uses
`pull_request_target` or `workflow_run`, and `ci.yml:23-25` says the first of
those is deliberate and must stay that way. No `run:` step anywhere interpolates
`github.event.*`.

**Recommendation.** Pin the two third party actions by commit SHA.

### H5. `unsafe impl Send` rests on call site discipline, not on a type

`cpal_host.rs:79` and `cpal_host.rs:185` assert `Send` for two structs whose only
real field is a `cpal::Stream`, which is `!Send`. The comments argue the promise
is kept "structurally rather than by convention: nothing else in the crate can
name this type".

Naming the concrete type is not the way it would escape. `capture.rs:68` declares
`pub trait CaptureStream: Send` and `playback.rs:79` the same for
`PlaybackStream`, so the `Box<dyn CaptureStream>` that `open` returns **is**
`Send` and any holder may move it to another thread. What keeps it on the audio
thread is that `thread.rs` happens to store it in a local and never sends it.
That is the convention the comment says it is not relying on. Dropping a cpal
stream on a thread that did not create it is undefined behaviour on the hosts
that care, CoreAudio among them.

Nothing attacker-controlled is anywhere near this, and nothing moves the box
today.

**Recommendation.** Drop `: Send` from both traits. The boxes then become
`!Send`, the compiler enforces what the comment claims, and nothing in the
current arrangement has to change, because the streams never cross a thread
boundary anyway. It is a change to two public traits, so it is written up.

### H6. No signed artifacts and no update mechanism

There is no updater. `tauri-plugin-updater` is not a dependency, `tauri.conf.json`
carries no `updater` key, and there is no `pubkey` or endpoint anywhere in the
tree. Nothing is signed either, which `release.yml:56-62` says outright and the
README repeats.

For a desktop application handed to friends, the consequence is the part worth
writing down: there is no mechanism by which a security fix reaches anybody. A
user who installed 0.8.0 keeps running 0.8.0 until they happen to look. The deb
and the Arch packages are not served from a repository, so no `apt upgrade` or
`pacman -Syu` finds them either, and the `consort-git` AUR package is the only
route that updates by itself.

This is not an attack. It is the thing that decides how much every other finding
costs, and it argues for pinning the release recipe (H3) before it argues for
anything else.

### H7. A call participant can draw an icon beside somebody else

`notices.rs` carries deafen and away over the call's own LiveKit data channel,
and a notice names its own sender in a `member_id` field it also writes. The code
documents this in full at `notices.rs:198-230`, including that "anybody in the
call can send a notice carrying somebody else's membership id", and mitigates it
with `contested`: a membership claimed by two participants is dropped from both
flags, so a forgery aimed at somebody who is also in the call and running Consort
cancels out at the next roster change.

The gaps the code already names are real. Somebody running Element Call sends no
notice, so a claim about them meets no opposition, and the cancellation only
lands when the roster next changes. Impact is a wrong icon: a person shown as not
listening when they are. No data, no audio, no credential. It is here rather than
in Informational because the attack is real, the impact is merely small, and the
fix is identified upstream already.

### H8. The SQLite store key is never zeroized

`store_key.rs:41` holds the key in a plain `[u8; 32]` with no `Drop`. The file
goes to the trouble of a hand-written `Debug` so the key cannot reach a log
(`store_key.rs:47-51`), which is the same concern one step earlier; a core dump
or a swapped page is the step it does not cover. `Credentials` in `auth.rs:37-50`
redacts for the same reason and has the same gap.

Cheap to close with `zeroize` on `Drop` for both. Not applied because it adds a
dependency, which is a call for whoever maintains the graph.

## Informational

Worth knowing. No action implied.

- **`mailto:` is in the link allowlist.** `commands.rs:2071` admits three
  schemes, so a link in somebody else's message can open the reader's mail
  composer with headers the sender chose. The historic `?attach=` class is fixed
  in mainstream clients, and the allowlist is doing its job in refusing
  everything else. Named because it is the edge of a deliberate decision.
- **The umask test mutates process-global state.** `atomic.rs:195-202` sets the
  umask to 0 and restores it, inside a test binary whose tests run on several
  threads. It can perturb a sibling test's file modes and a sibling can perturb
  it. The test is a good test and the race is in the harness, not the product.
- **The server field accepts a URL.** `normalise_server` at `auth.rs:380` trims
  and rejects whitespace and nothing else, so a typed `http://host` reaches
  `server_name_or_homeserver_url` and the password goes out in the clear. The
  field asks for a bare server name and says so on screen
  (`LoginScreen.tsx:96-100`), and the ordinary path is `.well-known` discovery
  over TLS, so this needs somebody to type a scheme they were not asked for.
- **One upstream note can be retired.** `PLAN-call-encryption.md` records that
  `matrix-rtc-livekit/src/call.rs:437`'s `unwrap_or(true)` covers only the `Err`
  case, so its stated rule that unknown counts as encrypted "is not what its code
  does", and marks it worth fixing upstream. The code shape is as described and
  the case is unreachable: `Room::latest_encryption_state` calls
  `request_encryption_state` first (`matrix-sdk/src/room/mod.rs:1103-1107`),
  which marks the state synced before returning
  (`mod.rs:1078-1081`), and `RoomInfo::encryption_state` answers `Unknown` only
  when that flag is unset (`matrix-sdk-base/src/room/room_info.rs:767-774`).
  Either the request succeeds and the answer is `Encrypted` or `NotEncrypted`, or
  it fails and `unwrap_or(true)` catches it. Consort's own `Encrypts::Unknown`
  arm in `gate.rs` is belt beside those braces and is still worth keeping.
- **A custom emoji tells its own server it was looked at.** `mxcUrl` admits any
  `mxc://`, including one naming somebody else's server, and the fetch goes
  through this account's homeserver, which federates for it. The reader's IP does
  not leave, which is what the comment claims and what matters; the remote server
  does learn the media was wanted. Standard for Matrix and not specific to this
  client.
- **`cargo audit`: no vulnerabilities.** 864 crates. Thirteen warnings before
  the `chacha20` bump below and twelve after it, all of them now unmaintained or
  unsound rather than yanked, and almost all of them Tauri's GTK3 bindings. One
  advisory is ignored, `RUSTSEC-2026-0292`, and `.cargo/audit.toml:14-38` carries
  a reason, the reachability argument, why a version bump cannot fix it, and what
  clears it. The version claim was verified against the lockfile:
  `imbl-sized-chunks 0.1.3` is still what the graph resolves, and no imbl version
  the pinned matrix-sdk fork accepts asks for the patched `0.2`. The
  reachability argument reads correctly and was not independently proved, which
  would mean enumerating every type that reaches an imbl collection.
- **`pnpm audit`: clean** at every level, not just `moderate`.

## Checked and found clean

A clean result is a result. These were looked at and no path was found.

### Tauri configuration and capabilities

- The content security policy exists and is restrictive
  (`tauri.conf.json:27`). `default-src 'self'` with no `script-src` override, so
  no inline and no remote script. `'unsafe-inline'` appears for `style-src`
  only, which React's inline styles need. `base-uri 'self'` and
  `form-action 'none'` are both set. `object-src` and `frame-src` are not named
  and fall to `default-src 'self'`.
- `data:` in `img-src` is load bearing, not slack: `LoginScreen.css:47` and
  `OpeningPane.css:35` draw the grain texture from an inline SVG.
- No dangerous option is set anywhere.
  `dangerousDisableAssetCspModification`, `withGlobalTauri`, `pattern`,
  `assetProtocol` and `freezePrototype` are all absent, so each takes its
  default, and for the first two the default is the safe one.
- Devtools are off in a release build. The capability grants `core:default`,
  which includes `core:webview:allow-internal-toggle-devtools`, but that command
  is compiled only under `debug_assertions` or the `devtools` feature
  (`tauri-2.11.5/src/webview/plugin.rs:180,253`) and `app/src-tauri/Cargo.toml:51`
  asks for `tray-icon` alone.
- There is one capability file, it names one window, and it grants
  `core:default` and nothing else (`capabilities/default.json:5-6`). No `fs`,
  `shell` or `http` scope exists to be too wide: those plugins are not
  registered for the webview at all. `tauri-plugin-dialog` and
  `tauri-plugin-clipboard-manager` are initialised in `lib.rs:165-166` and
  reached only from Rust; with no permission granted, the page cannot invoke
  their commands.

### The IPC surface

- 76 commands. Every `#[tauri::command]` in the tree is registered in
  `generate_handler!` (`lib.rs:269-346`) and every registered name exists, so
  there is no orphan and no unreachable command. The 77th match for the
  attribute is inside a doc comment at `commands.rs:6`.
- One command takes a filesystem path, and it is H1. One takes a URL, and it is
  allowlisted by `checked_link`. One takes a filename, and that is what this PR
  fixed.
- Nothing shells out. The only process spawn is `open::that_detached` in
  `open_link` (`commands.rs:2111`), which receives `Url::as_str()` of an
  already-parsed URL whose scheme is one of three, so there is no leading dash
  and no argument to inject.
- Every other command is parameterless, takes a bool or a bounded number, or
  takes an identifier. Identifiers are parsed, never interpolated:
  `RoomId::parse`, `EventId::parse` and `UserId::parse` appear 29 times in
  `consort-matrix` and `room_of` at `membership.rs:235` is the shape they all
  follow. No API URL is built by string formatting anywhere. The one formatted
  URL in the tree is `discovery.rs:72`, whose input is the authenticated user's
  own ruma-validated server name.
- `preview_application_scale` clamps through
  `settings::application_scale_within_range` (`settings.rs:212`) before the zoom.

### Custom schemes and deep links

- There is no deep link handling and no registered external scheme. No
  `tauri-plugin-deep-link`, no `MimeType=` and no `%u` or `%U` in
  `packaging/linux/consort.desktop:26`, so nothing outside the application can
  cause the binary to be launched with an argument.
- `tauri-plugin-single-instance`'s callback discards the second process's argv
  and cwd (`lib.rs:168`) and only raises the window.
- The one custom scheme, `consortmedia`, is internal and the webview is its only
  client. Its path is base64 of the event's own media source, decoded at
  `media.rs:75-78`, and a path that is not that is a 400. A forged handle buys
  the page a fetch of an `mxc://` the account could already fetch.
- The scheme can never serve HTML or script. The content type is sniffed from
  the bytes, never taken from the sender, and the sniffers answer only from a
  fixed list of image, video and audio types (`consort-matrix/src/media.rs:21-121`).
  Anything unrecognised becomes an error and a 404 (`timeline/media.rs:109`).
  The sender's own `mime` is kept only when it starts with `image/` or `video/`
  (`timeline/facts.rs:769-774`) and is a frontend hint, never the served header.
- The range parser is bounds-correct and every arm is tested
  (`media.rs:97-157`). The cache is bounded by count and by weight
  (`media.rs:60-67`).

### Rendering untrusted content

- `formatted_body` is parsed into an inert `DOMParser` document and rebuilt from
  an allowlist of 22 tags (`FormattedBody.tsx:20-52,232-241`). Nothing off the
  wire returns to the parser that owns the page. There is no
  `dangerouslySetInnerHTML`, `innerHTML`, `insertAdjacentHTML`, `eval`,
  `new Function`, `document.write`, `iframe` or `srcdoc` anywhere in
  `app/src`.
- Four attributes are read by name and each is checked. An anchor's `href` must
  parse as `http`, `https` or `mailto` (`FormattedBody.tsx:61-73`); an image's
  `src` must be an `mxc://` (`api.ts:2602-2605`); `alt` and `title` are escaped
  by React like any text.
- An unknown tag is dropped and its children kept, which is the safe half: a
  `<script>` becomes its own text, visible and inert.
- Plain text linkifying requires a scheme and matches only `https?://`
  (`links.ts:30`), and the result is re-checked in Rust. The frontend list and
  the Rust list are deliberately separate and the comments on both say so.
- No clickable link follows in place. `ExternalLink` cancels the event and calls
  the command (`ExternalLink.tsx:39-46`).
- No CSS injection sink. Every dynamic `style` prop in the tree carries a number
  computed locally, and there is no `setProperty` or `cssText` anywhere.
- A hostile image reaches the system WebKit decoders through an `img` or `video`
  element, which is the same exposure every webview application has and is the
  distribution's webkit2gtk to patch. Consort's own parsers over attacker bytes
  are the sniffers and `pixel_size`, and every one of them is length-checked
  before it indexes. There are three slices with arithmetic in production code
  across the whole tree and all three are provably in range.

### Keys, tokens and verification

- The access token, the refresh token and the SQLite store key go to the
  platform keyring (`secrets/mod.rs:100-117`). The fallback when no keyring
  answers is a file created `0600` at creation time rather than chmod-ed
  afterwards, in a `0700` directory, replaced atomically with an `fsync` before
  the rename and one after (`atomic.rs:29,37-64,95-117`). Which store was used
  is logged and shown in the UI rather than assumed.
- The SDK's state and crypto stores are encrypted. `base_builder` passes a
  32-byte key from the OS CSPRNG as the store key rather than a passphrase
  (`auth.rs:330-332`, `store_key.rs:37-80`), and `store_key.rs:22-30` is honest
  about what that does and does not protect against.
- `session.json` holds no secret, by construction and by a test
  (`session.rs:66-78`, `session.rs:785-795`).
- This session's own verification is enforced where it is claimed to be. The
  voice gate is live rather than a snapshot, refuses an encrypted room's call
  from a session that cannot distribute media keys, and treats "could not
  establish whether the room encrypts" as encrypted
  (`gate.rs:70-93`, `readiness.rs`). The plan documents say this should happen
  and it does.
- Before verification, history does not decrypt, which is the SDK's behaviour
  and what `PLAN-verification.md` describes. What the plans do **not** promise,
  and what therefore is not a documentation mismatch, is anything about other
  users' devices. That gap is E1.
- No sender trust is drawn in the timeline on `main`. PR #148 adds it.

### The call path

- Media frame encryption follows the room and cannot be turned off by a peer.
  An encrypted room gets GCM frame encryption, an unencrypted room gets none
  because MSC4143 forbids it there, and a failure to ask counts as encrypted
  (`matrix-rtc-livekit/src/call.rs:433-440` at the pinned rev). The reachability
  of the one suspicious arm is in Informational above.
- Consort never handles a TURN credential. The only credential it obtains is a
  short-lived LiveKit JWT, in exchange for a Matrix OpenID token the homeserver
  mints, presented to the focus's token endpoint
  (`matrix-rtc-livekit/src/lib.rs:104-134`). ICE and TURN servers arrive inside
  the SFU session. Neither token is logged anywhere.
- The SFU address cannot be set from the webview. `service_url_fallback` is read
  from the on-disk settings file and has no setter command
  (`settings.rs:250-281`, `commands.rs:1305-1308`), which is deliberate and
  documented.
- Reading a server's discovery document is bounded in size and in time: 64 KB
  with a chunked read rather than `text()`, and a 5 second timeout inside the
  join budget (`discovery.rs:44-63`, `livekit.rs:84,178-196`). It reuses the
  SDK's HTTP client rather than opening one with its own TLS setup.
- Remote input on the data channel is a 4-field JSON struct, version-checked,
  and anything that does not decode is dropped without a log line
  (`notices.rs:117-120`). The one thing it says that cannot be checked is
  covered in H7.

### Secrets in the wrong place

- No committed credential. The `syt_` strings are fixtures, several of them in
  assertions that the value must **not** reach disk (`session.rs:434-435`), the
  passwords are `hunter2`, and `testing/synapse/up.sh:13` is a throwaway
  Synapse bound to `127.0.0.1:8008` whose own header says never to reuse it
  (`docker-compose.yml:1-8,24-26`). Its generated state is gitignored.
- No secret reaches a log. All 150 `tracing` call sites were read; the fields are
  errors, identifiers, enum states and fixed strings. `Credentials` and
  `StoreKey` have hand-written redacting `Debug` impls, `MatrixSession` and
  `SessionTokens` redact upstream, and a test asserts a rendered `StoredSession`
  never contains a token. No `panic!`, `expect` or `assert!` message
  interpolates a secret. No `console.*` call prints message content or a
  credential; `RecoveryKey.tsx:36-39` logs the error text and never the typed
  key, and the error variants it can produce never quote the input
  (`error.rs:221-262`).
- No telemetry, no analytics, no crash reporter, no panic hook. The only
  outbound request that is not the Matrix SDK's own is the `.well-known` read
  above. There is no `fetch`, `XMLHttpRequest` or `sendBeacon` in `app/src`.
- The hygiene job gates on three content rules, each failing the build: no em or
  en dashes, no private homeserver name, no `dbg!` or `println!("DEBUG`
  (`ci.yml:282-361`). None of them greps for secret patterns, which is worth
  knowing but is what `.gitignore:28-33` and the keyring are for.

### `unsafe` and FFI

Five occurrences in first-party code, two of them test-only. All five carry a
stated invariant; four establish it and the fifth is H5.

| Where | What | Invariant |
|---|---|---|
| `cpal_host.rs:79` | `unsafe impl Send for OpenStream` | Never moved off the audio thread. See H5. |
| `cpal_host.rs:185` | `unsafe impl Send for PlayingStream` | The same. |
| `renderer.rs:45,57` | `unsafe fn configure`, `set_var` | No other thread exists yet. |
| `lib.rs:140` | the call to it | Holds: nothing above it in `run` spawns a thread. |
| `atomic.rs:195-215` | `umask` via `extern "C"` | Test-only, `cfg(unix)`. See Informational. |

`consort-call` contains no `unsafe` at all. There is no `#[link]`, no
`#[no_mangle]`, no `libloading`, no bindgen and no raw pointer dereference in
first-party code. The one FFI declaration is the test-only `umask`. The
appindicator library that `tray-icon` opens with `libloading` is a dependency's
business and `tray.rs:113` wraps the tray construction in `catch_unwind` because a
missing library panics rather than failing to link. No crate sets
`forbid(unsafe_code)` and there is no `[lints]` table in any manifest.

### Packaging

- No maintainer script anywhere. No `postinst`, `preinst`, `prerm`, `postrm`,
  and no Arch `.install`. Nothing runs as root at install time.
- Every installed file is `0644` except the binary at `0755`, all of them under
  `/usr`, none setuid, setgid or world-writable
  (`packaging/arch/PKGBUILD:94-110`, `packaging/aur/PKGBUILD:192-211`). Nothing
  is written to `/etc`, `/opt`, `/var` or a home directory at build or install
  time; the build keeps its cargo and pnpm caches under `$srcdir` on purpose.
- No systemd unit, timer, socket, udev rule, polkit policy, D-Bus service file
  or PAM config is shipped or installed.
- No step pipes a download into a shell.

### Release

- Artifacts are a Windows NSIS installer, a `.deb` and an Arch package. They are
  unsigned and unchecksummed, which the workflow says in its own header. That is
  H6, and it is a known position rather than an oversight.
- Permissions are `contents: write` and `actions: read` and nothing broader. No
  `id-token`, no `packages`.

## Changed in this pull request

Three changes. The first two were written test first, watched failing for the
right reason, and mutation checked. The third is a lockfile pin, which has no
unit test to write and is checked by the tool that reported it.

**The Save As dialog no longer opens on a name a stranger wrote.**
`timeline_media_save` passed the attachment's name straight to
`set_file_name`, and that name is `filename` or `body` out of the event, which
the sender writes. A filename entry in a native dialog resolves a relative path,
so a message named `../../.bashrc` offered to save somewhere nobody chose, one
confirming click away. `attaching::suggested_name` (`attaching.rs:106`) now keeps
only the last component, over separators of either platform because the sender's
is unknown, drops control characters, and falls back to `attachment` when
nothing usable is left. Whether a given platform dialog would actually have
resolved the traversal was not tested, since that needs a desktop; the name had
no business reaching it either way. Four tests, and three mutations each
reddening exactly the test that guards them, out of 448.

**The content security policy no longer admits Tauri's asset protocol.**
`img-src` listed `asset:` and `http://asset.localhost`. The protocol serves
nothing unless `assetProtocol.enable` turns it on, which the config does not,
and nothing in the application calls `convertFileSrc` without naming the media
scheme. So the policy was granting a scheme no part of this build can reach, and
the next person to enable the protocol would have found it pre-admitted rather
than deciding to admit it. The new test ties the two together rather than
banning the string, so enabling the protocol later makes the test agree instead
of fail. One test, 1531 frontend tests still green, and the mutation reddens
only it.

**`chacha20` moves off a yanked release.** `cargo audit` reported
`chacha20 0.10.1` yanked. It arrives as `rand 0.10.2`'s ChaCha core, under
`matrix-sdk-store-encryption`, so it sits beside the crypto rather than in it;
the AEAD is `chacha20poly1305` on `chacha20 0.9.1`, which is not yanked. 0.10.2
is published and not yanked. `cargo update -p chacha20@0.10.1 --precise 0.10.2`
moves one version and one dependency edge and nothing else, which the lockfile
diff shows. `cargo audit` goes from 13 warnings to 12 and the yank is the one
that goes. The eight hand-pinned livekit crates are untouched, which is the
thing the root manifest warns a bare `cargo update` would break. There is no
unit test to write for a lockfile pin, so the check that failed before and
passes after is `cargo audit` itself, and `cargo build -p consort-app` compiles
the new version in the real graph.

## Collisions with open work

None. PRs #135 and #140 both touch `Cargo.lock` and `app/src/lib/api.test.ts`,
which are two of the files changed here, and the hunks do not overlap:
theirs are at `Cargo.lock` 963, 1035, 1045, 3033 and 4418 and in the test file at
lines 31 to 33 and 1240 to 1275, and these are at `Cargo.lock` 806 and 5707 and
at the end of the test file. No open PR touches `tauri.conf.json`,
`app/src-tauri/src/attaching.rs` or `app/src-tauri/src/commands.rs`'s save path.
