#!/usr/bin/env bash
# teardown.sh — stop and remove glia-test containers.
#
# Does NOT remove persistent volumes (openbao_data, redis_data) so
# the next run keeps state.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
cd "$ROOT"

docker compose \
  -f docker-compose.yml \
  -f glia-test/compose.test.yml \
  down

echo "glia-test stack stopped (volumes preserved)."
