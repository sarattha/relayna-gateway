import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
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
const cargoToml = read("Cargo.toml");
const changelog = read("CHANGELOG.md");

test("freeze baseline release metadata remains v0.0.9", () => {
  assert.match(cargoToml, /\[workspace\.package\][\s\S]*version = "0\.0\.9"/);
  assert.match(changelog, /^## 0\.0\.9 -/m);
  const tag = execFileSync("git", ["tag", "-l", "v0.0.9"], {
    cwd: root,
    encoding: "utf8",
  }).trim();
  assert.equal(tag, "v0.0.9");
});

test("control-plane public route inventory is pinned", () => {
  const routes = unique(
    [...app.matchAll(/\.route\(\s*"([^"]+)"/g)].map((match) => match[1]),
  );
  assert.deepEqual(sorted(routes), sorted([
    "/admin-ui",
    "/admin-ui/{*path}",
    "/admin/guardrails",
    "/admin/guardrails/{name}",
    "/admin/guardrails/executions",
    "/admin/guardrails/summary",
    "/admin/keys",
    "/admin/keys/{key_id}",
    "/admin/keys/{key_id}/disable",
    "/admin/keys/{key_id}/enable",
    "/admin/keys/{key_id}/revoke",
    "/admin/keys/{key_id}/usage",
    "/admin/openai-routes",
    "/admin/openai-routes/{route_id}/disable",
    "/admin/openai-routes/{route_id}/enable",
    "/admin/operator-token/rotate",
    "/admin/projects",
    "/admin/projects/{project_id}",
    "/admin/projects/{project_id}/usage",
    "/admin/provider-health",
    "/admin/providers",
    "/admin/providers/{provider_id}",
    "/admin/providers/{provider_id}/disable",
    "/admin/providers/{provider_id}/enable",
    "/admin/services",
    "/admin/services/{service_name}",
    "/admin/services/{service_name}/disable",
    "/admin/services/{service_name}/enable",
    "/admin/services/{service_name}/sync-status",
    "/admin/services/import",
    "/admin/services/sync",
    "/admin/studio/connection",
    "/admin/studio/connection/test",
    "/admin/studio/services",
    "/admin/tasks/{task_id}/usage",
    "/admin/usage/by-key",
    "/admin/usage/by-model",
    "/admin/usage/by-project",
    "/admin/usage/by-provider",
    "/admin/usage/by-service",
    "/admin/usage/by-task",
    "/admin/usage/summary",
    "/admin/usage/timeseries",
    "/healthz",
    "/metrics",
    "/readyz",
    "/v1/guardrails",
    "/v1/guardrails/test",
  ]));
});

test("proxy route resolver keeps v0.0.9 public route semantics", () => {
  for (const route of [
    "/v1/chat/completions",
    "/v1/responses",
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
    "DirectOpenAi",
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
    "expired_virtual_key",
    "guardrail_blocked",
    "guardrail_forbidden",
    "guardrail_unavailable",
    "incomplete_service",
    "invalid_configuration",
    "invalid_guardrail_request",
    "invalid_operator_token",
    "invalid_project_payload",
    "invalid_provider_config_payload",
    "invalid_service_payload",
    "invalid_service_upstream",
    "invalid_studio_connection_payload",
    "invalid_virtual_key",
    "malformed_authorization",
    "missing_authorization",
    "missing_project",
    "missing_provider_config",
    "missing_service",
    "policy_denied",
    "project_in_use",
    "rate_limit_exceeded",
    "request_body_too_large",
    "revoked_virtual_key",
    "store_unavailable",
    "studio_unavailable",
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
    [...fromEnv.matchAll(/(?:required|optional)\("([A-Z0-9_]+)"\)/g)].map(
      (match) => match[1],
    ),
  );
  assert.deepEqual(sorted(envNames), sorted([
    "DATABASE_URL",
    "DIRECT_OPENAI_BASE_URL",
    "DIRECT_OPENAI_SERVICE_KEY",
    "GATEWAY_BIND_ADDR",
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
  ]);
});

test("Redis key formats and TTLs are pinned", () => {
  assert.match(rateLimits, /format!\("rl:req:\{key_id\}:\{\}", now\.format\("%Y%m%d%H%M"\)\)/);
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
    "/admin/guardrails",
    "/admin/guardrails/executions",
    "/admin/guardrails/summary",
    "/admin/keys",
    "/admin/openai-routes",
    "/admin/operator-token/rotate",
    "/admin/projects",
    "/admin/provider-health",
    "/admin/providers",
    "/admin/services",
    "/admin/services/import",
    "/admin/studio/connection",
    "/admin/studio/connection/test",
    "/admin/studio/services",
    "/admin/usage/by-key",
    "/admin/usage/by-project",
    "/admin/usage/by-service",
    "/admin/usage/summary",
    "/readyz",
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
