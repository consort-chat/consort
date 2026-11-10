# The harness that proves a call

ADR-0021 named the gate for leaving the pinned forks: "Two real clients in the
state dialect, heard by two people, because every other check this repository
has passes already and none of them can tell a call that connects from a call
somebody can hear."

This is that gate, without the two people. `testing/call/` stands up a real
SFU, and `crates/consort-call/tests/two_peers_hear_the_same_chime.rs` puts two
Consort peers in one MatrixRTC call through it, publishes a known signal from
one, and asserts the other receives that signal rather than merely receiving
something.

It is also the only coverage `crates/consort-call/src/livekit.rs` has ever had.

## What the test asserts

The signal is `consort_audio::Tone`: A4 at 440 Hz for 160 ms, then E5 at
659.25 Hz for 160 ms. `Tone::SAMPLES` is a whole number of 10 ms frames, so the
test loops it with no seam, and the call carries a 320 ms chime on repeat.

The sender publishes it through `CallSession::publish_microphone` and
`PublishedAudio::send`. The listener receives it through
`CallSession::listen(&ears)` with an `Arc<dyn Heard>` that records every sample
handed to it: the same seam the mixer sits behind in the application.

Two assertions follow.

**The listener hears the chime.** Not sample equality, which Opus makes
unavailable at any price. Frequency content, which is the stronger claim
anyway. The received PCM is cut into 40 ms windows, a quarter of a note, and
each window is measured with a Goertzel bin at each note and at four controls
(311, 523.25, 1100 and 1550 Hz, chosen to be harmonics of neither note). A
window carries a note when that note is above an absolute floor and four times
above every control. The chime is heard when at least ten windows carry each
note and the loudest control across the whole capture stays four times below
the loudest note.

Requiring **both** notes, each in windows of its own, is what makes this the
chime rather than any loud thing. A steady 440 Hz hum fails. So does noise,
which puts as much in the controls as in the notes. So does silence, and so do
the wrong notes.

**The SFU cannot hear it.** A third subscriber joins the same LiveKit room with
`RoomOptions::default()`, which carries no key provider and no frame
encryption, and the test asserts that what is on the wire is *not* the chime.
Measured: the legitimate peer reads the notes at amplitude 7210 against a
loudest control of 619, and the wire reads 0 windows at either note with the
spectrum flat. Without this half, everything above would pass just as well on a
call with no encryption at all, which is how the unencrypted room this test
used to make went unnoticed. See "What mutation testing found", below.

The chime detector is proven able to discriminate with no harness at all. Five
`#[test]`s in the same file feed it the chime, silence, loud noise, a steady
tone at one of the notes, and the chime at the wrong pitch, and assert the
predicate the live test uses accepts only the first. Those run in every
`cargo test --workspace`.

## What stands the harness up

Three containers on one user-defined bridge network.

| piece | image | where |
|---|---|---|
| Synapse | `ghcr.io/element-hq/synapse` | `127.0.0.1:8018` client, `:8448` federation TLS inside the network |
| LiveKit | `livekit/livekit-server:v1.13.11` | `172.30.0.10:7880`, fixed |
| lk-jwt-service | `ghcr.io/element-hq/lk-jwt-service:0.9.0` | `127.0.0.1:8070` |

Three things make it work, and all three were read out of upstream's own e2e
harness in `element-hq/lk-jwt-service`, `e2e-tests/docker/`.

**lk-jwt verifies the OpenID token over federation**, which is TLS only. So
Synapse serves a self-signed certificate on a federation listener, lk-jwt is
given `LIVEKIT_INSECURE_SKIP_VERIFY_TLS`, and the Synapse service carries the
network alias `consort.test` so the server name out of the token resolves
inside the network.

**`SfuResponse.url` is whatever `LIVEKIT_URL` says**, verbatim, and that one URL
has to be reachable by lk-jwt *and* by the test process on the host. A compose
service name fails on the host and `127.0.0.1` fails inside the container, so
LiveKit gets a fixed address on the bridge, which both can route to.

**`use_external_ip: false`** makes LiveKit advertise that same bridge address as
its ICE candidate, so media needs no published ports at all.

Synapse's config is written out rather than generated, because three settings
have to be explicit: `max_event_delay_duration: 1h`, so the MSC4140 delayed
leave a join arms is accepted rather than degraded; `ip_range_blacklist: []`,
because Synapse's default deny list covers exactly the RFC1918 space the bridge
lives in; and open registration with the rate limits lifted, because the test
registers accounts of its own.

