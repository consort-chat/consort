# Plan: end-to-end encryption verification

Status: **Phases 0 to 5 done, Phase 6 next.** Every API named here was checked
against the pinned SDK rev rather than recalled, and the file and function
names are the ones to create.

This is the milestone after authentication and before voice. Not by preference:
MSC4153 requires a cross-signed device to join an encrypted call, so the voice
layer cannot be tested at all until a Consort session can be verified. The same
work is what lets encrypted room history decrypt, which is the more visible
half.

It is also what makes the authentication milestone finishable. Signing in
currently produces a session that cannot read anything and that no call will
accept, which is an account rather than a usable client.

## What "verified" has to mean by the end

1. Consort's device can be verified by emoji (SAS) against another client the
   user is already signed into, in **both** directions. Consort accepts a
   request that Element started, and Consort starts one that Element accepts.
2. Consort can be verified with a recovery key when the other device is not to
   hand.
3. After either, Consort holds the cross-signing private keys and the megolm
   backup key, so it can decrypt existing history and verify other devices
   itself.
4. The UI states plainly whether this session is verified, and does not lie
   about it while it does not know.

## What is already true

Two things worth knowing before planning around them, both checked against the
pinned SDK rev.

**Login will not reset an existing cross-signing identity.** `auth.rs` sets
`auto_enable_cross_signing: true`, which sounds like it might clobber the
identity a user already has and un-verify every other device they own. It does
not. The SDK routes that setting through `bootstrap_cross_signing_if_needed`,
which checks `get_user_identity().is_none()` first and does nothing when an
identity exists.

**Backup and recovery setup already run at login.** The same task calls
`backups().setup_and_resume()` and `recovery().setup()`. Phase 5 is wiring up
what those produce, not starting from nothing.

## Risks, largest first

**There is no sync loop.** Consort has never called `client.sync()`.
Verification is driven entirely by to-device events, which arrive only through
sync. Nothing from Phase 2 onward can work until this exists, and it is
invisible from the current code: everything today is request and response
through a Tauri command, and this is the first thing that is not.

**The IPC is request/response only.** `api.ts` is four `invoke` wrappers and
nothing else. `grep -rn emit app/src-tauri/src` returns nothing. Verification is
push-driven, so this is new plumbing in both directions rather than an
extension of what is there.

**The protocol needs two live clients to test.** The state mapping and the
error handling are unit-testable and the request arriving is testable against
the mock server, but the SAS handshake itself is real olm between two real
devices and no mock produces it. Phase 0 therefore ships a test harness, not
just a feature.

**Timing is part of the protocol.** A verification request expires after ten
minutes, either side can cancel at any point, and both have to be online at the
same time. Expiry and cancellation are states to design for, not error paths to
bolt on afterwards.

## Design decisions taken up front

These are the choices that would otherwise be made three times inconsistently.

**Flows are addressed by `flow_id`, never held in `AppState`.** The obvious
shape is a `Mutex<Option<SasVerification>>` in state, and it is wrong: it makes
a second concurrent request unrepresentable and it puts a lifetime we have to
manage next to one we already do. Every command instead takes the `flow_id`
string and re-resolves through `encryption().get_verification(user_id,
flow_id)`, which is what the SDK's own registry is for. `AppState` gains one
field for the sync task's `JoinHandle` and nothing else.

