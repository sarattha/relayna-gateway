#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
REPORT_DIR="$ROOT_DIR/internal/test-reports/entra-front-door-real-env"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
RESULT_JSON="$REPORT_DIR/results.json"
REPORT_MD="$REPORT_DIR/report.md"

cd "$REPORT_DIR"

docker compose -f "$COMPOSE_FILE" up -d --build

echo "Waiting for mock app and gateway..."
for _ in $(seq 1 120); do
  if curl -fsS http://127.0.0.1:18082/healthz >/dev/null \
    && curl -fsS http://127.0.0.1:18081/admin-ui/readyz >/dev/null; then
    break
  fi
  sleep 1
done

curl -fsS -X POST http://127.0.0.1:18082/run-tests -o "$RESULT_JSON"

node - "$RESULT_JSON" "$REPORT_MD" <<'NODE'
const fs = require("node:fs");
const [resultPath, reportPath] = process.argv.slice(2);
const result = JSON.parse(fs.readFileSync(resultPath, "utf8"));
const rows = Object.entries(result.checks)
  .map(([name, check]) => `| ${name.replaceAll("_", " ")} | ${check.ok ? "PASS" : "FAIL"} | ${check.status ?? "n/a"} | ${check.body?.error?.code ?? ""} |`)
  .join("\n");
const upstreamRows = result.upstreamRequests
  .map((capture) => `| ${capture.path} | ${capture.authorization} | ${capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no"} | ${capture.hasApigeeIdentity ? "yes" : "no"} |`)
  .join("\n");
const markdown = `# Entra Front Door Real Environment Test Report

Generated: ${result.generatedAt}

Overall result: **${result.ok ? "PASS" : "FAIL"}**

## Environment

- Gateway proxy: \`${result.environment.gatewayProxyUrl}\`
- Gateway control: \`${result.environment.gatewayControlUrl}\`
- Mock OIDC issuer: \`${result.environment.issuer}\`
- Audience: \`${result.environment.audience}\`
- Tenant: \`${result.environment.tenantId}\`
- Relayna key header: \`${result.environment.relaynaKeyHeader}\`
- Trusted Apigee header mode: \`${result.environment.apigeeTrustedHeader}\`

## Checks

| Check | Result | Status | Error code |
| --- | --- | ---: | --- |
${rows}

## Upstream Credential Capture

| Path | Upstream authorization | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
${upstreamRows}

## Screenshot Artifacts

- \`screenshots/entra-review-dashboard.jpg\`
- \`screenshots/entra-review-results-json.jpg\`

## Raw Results

See \`results.json\`.
`;
fs.writeFileSync(reportPath, markdown);
NODE

echo "Report written to $REPORT_MD"
