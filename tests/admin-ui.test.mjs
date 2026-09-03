import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const uiDir = join(root, "crates/gateway-api/src/static/admin-ui");
const sourceDir = join(root, "crates/gateway-api/admin-ui/src");
const html = readFileSync(join(uiDir, "index.html"), "utf8");
const deployedJs = readFileSync(join(uiDir, "app.js"), "utf8");
const sourceJs = readFileSync(join(sourceDir, "main.ts"), "utf8");
// Source-level behavior assertions must not depend on Rollup's internal symbol
// naming. The deployed bundle remains part of endpoint-contract coverage below.
const js = `${sourceJs}\n${deployedJs}`;
const css = readFileSync(join(uiDir, "app.css"), "utf8");
const microsoftSignInSvg = readFileSync(join(uiDir, "microsoft-sign-in.svg"), "utf8");

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    console.error(`not ok - ${name}`);
    throw error;
  }
}

function sourceFunction(name) {
  const start = sourceJs.indexOf(`function ${name}(`);
  assert.notEqual(start, -1, `missing source function ${name}`);
  const bodyStart = sourceJs.indexOf("{", start);
  let depth = 0;
  for (let index = bodyStart; index < sourceJs.length; index += 1) {
    if (sourceJs[index] === "{") depth += 1;
    if (sourceJs[index] === "}") depth -= 1;
    if (depth === 0) return sourceJs.slice(start, index + 1);
  }
  throw new Error(`unterminated source function ${name}`);
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
  assert.match(html, /aria-label="Current Relayna Gateway version"[\s\S]*v0\.1\.31/);
  assert.match(js, /Release target[\s\S]*v0\.1\.31/);
});

