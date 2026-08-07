#!/usr/bin/env bash
set -euo pipefail

REPORT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
PROJECT_NAME="relayna-endpoint-monitoring"
: "${RELAYNA_GATEWAY_IMAGE:?set RELAYNA_GATEWAY_IMAGE to the freshly built local image}"
TOKEN_SUFFIX="$(openssl rand -hex 24)"
export GATEWAY_ADMIN_TOKEN="op_live_${TOKEN_SUFFIX}"

docker image inspect "$RELAYNA_GATEWAY_IMAGE" >/dev/null
docker compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
RELAYNA_GATEWAY_IMAGE="$RELAYNA_GATEWAY_IMAGE" docker compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" up -d --force-recreate

cd "$REPORT_DIR"
RELAYNA_GATEWAY_IMAGE="$RELAYNA_GATEWAY_IMAGE" node runner.mjs

echo "Real environment remains available at http://127.0.0.1:19281/admin-ui for Computer Use verification."
