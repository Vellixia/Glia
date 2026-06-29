#!/usr/bin/env bash
# setup.sh — bring up the full glia test stack via docker compose.
#
# Usage:
#   cp .env.example .env       # then edit
#   bash glia-test/setup.sh
#
# Requires:
#   - docker + docker compose v2
#   - .env file with GLIA_JWT_SECRET and GLIA_ADMIN_HASH
#
# After this returns, the Hub is reachable at http://127.0.0.1:3000
# and the Web dashboard at http://127.0.0.1:3001.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
cd "$ROOT"

if [ ! -f .env ]; then
  echo "ERROR: .env not found at $ROOT/.env"
  echo "Copy .env.example to .env and edit the values." >&2
  exit 1
fi

# shellcheck disable=SC1091
set -a; . .env; set +a

if [ -z "${GLIA_JWT_SECRET:-}" ] || [ -z "${GLIA_ADMIN_HASH:-}" ]; then
  echo "ERROR: GLIA_JWT_SECRET and GLIA_ADMIN_HASH must be set in .env" >&2
  exit 1
fi

echo "Starting glia test stack..."
docker compose \
  -f docker-compose.yml \
  -f glia-test/compose.test.yml \
  up -d

echo "Waiting for Hub /healthz..."
for i in $(seq 1 60); do
  if curl -sf http://127.0.0.1:3000/healthz >/dev/null; then
    echo "Hub reachable at http://127.0.0.1:3000 (took ${i}s)"
    exit 0
  fi
  sleep 1
done

echo "ERROR: Hub did not come up in 60s" >&2
exit 1
