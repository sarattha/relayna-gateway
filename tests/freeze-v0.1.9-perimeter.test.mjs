import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

function read(path) {
  return readFileSync(join(root, path), "utf8");
}

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    console.error(`not ok - ${name}`);
    throw error;
  }
}

function sorted(values) {
  return [...values].sort();
}

function unique(values) {
  return [...new Set(values)];
}

function stringLiterals(source) {
  return [...source.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

const app = read("crates/gateway-api/src/app.rs");
const routing = read("crates/gateway-core/src/routing.rs");
const errors = read("crates/gateway-core/src/errors.rs");
const config = read("crates/gateway-api/src/config.rs");
const budgets = read("crates/gateway-core/src/budgets.rs");
const rateLimits = read("crates/gateway-core/src/rate_limits.rs");
const redis = read("crates/gateway-store/src/redis.rs");
const adminJs = read("crates/gateway-api/src/static/admin-ui/app.js");
const adminUiTest = read("tests/admin-ui.test.mjs");
const kubernetes = read("deploy/kubernetes/relayna-gateway.yaml");
const cargoToml = read("Cargo.toml");
const changelog = read("CHANGELOG.md");
const releaseWorkflow = read(".github/workflows/release.yml");
const freezeVersion = "0.1.9";

test("current release metadata is valid and v0.1.9 is the freeze baseline", () => {
  const currentVersion = cargoToml.match(
    /\[workspace\.package\][\s\S]*?version = "([^"]+)"/,
  )?.[1];
  assert.match(currentVersion, /^\d+\.\d+\.\d+$/);
  assert.match(changelog, new RegExp(`^## ${currentVersion} -`, "m"));
  assert.match(changelog, new RegExp(`^## ${freezeVersion} -`, "m"));
});

