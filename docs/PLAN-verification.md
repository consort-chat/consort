# Plan: end-to-end encryption verification

Status: **Phases 0 to 2 done, Phase 3 next.** Every API named here was checked
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

### Phase 3: initiator path (next)

Consort starts the request.

Build:

- `verification_request_own_device`, via `get_user_identity(own_user_id)` then
  `UserIdentity::request_verification()`.
- A "Verify this session" affordance on the banner from Phase 1.
- `has_devices_to_verify_against()` decides whether to offer this at all. With
  no other device signed in, the honest answer is to send the user to Phase 4.

Reuses Phase 2's state machine almost entirely: the same `changes()` stream,
the same DTO, the same task. What differs is who called first, which
`we_started()` already reports. Two small things Phase 2 left for it: `Flow`
gains a `we_started` field, since the responder never needed one, and
`follow_sas` already skips its automatic accept when we started the exchange.

Done when: the flow works in the opposite direction and the emoji screen is the
same code.

### Phase 4: recovery key

The path that works when the phone is in another room.

Build:

- `encryption().recovery().recover(&key)`.
- Real handling for a mistyped key. This is the most likely failure in the whole
  milestone and a generic "verification failed" would be a bad answer to it.
  Distinguish a malformed key from a well-formed key that does not decrypt.
- Check `recovery().state()` first. `RecoveryState::Disabled` means the account
  has no recovery set up and there is nothing to type, which is a different
  screen from a wrong key.

Done when: a correct key verifies the session, a mistyped one says so and
leaves the session usable, and an account with recovery disabled is told that
rather than being shown an input that cannot work.

### Phase 5: key backup

Build:

- Inspect `backups().state()` and act on it: `Unknown` needs
  `fetch_exists_on_server()`, `Enabled` needs a restore, absent needs a
  `create()`.
- Restore room keys from the server-side backup after verification succeeds.

Without this, verification succeeds and old messages still do not decrypt,
which reads to a user as a broken client rather than a missing feature. It is
the difference between the milestone working and the milestone looking like it
worked.

Done when: a session verified by either route decrypts history that predates
it, and an account with no backup gets one created rather than an error.

### Phase 6: gate voice on it

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
