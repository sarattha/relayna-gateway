#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
REPORT_DIR="$ROOT_DIR/internal/test-reports/litellm-real-passthrough"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
RESULT_JSON="$REPORT_DIR/results.json"
REPORT_MD="$REPORT_DIR/report.md"

cd "$REPORT_DIR"

docker compose -f "$COMPOSE_FILE" down -v --remove-orphans >/dev/null 2>&1 || true
docker compose -f "$COMPOSE_FILE" up -d --build --force-recreate

echo "Waiting for real LiteLLM, mock provider, and gateway..."
for _ in $(seq 1 180); do
  if curl -fsS http://127.0.0.1:19182/healthz >/dev/null \
    && curl -fsS http://127.0.0.1:19183/health/readiness >/dev/null \
    && curl -fsS http://127.0.0.1:19181/admin-ui/readyz >/dev/null; then
    break
  fi
  sleep 1
done

curl -fsS -X POST http://127.0.0.1:19182/run-tests -o "$RESULT_JSON"

node - "$RESULT_JSON" "$REPORT_MD" <<'NODE'
const fs = require("node:fs");
const [resultPath, reportPath] = process.argv.slice(2);
const result = JSON.parse(fs.readFileSync(resultPath, "utf8"));
const rows = Object.entries(result.checks)
  .map(([name, check]) => `| ${name.replaceAll("_", " ")} | ${check.ok ? "PASS" : "FAIL"} | ${check.status ?? "n/a"} | ${check.body?.error?.code ?? ""} |`)
  .join("\n");
const providerRows = result.providerRequests
  .map((capture) => `| ${capture.method} ${capture.path} | ${capture.authorization} | ${capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no"} | ${capture.hasApigeeIdentity ? "yes" : "no"} |`)
  .join("\n");
const frontDoorRows = result.frontDoorRequests
  .map((capture) => `| ${capture.method} ${capture.path} | ${capture.authorization || ""} | ${capture.litellmApiKey || ""} | ${capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no"} |`)
  .join("\n");
const markdown = `# LiteLLM Real Passthrough Test Report

Generated: ${result.generatedAt}

Overall result: **${result.ok ? "PASS" : "FAIL"}**

${result.overallOutcome}

## Environment

- Gateway proxy: \`${result.environment.gatewayProxyUrl}\`
- Gateway control: \`${result.environment.gatewayControlUrl}\`
- LiteLLM upstream: \`${result.environment.litellmUrl}\`
- LiteLLM front door: \`http://litellm-front-door:4000\`
- LiteLLM image: \`docker.io/litellm/litellm:latest\`
- LiteLLM image digest pulled locally: \`sha256:cae1ac3492d6d0bea69c26f4485381624e073eb753f3534ae7703a4204a4ce6b\`
- Mock OIDC issuer: \`${result.environment.issuer}\`
- Audience: \`${result.environment.audience}\`
- Tenant: \`${result.environment.tenantId}\`
- Relayna key header: \`${result.environment.relaynaKeyHeader}\`
- Trusted Apigee header mode: \`${result.environment.apigeeTrustedHeader}\`

## Checks

| Check | Result | Status | Error code |
| --- | --- | ---: | --- |
${rows}

## Provider Capture Behind LiteLLM

| Request | Authorization seen by mock provider | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
${providerRows}

## LiteLLM Front-Door Capture

| Request | Authorization from Gateway | x-litellm-api-key from Gateway | Client credential leaked? |
| --- | --- | --- | --- |
${frontDoorRows}

Observed LiteLLM credential precedence:
\`${result.mappingCredentialsObserved.join(" -> ")}\`

## Wildcard Coverage

The current branch routes managed canonical calls through LiteLLM, can switch a
canonical route to direct LiteLLM passthrough, and forwards enabled wildcard
\`/v1/*\` calls while preserving path and query.

The browser-safe LiteLLM UI path is also covered: unauthenticated
\`/admin-ui/litellm-ui/\` is rejected, while the operator-authenticated path
reaches the real LiteLLM \`/ui/\` through Gateway with only the server-side
LiteLLM credential forwarded.

The literal alias probes below reached real LiteLLM and were rejected there with
404 or 400 responses, proving they were not stopped by the Gateway router:

- \`/v1/chatcompletion\`
- \`/v1/response\`
- \`/v1/embedding\`
- \`/v1/rerank\`

## Screenshot Artifacts

- \`screenshots/01-admin-ui-providers-litellm-mapping.png\`
- \`screenshots/02-admin-ui-project-mapping-control.png\`
- \`screenshots/03-real-env-report-overview.png\`
- \`screenshots/04-real-env-credential-capture.png\`
- \`screenshots/05-real-litellm-issue-64-report.png\`
- \`screenshots/06-admin-ui-litellm-passthrough-controls.png\`
- \`screenshots/07-admin-ui-route-mode-controls.png\`
- \`screenshots/08-litellm-ui-proxy-real-env.png\`
- \`screenshots/09-real-env-issue-66-report.png\`

## Raw Results

See \`results.json\`.
`;
fs.writeFileSync(reportPath, markdown);
NODE

echo "Report written to $REPORT_MD"
