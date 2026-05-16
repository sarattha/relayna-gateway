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
  for (const view of ["overview", "projects", "keys", "providers", "routes", "services", "usage", "health", "settings"]) {
    assert.match(html, new RegExp(`data-view="${view}"`));
  }
  assert.match(
    html,
    /data-view="overview"[\s\S]*data-view="providers"[\s\S]*data-view="services"[\s\S]*data-view="routes"[\s\S]*data-view="projects"[\s\S]*data-view="keys"[\s\S]*data-view="usage"[\s\S]*data-view="health"[\s\S]*data-view="settings"/,
  );
  assert.match(html, /id="operator-token"/);
  assert.match(html, /id="rotate-token"/);
});

test("admin portal calls the expected gateway admin APIs", () => {
  for (const endpoint of [
    "/admin/usage/summary",
    "/admin/provider-health",
    "/admin/projects",
    "/admin/providers",
    "/admin/openai-routes",
    "/admin/keys",
    "/admin/services",
    "/admin/studio/connection",
    "/admin/studio/connection/test",
    "/admin/studio/services",
    "/admin/operator-token/rotate",
    "/readyz",
  ]) {
    assert.match(js, new RegExp(endpoint.replaceAll("/", "\\/")));
  }
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

test("admin portal uses structured project and provider controls", () => {
  assert.match(js, /async function projects\(\)/);
  assert.match(js, /async function providers\(\)/);
  assert.match(js, /function projectOptions\(selected = ""\)/);
  assert.match(js, /function projectServiceForm\(project\)/);
  assert.match(js, /data-project-services-form/);
  assert.match(js, /serviceSelectionControl\(project\.service_names \|\| \[\], "service_names", "Project services"\)/);
  assert.match(js, /service_names: form\.getAll\("service_names"\)/);
  assert.match(js, /function providerPolicySelect\(selected = \[\]\)/);
  assert.match(js, /name="allowed_providers" type="checkbox"/);
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
  assert.match(js, /function serviceRouteOptions\(\)/);
  assert.match(js, /Import from Studio/);
  assert.match(js, /function studioImportTable\(rows\)/);
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
  assert.match(js, /api\("\/admin\/projects"\)/);
  assert.match(js, /api\("\/admin\/keys"\)/);
  assert.match(js, /api\("\/admin\/services"\)/);
  for (const field of ["project_id", "key_id", "service", "route"]) {
    assert.match(js, new RegExp(`name="${field}"`));
  }
  assert.match(js, /\/admin\/usage\/by-project/);
  assert.match(js, /\/admin\/usage\/by-key/);
  assert.match(js, /\/admin\/usage\/by-service/);
});

test("virtual keys expose explicit no-expiration controls", () => {
  assert.match(js, /name="no_expires_at" type="checkbox"/);
  assert.match(js, /No expiration/);
  assert.match(js, /function keyExpiry\(key\)/);
  assert.match(js, /non-expiring/);
});

test("service editor closes after a successful save", () => {
  assert.match(
    js,
    /async function patchService\(event\) \{[\s\S]*await api\(`\/admin\/services\/\$\{serviceName\}`,[\s\S]*state\.editingServiceName = null;[\s\S]*await services\(\);[\s\S]*\}/,
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
