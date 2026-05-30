#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
REPORT_DIR="$ROOT_DIR/internal/test-reports/front-door-penetration"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
RESULT_JSON="$REPORT_DIR/results.json"
REPORT_MD="$REPORT_DIR/report.md"

cd "$REPORT_DIR"

docker compose -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
docker compose -f "$COMPOSE_FILE" up -d --build --force-recreate

echo "Waiting for mock provider, LiteLLM, and all front-door gateways..."
for _ in $(seq 1 180); do
  if curl -fsS http://127.0.0.1:18282/healthz >/dev/null \
    && curl -fsS http://127.0.0.1:18283/health/readiness >/dev/null \
    && curl -fsS http://127.0.0.1:18281/admin-ui/readyz >/dev/null \
    && curl -fsS http://127.0.0.1:18291/admin-ui/readyz >/dev/null \
    && curl -fsS http://127.0.0.1:18301/admin-ui/readyz >/dev/null; then
    break
  fi
  sleep 1
done

curl -fsS -X POST http://127.0.0.1:18282/run-tests -o "$RESULT_JSON"

node - "$RESULT_JSON" "$REPORT_MD" <<'NODE'
const fs = require("node:fs");
const [resultPath, reportPath] = process.argv.slice(2);
const result = JSON.parse(fs.readFileSync(resultPath, "utf8"));
const rows = Object.entries(result.checks)
  .map(([name, check]) => `| ${name.replaceAll("_", " ")} | ${check.ok ? "PASS" : "FAIL"} | ${check.status ?? "n/a"} | ${check.error_code ?? ""} | ${check.notes ?? ""} |`)
  .join("\n");
const providerRows = result.providerRequests
  .map((capture) => `| ${capture.method} ${capture.path} | ${capture.authorization} | ${capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no"} | ${capture.hasApigeeIdentity ? "yes" : "no"} |`)
  .join("\n");
const markdown = `# Front Door Penetration Test Report

Generated: ${result.generatedAt}

Overall result: **${result.ok ? "PASS" : "FAIL"}**

## Environment

- LiteLLM upstream: \`${result.environment.litellmUrl}\`
- LiteLLM image: \`docker.io/litellm/litellm:latest\`
- Mock OIDC issuer: \`${result.environment.issuer}\`
- Audience: \`${result.environment.audience}\`
- Tenant: \`${result.environment.tenantId}\`
- Required scope: \`${result.environment.requiredScope}\`
- Allowed group: \`${result.environment.allowedGroup}\`

## Front Doors

| Path | Proxy URL | Control URL |
| --- | --- | --- |
${Object.entries(result.environment.frontDoors).map(([, frontDoor]) => `| ${frontDoor.label} | \`${frontDoor.proxyUrl}\` | \`${frontDoor.controlUrl}\` |`).join("\n")}

## Summary

- Checks: ${result.summary.checks}
- Passed: ${result.summary.passed}
- Failed: ${result.summary.failed}
- Provider captures behind LiteLLM: ${result.summary.providerCaptures}

## Attack Checks

| Check | Result | Status | Error code | Notes |
| --- | --- | ---: | --- | --- |
${rows}

## Provider Capture Behind LiteLLM

| Request | Authorization seen by mock provider | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
${providerRows}

## Interesting Findings

- Canonical \`/v1/chat/completions\`, \`/v1/responses\`, and \`/v1/embeddings\` pass through all three front-door paths to LiteLLM.
- Direct Relayna-key auth remains isolated to the no-Entra path and does not bypass Entra mode.
- Entra rejects missing, expired, wrong-audience, missing-scope, and tampered-signature JWTs before LiteLLM.
- Trusted Apigee mode rejects missing proof, missing signature, bad signature, and missing scope before LiteLLM.
- LiteLLM/mock provider only receives the internal provider credential; Relayna keys, Entra JWTs, and Apigee proof headers do not reach the provider.
- Alias \`/v1/embedding\` and unsupported \`/v1/rerank\` still return \`unsupported_route\` before reaching LiteLLM.

## Screenshot Artifacts

- \`screenshots/01-dashboard.png\`
- \`screenshots/02-attack-checks.png\`
- \`screenshots/03-provider-capture.png\`
- \`screenshots/04-interesting-findings.png\`

## Raw Results

See \`results.json\`.
`;
fs.writeFileSync(reportPath, markdown);
NODE

echo "Report written to $REPORT_MD"
