#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
HARNESS_DIR="$ROOT_DIR/tests/e2e/litellm-real-passthrough"
REPORT_DIR="$ROOT_DIR/target/e2e/litellm-real-passthrough"
COMPOSE_FILE="$HARNESS_DIR/docker-compose.yml"
RESULT_JSON="$REPORT_DIR/results.json"
REPORT_MD="$REPORT_DIR/report.md"

mkdir -p "$REPORT_DIR"
cd "$HARNESS_DIR"

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
function redactReportSecrets(value) {
  if (Array.isArray(value)) {
    return value.map(redactReportSecrets);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [
        key,
        key.toLowerCase() === "token" ? "[redacted]" : redactReportSecrets(entry),
      ]),
    );
  }
  return value;
}
const sanitized = redactReportSecrets(result);
fs.writeFileSync(resultPath, `${JSON.stringify(sanitized, null, 2)}\n`);
const rows = Object.entries(result.checks)
  .map(([name, check]) => `| ${name.replaceAll("_", " ")} | ${check.ok ? "PASS" : "FAIL"} | ${check.status ?? "n/a"} | ${check.body?.error?.code ?? ""} |`)
  .join("\n");
const providerRows = result.providerRequests
  .map((capture) => `| ${capture.method} ${capture.path} | ${capture.authorization} | ${capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no"} | ${capture.hasApigeeIdentity ? "yes" : "no"} |`)
  .join("\n");
const markdown = `# LiteLLM Real Passthrough Test Report

Generated: ${result.generatedAt}

Overall result: **${result.ok ? "PASS" : "FAIL"}**

${result.overallOutcome}

## Environment

- Gateway proxy: \`${result.environment.gatewayProxyUrl}\`
- Gateway control: \`${result.environment.gatewayControlUrl}\`
- LiteLLM direct upstream: \`${result.environment.litellmUrl}\`
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

## Wildcard Coverage

The current branch routes managed canonical calls through LiteLLM, can switch a
canonical OpenAI or Anthropic route to direct LiteLLM passthrough, and keeps
canonical \`/v1/models\` ahead of wildcard passthrough.

The browser-safe LiteLLM UI path is also covered: unauthenticated
\`/admin-ui/litellm-ui/\` is rejected, while the operator-authenticated path
reaches the real LiteLLM \`/ui/\` through Gateway.

The trusted-ingress LiteLLM UI path is covered with Relayna Gateway Entra and
Apigee checks disabled: unauthenticated \`/ui/\` and the UI support endpoint
\`/user/info\` reach real LiteLLM. The model endpoint \`/v1/models\` remains a
canonical Anthropic route and still requires normal Gateway route authentication.

The literal unsupported probes below reached real LiteLLM and were rejected
there with 404 responses, proving they were not stopped by the Gateway router:

- \`/v1/chatcompletion\`
- \`/v1/response\`
- \`/v1/embedding\`

The governed rerank route separately verifies successful requests through all
LiteLLM aliases: \`/rerank\`, \`/v1/rerank\`, and \`/v2/rerank\`.

## Raw Results

See \`results.json\`.
`;
fs.writeFileSync(reportPath, markdown);
NODE

echo "Report written to $REPORT_MD"
