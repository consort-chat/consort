# A Synapse to throw away

Two accounts on one homeserver, so verification can be tested against real
olm rather than a mock.

```sh
./up.sh      # start it, create one account per live test
./down.sh    # stop it and delete every trace
```

## Why this exists

`MatrixMockServer` covers a surprising amount: it can inject a to-device
`m.key.verification.request` into a sync response, so "the request arrives and
reaches our handler" is a normal `cargo test`. What it cannot do is olm. The
SAS handshake is real cryptography between two real devices, and there is no
way to fake half of it.

So the verification tests come in two kinds. The ones that do not need a
handshake run against the mock and are part of `cargo test --workspace`. The
ones that do are `#[ignore]`d and gated on `CONSORT_TEST_HOMESERVER`, which
means CI and a normal test run both skip them and nobody has to have Docker
installed to contribute.

## Running the live tests

```sh
./up.sh
export CONSORT_TEST_HOMESERVER=http://localhost:8008
cargo test --workspace -- --ignored
```

## What it is not

Not a deployment, and not an example of one. No TLS, no reverse proxy, no
Postgres, registration wide open, and the password is in the script. It listens
on `127.0.0.1` only. Do not point anything you care about at it, and do not
copy this compose file anywhere that matters.
