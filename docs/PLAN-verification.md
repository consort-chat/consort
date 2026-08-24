# Plan: end-to-end encryption verification

Status: **accepted, not started.** Every API named here was checked against the
pinned SDK rev rather than recalled, and the file and function names are the
ones to create.

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

### Phase 0: prerequisites

Produces nothing a user can see, and is the bulk of the work.

Build:

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

Done when: signing in starts a sync loop that survives a homeserver restart,
signing out stops it, killing the network moves the UI to a disconnected state
and restoring it moves back, and no task outlives the client that owns it.

### Phase 1: surface the state

Build:

- Subscribe to `encryption().verification_state()`, which is a `Subscriber<
  VerificationState>`, and emit on change.
- Map `Verified` / `Unverified(level)` onto the DTO. The level is worth keeping:
  "unverified" and "signed by an identity we do not trust" are different
  sentences.
- Replace the "Session verification is next" placeholder on the signed-in
  screen with a real unverified-session banner.

Small, and it makes every later phase observable. Do it before Phase 2, not
after, because otherwise the first flow is debugged with no way to see whether
it worked.

Done when: the banner is correct on a fresh login, on a restored session, and
after an SDK-side change with no user action, and it renders an unknown state
as unknown rather than as verified.

### Phase 2: emoji verification, responder path

The one actually asked for. Consort accepts a request that Element started.

Build:

- `crates/consort-matrix/src/verification/` as a directory, not one file:
  `mod.rs` for the entry points, `flow.rs` for the state machine driver,
  `dto.rs` for the wire types and the `From` impls.
- An event handler for `ToDeviceKeyVerificationRequestEvent` plus the in-room
  `OriginalSyncRoomMessageEvent` with an `m.key.verification.request` body.
  Element sends the in-room flow for self-verification in some versions, so
  handling only the to-device one gets a request that never appears.
- On arrival, resolve `encryption().get_verification_request(user_id, flow_id)`
  and spawn the flow task on its `changes()` stream.
- Commands: `verification_accept`, `verification_start_sas`,
  `verification_confirm`, `verification_mismatch`, `verification_cancel`, each
  taking a `flow_id`.
- `VerificationRequestState::Transitioned { verification }` is where a
  `SasVerification` appears. Switch to its `changes()` stream there.
- Render the seven emoji from `.emoji()` with their descriptions. `emoji()`
  returns `Option<[Emoji; 7]>` and is `None` until `SasState::KeysExchanged`,
  so the UI needs a real waiting state rather than an empty row.
- `supports_emoji()` can be false. Fall back to `decimals()` rather than
  showing nothing.
- Cancellation and expiry as first-class states, driven by
  `SasState::Cancelled` and `cancel_info()`. Expiry needs our own timer: the
  ten-minute window is in the protocol, not in a stream event.

Done when: Element starts a self-verification, Consort shows the request,
accepting it shows seven matching emoji, confirming on both sides leaves both
verified, and rejecting on either side leaves both cancelled with the reason
named. All four of accept, confirm, mismatch and cancel are exercised against
the live Synapse from Phase 0.

### Phase 3: initiator path

Consort starts the request.

Build:

- `verification_request_own_device`, via `get_user_identity(own_user_id)` then
  `UserIdentity::request_verification()`.
- A "Verify this session" affordance on the banner from Phase 1.
- `has_devices_to_verify_against()` decides whether to offer this at all. With
  no other device signed in, the honest answer is to send the user to Phase 4.

Reuses Phase 2's state machine almost entirely: the same `changes()` stream,
the same DTO, the same task. What differs is who called first, which
`we_started()` already reports.

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

**Live, in `tests/two_devices.rs`, `#[ignore]`d.** The second device is
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
