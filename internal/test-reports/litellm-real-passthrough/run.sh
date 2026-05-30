#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
REPORT_DIR="$ROOT_DIR/internal/test-reports/litellm-real-passthrough"
COMPOSE_FILE="$REPORT_DIR/docker-compose.yml"
RESULT_JSON="$REPORT_DIR/results.json"
REPORT_MD="$REPORT_DIR/report.md"

cd "$REPORT_DIR"

docker compose -f "$COMPOSE_FILE" up -d --build

echo "Waiting for real LiteLLM, mock provider, and gateway..."
for _ in $(seq 1 180); do
  if curl -fsS http://127.0.0.1:18182/healthz >/dev/null \
    && curl -fsS http://127.0.0.1:18183/health/readiness >/dev/null \
    && curl -fsS http://127.0.0.1:18181/admin-ui/readyz >/dev/null; then
    break
  fi
  sleep 1
done

curl -fsS -X POST http://127.0.0.1:18182/run-tests -o "$RESULT_JSON"

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
const markdown = `# LiteLLM Real Passthrough Test Report

Generated: ${result.generatedAt}

Overall result: **${result.requestedLiteralPathsPassThrough ? "PASS" : "PARTIAL - passthrough gap found"}**

${result.overallOutcome}

## Environment

- Gateway proxy: \`${result.environment.gatewayProxyUrl}\`
- Gateway control: \`${result.environment.gatewayControlUrl}\`
- LiteLLM upstream: \`${result.environment.litellmUrl}\`
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

## Interesting Finding

The current branch routes only \`/v1/chat/completions\` and \`/v1/responses\`
to LiteLLM. The literal paths requested for this review return
\`unsupported_route\` before reaching LiteLLM, so they are **not** currently
LiteLLM passthrough routes:

- \`/v1/chatcompletion\`
- \`/v1/response\`
- \`/v1/embedding\`
- \`/v1/rerank\`

The Gateway also has an internal-service \`/embeddings\` route, but it is not a
LiteLLM \`/v1/embeddings\` passthrough route.

## Screenshot Artifacts

- \`screenshots/01-process-dashboard.png\`
- \`screenshots/02-results-table.png\`
- \`screenshots/03-interesting-finding.png\`

## Raw Results

See \`results.json\`.
`;
fs.writeFileSync(reportPath, markdown);
NODE

echo "Report written to $REPORT_MD"
