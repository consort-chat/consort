# A call stack to throw away

Synapse, LiveKit and lk-jwt-service on one bridge network, so a MatrixRTC call
in the legacy state dialect can connect to a real SFU.

```sh
./up.sh      # start it and wait until it answers
./down.sh    # stop it and delete every trace
```

## Running the live call test

Not `#[ignore]`d. With the two variables unset it prints a skip notice and
passes, so `cargo test --workspace` stays green with nothing running; with them
set it joins a real call.

```sh
./up.sh
export CONSORT_TEST_CALL_HOMESERVER=http://localhost:8018
export CONSORT_TEST_CALL_SFU=http://localhost:8070
cargo test -p consort-call --test two_peers_hear_the_same_chime -- --nocapture
```

## Linux only

LiveKit sits at a fixed address, 172.30.0.10, on a bridge network. A Linux host
routes to that subnet directly, so the one address in lk-jwt-service's answer
works for the containers and for a test process on the host. Docker Desktop
does not route to it. Ports here are 8018, 8070 and 7880, so it runs beside
`testing/synapse/`.

Not a deployment: self-signed certificate, open registration, keys in the
files. Detail in [docs/CALL-HARNESS.md](../../docs/CALL-HARNESS.md).