test("control-plane public route inventory is pinned", () => {
  const routes = unique(
    [...app.matchAll(/\.route\(\s*"([^"]+)"/g)].map((match) => match[1]),
  );
  assert.deepEqual(sorted(routes), sorted([
    "/admin-ui",
    "/admin-ui/{*path}",
    "/admin-ui/admin/audit-events",
    "/admin-ui/admin/auth/front-door",
    "/admin-ui/admin/guardrails",
    "/admin-ui/admin/guardrails/{name}",
    "/admin-ui/admin/guardrails/executions",
    "/admin-ui/admin/guardrails/summary",
    "/admin-ui/admin/keys",
    "/admin-ui/admin/keys/{key_id}",
    "/admin-ui/admin/keys/{key_id}/disable",
    "/admin-ui/admin/keys/{key_id}/enable",
    "/admin-ui/admin/keys/{key_id}/revoke",
    "/admin-ui/admin/keys/{key_id}/usage",
    "/admin-ui/admin/openai-routes",
    "/admin-ui/admin/openai-routes/{route_id}/disable",
    "/admin-ui/admin/openai-routes/{route_id}/enable",
    "/admin-ui/admin/openai-routes/{route_id}/mode",
    "/admin-ui/admin/operator-token/rotate",
    "/admin-ui/admin/policy-layers",
    "/admin-ui/admin/policy-layers/{layer_id}",
    "/admin-ui/admin/policy/simulate",
    "/admin-ui/admin/projects",
    "/admin-ui/admin/projects/{project_id}",
    "/admin-ui/admin/projects/{project_id}/usage",
    "/admin-ui/admin/provider-health",
    "/admin-ui/admin/provider-health/check",
    "/admin-ui/admin/provider-health/state",
    "/admin-ui/admin/providers",
    "/admin-ui/admin/providers/litellm-credentials",
    "/admin-ui/admin/providers/litellm-credentials/{mapping_id}",
    "/admin-ui/admin/providers/litellm-credentials/{mapping_id}/disable",
    "/admin-ui/admin/providers/litellm-credentials/{mapping_id}/enable",
    "/admin-ui/admin/providers/litellm-passthrough",
    "/admin-ui/admin/providers/{provider_id}",
    "/admin-ui/admin/providers/{provider_id}/disable",
    "/admin-ui/admin/providers/{provider_id}/enable",
    "/admin-ui/admin/services",
    "/admin-ui/admin/services/{service_name}",
    "/admin-ui/admin/services/{service_name}/disable",
    "/admin-ui/admin/services/{service_name}/enable",
    "/admin-ui/admin/services/{service_name}/sync-status",
    "/admin-ui/admin/services/import",
    "/admin-ui/admin/services/import/activate",
    "/admin-ui/admin/services/import/preview",
    "/admin-ui/admin/services/import/rollback/{version}",
    "/admin-ui/admin/services/import/versions",
    "/admin-ui/admin/services/sync",
    "/admin-ui/admin/studio/connection",
    "/admin-ui/admin/studio/connection/test",
    "/admin-ui/admin/studio/services",
    "/admin-ui/admin/tasks/{task_id}/usage",
    "/admin-ui/admin/usage/by-key",
    "/admin-ui/admin/usage/by-model",
    "/admin-ui/admin/usage/by-project",
    "/admin-ui/admin/usage/by-provider",
    "/admin-ui/admin/usage/by-service",
    "/admin-ui/admin/usage/by-task",
    "/admin-ui/admin/usage/export.csv",
    "/admin-ui/admin/usage/export.json",
    "/admin-ui/admin/usage/summary",
    "/admin-ui/admin/usage/timeseries",
    "/admin-ui/admin/usage/unused-keys",
    "/admin-ui/admin/debug-bundles/{request_id}",
    "/admin-ui/healthz",
    "/admin-ui/metrics",
    "/admin-ui/readyz",
    "/admin-ui/v1/guardrails",
    "/admin-ui/v1/guardrails/test",
  ]));
});

test("proxy route resolver keeps v0.1.9 public route semantics", () => {
  for (const route of [
    "/v1/chat/completions",
    "/v1/responses",
    "/v1/embeddings",
    "/litellm/*",
    "/providers/openai/",
    "/services/",
    "/summary",
    "/translation",
    "/ocr",
    "/embeddings",
  ]) {
    assert.ok(routing.includes(route), `expected routing.rs to include ${route}`);
  }
  for (const routeName of [
    "ChatCompletions",
    "Responses",
    "LiteLlmEmbeddings",
    "DirectOpenAi",
    "LiteLlmPassthrough",
    "Summary",
    "Translation",
    "Ocr",
    "Embeddings",
    "ServiceWildcard",
  ]) {
    assert.match(routing, new RegExp(`\\b${routeName}\\b`));
  }
  assert.match(routing, /method == Method::POST/);
  assert.match(routing, /method == Method::POST && path\.starts_with\("\/providers\/openai\/"\)/);
  assert.match(routing, /path\.starts_with\("\/services\/"\)/);
  assert.match(routing, /timeout_ms: 120_000/);
  assert.match(routing, /timeout_ms: 60_000/);
  assert.match(routing, /max_body_bytes: 1_048_576/);
  assert.match(routing, /max_body_bytes: 2_097_152/);
});

test("public gateway error codes are pinned", () => {
  const codes = unique(
    [...errors.matchAll(/=>\s*"([a-z0-9_]+)"/g)].map((match) => match[1]),
  );
  assert.deepEqual(sorted(codes), sorted([
    "budget_exceeded",
    "control_state_unavailable",
    "disabled_operator_token",
    "disabled_route",
    "disabled_service",
    "disabled_virtual_key",
    "duplicate_project",
    "duplicate_provider_config",
    "duplicate_service",
    "expired_entra_token",
    "expired_virtual_key",
    "guardrail_blocked",
    "guardrail_forbidden",
    "guardrail_unavailable",
    "incomplete_service",
    "insufficient_operator_scope",
    "invalid_configuration",
    "invalid_entra_audience",
    "invalid_entra_issuer",
    "invalid_entra_token",
    "invalid_guardrail_request",
    "invalid_operator_token",
    "invalid_project_payload",
    "invalid_provider_config_payload",
    "invalid_service_payload",
    "invalid_service_upstream",
    "invalid_studio_connection_payload",
    "invalid_virtual_key",
    "insufficient_entra_authorization",
    "malformed_authorization",
    "malformed_entra_authorization",
    "missing_authorization",
    "missing_entra_authorization",
    "missing_project",
    "missing_provider_config",
    "missing_service",
    "policy_denied",
    "project_in_use",
    "rate_limit_exceeded",
    "request_body_too_large",
    "response_body_too_large",
    "revoked_virtual_key",
    "store_unavailable",
    "studio_unavailable",
    "token_rate_limit_exceeded",
    "untrusted_apigee_identity",
    "unsupported_route",
    "upstream_connection",
    "upstream_timeout",
  ]));
  for (const status of [
    "UNAUTHORIZED",
    "NOT_FOUND",
    "FORBIDDEN",
    "PAYLOAD_TOO_LARGE",
    "TOO_MANY_REQUESTS",
    "PAYMENT_REQUIRED",
    "BAD_REQUEST",
    "BAD_GATEWAY",
    "CONFLICT",
    "SERVICE_UNAVAILABLE",
    "GATEWAY_TIMEOUT",
    "INTERNAL_SERVER_ERROR",
  ]) {
    assert.match(errors, new RegExp(`StatusCode::${status}`));
  }
});

test("release configuration environment variables are pinned", () => {
  const fromEnv = config.slice(
    config.indexOf("pub fn from_env()"),
    config.indexOf("fn required("),
  );
  const envNames = unique(
    [...fromEnv.matchAll(/(?:required|optional|optional_bool|optional_csv|optional_u64|optional_i64)\("([A-Z0-9_]+)"\)/g)].map(
      (match) => match[1],
    ),
  );
  assert.deepEqual(sorted(envNames), sorted([
    "DATABASE_URL",
    "APIGEE_TRUSTED_HEADER_ENABLED",
    "APIGEE_TRUSTED_HEADER_SECRET",
    "DIRECT_OPENAI_BASE_URL",
    "DIRECT_OPENAI_SERVICE_KEY",
    "ENTRA_ACCEPTED_ALGORITHMS",
    "ENTRA_ALLOWED_GROUPS",
    "ENTRA_AUDIENCE",
    "ENTRA_AUTH_ENABLED",
    "ENTRA_CLOCK_SKEW_SECONDS",
    "ENTRA_ISSUER",
    "ENTRA_JWKS_CACHE_TTL_SECONDS",
    "ENTRA_OIDC_DISCOVERY_URL",
    "ENTRA_REQUIRED_ROLE",
    "ENTRA_REQUIRED_SCOPE",
    "ENTRA_RELAYNA_KEY_HEADER",
    "ENTRA_TENANT_ID",
    "GATEWAY_BIND_ADDR",
    "GATEWAY_ADMIN_TOKEN",
    "GATEWAY_CONTROL_BIND_ADDR",
    "GUARDRAIL_MAPPING_ENCRYPTION_KEY",
    "GUARDRAIL_PII_MAPPING_TTL_SECONDS",
    "LITELLM_BASE_URL",
    "LITELLM_SERVICE_KEY",
    "LOG_LEVEL",
    "REDIS_URL",
    "RELAYNA_STUDIO_BASE_URL",
    "RELAYNA_STUDIO_TOKEN",
    "RELAYNA_WORKER_TOKEN",
  ]));
  assert.match(config, /unwrap_or\(3600\)/);
});

test("PostgreSQL migration inventory is pinned", () => {
  const migrations = readdirSync(join(root, "crates/gateway-store/migrations"))
    .filter((name) => name.endsWith(".sql"))
    .sort();
  assert.deepEqual(migrations, [
    "20260508000100_phase_1_core_proxy.sql",
    "20260509000100_phase_2_policy_keys_limits_budget.sql",
    "20260509000200_phase_3_5_usage_routing_observability.sql",
    "20260510000100_phase_6_service_registry.sql",
    "20260510000200_phase_7_operator_tokens.sql",
    "20260511000100_openai_route_settings.sql",
    "20260512000100_admin_projects_provider_configs.sql",
    "20260514000100_project_first_key_service_links.sql",
    "20260516000100_studio_connection_settings.sql",
    "20260516000200_guardrail_registry_policy_events.sql",
    "20260516000600_key_guardrail_config_overrides.sql",
    "20260522000100_operator_scopes_audit_events.sql",
    "20260523000100_policy_governance_lifecycle.sql",
    "20260523000200_provider_intelligence.sql",
    "20260523000300_phase_4_observability_analytics.sql",
    "20260525000100_service_health_check_paths.sql",
    "20260530000100_litellm_embeddings_route.sql",
    "20260530000200_gateway_auth_settings.sql",
    "20260531000100_litellm_credential_mapping.sql",
    "20260618000100_litellm_passthrough_settings.sql",
  ]);
});

test("Redis key formats and TTLs are pinned", () => {
  assert.match(rateLimits, /format!\("rl:req:\{key_id\}:\{\}", now\.format\("%Y%m%d%H%M"\)\)/);
  assert.match(rateLimits, /format!\("rl:tpm:\{key_id\}:\{\}", now\.format\("%Y%m%d%H%M"\)\)/);
  assert.match(budgets, /format!\("budget:daily:\{key_id\}:\{\}", now\.format\("%Y%m%d"\)\)/);
  assert.match(budgets, /format!\("budget:monthly:\{key_id\}:\{\}", now\.format\("%Y%m"\)\)/);
  assert.match(budgets, /format!\("budget:reservation:\{key_id\}:\{request_id\}"\)/);
  assert.match(redis, /\.arg\(70\)/);
  assert.match(redis, /\.arg\(172_800\)/);
  assert.match(redis, /\.arg\(5_356_800\)/);
  assert.match(redis, /\.arg\(3600\)/);
  assert.match(redis, /format!\("\{amount_usd\}\|\{daily_key\}\|\{monthly_key\}"\)/);
});

test("admin portal static test covers all control endpoints it depends on", () => {
  const routeStrings = stringLiterals(app).filter((value) => value.startsWith("/"));
  const requiredUiEndpoints = [
    "/admin-ui/admin/guardrails",
    "/admin-ui/admin/guardrails/executions",
    "/admin-ui/admin/guardrails/summary",
    "/admin-ui/admin/keys",
    "/admin-ui/admin/openai-routes",
    "/admin-ui/admin/operator-token/rotate",
    "/admin-ui/admin/projects",
    "/admin-ui/admin/provider-health",
    "/admin-ui/admin/provider-health/check",
    "/admin-ui/admin/provider-health/state",
    "/admin-ui/admin/providers",
    "/admin-ui/admin/providers/litellm-credentials",
    "/admin-ui/admin/providers/litellm-passthrough",
    "/admin-ui/admin/services",
    "/admin-ui/admin/services/import",
    "/admin-ui/admin/services/import/activate",
    "/admin-ui/admin/services/import/preview",
    "/admin-ui/admin/services/import/rollback",
    "/admin-ui/admin/services/import/versions",
    "/admin-ui/admin/studio/connection",
    "/admin-ui/admin/studio/connection/test",
    "/admin-ui/admin/studio/services",
    "/admin-ui/admin/usage/by-key",
    "/admin-ui/admin/usage/by-model",
    "/admin-ui/admin/usage/by-project",
    "/admin-ui/admin/usage/by-provider",
    "/admin-ui/admin/usage/by-service",
    "/admin-ui/admin/usage/summary",
    "/admin-ui/admin/usage/unused-keys",
    "/admin-ui/readyz",
  ];
  for (const endpoint of requiredUiEndpoints) {
    assert.ok(
      routeStrings.some((route) => route === endpoint || route.startsWith(`${endpoint}/`)),
      `expected app router to expose ${endpoint}`,
    );
    assert.ok(
      adminUiTest.includes(endpoint) || adminJs.includes(endpoint),
      `expected admin portal assets or admin-ui.test.mjs to reference ${endpoint}`,
    );
  }
});

test("kubernetes control probes use the admin-ui base path", () => {
  assert.match(kubernetes, /path: \/admin-ui\/readyz/);
  assert.match(kubernetes, /path: \/admin-ui\/healthz/);
  assert.match(kubernetes, /path: \/admin-ui\/metrics/);
  assert.doesNotMatch(kubernetes, /path: \/(readyz|healthz|metrics)\b/);
});

test("kubernetes production hardening remains enabled", () => {
  for (const expected of [
    /runAsNonRoot: true/,
    /runAsUser: 10001/,
    /runAsGroup: 10001/,
    /fsGroup: 10001/,
    /seccompProfile:\s*\n\s*type: RuntimeDefault/,
    /readOnlyRootFilesystem: true/,
    /allowPrivilegeEscalation: false/,
    /drop:\s*\n\s*- ALL/,
    /name: relayna-gateway-proxy/,
    /name: relayna-gateway-control/,
  ]) {
    assert.match(kubernetes, expected);
  }
  assert.doesNotMatch(kubernetes, /ingress:\s*\n\s*- \{\}/);
  assert.doesNotMatch(kubernetes, /egress:\s*\n\s*- \{\}/);
});

test("release image latest tag is gated only by explicit metadata tag", () => {
  assert.match(releaseWorkflow, /flavor:\s*\|\s*\n\s*latest=false/);
  assert.match(
    releaseWorkflow,
    /type=raw,value=latest,enable=\$\{\{ steps\.latest_tag\.outputs\.enabled == 'true' \}\}/,
  );
});

test("release workflow publishes supply-chain artifacts", () => {
  for (const expected of [
    /id-token: write/,
    /attestations: write/,
    /anchore\/sbom-action\/download-syft@v0/,
    /spdx-json=relayna-gateway-\$\{\{ github\.ref_name \}\}\.spdx\.json/,
    /aquasecurity\/trivy-action@v0\.36\.0/,
    /docker\/setup-buildx-action@v3/,
    /docker\/build-push-action@v6/,
    /provenance: true/,
    /sigstore\/cosign-installer@v3/,
    /cosign sign/,
    /actions\/attest-build-provenance@v2/,
  ]) {
    assert.match(releaseWorkflow, expected);
  }
});