test("portal uses Entra BFF sessions and preserves explicit break-glass access", () => {
  assert.match(sourceJs, /const state = \{\s*view: "overview",/);
  assert.match(html, /id="entra-sign-in"[\s\S]*src="\/admin-ui\/microsoft-sign-in\.svg"[\s\S]*alt="Sign in with Microsoft"/);
  assert.match(html, /Sign in with your work or school account/);
  assert.doesNotMatch(html, /Sign in with Microsoft Entra ID/);
  assert.match(microsoftSignInSvg, /width="215" height="41" viewBox="0 0 215 41"/);
  assert.match(microsoftSignInSvg, /fill="#f25022"[\s\S]*fill="#00a4ef"[\s\S]*fill="#7fba00"[\s\S]*fill="#ffb900"/);
  assert.match(html, /class="break-glass-login"/);
  assert.match(html, /id="access-state-sign-out"[^>]*>Sign out and switch account/);
  assert.match(html, /Governed access\.[\s\S]*Operational clarity\./);
  assert.match(html, /Your identity tokens stay server-side/);
  assert.match(css, /\.login-primary img[\s\S]*width: 215px[\s\S]*height: auto/);
  assert.match(css, /\.login-primary \{[\s\S]*width: 100%[\s\S]*justify-content: center/);
  assert.match(css, /\.nav-groups \{[\s\S]*align-content: start/);
  assert.match(css, /\.login-shell[\s\S]*grid-template-columns: minmax\(430px, 56fr\) minmax\(420px, 44fr\)/);
  assert.match(css, /@media \(max-width: 760px\)[\s\S]*\.login-shell \{[\s\S]*display: block/);
  assert.match(js, /\/admin-ui\/auth\/config/);
  assert.match(js, /\/admin-ui\/auth\/session/);
  assert.match(js, /\/admin-ui\/auth\/logout/);
  assert.match(sourceJs, /logoutUrl = result\?\.logout_url \?\? null/);
  assert.match(sourceJs, /if \(logoutUrl\) location\.assign\(logoutUrl\)/);
  assert.match(sourceJs, /#access-state-sign-out"\)\.addEventListener\("click", signOut\)/);
  const accessState = sourceFunction("renderAccessState");
  assert.match(accessState, /\.break-glass-login"\)\.classList\.remove\("hidden"\)/);
  assert.match(accessState, /#access-state-sign-out"\)\.classList\.remove\("hidden"\)/);
  assert.match(js, /"x-csrf-token": state\.session\.csrf_token/);
  assert.match(js, /session\.member\.status !== "active"/);
  assert.match(sourceJs, /const returnTo = `\$\{location\.pathname\}\$\{location\.hash\}`/);
  assert.doesNotMatch(sourceJs, /location\.hash \|\| "#\/my-projects"/);
  assert.doesNotMatch(js, /sessionStorage\.setItem\([^,]+,\s*.*id_token/);
});

test("admin members and managed identities expose exact service bindings", () => {
  for (const view of ["members", "managed-identities"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(js, /async function members\(\)/);
  assert.match(js, /\/admin-ui\/admin\/members/);
  assert.match(js, /data-membership-form/);
  assert.match(js, /option\("viewer", "viewer"\)/);
  assert.match(js, /option\("owner", ""\)/);
  assert.match(js, /async function managedIdentities\(\)/);
  assert.match(js, /\/admin-ui\/admin\/managed-identities/);
  assert.match(js, /gateway\.monitor\.read/);
});

test("service-owner workspace uses scoped owner APIs and responsive dashboards", () => {
  for (const view of ["my-services", "service-dashboard"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(html, /id="workspace-select"/);
  assert.match(js, /async function myServices\(\)/);
  assert.match(js, /api\("\/owner\/v1\/services"\)/);
  assert.match(js, /async function serviceDashboard\(\)/);
  assert.match(js, /\/owner\/v1\/services\/\$\{encodeURIComponent\(serviceName\)\}\/dashboard/);
  assert.match(js, /\/owner\/v1\/services\/\$\{encodeURIComponent\(serviceName\)\}\/events\?\$\{eventsQuery\}/);
  assert.match(js, /id="owner-incident-chart" role="img"/);
  assert.match(js, /function renderOwnerIncidentChart\(rows, markers\)/);
  assert.match(js, /Error rate \(%\)/);
  assert.match(js, /P95 latency \(ms\)/);
  assert.match(js, /Version \$\{version\} first observed by Gateway/);
  for (const control of ["owner-range-select", "owner-outcome-select", "owner-status-select"]) {
    assert.match(js, new RegExp(`id="${control}"`));
  }
  for (const range of ["6h", "24h", "7d"]) assert.match(js, new RegExp(`option\\("${range}"`));
  assert.match(js, /data-owner-page="previous"/);
  assert.match(js, /data-owner-page="next"/);
  assert.match(js, /data-owner-request/);
  assert.match(js, />View details<\/button>/);
  assert.match(js, /async function openOwnerRequestDetails\(event\)/);
  assert.match(js, /\/requests\/\$\{encodeURIComponent\(requestId\)\}/);
  assert.match(js, /restoreFocus: trigger/);
  assert.match(js, /owner-request-drawer/);
  assert.match(js, /no proxy debug bundle was captured/);
  assert.match(js, /badge\(row\.status, row\.status === "success" \? "good" : "bad"\)/);
  const serviceDashboardSource = sourceFunction("serviceDashboard");
  assert.match(serviceDashboardSource, /const generation = \+\+ownerDashboardGeneration/);
  assert.match(serviceDashboardSource, /if \(isStale\(\)\) return/);
  assert.match(serviceDashboardSource, /statusCodes\.push\(state\.ownerDashboardFilters\.statusCode\)/);
  assert.match(css, /\.owner-dashboard-grid/);
  assert.match(css, /\.owner-service-grid/);
  assert.match(css, /\.owner-incident-chart-wrap/);
  assert.match(css, /\.owner-request-filters/);
  assert.match(css, /\.owner-request-drawer/);
  assert.match(css, /@media \(max-width: 700px\)[\s\S]*\.owner-dashboard-grid/);
});

test("project-owner workspace uses exact project APIs and the shared monitoring role", () => {
  for (const view of ["my-projects", "project-dashboard"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(sourceJs, /state\.session\?\.project_memberships\?\.length/);
  assert.match(js, /async function myProjects\(\)/);
  assert.match(js, /api\("\/owner\/v1\/projects"\)/);
  assert.match(js, /async function projectDashboard\(\)/);
  assert.match(js, /\/owner\/v1\/projects\/\$\{encodeURIComponent\(projectId\)\}\/dashboard/);
  assert.match(js, /\/owner\/v1\/projects\/\$\{encodeURIComponent\(projectId\)\}\/events\?\$\{eventsQuery\}/);
  assert.match(sourceJs, /usageEventsTable\(events\.rows \|\| \[\], \{ ownerProject: projectId \}\)/);
  assert.match(sourceJs, /\/admin-ui\/admin\/managed-identity-projects/);
  assert.match(sourceJs, /The same <code>gateway\.monitor\.read<\/code> app role is narrowed to one exact project/);
  assert.match(sourceJs, /data-project-membership-form/);
  const projectDashboardSource = sourceFunction("projectDashboard");
  assert.match(projectDashboardSource, /const generation = \+\+ownerDashboardGeneration/);
  assert.match(projectDashboardSource, /Project incident signals/);
  assert.match(projectDashboardSource, /Service usage/);
});

test("admin portal exposes responsive operator navigation and accessible workflows", () => {
  for (const marker of [
    'class="skip-link"',
    'id="nav-toggle"',
    'id="command-trigger"',
    'id="governed-change-trigger"',
    'id="last-refreshed"',
    'id="content" tabindex="-1"',
    'role="dialog" aria-modal="true"',
  ]) {
    assert.match(html, new RegExp(marker));
  }
  for (const behavior of [
    "function mountDialog",
    'window.addEventListener("hashchange"',
    'setAttribute("aria-current", "page")',
    "function showCommandPalette",
    "function openNavigation",
    "function formSection",
    'id="overview-chart" role="img"',
    "new Chart",
  ]) {
    assert.match(js, new RegExp(behavior.replace(/[()]/g, "\\$&")));
  }
  for (const style of [".nav-backdrop", ".command-palette", ".form-section", ".sticky-form-actions", ".overview-chart-wrap"]) {
    assert.match(css, new RegExp(style.replace(".", "\\.")));
  }
  assert.match(css, /@media \(max-width: 920px\)/);
});

test("command palette respects the active workspace and keeps rows readable", () => {
  assert.match(js, /const ownerViewIds = new Set\(\["my-services", "service-dashboard", "my-projects", "project-dashboard"\]\)/);
  assert.match(js, /function commandViewsForWorkspace\(\)/);
  assert.match(js, /const commands = commandViewsForWorkspace\(\)/);
  assert.match(js, /state\.workspace === "owner"/);
  assert.match(css, /\.command-palette\s*\{[^}]*overflow:\s*hidden/s);
  assert.match(css, /\.command-list\s*\{[^}]*grid-auto-rows:\s*minmax\(48px, auto\)/s);
  assert.match(css, /\.command-item\s*\{[^}]*white-space:\s*normal/s);
  assert.match(css, /\.command-item > span\s*\{[^}]*min-width:\s*0/s);
});

test("admin portal calls the expected gateway admin APIs", () => {
  for (const endpoint of [
    "/admin-ui/admin/usage/dashboard",
    "/admin-ui/admin/usage/events",
    "/admin-ui/admin/usage/filter-values",
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
    "/admin-ui/admin/anthropic-routes",
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
    assert.ok(deployedJs.includes(endpoint), `expected app.js to call ${endpoint}`);
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
  assert.match(js, /Application ID \/ token audience/);
  assert.match(js, /placeholder="gateway\.invoke"/);
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
  assert.match(js, /Use raw for headers like x-litellm-api-key/);
  assert.match(js, /x-litellm-key usually needs bearer/);
  assert.match(js, /async function updateProviderAuthSettings\(event\)/);
  assert.match(js, /async function saveLiteLlmCredentialMapping\(event\)/);
  assert.match(js, /async function liteLlmCredentialMappingAction\(event\)/);
  assert.match(js, /async function saveLiteLlmPassthroughSettings\(event\)/);
  assert.match(js, /\/admin\/providers\/litellm-credentials/);
  assert.match(js, /\/admin-ui\/admin\/providers\/litellm-passthrough/);
  assert.match(js, /name="timeout_ms"/);
  assert.match(js, /name="max_request_body_bytes"/);
  assert.match(js, /name="max_response_body_bytes"/);
  assert.match(js, /LiteLLM passthrough/);
  assert.match(js, /Exposure risk/);
  assert.match(js, /credential_configured/);
  assert.match(js, /name="credential" type="password"/);
  assert.doesNotMatch(js, /row\.credential_secret/);
});

test("routes view exposes canonical provider route modes", () => {
  assert.match(js, /managed_by_gateway/);
  assert.match(js, /direct_litellm_passthrough/);
  assert.match(js, /OpenAI-compatible and rerank routes/);
  assert.match(js, /"\/v1\/rerank"/);
  assert.match(js, /Anthropic Claude routes/);
  assert.match(css, /\.route-logo\.openai/);
  assert.match(css, /\.route-logo\.anthropic/);
  assert.match(js, /class="inline-form route-config-form"/);
  assert.match(js, /Mode<select name="mode"/);
  assert.match(js, /Timeout ms<input name="timeout_ms"/);
  assert.match(js, /Max request bytes<input name="max_request_body_bytes"/);
  assert.match(js, /Max response bytes<input name="max_response_body_bytes"/);
  assert.match(css, /\.route-config-form/);
  assert.match(css, /\.route-config-field/);
  assert.match(js, /async function saveOpenAiRouteMode\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/openai-routes\/\$\{routeId\}\/config/);
  assert.match(js, /async function saveAnthropicRouteMode\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/anthropic-routes\/\$\{routeId\}\/config/);
  assert.match(js, /data-service-route-timeout-form/);
  assert.match(js, /function serviceRouteTimeoutForm\(row\)/);
  assert.match(js, /async function saveServiceRouteTimeout\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/services\/\$\{serviceName\}/);
  assert.match(js, /JSON\.stringify\(\{ timeout_ms: timeoutMs \}\)/);
  assert.match(js, /Timeout must be a whole number between 1 and 600000 ms\./);
  assert.match(js, /function routeConfigPayload\(form\)/);
  assert.match(js, /max_request_body_bytes: nullableNumber\(form\.get\("max_request_body_bytes"\)\)/);
  assert.match(js, /max_response_body_bytes: nullableNumber\(form\.get\("max_response_body_bytes"\)\)/);
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
  assert.match(js, /class="service-stack"/);
  assert.match(js, /Import from Studio/);
  assert.match(js, /function studioImportTable\(rows\)/);
  assert.match(js, /async function syncSelectedStudioServices\(event\)/);
  assert.match(js, /\/admin-ui\/admin\/services\/sync/);
  assert.match(js, /data-import-sync/);
  assert.match(js, /function importDiffTemplate/);
  assert.match(js, /Fixed records the configured estimate per request/);
  assert.match(js, /Passthrough records provider-reported response cost/);
  assert.match(js, /function pricingRulesEditor\(rules\)/);
  assert.match(js, /data-pricing-rule-editor/);
  assert.match(js, /name="pricing_rules"/);
  assert.match(js, /data-pricing-rule-action="add"/);
  assert.match(js, /data-pricing-rule-field="json_pointer"/);
  assert.match(js, /Use JSON Pointer selectors such as \/model or \/payload\/page_count/);
  assert.match(js, /function openApiEndpointPricingEditor\(service\)/);
  assert.match(js, /name="endpoint_pricing_rules"/);
  assert.match(js, /openapi_source_path: patch \? nullableString/);
  assert.match(js, /data-openapi-action="preview"/);
  assert.match(js, /\/openapi\/preview/);
  assert.match(js, /\/openapi\/sync/);
  assert.match(js, /New Relayna endpoints default to none/);
  assert.match(js, /does not forward service credentials/);
  assert.doesNotMatch(js, /<label>Pricing rules<textarea/);
  assert.match(css, /\.pricing-rule-row/);
  assert.match(css, /\.service-stack/);
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
  for (const field of ["project_id", "key_id", "service", "route", "method", "endpoint", "provider", "model", "task_id", "run_id", "trace_id", "status", "status_code", "time_preset", "from", "to", "interval", "min_cost_usd", "breakdown_limit", "sort_by", "limit"]) {
    assert.match(js, new RegExp(`name="${field}"`));
  }
  for (const preset of ["last_1h", "last_6h", "last_24h", "last_7d", "last_30d", "today", "yesterday", "this_week", "this_month", "custom"]) {
    assert.match(js, new RegExp(`value="${preset}"`));
  }
  assert.match(js, /type="datetime-local"/);
  assert.match(js, /query\.set\("from", range\.from\)/);
  assert.match(js, /query\.set\("to", range\.to\)/);
  assert.match(js, /query\.set\("interval", interval\)/);
  assert.match(js, /Usage start time must be before the end time/);
  assert.match(js, /function usageDateRange\(form\)/);
  assert.match(js, /function usageFilterValuesQueryFromForm\(formElement = document\.querySelector\("#usage-form"\)\)/);
  assert.match(js, /const filterQuery = usageFilterValuesQueryFromForm\(/);
  assert.match(js, /\/admin-ui\/admin\/usage\/filter-values\?\$\{filterQuery\}&field=route/);
  assert.match(js, /\/admin-ui\/admin\/usage\/filter-values\?\$\{filterQuery\}&field=endpoint/);
  assert.match(js, /\/admin-ui\/admin\/usage\/dashboard/);
  assert.match(js, /\/admin-ui\/admin\/usage\/events/);
  assert.match(js, /\/admin-ui\/admin\/usage\/filter-values/);
  assert.match(js, /\/admin-ui\/admin\/usage\/export\.\$\{format\}/);
  assert.match(js, /value="json">JSON/);
  assert.match(js, /value="csv">CSV/);
  assert.match(js, /\/admin-ui\/admin\/tasks\/\$\{encodeURIComponent\(taskId\)\}\/usage/);
  assert.match(js, /async function usageExportAction\(event\)/);
  for (const field of ["export_limit", "export_from", "export_to", "export_offset"]) {
    assert.match(sourceJs, new RegExp(`name="${field}"`));
  }
  assert.match(sourceJs, /value="all">All rows \(download\)/);
  assert.match(sourceJs, /const usageExportBatchSize = 10_000/);
  assert.match(sourceJs, /function usageExportDateRange\(form\)/);
  assert.match(sourceJs, /function usageQueryFromForm\(formElement = document\.querySelector\("#usage-form"\), \{ includeDateRange = true \} = \{\}\)/);
  assert.match(sourceJs, /const exportRange = data \? usageExportDateRange\(data\) : null;\s+const query = usageQueryFromForm\(undefined, \{ includeDateRange: !exportRange \}\);/);
  assert.match(sourceJs, /function usageExportHeaders\(\)/);
  assert.match(sourceJs, /state\.session\?\.csrf_token/);
  assert.match(sourceJs, /headers: usageExportHeaders\(\)/);
  assert.match(sourceJs, /function syncUsageExportControls\(\)/);
  assert.match(sourceJs, /button\.dataset\.usageExportAction !== "download"/);
  assert.match(sourceJs, /Export start time must be before the end time/);
  assert.match(sourceJs, /All rows requires a bounded start and end time/);
  assert.match(sourceJs, /query\.set\("limit", String\(usageExportBatchSize\)\)/);
  assert.match(sourceJs, /query\.set\("offset", String\(offset\)\)/);
  assert.match(sourceJs, /offset \+= pageRows/);
  assert.match(sourceJs, /function splitUsageCsvPage\(csv\)/);
  assert.match(sourceJs, /function countCsvRecords\(csv\)/);
  assert.match(sourceJs, /page\.header !== csvHeader/);
  assert.match(sourceJs, /class="actions usage-export-actions"/);
  assert.match(css, /\.usage-export-form/);
  assert.match(css, /\.usage-export-actions/);
  assert.match(js, /GATEWAY_OPERATOR_TOKEN/);
  assert.match(js, /data-debug-request/);
  assert.match(js, /dashboard\.breakdowns\.endpoints \|\| \[\]/);
  assert.match(js, /dashboard\.breakdowns\.project_key_services \|\| \[\]/);
  assert.match(js, /function usageProjectKeyServiceHierarchy\(rows\)/);
  assert.match(js, /function usageProjectKeyServiceTable\(rows\)/);
  assert.match(js, /data-usage-project=/);
  assert.match(js, /data-usage-key=/);
  assert.match(js, /projectName\(projectId\)/);
  assert.match(js, /keyName\(keyId\)/);
  assert.match(js, /row\.service_name == null \? "Unattributed"/);
  assert.match(js, /row\.endpoint_template \|\| row\.endpoint_path/);
  assert.match(js, /esc\(row\.status_code\)/);
  assert.match(js, /async function loadTaskUsage\(event\)/);
  assert.match(js, /function usageTimeseriesTable\(rows\)/);
  assert.match(js, /Service timeseries/);
  assert.match(js, /function usageServiceTimeseriesTable\(rows\)/);
  assert.match(js, /dashboard\.service_timeseries \|\| \[\]/);
  assert.match(js, /Rows per page/);
  assert.match(js, /function resetUsagePagination\(\)/);
  assert.match(js, /function usagePagedTable\(title, section, tableMarkup, page = \{\}, rowCount = 0\)/);
  assert.match(js, /query\.set\("timeseries_limit", String\(pageSize\)\)/);
  assert.match(js, /query\.set\("service_timeseries_offset", String\(state\.usagePagination\.serviceTimeseriesOffset\)\)/);
  assert.match(js, /data-usage-page-direction="next"/);
  assert.match(css, /\.table-pager/);
  assert.match(css, /\.usage-hierarchy-project/);
  assert.match(css, /\.usage-hierarchy-key/);
});

test("all-rows CSV pagination counts quoted multiline records", () => {
  const countCsvRecords = Function(`return (${sourceFunction("countCsvRecords")})`)();
  const splitUsageCsvPage = Function(`return (${sourceFunction("splitUsageCsvPage")})`)();
  const csv = 'request_id,status\nreq-1,success\nreq-2,"failed\nwith detail"\n';
  const page = splitUsageCsvPage(csv);

  assert.equal(page.header, "request_id,status\n");
  assert.equal(page.body, 'req-1,success\nreq-2,"failed\nwith detail"\n');
  assert.equal(countCsvRecords(page.body), 2);
  assert.equal(countCsvRecords("req-3,success"), 1);
  assert.equal(countCsvRecords(""), 0);
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
  assert.match(js, /clearPolicySimulationResult\(\)/);
  assert.match(js, /function validatePolicySimulationServicePath\(path, serviceName\)/);
  assert.match(js, /state\.services\.find\(\(item\) => item\.name === serviceName\)/);
  assert.match(js, /policySimulationPathMatchesRoutePattern\(trimmedPath, service\.route_pattern\)/);
  assert.match(js, /function policySimulationPathMatchesRoutePattern\(path, routePattern\)/);
  assert.match(js, /trimmedPath\.includes\("\*"\)/);
  assert.match(js, /trimmedPath === "\/services" \|\| trimmedPath === "\/services\/"/);
  assert.match(js, /path === prefix \|\| path\.startsWith\(`\$\{prefix\}\/`\)/);
  assert.match(js, /Path service \$\{segments\[1\]\} does not match selected service \$\{serviceName\}/);
  assert.match(js, /if \(!serviceMode && serviceSelect\) serviceSelect\.value = ""/);
  assert.match(js, /service_name: serviceName/);
  assert.match(js, /Choose a concrete service path such as/);
  assert.match(js, /Policy warnings/);
  assert.match(js, /applied_layers/);
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

test("global overlays stay above sticky shell chrome on every view", () => {
  const cssLayer = (selector) => {
    const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const layers = [...css.matchAll(new RegExp(`${escapedSelector}\\s*\\{[^}]*?z-index:\\s*(\\d+)`, "g"))].map((match) => Number(match[1]));
    assert.ok(layers.length > 0, `missing z-index for ${selector}`);
    return Math.max(...layers);
  };

  const shellLayer = Math.max(cssLayer(".global-toolbar"), cssLayer(".action-menu"), cssLayer(".nav-backdrop"), cssLayer(".sidebar"));
  assert.ok(cssLayer(".modal-backdrop") > shellLayer, "dialogs and drawers must cover the complete application shell");
  assert.ok(cssLayer(".message-box") > cssLayer(".modal-backdrop"), "notifications must remain visible above dialogs");
  assert.ok(cssLayer(".skip-link") > cssLayer(".message-box"), "the focused skip link must remain the top accessibility layer");
});

test("overview ignores clean provider health rows without a status field", () => {
  const overviewRisks = Function(`return (${sourceFunction("overviewRisks")})`)();
  const cleanRow = {
    name: "clean-provider",
    request_count: 10,
    error_count: 0,
    timeout_count: 0,
    fallback_count: 0,
  };
  assert.deepEqual(overviewRisks([cleanRow]), []);
  assert.equal(overviewRisks([{ ...cleanRow, error_count: 1 }]).length, 1);
});

test("top dialog selection is independent of later section elements", () => {
  const calls = [];
  const firstBackdrop = { closeDialog: () => calls.push("first") };
  const topBackdrop = { closeDialog: (value) => calls.push(["top", value]) };
  const document = {
    querySelectorAll(selector) {
      assert.equal(selector, ".modal-backdrop");
      return [firstBackdrop, topBackdrop];
    },
  };
  const closeTopDialog = Function("document", `return (${sourceFunction("closeTopDialog")})`)(document);
  closeTopDialog(true);
  assert.deepEqual(calls, [["top", true]]);
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
