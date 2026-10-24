// Synthetic visual-design fixture. Never use its data as health/performance evidence.
// No upstream, credentials, token files, persistence, or production writes.
// Run node tests/fixtures/admin-ui-design-preview.mjs --help for controls.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const assets = new URL("../../crates/gateway-api/src/static/admin-ui/", import.meta.url);
const modes = ["both", "admin", "owner", "login", "pending", "blocked"];
const minute = 60_000;
const day = 86_400_000;
const id = (n) => `d3519000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const numberParam = (query, name, fallback, maximum = 500) => {
  const value = query.get(name);
  return value !== null && /^\d+$/.test(value) ? Math.min(Number(value), maximum) : fallback;
};
const copy = (value) => structuredClone(value);

export function createDesignDataset(now = Date.now()) {
  const captured = new Date(now).getTime();
  if (!Number.isFinite(captured)) throw new Error("--now must be a valid ISO timestamp");
  const at = (ago = 0) => new Date(captured - ago).toISOString();
  const dates = { created_at: at(45 * day), updated_at: at(2 * day) };
  const tenant = id(900);
  const audience = id(901);
  const entra = { audience, required_scopes: ["gateway.invoke"], required_roles: [], allowed_groups: [], allow_apigee: false };
  const socket = { idle_timeout_ms: 60000, max_connections: 100, max_connections_per_key: 10, max_frame_bytes: 1048576, max_session_duration_seconds: 3600 };
  const access = { skip_entra: true, entra: null, accessa: null };
  const guardrailPolicy = { mandatory_guardrails: ["payload-safety"], optional_guardrails: ["data-classification"], forbidden_guardrails: [], guardrail_config_overrides: {} };
  const policy = {
    deny: false, allowed_routes: ["/v1/chat/completions", "/v1/responses", "/v1/messages", "/v1/embeddings", "/services/*"],
    allowed_models: [], allowed_providers: ["litellm", "internal-service"], allowed_services: [],
    rpm_limit: 600, tpm_limit: 120000, daily_budget_usd: 50, monthly_budget_usd: 1000,
    allow_streaming: true, allow_tools: true, max_requests_per_day: 20000,
    max_tokens_per_day: 2000000, max_cost_per_request: 1, max_input_tokens_per_request: 16000,
    max_output_tokens_per_request: 4096, allowed_hours_utc: [], unused_key_auto_disable_after_days: 90,
    max_request_body_bytes: 2097152, max_response_body_bytes: 4194304,
    max_stream_duration_seconds: 300, max_sse_event_bytes: 65536, max_tool_call_count: 8,
    max_tool_schema_bytes: 65536, policy_version: 3,
  };
  const projects = [
    { id: id(1), name: "Customer operations", service_names: ["invoice-parser", "support-stream", "accessa-assistant"], ...dates },
    { id: id(2), name: "Research platform", service_names: ["invoice-parser", "foundry-assistant"], ...dates },
    { id: id(3), name: "Partner sandbox", service_names: [], ...dates },
  ];
  const service = (name, extra = {}) => ({
    name, project_id: null, studio_service_id: null, foundry: null,
    route_pattern: `/services/${name}/*`, upstream_base_url: `https://${name}.example.test`,
    health_check_path: "/health", health_check_method: "GET", enabled: true,
    allowed_methods: ["POST"], credential_configured: true, timeout_ms: 60000,
    max_body_bytes: 2097152, cost_mode: "fixed", estimated_cost_usd: 0.008,
    pricing_rules: [], openapi_source_path: "/openapi.json", openapi_schema_hash: "synthetic-schema-v2",
    openapi_synced_at: at(day),
    openapi_endpoints: [{ method: "POST", path_template: "/run", operation_id: "run", summary: "Synthetic operation", relayna_default: true }],
    endpoint_pricing_rules: [{ method: "POST", path_template: "/run", operation_id: "run", cost_mode: "fixed", estimated_cost_usd: 0.008 }],
    fallback_services: [], source: "gateway", sync_status: "local", last_synced_at: null,
    disabled_at: null, missing_runtime_fields: [], access: copy(access), ...dates, ...extra,
  });
  const services = [
    service("invoice-parser", { source: "studio", sync_status: "synced", studio_service_id: "synthetic-invoice", last_synced_at: at(day), access: { ...access, skip_entra: false, entra } }),
    service("support-stream", { allowed_methods: ["GET", "POST"], access: { ...access, websocket: socket } }),
    service("accessa-assistant", { route_pattern: "/app/design-assistant/channel/web/v1/*", allowed_methods: ["GET", "POST"], access: { ...access, skip_entra: false, entra, accessa: { app: "design-assistant", channel: "web", idle_timeout_ms: 60000, max_connections: 100, max_connections_per_key: 10, max_frame_bytes: 1048576 }, user_token_forwarding: { enabled: true, header: "x-user-token" } } }),
    service("foundry-assistant", { upstream_base_url: null, foundry: { mode: "registered_agent", provider_id: id(103), agent_name: "design-assistant", agent_version: "2" }, health_check_path: null, credential_configured: false, cost_mode: "passthrough", estimated_cost_usd: null }),
    service("catalog-search", { source: "studio", sync_status: "stale", studio_service_id: "synthetic-catalog", last_synced_at: at(5 * day), allowed_methods: ["GET", "POST"], credential_configured: false }),
    service("legacy-enrichment", { enabled: false, disabled_at: at(day), source: "studio", sync_status: "incomplete", credential_configured: false, upstream_base_url: null, missing_runtime_fields: ["upstream_base_url", "credential"], openapi_endpoints: [], endpoint_pricing_rules: [] }),
  ];
  services[0].access.authentication_profiles = {
    revision: 2,
    profiles: [
      { id: "workload", name: "Workload identity", enabled: true, type: "entra_and_relayna_key", entra },
      { id: "key-only", name: "Key only", enabled: true, type: "relayna_key_only" },
    ],
    bindings: [{ key_id: id(201), profile_id: "workload" }, { key_id: id(202), profile_id: "key-only" }],
  };
  const providers = [
    { id: id(101), provider: "litellm", name: "LiteLLM primary", base_url: "https://litellm.example.test", enabled: true, credential_configured: true },
    { id: id(102), provider: "internal-service", name: "Internal HTTP services", base_url: "https://services.example.test", enabled: true, credential_configured: true },
    { id: id(103), provider: "azure-foundry", name: "Foundry workload identity", base_url: "https://design.services.ai.azure.com/api/projects/design", enabled: true, credential_configured: false, foundry: { method: "workload_identity", tenant_id: tenant, client_id: id(902) } },
    { id: id(104), provider: "litellm", name: "LiteLLM recovery", base_url: "https://recovery.example.test", enabled: true, credential_configured: false },
    // Deliberately incomplete historical DTO: absence must render as unknown, not missing.
    { id: id(105), provider: "internal-service", name: "Legacy configuration · unknown credential", base_url: "https://legacy.example.test", enabled: true },
    { id: id(106), provider: "azure-foundry", name: "Foundry retired connection", base_url: "https://retired.services.ai.azure.com/api/projects/design", enabled: false, credential_configured: false, foundry: { method: "client_secret", tenant_id: tenant, client_id: id(903) } },
  ].map((row) => ({ foundry: null, credential_header_mode: "authorization_bearer", credential_header_name: null, credential_header_value_format: "raw", ...dates, ...row }));
  const keys = [
    { name: "Customer automation", project_id: id(1), owner_type: "project", expires_at: at(-30 * day) },
    { name: "Research notebooks", project_id: id(2), owner_type: "project", expires_at: null },
    { name: "Expired partner evaluation", project_id: id(3), owner_type: "project", expires_at: at(day) },
    { name: "Revoked migration worker", project_id: id(1), owner_type: "project", revoked_at: at(2 * day) },
    { name: "Disabled investigation", project_id: null, owner_type: "individual", disabled: true },
    { name: "Personal exploration", project_id: null, owner_type: "individual", rotation_due_at: at(day), service_names: ["catalog-search"], expires_at: at(-5 * day) },
  ].map((row, i) => ({
    id: id(201 + i), key_prefix: `rk_live_design${i + 1}`, disabled: false, revoked_at: null,
    expires_at: null, rotation_due_at: at(-14 * day), last_used_at: i === 2 || i === 4 ? null : at((i + 1) * minute),
    service_names: [], policy: copy(policy), guardrail_policy: copy(guardrailPolicy), ...dates, ...row,
  }));
  const member = (n, name, status, roles = []) => ({ id: id(300 + n), tenant_id: tenant, object_id: id(400 + n), email: `${name.toLowerCase().replaceAll(" ", ".")}@example.test`, display_name: name, status, roles, last_sign_in_at: at(n * 12 * minute), ...dates });
  const membership = (memberId, field, value, role) => ({ member_id: memberId, [field]: value, role, ...dates });
  const serviceMemberships = [0, 1, 2, 4].map((n, i) => membership(id(301), "service_name", services[n].name, i % 2 ? "viewer" : "owner"));
  const projectMemberships = projects.slice(0, 2).map((row, i) => membership(id(301), "project_id", row.id, i ? "viewer" : "owner"));
  const members = [
    { member: member(1, "Design preview", "active", ["admin"]), service_memberships: serviceMemberships, project_memberships: projectMemberships },
    { member: member(2, "Mira Chen", "active"), service_memberships: [membership(id(302), "service_name", services[0].name, "owner")], project_memberships: [membership(id(302), "project_id", id(1), "owner")] },
    { member: member(3, "Owen Park", "pending"), service_memberships: [], project_memberships: [] },
    { member: member(4, "Sam Rivera", "blocked"), service_memberships: [membership(id(304), "service_name", services[4].name, "viewer")], project_memberships: [] },
    { member: member(5, "Alex Morgan", "active", ["admin"]), service_memberships: [], project_memberships: [] },
  ];
  const identity = (n, enabled, extra) => ({ id: id(500 + n), display_name: `Synthetic monitor ${n}`, tenant_id: tenant, client_id: id(600 + n), object_id: n === 1 ? id(700 + n) : null, required_role: "gateway.monitor.read", enabled, ...dates, ...extra });
  const managedIdentities = [identity(1, true, { service_name: services[0].name }), identity(2, false, { service_name: services[4].name })];
  const managedIdentityProjects = [identity(3, true, { project_id: id(1) }), identity(4, false, { project_id: id(2) })];
  const route = (route_id, path, mode = "managed_by_gateway", enabled = true) => ({ route_id, route: path, enabled, mode, timeout_ms: 120000, max_request_body_bytes: 2097152, max_response_body_bytes: 4194304, ...dates });
  const openaiRoutes = [route("chat-completions", "/v1/chat/completions"), route("responses", "/v1/responses", "direct_litellm_passthrough"), route("embeddings", "/v1/embeddings"), route("rerank", "/v1/rerank", "managed_by_gateway", false)];
  const anthropicRoutes = [route("messages", "/v1/messages"), route("messages-count-tokens", "/v1/messages/count_tokens", "direct_litellm_passthrough"), route("message-batches", "/v1/messages/batches"), route("message-batch", "/v1/messages/batches/*"), route("message-batch-results", "/v1/messages/batches/*/results", "direct_litellm_passthrough"), route("message-batch-cancel", "/v1/messages/batches/*/cancel", "managed_by_gateway", false), route("models", "/v1/models", "direct_litellm_passthrough")];
  const routeIdentities = [...openaiRoutes, ...anthropicRoutes].map((row) => ({ route: row.route, access: row.route === "/v1/responses" ? { ...access, websocket: socket } : { ...access, skip_entra: false, entra } }));
  routeIdentities.push({ route: "/v1/*", access: copy(access) });
  const policyLayers = [
    ["global", null], ["project", id(1)], ["team", "customer-platform"], ["route", "/v1/chat/completions"], ["model", "gpt-4.1-mini"],
  ].map(([kind, scope_id], i) => ({ id: id(800 + i), kind, scope_id, policy: { ...policy, allowed_models: kind === "model" ? [scope_id] : [] }, guardrail_policy: copy(guardrailPolicy), ...dates }));
  const guardrails = [
    { name: "payload-safety", description: "Synthetic built-in payload validation", provider_kind: "built_in", modes: ["pre_call"], default_on: true, failure_policy: "fail_closed", enabled: true, endpoint_configured: false, endpoint_url: null, timeout_ms: null, token_configured: false },
    { name: "data-classification", description: "Synthetic data classification service", provider_kind: "http", modes: ["pre_call", "post_call"], default_on: false, failure_policy: "fail_closed", enabled: true, endpoint_configured: true, endpoint_url: "https://classification.example.test/check", timeout_ms: 1500, token_configured: true },
    { name: "stream-review", description: "Synthetic streaming review pilot", provider_kind: "http", modes: ["during_call"], default_on: false, failure_policy: "dry_run", enabled: false, endpoint_configured: true, endpoint_url: "https://review.example.test/check", timeout_ms: 1200, token_configured: false },
  ].map((row) => ({ config_schema: {}, runtime_config: {}, ...row }));
  const instanceId = id(950);
  const websocket = (started, closed, accessa = false) => ({ state: closed ? "closed" : "open", app: accessa ? "design-assistant" : null, channel: accessa ? "web" : "responses", opened_at: started, observed_at: closed || at(), closed_at: closed, duration_ms: 4200, client_bytes: 2048, upstream_bytes: 12288, client_frames: 5, upstream_frames: 18, client_close_frame: Boolean(closed), upstream_close_frame: Boolean(closed), last_client_activity_at: started, last_upstream_activity_at: closed || at(), idle_timeout_ms: 60000, session_expires_at: at(-3600000), max_frame_bytes: 1048576, close_cause: closed ? "normal_close" : null });
  const events = Array.from({ length: 420 }, (_, i) => {
    const serviceIndex = i % 5;
    const chosen = services[serviceIndex];
    const keyIndex = serviceIndex === 4 ? 5 : serviceIndex === 3 ? 1 : 0;
    const key = keys[keyIndex];
    const created_at = at(i < 90 ? (1 + i / 6) * minute : (30 + (i - 90) * 120) * minute);
    const canonical = i % 11 === 5 ? ["/v1/chat/completions", "/v1/responses", "/v1/messages", "/v1/embeddings"][i % 4] : null;
    const failure = i % 17 === 0 ? "upstream_timeout" : i % 29 === 0 ? "policy_denied" : i % 37 === 0 ? "rate_limit_exceeded" : i % 41 === 0 ? "budget_exceeded" : i % 43 === 0 ? "guardrail_blocked" : null;
    const ws = !canonical && (serviceIndex === 1 || serviceIndex === 2);
    const status_code = failure === "upstream_timeout" ? 504 : failure === "rate_limit_exceeded" || failure === "budget_exceeded" ? 429 : failure ? 403 : ws ? 101 : 200;
    const direct = canonical === "/v1/responses";
    const tokens = !failure && !ws && !direct ? 180 + (i % 23) * 37 : 0;
    const cost = !failure && !ws && !direct ? canonical || chosen.cost_mode === "passthrough" ? Number((0.002 + (i % 9) * 0.001).toFixed(3)) : chosen.estimated_cost_usd : null;
    return {
      request_id: `design-request-${String(i + 1).padStart(4, "0")}`, key_id: key.id, project_id: key.project_id,
      route: canonical || "/services/*", model: canonical === "/v1/messages" ? "claude-sonnet-4" : canonical === "/v1/embeddings" || serviceIndex === 4 ? "text-embedding-3-small" : canonical || serviceIndex === 3 ? "gpt-4.1-mini" : null,
      provider: canonical ? "litellm" : "internal-service", status: failure ? "failure" : "success",
      status_code, latency_ms: failure === "upstream_timeout" ? 30000 : ws && !failure ? 4200 : 160 + (i % 19) * 47,
      input_tokens: Math.floor(tokens * 0.7), output_tokens: tokens - Math.floor(tokens * 0.7), total_tokens: tokens,
      estimated_cost_usd: cost, cost_source: ws || direct ? "none" : cost === null ? null : canonical ? "route_default" : chosen.cost_mode === "passthrough" ? "service_default_passthrough" : "service_default_fixed", cost_mode: ws || direct ? "none" : canonical ? null : chosen.cost_mode, pricing_rule_name: null,
      service_name: canonical ? null : chosen.name, service_version: canonical ? null : i < 150 ? "2.3.0" : "2.2.0", http_method: ws ? "GET" : "POST",
      endpoint_path: canonical || "/run", endpoint_template: canonical || "/run", task_id: `design-task-${i % 3 + 1}`, run_id: `design-run-${i % 6 + 1}`,
      trace_id: `design-trace-${String(i + 1).padStart(4, "0")}`, fallback_count: i % 31 === 0 ? 1 : 0, guardrail_action_count: failure === "guardrail_blocked" ? 1 : 0, created_at,
      diagnostics: { protocol: ws ? "websocket" : "http", websocket: ws && !failure ? websocket(created_at, new Date(Date.parse(created_at) + 4200).toISOString(), serviceIndex === 2) : null, routing_mode: direct ? "litellm_passthrough" : "managed_by_gateway", traffic_id: id(1000 + i), failure_stage: failure ? failure === "upstream_timeout" ? "upstream" : "policy" : null, failure_code: failure, failure_source: failure ? "gateway" : null, outcome: failure ? "failed" : "success", upstream_status: failure ? null : status_code, instance_id: instanceId },
    };
  });
  const trafficFromEvent = (row) => ({
    id: row.diagnostics.traffic_id, request_id: row.request_id, instance_id: instanceId,
    started_at: row.created_at, completed_at: new Date(Date.parse(row.created_at) + row.latency_ms).toISOString(), elapsed_ms: row.latency_ms,
    method: row.http_method, endpoint: row.endpoint_template, service: row.service_name, provider: row.provider,
    key_id: row.key_id, key_prefix: keys.find((key) => key.id === row.key_id).key_prefix, project_id: row.project_id,
    stage: "completed", client_status: row.status_code, attempts: 1 + row.fallback_count, streaming: row.diagnostics.protocol === "websocket",
    completed: true, summary_only: false, diagnostics: copy(row.diagnostics), timeline_truncated: false, recording_failures: [],
    timeline: [{ stage: "received", elapsed_ms: 0, code: null, upstream_status: null, attempt: 0 }, { stage: "authenticated", elapsed_ms: 2, code: null, upstream_status: null, attempt: 0 }, { stage: "completed", elapsed_ms: row.latency_ms, code: row.diagnostics.failure_code, upstream_status: row.diagnostics.upstream_status, attempt: 1 }],
    upstream_timings: [], usage: Object.fromEntries(["model", "input_tokens", "output_tokens", "total_tokens", "estimated_cost_usd", "cost_source", "pricing_rule_name", "service_version", "trace_id", "task_id", "run_id"].map((field) => [field, row[field]])), debug_bundle: null,
  });
  const trafficHistory = events.map(trafficFromEvent).sort((a, b) => b.completed_at.localeCompare(a.completed_at));
  const activeTraffic = events.slice(1, 4).map((row, i) => {
    const active = trafficFromEvent(row);
    active.id = id(2000 + i); active.request_id = `design-active-${i + 1}`;
    active.started_at = at((i + 1) * 1200); active.completed_at = null; active.elapsed_ms = (i + 1) * 1200;
    active.completed = false; active.stage = "upstream"; active.client_status = i < 2 ? 101 : null;
    active.streaming = true; active.timeline = active.timeline.slice(0, 2); active.usage = null;
    active.diagnostics = { ...active.diagnostics, traffic_id: active.id, failure_code: null, failure_stage: null, outcome: null, websocket: i < 2 ? websocket(active.started_at, null, i === 1) : null, protocol: i < 2 ? "websocket" : "http" };
    return active;
  });
  const auditEvents = ["member.approve", "service.update", "key.disable", "policy_layer.upsert", "provider.update", "service_membership.upsert", "project_membership.upsert", "auth_settings.update", "key.create", "service.import"].map((action, i) => ({
    id: id(3000 + i), actor_member_id: i % 3 ? id(301) : null, actor_token_id: null,
    action, target_type: action.split(".")[0], target_id: i % 2 ? services[0].name : id(1), request_id: `design-audit-${i + 1}`,
    ip: "192.0.2.10", user_agent: "Synthetic design preview", before: { enabled: false }, after: { enabled: true, synthetic: true }, created_at: at((i + 1) * 43 * minute),
  }));
  const executions = events.filter((row) => row.diagnostics.routing_mode === "managed_by_gateway").slice(0, 24).map((row, i) => ({ request_id: row.request_id, key_id: row.key_id, project_id: row.project_id, route: ({ "/v1/chat/completions": "ChatCompletions", "/v1/messages": "AnthropicMessages", "/v1/embeddings": "LiteLlmEmbeddings" })[row.route] || "ServiceWildcard", model: row.model, provider: row.provider === "litellm" ? "LiteLlm" : "InternalService", guardrail_name: i % 2 ? "data-classification" : "payload-safety", mode: i % 3 ? "pre_call" : "post_call", action: i % 7 ? "allow" : "block", failure_policy: "fail_closed", latency_ms: 4 + i % 8, reason: "Synthetic decision metadata only", metadata: { synthetic: true }, created_at: row.created_at }));
  return {
    captured_at: at(), captured, projects, services, providers, keys, members, serviceMemberships, projectMemberships,
    managedIdentities, managedIdentityProjects, openaiRoutes, anthropicRoutes, routeIdentities, policyLayers, guardrails,
    events, trafficHistory, activeTraffic, auditEvents, executions, instanceId,
    studioConnection: { base_url: "https://studio.example.test", token_configured: true, source: "persisted", updated_at: at(day) },
    authSettings: { unverified_bearer_enabled: false, source: "persisted", updated_at: at(day), relayna_key_header: "X-Relayna-Key", entra: { enabled: true, tenant_id: tenant, audience, issuer: "https://identity.example.test/tenant/v2.0", oidc_discovery_url: "https://identity.example.test/tenant/v2.0/.well-known/openid-configuration", required_scope: "gateway.invoke", required_role: null, allowed_groups: [], accepted_algorithms: ["RS256"], jwks_cache_ttl_seconds: 300, clock_skew_seconds: 60 }, apigee: { trusted_header_enabled: false, secret_configured: false } },
    mappings: [{ id: id(4001), scope: "project", target_id: id(1), target_label: projects[0].name, credential_configured: true, enabled: true, ...dates }, { id: id(4002), scope: "key", target_id: id(202), target_label: keys[1].key_prefix, credential_configured: true, enabled: true, ...dates }, { id: id(4003), scope: "project", target_id: id(3), target_label: projects[2].name, credential_configured: false, enabled: false, ...dates }],
    passthrough: { enabled: true, authentication_mode: "gateway", allowed_paths: ["/v1/*"], blocked_paths: ["/config", "/config/*"], allowed_methods: ["GET", "POST"], timeout_ms: 120000, max_request_body_bytes: 2097152, max_response_body_bytes: 4194304, ui_exposure: "disabled", admin_api_exposure: "disabled", ...dates },
    importVersions: [{ version: 2, source: "synthetic-design-preview", activated_at: at(2 * day), rolled_back_from_version: null, diff: { added: ["invoice-parser"], changed: ["catalog-search"], removed: [], invalid: [] } }],
  };
}

function summary(rows) {
  const count = (predicate) => rows.filter(predicate).length;
  const sum = (key) => rows.reduce((total, row) => total + (row[key] || 0), 0);
  const latency = rows.map((row) => row.latency_ms).sort((a, b) => a - b);
  const costs = rows.filter((row) => row.estimated_cost_usd !== null);
  const fallback_count = sum("fallback_count");
  return {
    request_count: rows.length, success_count: count((row) => row.status === "success"), failure_count: count((row) => row.status !== "success"),
    input_tokens: sum("input_tokens"), output_tokens: sum("output_tokens"), total_tokens: sum("total_tokens"),
    estimated_cost_usd: costs.length ? Number(sum("estimated_cost_usd").toFixed(6)) : null,
    total_latency_ms: sum("latency_ms"), average_latency_ms: rows.length ? sum("latency_ms") / rows.length : null,
    p95_latency_ms: latency[Math.max(0, Math.ceil(latency.length * 0.95) - 1)] ?? null,
    fallback_count, fallback_rate: rows.length ? fallback_count / rows.length : 0,
    policy_denial_count: count((row) => row.diagnostics.failure_code === "policy_denied"),
    rate_limit_denial_count: count((row) => row.diagnostics.failure_code === "rate_limit_exceeded"),
    budget_denial_count: count((row) => row.diagnostics.failure_code === "budget_exceeded"),
    guardrail_block_count: count((row) => row.diagnostics.failure_code === "guardrail_blocked"),
    expensive_request_count: count((row) => row.estimated_cost_usd >= 0.1),
  };
}

function usageRows(data, query, source = data.events) {
  const fields = { project_id: "project_id", key_id: "key_id", route: "route", provider: "provider", service: "service_name", request_id: "request_id", method: "http_method", endpoint: "endpoint_template", task_id: "task_id", run_id: "run_id", model: "model", status: "status", status_code: "status_code", trace_id: "trace_id" };
  return source.filter((row) => {
    if (query.get("from") && Date.parse(row.created_at) < Date.parse(query.get("from"))) return false;
    if (query.get("to") && Date.parse(row.created_at) > Date.parse(query.get("to"))) return false;
    for (const [filter, field] of Object.entries(fields)) if (query.get(filter) && String(row[field]) !== query.get(filter)) return false;
    return !query.get("min_cost_usd") || row.estimated_cost_usd !== null && row.estimated_cost_usd >= Number(query.get("min_cost_usd"));
  });
}

function breakdown(rows, field) {
  const groups = new Map();
  for (const row of rows) {
    const name = row[field] ?? "Unattributed";
    if (!groups.has(name)) groups.set(name, []);
    groups.get(name).push(row);
  }
  return [...groups].map(([name, entries]) => ({ name, summary: summary(entries) })).sort((a, b) => b.summary.request_count - a.summary.request_count);
}

function timeseries(rows, query, service = false) {
  const interval = query.get("interval") === "day" ? day : 60 * minute;
  const groups = new Map();
  for (const row of rows) {
    const bucket = new Date(Math.floor(Date.parse(row.created_at) / interval) * interval).toISOString();
    const key = service ? `${bucket}|${row.service_name}` : bucket;
    if (!groups.has(key)) groups.set(key, { bucket, service_name: row.service_name, rows: [] });
    groups.get(key).rows.push(row);
  }
  return [...groups.values()].sort((a, b) => a.bucket.localeCompare(b.bucket)).map((group) => ({ bucket: group.bucket, ...(service ? { service_name: group.service_name } : {}), summary: summary(group.rows) }));
}

function page(rows, query, prefix = "") {
  const limit = Math.max(1, numberParam(query, `${prefix}limit`, 20));
  const offset = numberParam(query, `${prefix}offset`, 0, 100000);
  return { rows: rows.slice(offset, offset + limit), limit, offset, has_more: offset + limit < rows.length };
}

function eventPage(rows, query) {
  const sorted = [...rows];
  const field = { cost: "estimated_cost_usd", latency: "latency_ms", tokens: "total_tokens", status_code: "status_code" }[query.get("sort_by")];
  const direction = query.get("sort_order") === "asc" ? 1 : -1;
  sorted.sort((a, b) => direction * (field ? (a[field] ?? -1) - (b[field] ?? -1) : a.created_at.localeCompare(b.created_at)));
  const withCursor = new URLSearchParams(query);
  const cursor = query.get("cursor");
  if (cursor) withCursor.set("offset", /^design-offset-\d+$/.test(cursor) ? cursor.slice(14) : "0");
  const result = page(sorted, withCursor);
  return { ...result, ...(cursor ? { snapshot_cursor: `design-offset-${result.offset}`, ...(result.has_more ? { next_cursor: `design-offset-${result.offset + result.limit}` } : {}) } : {}) };
}

function trafficRows(rows, query) {
  const timeField = query.get("time_basis") === "started" ? "started_at" : "completed_at";
  return rows.filter((row) => {
    for (const field of ["id", "service", "project_id", "key_id", "instance_id"]) if (query.get(field) && query.get(field) !== "all" && row[field] !== query.get(field)) return false;
    if (query.get("request_id") && !row.request_id.includes(query.get("request_id"))) return false;
    if (query.get("status") && String(row.client_status) !== query.get("status")) return false;
    if (query.get("failure_code") && row.diagnostics.failure_code !== query.get("failure_code")) return false;
    if (query.get("protocol") && row.diagnostics.protocol !== query.get("protocol")) return false;
    if (query.get("failures_only") === "true" && !row.diagnostics.failure_code) return false;
    if (query.get("from") && row[timeField] && Date.parse(row[timeField]) < Date.parse(query.get("from"))) return false;
    if (query.get("to") && row[timeField] && Date.parse(row[timeField]) > Date.parse(query.get("to"))) return false;
    if (query.get("before") && (row[timeField] || row.started_at) >= query.get("before")) return false;
    return true;
  });
}

function session(data, mode) {
  if (mode === "login") return { authenticated: false, service_memberships: [], project_memberships: [] };
  const member = copy(data.members[0].member);
  member.roles = mode === "owner" ? [] : ["admin"];
  member.status = ["pending", "blocked"].includes(mode) ? mode : "active";
  return { authenticated: true, member, service_memberships: mode === "admin" ? [] : data.serviceMemberships, project_memberships: mode === "admin" ? [] : data.projectMemberships };
}

function runtimeStatus(data, query) {
  const window = [60, 300, 900].includes(Number(query.get("window_seconds"))) ? Number(query.get("window_seconds")) : 60;
  const counters = { started: 93, completed: 90, succeeded: 85, failed: 5, rejected: 2, unattributed: 0, http_completed: 51, sse_completed: 3, websocket_completed: 36 };
  const histogram = { upper_bounds_us: Array.from({ length: 25 }, (_, i) => 1000 * 2 ** i), bucket_counts: Array(26).fill(0), sample_count: 0, sum_us: 0, p50_ms: null, p95_ms: null, p99_ms: null };
  const latency = Object.fromEntries(["http_duration", "http_success_duration", "upstream_duration", "ttfb", "ttft", "websocket_handshake", "websocket_session", "admission_decision", "admission_rejection", "body_retention"].map((key) => [key, copy(histogram)]));
  const runtime = { schema_version: 1, window_seconds: window, sampled_at_unix_ms: data.captured, uptime_seconds: 3600, observed_seconds: window, instance_count: 1, incompatible_instances: 0, counters, totals: counters, active: { http: 1, sse: 0, websocket: 2 }, body_capacity: { max_requests: 16, used_requests: 3, free_requests: 13, max_bytes: 536870912, used_bytes: 25165824, free_bytes: 511705088 }, admission_denials: { concurrency: 0, bytes: 2, other: 0 }, recording_failures: {}, recording_failures_total: {}, latency, process: { rss_bytes: null, cgroup_memory_current_bytes: null, cgroup_memory_limit_bytes: null, cgroup_cpu_usage_usec: null, cgroup_cpu_limit_cores: null, cgroup_cpu_periods: null, cgroup_cpu_throttled_periods: null, cgroup_cpu_throttled_usec: null, cgroup_oom_events: null, cgroup_oom_kills: null, restarts: null } };
  const selected = !query.get("instance_id") || ["all", data.instanceId].includes(query.get("instance_id"));
  return { scope: query.get("instance_id") && query.get("instance_id") !== "all" ? "instance" : "cluster", sampled_at: data.captured_at, window_seconds: window, partial: !selected, unavailable: false, instances: selected ? [{ instance_id: data.instanceId, sampled_at: data.captured_at, revision: 1, stale: false, runtime, dependencies: { postgres_ready: null, postgres_readiness_latency_ms: null, postgres_pool_connections: 0, postgres_pool_idle: 0, redis_ready: null, redis_readiness_latency_ms: null, publication_failed: false } }] : [], aggregate: selected ? runtime : null };
}

function ownerResponse(data, currentSession, match, query) {
  const [, kind, encoded, action = "", requestId] = match;
  const resource = decodeURIComponent(encoded);
  const project = kind === "projects";
  const field = project ? "project_id" : "service_name";
  const membership = (project ? currentSession.project_memberships : currentSession.service_memberships).find((row) => row[field] === resource);
  const entity = (project ? data.projects : data.services).find((row) => (project ? row.id : row.name) === resource && (project || row.enabled));
  if (!membership || !entity) return null;
  const scope = data.events.filter((row) => row[field] === resource);
  if (!action) return { [project ? "project" : "service"]: entity, role: membership.role };
  if (action === "requests") {
    const request = scope.find((row) => row.request_id === decodeURIComponent(requestId || ""));
    return request ? { request, debug_bundle: null } : null;
  }
  // Caller filters may narrow an exact scope; they can never replace it.
  const rows = usageRows(data, query, scope);
  if (action === "events" || action === "usage") return eventPage(rows, query);
  if (action !== "dashboard") return null;
  return {
    [field]: resource, role: membership.role, summary: summary(rows), timeseries: timeseries(rows, query),
    ...(project ? { services: breakdown(rows, "service_name") } : {}),
    endpoints: breakdown(rows, "endpoint_template"), providers: breakdown(rows, "provider"), models: breakdown(rows, "model"),
    status_codes: [...new Set(rows.map((row) => row.status_code))].sort((a, b) => a - b),
    version_markers: [...new Set(rows.map((row) => row.service_version).filter(Boolean))].map((service_version) => ({ service_version, first_observed_at: rows.filter((row) => row.service_version === service_version).map((row) => row.created_at).sort()[0] })).sort((a, b) => a.first_observed_at.localeCompare(b.first_observed_at)),
  };
}

function adminResponse(data, path, query) {
  const inventories = {
    projects: data.projects, services: data.services, keys: data.keys, providers: data.providers,
    "providers/litellm-credentials": data.mappings, "providers/litellm-passthrough": data.passthrough,
    "openai-routes": data.openaiRoutes, "anthropic-routes": data.anthropicRoutes,
    "route-identities": data.routeIdentities, "policy-layers": data.policyLayers, members: data.members,
    "managed-identities": data.managedIdentities, "managed-identity-projects": data.managedIdentityProjects,
    "studio/connection": data.studioConnection, "auth/front-door": data.authSettings,
    "services/import/versions": data.importVersions, guardrails: { guardrails: data.guardrails },
    "guardrails/executions": { executions: data.executions.slice(0, numberParam(query, "limit", 50)) },
    "guardrails/summary": { summary: data.guardrails.map((row) => ({ guardrail_name: row.name, mode: row.modes[0], action: "allow", failure_policy: row.failure_policy, count: data.executions.filter((entry) => entry.guardrail_name === row.name && entry.action === "allow").length, total_latency_ms: data.executions.filter((entry) => entry.guardrail_name === row.name && entry.action === "allow").reduce((sum, entry) => sum + entry.latency_ms, 0) })) },
  };
  if (Object.hasOwn(inventories, path)) return inventories[path];
  if (path === "audit-events") return data.auditEvents.filter((row) => ["action", "target_type", "target_id", "actor_token_id", "actor_member_id"].every((key) => !query.get(key) || row[key] === query.get(key))).slice(0, numberParam(query, "limit", 100));
  if (path === "studio/services") return data.services.filter((row) => row.source === "studio").map((row) => ({ studio_service_id: row.studio_service_id, name: row.name, gateway_service_name: row.name, display_name: row.name, base_url: row.upstream_base_url, health_check_path: row.health_check_path, health_check_method: row.health_check_method, environment: "Synthetic preview", tags: ["synthetic"], status: row.sync_status, auth_mode: "bearer", default_route_pattern: row.route_pattern, route_pattern: row.route_pattern, allowed_methods: row.allowed_methods, default_pricing: null }));
  if (path === "gateway/status") return runtimeStatus(data, query);
  if (path === "traffic/history") return trafficRows(data.trafficHistory, query).slice(0, numberParam(query, "limit", 100, 200));
  if (path === "traffic/cluster") {
    const matched = trafficRows(data.activeTraffic, query);
    const limit = numberParam(query, "limit", 100, 200);
    const rows = matched.slice(0, limit);
    return { scope: "cluster", sampled_at: data.captured_at, partial: false, unavailable: false, instances: [{ instance_id: data.instanceId, sampled_at: data.captured_at, stale: false, revision: 1 }], rows, counts: { http: 1, websocket: 2 }, observed_instances: 1, matching: matched.length, has_more: matched.length > limit, truncated: false, before: rows.at(-1)?.started_at ?? null, before_id: rows.at(-1)?.id ?? null };
  }
  const resource = /^(services|keys|projects|providers)\/([^/]+)(?:\/(sync-status|litellm-spend))?$/.exec(path);
  if (resource) {
    const row = data[resource[1]].find((entry) => (resource[1] === "services" ? entry.name : entry.id) === decodeURIComponent(resource[2]));
    if (!row) return null;
    if (resource[3] === "sync-status" && resource[1] === "services") return { name: row.name, source: row.source, sync_status: row.sync_status, last_synced_at: row.last_synced_at, missing_runtime_fields: row.missing_runtime_fields };
    if (resource[3] === "litellm-spend" && resource[1] === "keys") return { status: "unavailable", mapping_scope: row.project_id ? "project" : "key", spend_usd: null, fetched_at: data.captured_at };
    return resource[3] ? null : row;
  }
  if (path.startsWith("debug-bundles/")) return null; // No invented legacy debug evidence.
  const task = /^tasks\/([^/]+)\/usage$/.exec(path);
  const rows = usageRows(data, query).filter((row) => !task || row.task_id === decodeURIComponent(task[1]));
  if (task || path === "usage/summary") return summary(rows);
  if (path === "provider-health") return breakdown(rows, "service_name").map((group) => ({ name: group.name, request_count: group.summary.request_count, error_count: group.summary.failure_count, timeout_count: rows.filter((row) => row.service_name === group.name && row.diagnostics.failure_code === "upstream_timeout").length, fallback_count: group.summary.fallback_count, total_latency_ms: group.summary.total_latency_ms }));
  if (path === "provider-health/state") return data.services.slice(0, 4).map((row, i) => ({ name: row.name, provider: "InternalService", status: i === 1 ? "degraded" : i === 3 ? "unknown" : "healthy", circuit_state: i === 1 ? "half_open" : "closed", active_check_ok: null, passive_success_count: 20 + i, passive_failure_count: i === 1 ? 4 : 0, consecutive_failures: i === 1 ? 1 : 0, average_latency_ms: i === 3 ? null : 250 + i * 80, last_error_code: i === 1 ? "upstream_timeout" : null, cooldown_until: null, updated_at: data.captured_at }));
  if (path === "usage/events") return eventPage(rows, query);
  if (path === "usage/filter-values") {
    const field = query.get("field");
    const key = { route: "route", endpoint: "endpoint_template", model: "model" }[field];
    return { field, values: key ? [...new Set(rows.map((row) => row[key]).filter((value) => value && value.includes(query.get("q") || "")))].slice(0, numberParam(query, "limit", 20)) : [] };
  }
  if (path === "usage/export.json") return { summary: summary(rows), rows: query.get("stream") === "true" ? rows : eventPage(rows, query).rows };
  const kind = path.replace(/^usage\/(?:sections\/|by-)?/, "");
  if (!path.startsWith("usage/")) return null;
  if (kind === "timeseries" || kind === "service-timeseries") {
    const result = page(timeseries(rows, query, kind === "service-timeseries"), query, kind === "timeseries" ? "timeseries_" : "service_timeseries_");
    return path.includes("/sections/") ? { rows: result.rows, page: { limit: result.limit, offset: result.offset, has_more: result.has_more } } : result.rows;
  }
  let groups;
  if (kind === "unused-keys") groups = data.keys.filter((row) => !row.last_used_at && (!query.get("project_id") || row.project_id === query.get("project_id"))).map((row) => ({ key_id: row.id, key_prefix: row.key_prefix, project_id: row.project_id, created_at: row.created_at, last_used_at: null }));
  else if (kind === "project-key-services") {
    const combinations = new Map();
    for (const row of rows) {
      const key = `${row.project_id}|${row.key_id}|${row.service_name}`;
      if (!combinations.has(key)) combinations.set(key, []);
      combinations.get(key).push(row);
    }
    groups = [...combinations.values()].map((entries) => ({ project_id: entries[0].project_id, key_id: entries[0].key_id, service_name: entries[0].service_name, summary: summary(entries) }));
  } else {
    const field = { project: "project_id", projects: "project_id", keys: "key_id", services: "service_name", endpoints: "endpoint_template", providers: "provider", models: "model", tasks: "task_id" }[kind];
    if (!field) return null;
    groups = breakdown(rows, field);
  }
  const limit = numberParam(query, "breakdown_limit", 20);
  const offset = numberParam(query, "breakdown_offset", 0, 100000);
  const selected = groups.slice(offset, offset + limit);
  return path.includes("/sections/") ? { rows: selected } : selected;
}

export function createDesignPreviewServer({ now = Date.now(), mode = "both" } = {}) {
  if (!modes.includes(mode)) throw new Error(`Unknown fixture mode: ${mode}`);
  const data = createDesignDataset(now);
  const server = createServer(async (request, response) => {
    response.setHeader("cache-control", "no-store");
    response.setHeader("x-content-type-options", "nosniff");
    response.setHeader("x-design-preview", "synthetic; not-health-or-performance-evidence");
    response.setHeader("x-design-preview-captured-at", data.captured_at);
    const send = (body, status = 200) => { response.writeHead(status, { "content-type": "application/json" }); response.end(JSON.stringify(body)); };
    const error = (status, message) => send({ error: { code: `design_preview_${status}`, message } }, status);
    if (request.method !== "GET") { response.setHeader("allow", "GET"); return error(405, "Design preview is GET-only; mutations are disabled"); }
    if (!/^(127\.0\.0\.1|localhost|\[::1\])(?::\d{1,5})?$/i.test(request.headers.host || "") || !["127.0.0.1", "::1", "::ffff:127.0.0.1"].includes(request.socket.remoteAddress)) return error(403, "Loopback Host and client required");
    try {
      if (!request.url?.startsWith("/") || request.url.startsWith("//")) return error(400, "Origin-relative path required");
      const url = new URL(request.url, "http://127.0.0.1");
      const path = url.pathname;
      const query = url.searchParams;
      const cookieMode = /(?:^|;\s*)design_preview_mode=([^;]+)/.exec(request.headers.cookie || "")?.[1];
      const currentMode = modes.includes(cookieMode) ? cookieMode : mode;
      const currentSession = session(data, currentMode);
      if (path === "/fixture" || path === "/fixture/") return send({
        name: "Design preview", synthetic: true, captured_at: data.captured_at, mode: currentMode,
        modes: Object.fromEntries(modes.map((value) => [value, `/fixture/${value}`])),
        limitations: ["UI design fixture only; no health, performance, availability, release or capacity evidence", "420 generated request outcomes across a finite 28-day sample, with a denser recent 15-minute sample; not complete history or production volumes", "Timestamps and active snapshots freeze at startup; runtime snapshots become stale normally", "The intentionally partial legacy provider omits credential_configured to exercise the unknown state", "Latency histograms, process measurements, active probes, live updates, legacy debug bundles and external LiteLLM spend are not measured", "All mutations return 405; sign-in links only select a synthetic fixture session"],
      });
      const control = /^\/fixture\/(both|admin|owner|login|pending|blocked|active)$/.exec(path);
      if (control || path === "/admin-ui/auth/login") {
        const nextMode = control?.[1] === "active" ? "both" : control?.[1] || "both";
        response.writeHead(302, { "set-cookie": `design_preview_mode=${nextMode}; Path=/; HttpOnly; SameSite=Strict`, location: nextMode === "owner" ? "/admin-ui#/my-services" : "/admin-ui#/overview" });
        return response.end();
      }
      if (path === "/admin-ui/auth/config") return send({ enabled: true });
      if (path === "/admin-ui/auth/session") return send(currentSession);
      if (path === "/admin-ui/healthz") return send({ status: "ok" });
      if (path === "/admin-ui/readyz") return send({ status: "ready", detail: "Synthetic design preview · readiness is not measured" });
      if (path.startsWith("/owner/v1/") || path.startsWith("/admin-ui/admin/")) {
        if (!currentSession.authenticated) return error(401, "Select a signed-in fixture mode at /fixture/both");
        if (currentSession.member.status !== "active") return error(403, "Synthetic member is not active");
        if (path.startsWith("/owner/v1/")) {
          if (path === "/owner/v1/services") return send(currentSession.service_memberships.flatMap((membership) => { const service = data.services.find((row) => row.name === membership.service_name && row.enabled); return service ? [{ service, role: membership.role }] : []; }));
          if (path === "/owner/v1/projects") return send(currentSession.project_memberships.flatMap((membership) => { const project = data.projects.find((row) => row.id === membership.project_id); return project ? [{ project, role: membership.role }] : []; }));
          const match = /^\/owner\/v1\/(services|projects)\/([^/]+)(?:\/(dashboard|events|usage|requests)(?:\/([^/]+))?)?$/.exec(path);
          const value = match ? ownerResponse(data, currentSession, match, query) : null;
          return value === null ? error(404, "Resource or request is not in this exact fixture scope") : send(value);
        }
        if (!currentSession.member.roles.includes("admin")) return error(403, "Admin fixture mode required");
        const apiPath = path.slice("/admin-ui/admin/".length);
        if (apiPath === "traffic/live") {
          // One finite SSE snapshot; intentionally no simulated progress or timers.
          const batch = { instance_id: data.instanceId, cursor: `${data.instanceId}:1`, gap: false, evicted_updates: 0, rows: trafficRows([...data.activeTraffic, ...data.trafficHistory.slice(0, 20)], query) };
          response.writeHead(200, { "content-type": "text/event-stream", "x-design-preview-history": "finite-snapshot; no-live-activity" });
          return response.end(`event: traffic\nid: ${batch.cursor}\ndata: ${JSON.stringify(batch)}\n\n`);
        }
        if (apiPath === "usage/export.csv") {
          const filtered = usageRows(data, query);
          const rows = query.get("stream") === "true" ? filtered : eventPage(filtered, query).rows;
          const fields = ["request_id", "created_at", "project_id", "service_name", "status", "status_code", "latency_ms", "estimated_cost_usd"];
          const cell = (value) => `"${String(value ?? "").replaceAll('"', '""')}"`;
          response.writeHead(200, { "content-type": "text/csv", "content-disposition": 'attachment; filename="synthetic-design-preview.csv"' });
          return response.end([fields.join(","), ...rows.map((row) => fields.map((field) => cell(row[field])).join(","))].join("\r\n"));
        }
        const value = adminResponse(data, apiPath, query);
        return value === null ? error(404, "GET contract is not in the design fixture") : send(value);
      }
      if (path === "/") { response.writeHead(302, { location: "/admin-ui" }); return response.end(); }
      const file = ["/admin-ui", "/admin-ui/"].includes(path) ? "index.html" : /^\/admin-ui\/([a-zA-Z0-9_.-]+)$/.exec(path)?.[1];
      if (!file || file.includes("..")) return error(404, "Not in design fixture");
      let body;
      try { body = await readFile(new URL(file, assets)); } catch { return error(404, "Checked-in asset is unavailable"); }
      if (file === "index.html") body = Buffer.from(body.toString().replace("<body>", '<body><aside role="note" style="padding:6px 16px;background:#fff6db;color:#493c18;font:12px system-ui;text-align:center">Design preview · synthetic data only · <a href="/fixture">fixture controls and history limitations</a> · no health or performance evidence</aside>'));
      const extension = file.split(".").at(-1);
      response.writeHead(200, { "content-type": { html: "text/html", js: "text/javascript", css: "text/css", svg: "image/svg+xml", png: "image/png", woff: "font/woff", woff2: "font/woff2" }[extension] || "application/octet-stream" });
      response.end(body);
    } catch { error(400, "Invalid design-preview request"); }
  });
  return server;
}

export const help = `Synthetic admin + owner design preview (GET-only, loopback-only).
Usage: node tests/fixtures/admin-ui-design-preview.mjs [--port 18464] [--mode both] [--now ISO]
  --port PORT  1..65535; GATEWAY_DESIGN_PREVIEW_PORT is also supported.
  --mode MODE  both (default), admin, owner, login, pending, blocked.
  --now ISO    Optional fixed timestamp for reproducible fixture data.
  --help      Print this help without starting a server.
Session controls: GET /fixture/both, /fixture/admin, /fixture/owner,
  /fixture/login, /fixture/pending, /fixture/blocked; /fixture/active aliases both.
Default both session: Design preview, admin role + exact service/project memberships.
GET /fixture lists limitations. No real OIDC, upstream calls, token files or writes.
Current checked-in assets are read on every request; build them separately if needed.
Data is synthetic, finite and frozen at startup. It is NOT health/performance evidence.`;

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.includes("--help")) console.log(help);
  else {
    try {
      const options = {};
      let port = process.env.GATEWAY_DESIGN_PREVIEW_PORT || "18464";
      for (let i = 0; i < args.length; i += 2) {
        if (!["--port", "--mode", "--now"].includes(args[i]) || !args[i + 1]) throw new Error("Unknown or incomplete option; use --help");
        if (args[i] === "--port") port = args[i + 1];
        else options[args[i].slice(2)] = args[i + 1];
      }
      if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) throw new Error("Port must be 1..65535");
      const server = createDesignPreviewServer(options);
      server.on("error", (error) => { console.error(`Design preview: ${error.code || "server error"}`); process.exitCode = 1; });
      server.listen(Number(port), "127.0.0.1", () => console.log(`Synthetic Design preview: http://127.0.0.1:${port}/admin-ui · controls /fixture · GET-only · not health/performance evidence`));
    } catch (error) { console.error(error.message); process.exitCode = 1; }
  }
}