It registers them per run, and that is not tidiness. Media keys go out through
the SDK's identity-based strategy, which refuses unless the account has a
cross-signing identity and this session is verified against it. Only a **first**
login keeps the cross-signing private keys, so only a first login is
`CallReadiness::Ready`. `crates/consort-matrix/src/calls/readiness.rs` has the
measurement. The test asserts that readiness before it joins, because a session
that cannot distribute a key joins a call that connects perfectly and is heard
by nobody, which is the exact failure this test would otherwise report as a
transport fault.

Two other preconditions the test sets and checks, both learned the hard way:

- The room grants power level 0 for `org.matrix.msc3401.call.member`. A state
  event needs 50 by default and the second peer joins as an ordinary member, so
  without it that peer's membership is refused with a 403 and the call it
  joined has nobody in it. Element Call writes the same entry.
- The room is **encrypted**. Frame encryption follows the room's own encryption
  state, spec 038 R11, through `call.encrypts_media()`. In a plain room there
  is no media key at all, so nothing about the key path can be asserted.

## Linux only

The design rests on the host routing to the bridge subnet, which a Linux host
does and Docker Desktop does not. That is what CI runs and what this project is
developed on. The ports are 8018, 8070 and 7880 so the stack runs beside
`testing/synapse/`.

## Where it runs in CI

Its own job, `A call two peers can hear`, on the runner host rather than in a
container. The harness is `docker compose` against config files in the
repository: a container job cannot mount them into service containers, which
start before the checkout, and reaching the host's Docker from inside the job
container would mean mounting the socket into it and then solving the network
in the other direction. The job builds `-p consort-call` only, so it needs
clang-21 and five libraries rather than the GTK and WebKit the Rust job
installs for the Tauri shell.

The job greps the test's output for a receipt the test prints only after both
assertions have held. Without that step the gate would be one renamed
environment variable away from passing on a test that returned before it
measured anything.

The live test is **not** `#[ignore]`d, because an ignored test is not a gate.
With the two variables unset it prints a skip notice and returns, so
`cargo test --workspace` stays green on a machine with nothing running, and the
five detector tests run there regardless.

## What mutation testing found

The test's whole value is that it fails when the audio does not get through, so
that was broken on purpose, six ways. All six are red.

| mutation | what happens |
|---|---|
| the sender publishes nothing, with the track still up | 100 windows of silence arrive, 0 notes |
| the sender publishes 300/900 Hz instead of the chime | loud audio arrives, loudest note 397 against a control of 5128 |
| the listener points at the wrong SFU | the join is refused, 404 from the authorisation service |
| the listener deafens itself before the measurement | 100 windows of silence, 0 notes |
| the sender cannot distribute a media key (second device on the account) | 100 windows, 0 notes |
| the room is left unencrypted | the chime is audible on the wire |

The fifth and sixth are the ones worth recording, because the fifth did **not**
go red the first time it was run, and the sixth is why.

The room was unencrypted. `call.encrypts_media()` was therefore false, no key
provider reached `RoomOptions`, and the media crossed the SFU in the clear. A
sender with no media key was heard perfectly, Consort's own `trouble.rs`
reported no fault, which was the correct report of what happened, and the
assertion was measuring a path with no cryptography in it at all.

Nothing was wrong with the migration: `e2ee` following the room's encryption
state is spec 038 R11, and `consort_matrix::calls::can_join` already declines
to gate a join in an unencrypted room for the same reason. The defect was the
test's, it was invisible to every green run, and the only thing that found it
was breaking the key on purpose and noticing that nothing happened.

Both halves of the fix are in the test now: the room is encrypted and that is
asserted on both sides, and the eavesdropper assertion means a future change
that quietly turns encryption off fails here rather than passing.

## What this gate does not hold

`crates/consort-call/src/livekit.rs` stays in the coverage exclusion list in
`ci.yml`. Coverage is measured in the `rust` job, which runs in a container with
no SFU, so taking the exclusion out would drop the number below the 90% gate for
reasons that have nothing to do with what is covered. Measuring coverage in this
job and merging the two reports is a separate change.

Not covered either: video and screen share, which this test leaves alone;
`LiveKitTransport::fallback_service_url`, because the test passes the service
URL as the operator override and `well_known_url` builds an `https` URL against
a server name that does not resolve here; and the sticky and current dialects,
which `Dialect::readable` refuses on this build anyway.
