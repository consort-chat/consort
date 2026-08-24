#!/usr/bin/env bash
#
# Start the throwaway Synapse and make sure the two test accounts exist.
#
# Safe to run repeatedly: config generation and account creation are both
# skipped when they have already happened.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

readonly SERVER_NAME=consort.test
readonly HOMESERVER=http://localhost:8008
readonly PASSWORD=consort-test-only

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose "$@"
  else
    docker-compose "$@"
  fi
}

image() {
  echo "${SYNAPSE_IMAGE:-ghcr.io/element-hq/synapse:latest}"
}

# Synapse will not start without a config, and the config carries the
# registration shared secret that `register_new_matrix_user` needs. Generated
# once into ./data, which is gitignored and which down.sh removes.
if [[ ! -f data/homeserver.yaml ]]; then
  echo "==> generating a homeserver config"
  mkdir -p data
  docker run --rm \
    -v "$PWD/data:/data" \
    -e SYNAPSE_SERVER_NAME="$SERVER_NAME" \
    -e SYNAPSE_REPORT_STATS=no \
    -e UID="$(id -u)" -e GID="$(id -g)" \
    "$(image)" generate
fi

# Settings the generated config does not have, appended once.
#
# Open registration, because the live verification tests need an account that
# has never had a cross-signing identity. The first login to an account
# bootstraps one and keeps the private keys; every later login finds an
# identity already on the server and does not. Two logins to a reused account
# therefore produce two devices where neither can sign the other, so the
# session under test can never become verified. Registering a fresh account
# per run is what makes those tests repeatable rather than correct once.
#
# Synapse refuses to start with `enable_registration` on and no verification
# unless the second line is there too, which is a sensible thing for it to
# insist on and harmless here: this server is bound to loopback and its state
# directory is deleted by down.sh.
#
# And no rate limits, which is not a nicety. Synapse ships `rc_login` at three
# attempts per burst, and a verification test signs in twice per test with
# several running at once, so the defaults turn the suite into a wall of 429s
# that matrix-sdk waits out. The symptom is a test that hangs for minutes with
# nothing in the log, which is a bad afternoon to diagnose.
if ! grep -q '^# consort test overrides' data/homeserver.yaml; then
  echo "==> applying test-server settings"
  cat >>data/homeserver.yaml <<'YAML'

# consort test overrides. Throwaway server, loopback only.
enable_registration: true
enable_registration_without_verification: true

rc_login:
  address:
    per_second: 1000
    burst_count: 1000
  account:
    per_second: 1000
    burst_count: 1000
  failed_attempts:
    per_second: 1000
    burst_count: 1000
rc_registration:
  per_second: 1000
  burst_count: 1000
rc_message:
  per_second: 1000
  burst_count: 1000
rc_key_requests:
  per_second: 1000
  burst_count: 1000
YAML
fi

echo "==> starting synapse"
compose up -d

echo -n "==> waiting for it to answer"
for _ in $(seq 1 60); do
  if curl -fsS "$HOMESERVER/_matrix/client/versions" >/dev/null 2>&1; then
    echo " ok"
    break
  fi
  echo -n .
  sleep 1
done

if ! curl -fsS "$HOMESERVER/_matrix/client/versions" >/dev/null 2>&1; then
  echo
  echo "synapse never came up. Logs:" >&2
  compose logs --tail 40 synapse >&2
  exit 1
fi

# Two accounts to poke at by hand. The live tests do not use these: they
# register an account of their own per run, for the reason in the registration
# block above.
for user in alice bob; do
  if compose exec -T synapse register_new_matrix_user \
    -c /data/homeserver.yaml \
    -u "$user" -p "$PASSWORD" --no-admin \
    "$HOMESERVER" >/dev/null 2>&1; then
    echo "==> created $user"
  else
    echo "==> $user already exists"
  fi
done

cat <<EOF

Ready.

  homeserver  $HOMESERVER
  accounts    alice, bob (the tests register their own)
  password    $PASSWORD

The live verification tests are skipped unless this is set:

  export CONSORT_TEST_HOMESERVER=$HOMESERVER

Stop and delete everything with ./down.sh
EOF
