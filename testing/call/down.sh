#!/usr/bin/env bash
#
# Stop the throwaway call stack and delete its state.
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

for dir in data certs; do
  if [[ -d $dir ]]; then
    echo "==> removing ./$dir"
    rm -rf "$dir"
  fi
done
