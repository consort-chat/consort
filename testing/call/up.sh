#!/usr/bin/env bash
#
# Start the throwaway Matrix + LiveKit + lk-jwt-service stack.
#
# Safe to run repeatedly: the certificate is only generated when absent.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

readonly HOMESERVER=http://localhost:8018
readonly SFU=http://localhost:8070

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose "$@"
  else
    docker-compose "$@"
  fi
}

mkdir -p data certs

# Synapse wants a federation certificate even when nothing verifies it.
if [[ ! -f certs/federation.crt || ! -f certs/federation.key ]]; then
  echo "==> generating a federation certificate"
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -subj '/CN=consort.test' -addext 'subjectAltName=DNS:consort.test' \
    -keyout certs/federation.key -out certs/federation.crt 2>/dev/null
fi

echo "==> starting synapse, livekit and lk-jwt-service"
compose up -d

wait_for() {
  local name=$1 url=$2
  echo -n "==> waiting for $name to answer"
  for _ in $(seq 1 60); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      echo " ok"
      return 0
    fi
    echo -n .
    sleep 1
  done
  echo
  echo "$name never came up. Logs:" >&2
  compose logs --tail 40 >&2
  exit 1
}

wait_for synapse "$HOMESERVER/_matrix/client/versions"
wait_for lk-jwt-service "$SFU/healthz"

cat <<EOT

Ready.

  homeserver  $HOMESERVER
  sfu auth    $SFU
  livekit     ws://172.30.0.10:7880

The live call tests are skipped unless these are set:

  export CONSORT_TEST_CALL_HOMESERVER=$HOMESERVER
  export CONSORT_TEST_CALL_SFU=$SFU

Stop and delete everything with ./down.sh
EOT
