#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export DATABASE_URL="${DATABASE_URL:-postgres://postgres:postgres@127.0.0.1:5432/relayna_gateway}"
export REDIS_URL="${REDIS_URL:-redis://127.0.0.1:6379}"
export LITELLM_BASE_URL="${LITELLM_BASE_URL:-http://127.0.0.1:4000}"
export LITELLM_SERVICE_KEY="${LITELLM_SERVICE_KEY:-test-litellm-key}"
export GATEWAY_ADMIN_TOKEN="${GATEWAY_ADMIN_TOKEN:-test-admin-token}"
export GATEWAY_BIND_ADDR="${GATEWAY_BIND_ADDR:-127.0.0.1:18080}"
export GATEWAY_CONTROL_BIND_ADDR="${GATEWAY_CONTROL_BIND_ADDR:-127.0.0.1:18081}"
export LOG_LEVEL="${LOG_LEVEL:-gateway_api=debug,gateway_proxy=debug,gateway_store=debug}"

echo "Starting Relayna Gateway"
echo "  proxy:   http://${GATEWAY_BIND_ADDR}"
echo "  control: http://${GATEWAY_CONTROL_BIND_ADDR}"
echo "  redis:   ${REDIS_URL}"
echo "  db:      ${DATABASE_URL}"
echo
echo "Logs are printed below as JSON lines. Press Ctrl-C to stop."
echo

exec cargo run -p gateway-api
