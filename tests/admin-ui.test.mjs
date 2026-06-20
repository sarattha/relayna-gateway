import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const uiDir = join(root, "crates/gateway-api/src/static/admin-ui");
const html = readFileSync(join(uiDir, "index.html"), "utf8");
const js = readFileSync(join(uiDir, "app.js"), "utf8");
const css = readFileSync(join(uiDir, "app.css"), "utf8");

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    console.error(`not ok - ${name}`);
    throw error;
  }
}

test("admin portal shell exposes all release-critical views", () => {
  for (const view of ["overview", "projects", "keys", "guardrails", "audit", "providers", "routes", "services", "usage", "health", "settings"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(
    html,
    /data-view="overview"[\s\S]*data-view="health"[\s\S]*data-view="usage"[\s\S]*data-view="providers"[\s\S]*data-view="services"[\s\S]*data-view="routes"[\s\S]*data-view="projects"[\s\S]*data-view="keys"[\s\S]*data-view="guardrails"[\s\S]*data-view="audit"[\s\S]*data-view="settings"/,
  );
  assert.match(html, /id="operator-token"/);
  assert.match(html, /id="rotate-token"/);
});

test("admin portal calls the expected gateway admin APIs", () => {
  for (const endpoint of [
    "/admin-ui/admin/usage/summary",
    "/admin-ui/admin/usage/by-model",
    "/admin-ui/admin/usage/by-provider",
    "/admin-ui/admin/usage/by-task",
    "/admin-ui/admin/usage/timeseries",
    "/admin-ui/admin/usage/export.json",
    "/admin-ui/admin/usage/export.csv",
    "/admin-ui/admin/usage/unused-keys",
    "/admin-ui/admin/provider-health",
    "/admin-ui/admin/provider-health/check",
    "/admin-ui/admin/provider-health/state",
    "/admin-ui/admin/audit-events",
    "/admin-ui/admin/tasks",
    "/admin-ui/admin/projects",
    "/admin-ui/admin/providers",
    "/admin-ui/admin/providers/litellm-credentials",
    "/admin-ui/admin/providers/litellm-passthrough",
    "/admin-ui/admin/openai-routes",
    "/admin-ui/admin/keys",
    "/admin-ui/admin/policy/simulate",
    "/admin-ui/admin/policy-layers",
    "/admin-ui/admin/guardrails",
    "/admin-ui/admin/guardrails/executions?limit=50",
    "/admin-ui/admin/guardrails/summary",
    "/admin-ui/admin/services",
    "/admin-ui/admin/studio/connection",
    "/admin-ui/admin/studio/connection/test",
    "/admin-ui/admin/studio/services",
    "/admin-ui/admin/auth/front-door",
    "/admin-ui/admin/services/sync",
    "/admin-ui/admin/services/import/preview",
    "/admin-ui/admin/services/import/activate",
    "/admin-ui/admin/services/import/versions",
    "/admin-ui/admin/services/import/rollback",
    "/admin-ui/admin/debug-bundles",
    "/admin-ui/admin/operator-token/rotate",
    "/admin-ui/readyz",
  ]) {
    assert.ok(js.includes(endpoint), `expected app.js to call ${endpoint}`);
  }
});

test("admin portal surfaces async action failures", () => {
  assert.match(js, /function handleAsync\(handler\)/);
  assert.match(js, /className = "message-box"/);
  assert.match(js, /data-close-message/);
  assert.match(css, /\.message-box/);
  assert.doesNotMatch(html, /id="notice"/);
  for (const handler of [
    "createProject",
    "createKey",
    "submitService",
    "patchService",
    "createProvider",
    "saveStudioConnection",
    "submitGuardrail",
  ]) {
    assert.match(js, new RegExp(`handleAsync\\(${handler}\\)`));
  }
});

test("guardrails view exposes catalog CRUD drawer controls", () => {
  assert.match(js, /New guardrail/);
  assert.match(js, /function guardrailDrawer\(guardrail\)/);
  assert.match(js, /id="guardrail-form"/);
  assert.match(js, /data-guardrail-edit/);
  assert.match(js, /data-guardrail-action="delete"/);
  assert.match(js, /method: creating \? "POST" : "PATCH"/);
  assert.match(js, /method: "DELETE"/);
  assert.match(js, /\/admin\/guardrails\/\$\{encodeURIComponent\(name\)\}/);
  assert.match(js, /\/admin\/guardrails\/\$\{encodeURIComponent\(formElement\.dataset\.guardrailName\)\}/);
  assert.match(js, /name="bearer_token" type="password"/);
  assert.match(js, /name="clear_token" type="checkbox"/);
  assert.match(js, /providerKind === "built_in"/);
  assert.match(js, /name="runtime_config"/);
  assert.match(js, /runtime_config: runtimeConfig/);
  assert.match(css, /\.guardrail-workspace/);
  assert.match(css, /\.guardrail-form/);
});

test("virtual keys expose per-guardrail config override controls", () => {
  assert.match(js, /function guardrailOverrideControls\(overrides = \{\}, selectedNames = \[\]\)/);
  assert.match(js, /function activeConfigurableGuardrails\(policy = \{\}\)/);
  assert.match(js, /function updateGuardrailOverrideControls\(form\)/);
  assert.match(js, /guardrail_config_overrides/);
  assert.match(js, /name="guardrail_override_names"/);
  assert.match(js, /data-guardrail-overrides/);
  assert.match(js, /Select mandatory or optional guardrails before setting config overrides/);
  assert.match(js, /class="check guardrail-override-toggle"/);
  assert.match(js, /<details>/);
  assert.match(js, /<summary>Config schema<\/summary>/);
  assert.match(js, /updateGuardrailOverrideControls\(form\)/);
  assert.match(js, /guardrail_override_forbidden/);
  assert.match(js, /invalid_guardrail_override/);
  assert.match(js, /JSON\.parse\(form\.get\(`guardrail_override_\$\{name\}`\) \|\| "\{\}"\)/);
  assert.match(css, /\.guardrail-override-toggle/);
  assert.match(css, /\.guardrail-override-row details/);
});

test("settings view configures studio connection without exposing token values", () => {
  assert.match(js, /async function settings\(\)/);
  assert.match(js, /\/admin\/studio\/connection/);
  assert.match(js, /\/admin\/studio\/connection\/test/);
  assert.match(js, /name="token" type="password"/);
  assert.match(js, /token_configured/);
  assert.match(js, /Check Settings for the Studio connection/);
  assert.doesNotMatch(js, /state\.studioConnection\.token\b/);
});

test("settings view configures Entra ID and Apigee front-door auth", () => {
  assert.match(js, /\/admin\/auth\/front-door/);
  assert.match(js, /name="entra_enabled" type="checkbox"/);
  assert.match(js, /name="apigee_trusted_header_enabled" type="checkbox"/);
  assert.match(js, /name="issuer" type="url"/);
  assert.match(js, /name="required_scope"/);
  assert.match(js, /name="allowed_groups"/);
  assert.match(js, /name="apigee_trusted_header_secret" type="password"/);
  assert.match(js, /secret_configured/);
  assert.match(js, /function apigeeSecretPlaceholder\(\)/);
  assert.match(js, /Re-enter secret to persist environment settings/);
  assert.match(js, /Re-enter the Apigee secret before saving environment-backed trusted-header settings/);
  assert.doesNotMatch(js, /state\.authSettings\.apigee\.secret\b/);
});

test("admin portal uses structured project and provider controls", () => {
  assert.match(js, /async function projects\(\)/);
  assert.match(js, /async function providers\(\)/);
  assert.match(js, /function projectOptions\(selected = ""\)/);
  assert.match(js, /function projectServiceForm\(project\)/);
  assert.match(js, /data-project-services-form/);
  assert.match(js, /serviceSelectionControl\(project\.service_names \|\| \[\], "service_names", "Project services"\)/);
  assert.match(js, /service_names: form\.getAll\("service_names"\)/);
  assert.match(js, /function providerPolicySelect\(selected = \[\], neutral = false\)/);
  assert.match(js, /name="allowed_providers" type="checkbox"/);
});

test("providers view configures LiteLLM credential headers and mappings without rendering secrets", () => {
  assert.match(js, /credential_header_mode/);
  assert.match(js, /credential_header_value_format/);
  assert.match(js, /authorization_bearer/);
  assert.match(js, /custom_header/);
  assert.match(js, /bearer/);
  assert.match(js, /x-litellm-api-key/);
  assert.match(js, /async function updateProviderAuthSettings\(event\)/);
  assert.match(js, /async function saveLiteLlmCredentialMapping\(event\)/);
  assert.match(js, /async function liteLlmCredentialMappingAction\(event\)/);
  assert.match(js, /async function saveLiteLlmPassthroughSettings\(event\)/);
  assert.match(js, /\/admin\/providers\/litellm-credentials/);
  assert.match(js, /\/admin-ui\/admin\/providers\/litellm-passthrough/);
  assert.match(js, /LiteLLM passthrough/);
  assert.match(js, /Exposure risk/);
  assert.match(js, /credential_configured/);
  assert.match(js, /name="credential" type="password"/);
  assert.doesNotMatch(js, /row\.credential_secret/);
});

test("routes view exposes canonical OpenAI route modes", () => {
  assert.match(js, /managed_by_gateway/);
  assert.match(js, /direct_litellm_passthrough/);
  assert.match(js, /async function saveOpenAiRouteMode\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/openai-routes\/\$\{routeId\}\/mode/);
});

test("virtual keys use explicit owner and service selection controls", () => {
  assert.match(js, /function keyOwnershipFields\(key = null\)/);
  assert.match(js, /name="owner_type"/);
  assert.match(js, /value="project"/);
  assert.match(js, /value="individual"/);
  assert.match(js, /function openServiceSelectionPicker\(trigger\)/);
  assert.match(js, /function serviceSelectionControl\(selected = \[\], name = "service_names", title = "Select services"\)/);
  assert.match(js, /data-service-picker/);
  assert.match(js, /service_names: form\.get\("owner_type"\) === "individual" \? form\.getAll\("service_names"\) : \[\]/);
});

test("virtual keys use guardrail picker controls for key guardrail policy", () => {
  assert.match(js, /function guardrailSelectionControl\(selected = \[\], name, title = "Select guardrails"\)/);
  assert.match(js, /function openGuardrailSelectionPicker\(trigger\)/);
  assert.match(js, /function guardrailPickerTable\(rows, selected\)/);
  assert.match(js, /data-guardrail-picker/);
  assert.match(js, /data-selection-label="guardrails"/);
  assert.match(js, /No \$\{esc\(label\)\} selected/);
  assert.match(js, /guardrail_name" type="checkbox"/);
  assert.match(js, /mandatory_guardrails: form\.getAll\("mandatory_guardrails"\)/);
  assert.match(js, /optional_guardrails: form\.getAll\("optional_guardrails"\)/);
  assert.match(js, /const forbidden = form\.getAll\("forbidden_guardrails"\)/);
  assert.match(css, /\.guardrail-picker-table table/);
});

test("routes view includes service route registrations", () => {
  assert.match(js, /Registered service routes/);
  assert.match(js, /function serviceRouteTable\(rows\)/);
  assert.match(js, /route_pattern/);
  assert.match(js, /allowed_methods/);
});

test("service methods use explicit checkbox controls", () => {
  assert.match(js, /function methodSelect\(selected = \[\]\)/);
  assert.match(js, /class="checkbox-group"/);
  assert.match(js, /name="allowed_methods" type="checkbox"/);
  assert.match(js, /form\.getAll\("allowed_methods"\)/);
  assert.match(css, /\.checkbox-group/);
});

test("services expose route choices and cost mode guidance", () => {
  assert.match(js, /service-routes/);
  assert.match(js, /placeholder="temp-service-2"/);
  assert.match(js, /Use lowercase letters, numbers, and hyphens/);
  assert.match(js, /function serviceRouteOptions\(\)/);
  assert.match(js, /Import from Studio/);
  assert.match(js, /function studioImportTable\(rows\)/);
  assert.match(js, /async function syncSelectedStudioServices\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/services\/sync/);
  assert.match(js, /data-import-sync/);
  assert.match(js, /function importDiffTemplate/);
  assert.match(js, /Fixed records the configured estimate per request/);
  assert.match(js, /Passthrough records provider-reported response cost/);
  assert.match(css, /\.help/);
});

test("studio import modal constrains and scrolls wide service tables", () => {
  assert.match(js, /class="modal-form"/);
  assert.match(js, /class="modal-scroll"/);
  assert.match(js, /studio-import-table/);
  assert.match(js, /service-picker-table/);
  assert.match(css, /\.modal-scroll/);
  assert.match(css, /max-height: calc\(100vh - 32px\)/);
  assert.match(css, /\.studio-import-table table/);
  assert.match(css, /\.service-picker-table table/);
});

test("usage view exposes project key service and route drilldown filters", () => {
  assert.match(js, /api\("\/admin-ui\/admin\/projects"\)/);
  assert.match(js, /api\("\/admin-ui\/admin\/keys"\)/);
  assert.match(js, /api\("\/admin-ui\/admin\/services"\)/);
  for (const field of ["project_id", "key_id", "service", "route", "provider", "model", "task_id", "run_id", "trace_id", "status", "min_cost_usd"]) {
    assert.match(js, new RegExp(`name="${field}"`));
  }
  assert.match(js, /\/admin-ui\/admin\/usage\/by-project/);
  assert.match(js, /\/admin-ui\/admin\/usage\/by-key/);
  assert.match(js, /\/admin-ui\/admin\/usage\/by-service/);
  assert.match(js, /\/admin-ui\/admin\/usage\/by-task/);
  assert.match(js, /\/admin-ui\/admin\/usage\/timeseries/);
  assert.match(js, /\/admin-ui\/admin\/usage\/export\.json/);
  assert.match(js, /\/admin-ui\/admin\/usage\/export\.csv/);
  assert.match(js, /\/admin-ui\/admin\/tasks\/\$\{encodeURIComponent\(taskId\)\}\/usage/);
  assert.match(js, /async function loadUsageExport\(event\)/);
  assert.match(js, /async function loadTaskUsage\(event\)/);
  assert.match(js, /function usageTimeseriesTable\(rows\)/);
});

test("audit view exposes read-only operator event filters and redacted snapshots", () => {
  assert.match(js, /async function audit\(\)/);
  assert.match(js, /async function loadAuditEvents\(event\)/);
  assert.match(js, /function auditEventTable\(rows\)/);
  assert.match(js, /\/admin-ui\/admin\/audit-events/);
  for (const field of ["action", "target_type", "target_id", "actor_token_id", "limit"]) {
    assert.match(js, new RegExp(`name="${field}"`));
  }
  assert.match(js, /Before\/after/);
  assert.doesNotMatch(js, /data-audit-action/);
});

test("governance and provider intelligence controls are present", () => {
  assert.match(js, /async function policyLayerAction\(event\)/);
  assert.match(js, /data-policy-layer-action="delete"/);
  assert.match(js, /\/admin-ui\/admin\/policy-layers\/\$\{layerId\}/);
  assert.match(js, /method: "DELETE"/);
  assert.match(js, /async function saveProviderHealthState\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/provider-health\/state", \{ method: "POST"/);
  assert.match(js, /data-health-state-edit/);
  assert.match(js, /active_check_ok/);
  assert.match(js, /passive_success_count/);
  assert.match(js, /circuit_state/);
});

test("virtual keys expose explicit no-expiration controls", () => {
  assert.match(js, /name="no_expires_at" type="checkbox"/);
  assert.match(js, /No expiration/);
  assert.match(js, /function keyExpiry\(key\)/);
  assert.match(js, /non-expiring/);
});

test("virtual keys expose policy simulator presets and lifecycle controls", () => {
  for (const field of [
    "preset",
    "rotation_due_at",
    "max_requests_per_day",
    "max_tokens_per_day",
    "max_cost_per_request",
    "max_request_body_bytes",
    "max_response_body_bytes",
    "allowed_hours_utc",
    "service_name",
  ]) {
    assert.match(js, new RegExp(`name="${field}"`));
  }
  assert.match(js, /developer/);
  assert.match(js, /production_worker/);
  assert.match(js, /external_partner/);
  assert.match(js, /async function simulatePolicy\(event\)/);
  assert.match(js, /data-policy-sim-service/);
  assert.match(js, /const serviceMode = provider === "internal-service" \|\| path\.startsWith\("\/services\/"\)/);
  assert.match(js, /const serviceName = serviceMode \? form\.get\("service_name"\) \|\| null : null/);
  assert.match(js, /if \(!serviceMode && serviceSelect\) serviceSelect\.value = ""/);
  assert.match(js, /service_name: serviceName/);
  assert.match(js, /Use a concrete service path such as/);
  assert.match(js, /async function savePolicyLayer\(event\)/);
  assert.match(js, /api\("\/admin-ui\/admin\/policy\/simulate"/);
  assert.match(js, /api\("\/admin-ui\/admin\/policy-layers"/);
});

test("floating notifications auto-dismiss and still support manual close", () => {
  assert.match(js, /let noticeTimer/);
  assert.match(js, /const delay = tone === "success" \? (4000|4e3) : (9000|9e3)/);
  assert.match(js, /setTimeout\(dismiss, delay\)/);
  assert.match(js, /data-close-message/);
  assert.match(js, /mouseenter/);
  assert.match(js, /focusin/);
});

test("service configuration exposes health check endpoint fields", () => {
  assert.match(js, /name="health_check_path"/);
  assert.match(js, /name="health_check_method"/);
  assert.match(js, /function healthCheckLabel\(row\)/);
  assert.match(js, /health_check_path: patch \? nullableString/);
  assert.match(js, /health_check_method: form\.get\("health_check_method"\) \|\| "GET"/);
});

test("service editor closes after a successful save", () => {
  assert.match(
    js,
    /async function patchService\(event\) \{[\s\S]*await api\(`\/admin-ui\/admin\/services\/\$\{serviceName\}`,[\s\S]*state\.editingServiceName = null;[\s\S]*await services\(\);[\s\S]*\}/,
  );
});

test("guardrail drawer closes after a successful save", () => {
  assert.match(
    js,
    /async function submitGuardrail\(event\) \{[\s\S]*await api\(path,[\s\S]*state\.editingGuardrailName = null;[\s\S]*await guardrails\(\);[\s\S]*\}/,
  );
});

test("admin portal escapes rendered user-controlled values", () => {
  assert.match(js, /function esc\(value\)/);
  for (const replacement of ["&amp;", "&lt;", "&gt;", "&quot;", "&#039;"]) {
    assert.match(js, new RegExp(replacement.replace("&", "&")));
  }
});

test("admin portal remains usable on narrow screens", () => {
  assert.match(css, /@media \(max-width: 760px\)/);
  assert.match(css, /grid-template-columns: 1fr/);
  assert.match(css, /overflow-x: auto/);
});
