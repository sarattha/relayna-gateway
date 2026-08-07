#!/usr/bin/env bash
set -euo pipefail

REPORT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
PROJECT_NAME="relayna-endpoint-monitoring"

RELAYNA_GATEWAY_IMAGE="${RELAYNA_GATEWAY_IMAGE:-relayna-gateway:endpoint-monitoring-test}" \
  GATEWAY_ADMIN_TOKEN="${GATEWAY_ADMIN_TOKEN:-unused-by-cleanup}" \
  docker compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" down -v --remove-orphans
