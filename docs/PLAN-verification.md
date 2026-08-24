# Plan: end-to-end encryption verification

Status: **draft, awaiting review.** Nothing here is built yet.

This is the milestone after authentication and before voice. Not by
preference: MSC4153 requires a cross-signed device to join an encrypted call,
so the voice layer cannot be tested at all until a Consort session can be
verified. The same work is what lets encrypted room history decrypt, which is
the more visible half.

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
pinned SDK rev rather than assumed.

**Login will not reset an existing cross-signing identity.**
`auth.rs` sets `auto_enable_cross_signing: true`, which sounds like it might
clobber the identity a user already has and un-verify every other device they
own. It does not. The SDK routes that setting through
`bootstrap_cross_signing_if_needed`, which checks `get_user_identity().is_none()`
first and does nothing when an identity exists.

**Backup and recovery setup already run at login.** The same task calls
`backups().setup_and_resume()` and `recovery().setup()`. Phase 5 is wiring up
what those produce, not starting from nothing.

## Risks, largest first

**There is no sync loop.** Consort has never called `client.sync()`. Verification
is driven entirely by to-device events, which arrive only through sync. Nothing
from Phase 2 onward can work until this exists, and it is invisible from the
current code: everything today is request and response through a Tauri command,
and this is the first thing that is not.

**The IPC is request/response only.** `api.ts` is all `invoke`. Verification is
push-driven, so it needs Tauri events on the Rust side and a `listen()` layer on
the frontend. New plumbing, not an extension of what is there.

**The protocol needs two live clients to test.** Unit tests cover the state
mapping and the error handling. The flow itself needs a second session. Plan on
a throwaway Synapse in Docker with two accounts rather than testing against a
production homeserver, and treat that container as part of the deliverable.

**Timing is part of the protocol.** A verification request expires after ten
minutes, either side can cancel at any point, and both have to be online at the
same time. Expiry and cancellation are states to design for, not error paths to
bolt on afterwards.

## Phases

### Phase 0: prerequisites

Produces nothing a user can see, and is the bulk of the work.

- A sync loop, with its lifetime tied to sign-in and sign-out. Use
  `client.sync(SyncSettings)` on a spawned task rather than pulling in
  `matrix-sdk-ui`'s `SyncService`: the latter drags the whole timeline and
  room-list stack in and has to be pinned to the same SDK rev.
- The task belongs in `AppState` next to the token-refresh task, which already
  established the pattern of owning a `JoinHandle` and aborting it.
- A Tauri event bridge, and a typed `listen()` wrapper in `api.ts` mirroring
  the existing `invoke` wrappers.
- Connection state surfaced to the UI, because a sync loop that has quietly
  died must not look like a client with nothing to say.

### Phase 1: surface the state

- Subscribe to `encryption().verification_state()` and emit changes.
- Replace the "Session verification is next" placeholder on the signed-in
  screen with an honest unverified-session banner.

Small, and it makes every later phase observable.

### Phase 2: emoji verification, responder path

The one actually asked for.

- Handlers for `ToDeviceKeyVerificationRequestEvent` and the in-room
  equivalent.
- Drive `VerificationRequest` through `accept()` then `start_sas()`, then the
  `SasVerification` state machine via its `changes()` stream.
- Render the seven emoji from `.emoji()` with their descriptions. Two buttons,
  wired to `confirm()` and `mismatch()`.
- Cancellation and expiry as first-class states.

### Phase 3: initiator path

- "Verify this session" from Consort, via the own-user
  `UserIdentity::request_verification()`.
- Reuses Phase 2's state machine almost entirely.

### Phase 4: recovery key

- `encryption().recovery().recover(&key)`, with real handling for a mistyped
  key rather than a generic failure.
- This is the path that works when the phone is in another room.

### Phase 5: key backup

- Check `backups().state()`, restore from the server-side backup, create one if
  the account has none.
- Without this, verification succeeds and old messages still do not decrypt,
  which reads to a user as a broken client rather than a missing feature.

### Phase 6: gate voice on it

- The MSC4153 check finally has something real to check.

## Shape

New module `crates/consort-matrix/src/verification.rs`, keeping the no-Tauri
boundary intact so the state machine is testable without a webview. The same
split that made `login` and `restore` reachable from
`against_a_mock_homeserver.rs` applies here: the protocol logic goes in the
crate, the Tauri commands stay one-line delegates.

## Complexity

High. Phase 0 is most of it and shows the user nothing. Phases 1 to 3 are the
feature. Phases 4 and 5 are what stop it feeling half-built.

## Deliberately out of scope

`generate_qr_code()` is available, and displaying a QR for a phone to scan is a
smoother path than emoji for desktop-to-phone verification. It is a small
addition on top of Phase 2 and worth doing eventually, but emoji is what was
asked for and QR is not on the critical path.

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