**One event channel per concern, carrying a tagged union.** `verification` for
flow transitions, `session-status` for verified or not, `connection` for the
health of the sync loop. serde's `#[serde(tag = "kind", rename_all =
"camelCase")]` on the payload enum gives TypeScript a discriminated union for
free, so the frontend switches on `kind` with the compiler checking
exhaustiveness.

**The SDK's state enums do not cross the IPC boundary.** `SasState` and
`VerificationRequestState` carry `DeviceData` and method lists that the UI has
no use for, and pinning our wire format to an upstream enum means an SDK bump
can silently change what the frontend receives. A `From` impl maps them onto a
small owned DTO, and that mapping is the most valuable unit test in the
milestone because it is pure and every branch is reachable.

**One task per flow, ending with the flow.** The `changes()` streams are the
only way to observe progress. A task spawned per flow that ends when the stream
ends needs no cancellation logic, unlike a long-lived task holding a registry.

**Verification lives in `consort-matrix`, not in the Tauri layer.** Same split
that made `login` and `restore` reachable from `against_a_mock_homeserver.rs`.
Commands stay one-line delegates.

## Phases

Each phase lists what to build and what has to be true before it is finished.

### Phase 0: prerequisites (done)

Produces nothing a user can see, and is the bulk of the work.

Built:

- `crates/consort-matrix/src/sync.rs`. A `start(client, on_event) ->
  JoinHandle<()>` over `client.sync_with_result_callback`, not `client.sync`.
  The callback form is what lets a failed iteration be observed and reported
  instead of being retried silently forever behind a spinner.
- The `JoinHandle` goes in `AppState` beside `refresh_task`, aborted in both
  `set_client` and `clear_client`. That pattern is already established and
  already tested; follow it rather than inventing a second one.
- `app/src-tauri/src/events.rs`. Typed payload enums and a thin `emit` wrapper
  over `AppHandle`, so no command formats an event name as a string literal.
- `listen()` wrappers in `app/src/lib/api.ts` mirroring the existing `invoke`
  wrappers, each returning its unlisten function so React effects can clean up.
- A connection state on the `connection` channel. A sync loop that has quietly
  died must not look like a client with nothing to say.
- `testing/synapse/`: a compose file for a throwaway Synapse with registration
  open, plus a script that creates two accounts. This is a deliverable, not
  scaffolding, because every later phase is tested against it.

Done. Signing in starts the loop, signing out stops it and says so, a failing
homeserver moves the header to "Reconnecting", and no task outlives the client
that owns it.

Two things came out of building it that the plan did not predict.

**matrix-sdk retries a 5xx for fifteen minutes on its own**, with no way for a
caller to see it happening, so the connection state above would have been
unreachable in practice: the loop sits inside one `sync_once` for a quarter of
an hour while the UI says everything is fine. `base_builder` now sets
`RequestConfig::short_retry()`, which bounds it to three attempts and roughly a
second and a half. Retrying is still right for most failures; it just has to
happen somewhere it can be reported. This changes login too, and for the
better: a login that hangs for fifteen minutes on a bad gateway is a form that
never comes back.

**The throwaway Synapse earned two live tests immediately** rather than waiting
for Phase 2. `tests/against_a_real_homeserver.rs` proves the harness end to
end, and proves that two logins to one account produce two devices that can
see each other, which is the precondition for every phase from 2 onwards and
was worth knowing before building on it.

### Phase 1: surface the state (done)

Built:

- `crates/consort-matrix/src/verification/`. A `watch(client, on_change) ->
  JoinHandle<()>` over `encryption().verification_state()`, filtered down to
  the changes: the SDK republishes the same answer after every `/keys/query`
  mentioning one of our own devices.
- A third task slot in `AppState` beside the sync loop and the token refresher,
  started and aborted in the same two places.
- A `verification` channel carrying `unknown` / `verified` / `unverified`.
- A real banner on the signed-in screen, replacing the "Session verification is
  next" placeholder. Amber for unverified, and not red: a session nobody has
  verified yet is where every login starts, not a fault.

Done. The banner is right on a fresh login, on a restored session, and after a
change with no user action, and an unknown state renders as "checking" rather
than as either answer.

Two corrections came out of building it.

**`Unverified` carries no level.** There are two types called
`VerificationState` in the SDK. The one describing who sent a message, in
`matrix_sdk_common::deserialized_responses`, has an `Unverified(VerificationLevel)`
carrying the detail this plan wanted to keep. The one describing our own
device, in `matrix_sdk::encryption`, is three plain variants and no payload,
which is right: for our own device there is only ever one signature in
question. The DTO is three states.

**The webview loses a race nobody had noticed, and it was losing it in Phase 0
too.** Both channels carry state rather than incidents, and both publish from
tasks that start with the session. On a restored session that is inside Tauri's
`setup`, before the webview has run a line of JavaScript. Whatever they said in
the meantime went to nobody, so the header could sit on "Connecting" against a
perfectly healthy sync loop for as long as the session lasted, since a working
loop has no further transitions to report. Fixed for every channel at once with
a sink that remembers the last event per channel and a `resend_state` command
the frontend calls once its listeners are attached. Phase 0's connection header
was wrong in exactly the same way and is fixed by the same change.

### Phase 2: emoji verification, responder path (done)

The one actually asked for. Consort accepts a request that Element started.

Built:

- `verification/dto.rs`. `Flow`, `FlowState` and the mappings onto them. Seven
  states, and the two that look redundant are not: `Ready` means both sides
  agreed and nobody has started the comparison, which is where the "show me the
  emoji" button belongs, and `Waiting` means it has started and the keys are in
  flight, which is a spinner.
- `verification/flow.rs`. Both event handlers, the state machine driver, and
  the five actions. Every action re-resolves through the SDK's registry from
  the `(user_id, flow_id)` pair the event carried, so nothing holds a flow.
- A `verification-flow` channel, and a panel that renders each state: the
  request with accept and decline, the seven emoji with their words, the
  decimal fallback, and each ending phrased as itself.
- `testing/synapse/` grew what the live tests need: open registration and no
  rate limits.

Done. Both sides of the emoji exchange run unattended against a real Synapse in
`against_a_real_homeserver.rs`, and accept, confirm, mismatch, cancel and
`start_sas` are each exercised there. The session reports itself verified
afterwards, which is the milestone rather than the protocol working.

Three corrections came out of building it.

**The responder has to accept the SAS as well as the request, and that is not a
second question for the user.** After `Transitioned` the flow sits in
`SasState::Started` and goes nowhere until `SasVerification::accept()` sends
`m.key.verification.accept`. It looks like a decision and is not one: it settles
which hash and which MAC the two devices will use, and the human decision was
made when they accepted the request. Putting a button in front of a choice
between `hkdf-hmac-sha256` and `hkdf-hmac-sha256.v2` would be asking somebody a
question they cannot have an opinion about. It is called automatically, and only
when `we_started()` is false.

**Expiry needs no timer of ours.** The plan said the ten-minute window is in the
protocol rather than in a stream event. It is in both: the SDK's crypto machine
garbage-collects timed-out flows at the start of every sync response and sets
the observable, so a `Cancelled(Timeout)` arrives on `changes()` within one sync
of the deadline. A timer here would race that and send a second cancel. What was
worth doing instead is making `timedOut` a reason of its own, so the interface
says the request expired rather than that somebody refused.

**A cancellation is not one thing.** `CancelCode::Accepted` means another of the
account's own devices answered first, which happens on every self-verification
because the request goes to all of them. Rendering that as "cancelled" reports a
problem to somebody whose verification is going fine on their phone. The DTO
narrows eleven cancel codes to four answers plus `Other`, and only `Mismatch`
gets the alarming treatment, because it is the only one that means somebody may
be listening.

Two things the mock cannot reach, recorded so they are not rediscovered.
`MatrixMockServer` can inject an `m.key.verification.request` to-device event,
but the crypto machine will not build a request object from it: it looks the
sender's device up in its own store first, and a mocked `/keys/query` has no
devices in it. So the mock proves the negative, that a request from a device we
cannot identify is not shown, and the live suite proves the rest. Separately,
Synapse's default `rc_login` is three attempts per burst, which turns a suite
that signs in twice per test into a wall of 429s that matrix-sdk waits out; the
symptom is a test that hangs for minutes with nothing in the log.

### Phase 3: initiator path (done)

Consort starts the request.

Built:

- `Initiator::verify_this_session`, via `get_user_identity(own_user_id)` then
  `UserIdentity::request_verification()`.
- A "Verify this session" affordance on the banner from Phase 1.
- `has_devices_to_verify_against()` decides whether to offer this at all. With
  no other session signed in, the banner says so instead, which is the honest
  answer until Phase 4 lands.

It did reuse Phase 2 almost entirely: the same `changes()` stream, the same
task, the same DTO plus one field, and the same emoji screen. Three things the
plan did not have.

**A request we send is never announced to us.** To-device messages are not
echoed, so the supervising task hears about every arriving flow and none of the
ones this session starts. The fix is that the channel the event handlers
publish on is handed back from `supervise` as well, in an `Initiator`, and
starting a flow announces it on that same channel. One code path, one owned
task per flow, one dedup, both directions. It is deliberately a separate value
from the task handle rather than a wrapper around it, so the task stays an
ordinary `JoinHandle` owned like the other three the application runs.

**The initiator has to send the start.** `follow_sas` already skipped its
automatic accept when we started, but nothing sent `m.key.verification.start`,
and the convention is that whoever asked starts the comparison. Both sides
waiting for the other is a flow that only ends by timing out. It is automatic
rather than a button for the same reason accepting the algorithms is: the
person has already said they want to verify.

**`has_devices_to_verify_against` asks the homeserver, not the crypto store.**
`get_user_devices` depends on a `/keys/query` having happened and on every
device having published keys. Being wrong in the "none" direction sends
somebody who does have a phone signed in to a recovery key they may never have
kept, so `GET /devices` is the better question: is anything else signed in.
It is also the only one of the two a mock can answer, which is why three of the
four new mock tests exist at all.

Done: the flow works in the opposite direction and the emoji screen is the same
code.

### Phase 4: recovery key (done)

The path that works when the phone is in another room, or when there is no
phone.

Built:

- `verification::recover`, over `encryption().recovery().recover(&key)`, with
  four separate answers rather than one "that did not work".
- `verification::has_recovery_set_up`, asked before the box is drawn, so an
  account with no secret storage gets a sentence instead of an input that
  cannot succeed.
- `RecoveryKeyForm`, beside the emoji button when both routes are open and on
  its own when it is the only one.

Five things the plan did not have.

**The type that tells the two failures apart is not re-exported.** Malformed
against wrong is decided by `DecodeError`, which lives in matrix-sdk-crypto.
`SecretStorageError::SecretStorageKey` carries it publicly and matrix-sdk does
not re-export the type, so matching on the variants means naming the crate.
`matrix-sdk-base` is now a direct dependency at the same git rev, which
resolves to the crate matrix-sdk already links rather than a second copy. If
the rev in the workspace manifest moves, that one moves with it.

**A passphrase account has no malformed input.** `from_account_data` tries the
passphrase first when the account has one, falls back to base58, and returns
the *passphrase* error when both fail. So everything typed at such an account
comes back as `Mac`, which is the "wrong key" answer, and that is correct:
against a passphrase, any string is a candidate and none of them is
misformatted.

**`recover` succeeding does not mean the session is verified.** Secret storage
is a bag of secrets rather than a fixed set. One holding only a megolm backup
key imports cleanly, signs nothing, and returns `Ok`. Somebody who typed 48
correct characters would see no error and no change. `recover` therefore checks
`cross_signing_status().has_self_signing` afterwards and reports
`RecoveryWithoutIdentity` when the keys were not there.

**`recovery().state()` is `Unknown` on a restored session.** The SDK fills it
in from the background task that login waits for and restore does not, so a
cached "we have not looked yet" is what somebody gets a second after launch.
`has_recovery_set_up` falls through to `secret_storage().is_enabled()` in that
case, which is the same question the SDK is about to ask.

**The mock server reached further than expected.** Everything up to the moment
the key is used is account data over HTTP, and both interesting failures are
decided before any secret is decrypted, so nine of the twelve new tests need no
homeserver. Only the success path does. That also turned up a wart in the
harness: `mount_login`'s prebuilt `mock_query_keys` insists on the mock crate's
own access token, which this suite never uses, so it had never matched.
Invisible until `import_secrets` became the first caller that reports the
failure instead of swallowing it.

Done: a correct key verifies the session against a real Synapse, a wrong one
says which kind of wrong and leaves the session usable, and an account with no
recovery is told so rather than shown a box.

### Phase 5: key backup (done)

Built:

- `consort_matrix::backup`, a fourth push channel reporting what is happening
  to room keys.
- Two encryption settings in `base_builder`, which is where most of this phase
  turned out to live.
- One notice on the signed-in screen, for the one state nothing else covers.

Four things worth recording.

**Most of it is a setting, not code.** "Absent needs a `create()`" is
`auto_enable_backups: true`, and the SDK's version of that check is better than
a hand-rolled one: it also honours the MSC4287 account data that says somebody
turned backups off deliberately. Writing the create loop by hand would have
overridden that choice silently.

**`OneShot` is the wrong download strategy and looks like the right one.** It
pulls the whole backup the moment the key arrives, which sounds like exactly
what "restore room keys after verification" means. The room keys endpoint is
not paginated, so that is one response holding every key the account has ever
had, decrypted in one go, and the SDK's own comment on the code says it does
not work for any sizeable account. `AfterDecryptionFailure` fetches the one key
a message needs when the message fails, which is what opening a channel and
scrolling up does anyway.

**`BackupState::Unknown` is two answers wearing one name.** It means "no backup
is active in this session", which covers both "there is one and this session
cannot read it yet" and "there is none, for anybody". Those are opposite pieces
of news: the first is fixed by verifying and the second means every key on this
machine dies with it. The SDK cannot tell them apart because homeservers do not
announce backups being created or deleted, so `backup::describe` resolves it
with `fetch_exists_on_server()` and reports `Unusable` or `Missing`.

**The mock harness was hiding a cliff.** An unmounted account data endpoint is
a 404 with no Matrix error in it, which the SDK reads as a transport failure
rather than "no such event", and one of those makes it abandon the whole of
recovery and backup setup at login. Nothing noticed until a test needed the
setup to have run. Mock tests that care about any of this now mount
`m.secret_storage.default_key` and friends with a real `M_NOT_FOUND` body.

Done: a message sent before a second session existed decrypts on it after a
recovery key, against a real Synapse. The first login on an account creates a
backup, and a second session reports `Unusable` until it is verified and
`Enabled` after.

**What is left over.** The live test asks for the room key download by hand.
In the application that call is the SDK's, triggered by a message failing to
decrypt, and there is no timeline for a message to fail in yet. So the strategy
is configured and the keys are proven readable, and the automatic trigger is
first exercised when the room list arrives. Worth re-checking there rather than
assuming.

### Phase 6: gate voice on it (next)

The MSC4153 check finally has something real to check. Refuse to join an
encrypted call from an unverified session, and say why.

## Testing

Three layers, because no one of them covers this.

**Unit, in `consort-matrix`.** The `From` impls in `dto.rs` and every error
mapping. Pure functions over enums, so every branch is reachable and the 90%
floor is met here rather than by accident elsewhere.

**Mock server, in `tests/against_a_mock_homeserver.rs`.** `MatrixMockServer`
has `mock_sync()` over a `SyncResponseBuilder`, and that builder has
`add_to_device_event(JsonValue)`. That is enough to assert that the sync loop
starts and stops with the session, that a raw `m.key.verification.request`
to-device event produces the event our frontend expects, and that a 5xx from
sync surfaces as disconnected rather than as silence. It is not enough to drive
a handshake: the mock cannot do olm.

**Live, in `tests/against_a_real_homeserver.rs`, `#[ignore]`d.** The second device is
matrix-sdk driven from the test itself, not Element, so the whole flow runs
unattended in one `cargo test` against the Phase 0 Synapse. Gate on
`CONSORT_TEST_HOMESERVER` being set so a normal `cargo test` and CI both skip
it, the same way the keyring tests are already handled. Element still gets
tried by hand before the milestone closes, because "works against another copy
of the same SDK" is not the same claim as "interoperates".

