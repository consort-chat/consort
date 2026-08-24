#!/usr/bin/env bash
#
# Stop the throwaway Synapse and delete its state.
#
# The state is deleted rather than kept. A homeserver that remembers the
# devices from last week is a test fixture that passes for reasons nobody
# chose, and rebuilding it costs seconds.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose "$@"
  else
    docker-compose "$@"
  fi
}

compose down -v --remove-orphans

if [[ -d data ]]; then
  echo "==> removing ./data"
  # Synapse runs as the invoking user (UID/GID are passed in), so this needs
  # no privileges. If it ever does, the container image changed.
  rm -rf data
fi