**Frontend.** Vitest with `listen` mocked, asserting the emoji screen renders
the waiting state before `KeysExchanged` and that unlisten runs on unmount. A
leaked listener across a sign out and sign in shows up as duplicated events,
which is easy to ship and unpleasant to debug.

## Complexity

High, and unevenly distributed. Phase 0 is most of the work and shows the user
nothing. Phases 1 to 3 are the feature. Phases 4 and 5 are what stop it feeling
half-built. Phase 6 is a conditional.

Phase 0 is also the phase most likely to be under-estimated, because "add a
sync loop" sounds like three lines and the three lines are the easy part. The
task lifetime, the event bridge, the connection state and the test homeserver
are the rest of it.

## Deliberately out of scope

`generate_qr_code()` and `scan_qr_code()` are available, and a QR for a phone to
scan is smoother than emoji for desktop-to-phone. It is a small addition on top
of Phase 2's request handling and worth doing eventually. Emoji is what was
asked for and QR is not on the critical path.

Verifying *other users* is also out of scope. The plumbing is the same, but the
UI question is different and there is no room list to hang it off yet.

## Open question: multiple accounts at once

Asked during review, recorded here because it shapes Phase 0.

It is feasible. matrix-sdk supports several independent `Client` instances in
one process, each with its own SQLite store, and the storage layer is already
shaped for it: `SessionStore::store_path_for` gives every account its own
directory, and secrets are keyed per user ID. The session tests already prove
two accounts' tokens can sit side by side.

What it costs is not storage. `AppState` becomes a map rather than a single
`Option<Client>`, every background task multiplies by the number of accounts,
and the UI has to distinguish an account from a space. Discord's left rail is
one identity and many servers; the thing being described is many identities,
which is closer to what Element calls a profile.

The recommendation is to keep the storage layer account-agnostic, which it now
is, and not to build the UI for it until after verification and voice. Doing it
before means designing the verification UI twice, since "verify this session"
becomes "verify which session".
