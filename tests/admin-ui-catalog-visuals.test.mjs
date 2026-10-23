import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/catalog-visuals.ts", import.meta.url), "utf8");
const {
  catalogCredentialState, catalogServiceTransport, projectCatalogInsights,
  providerCatalogInsights, serviceCatalogInsights, routeCatalogInsights,
} = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const main = readFileSync(new URL("../crates/gateway-api/admin-ui/src/main.ts", import.meta.url), "utf8");
function sourceFunction(name) {
  const start = main.indexOf(`function ${name}(`);
  assert.notEqual(start, -1, `${name} exists`);
  return main.slice(start, main.indexOf("\n}", start) + 2);
}
const esc = new Function(`${sourceFunction("esc")}\nreturn esc;`)();
const protocolSummary = new Function("esc", `${sourceFunction("serviceProtocolSummary")}\nreturn serviceProtocolSummary;`)(esc);
const values = visual => visual.distribution.map(item => item.value);

{
  const projects = [
    { id: "one", service_names: ["alpha", "alpha", "beta"] },
    { id: "two", service_names: [] },
    { id: "three" },
    { id: "four", service_names: ["not-loaded"] },
  ];
  const services = [{ name: "alpha", enabled: true }, { name: "beta", enabled: false }];
  const before = JSON.stringify({ projects, services });
  const visual = projectCatalogInsights(projects, services);
  assert.deepEqual(values(visual), [2, 1, 1], "missing project links remain a separate category");
  assert.equal(visual.items[0].value, null, "missing selections cannot produce an exact distinct total");
  assert.equal(visual.items[1].value, null, "unresolved registration state cannot be presented as zero enabled");
  assert.equal(visual.items[2].value, null);
  const knownLinks = projectCatalogInsights(projects.filter(project => project.service_names), services);
  assert.equal(knownLinks.items[0].value, 3, "shared and repeated service names are counted once");
  assert.equal(knownLinks.items[2].value, 1);
  assert.equal(projectCatalogInsights(projects.slice(0, 1), services).items[1].value, 1, "the caller can supply only the selected project scope");
  assert.equal(projectCatalogInsights([], services).items[1].value, 0, "a successfully loaded empty scope is a measured zero");
  assert.deepEqual(projectCatalogInsights([], services).distribution, [], "empty inventories show an empty figure rather than empty bars");
  assert.equal(JSON.stringify({ projects, services }), before, "visual derivation never mutates fetched data");
}

{
  const services = [
    { allowed_methods: ["POST"] },
    { allowed_methods: ["GET"], route_pattern: "/services/events/*", access: { websocket: {} } },
    { allowed_methods: ["POST"], route_pattern: "/services/events/*", access: { websocket: {} } },
    { allowed_methods: ["GET"], route_pattern: "/app/tara/channel/web/v1/*", access: { accessa: { app: "tara", channel: "web" } } },
    { allowed_methods: ["GET"], route_pattern: "/channel/web/v1/me", access: { accessa: { app: null, channel: "web" } } },
    { allowed_methods: ["GET"], route_pattern: "/services/alias/*", access: { accessa: { app: "tara", channel: "web" } } },
    { allowed_methods: ["POST"], foundry: { mode: "registered_agent" } },
  ];
  for (const service of services) {
    const configured = ["websocket", "accessa-websocket"].includes(catalogServiceTransport(service));
    assert.equal(configured, protocolSummary(service).includes("HTTP + WS"), "transport analysis matches the existing service protocol contract");
  }
  assert.equal(catalogServiceTransport({}), "unknown", "missing methods do not establish a transport contract");
  assert.equal(catalogServiceTransport({ ...services[1], enabled: false }), "websocket", "configuration is independent of availability");
  const rows = services.slice(0, 4).map((service, index) => ({ ...service, name: `service-${index}`, enabled: index !== 3, sync_status: ["local", "synced", "failed", "stale"][index] }));
  rows.push({ name: "unknown" });
  const visual = serviceCatalogInsights(rows, [{ service_names: ["service-0", "service-0"] }]);
  assert.deepEqual(values(visual), [1, 1, 2, 1]);
  assert.equal(visual.items[0].value, null);
  assert.equal(visual.items[1].value, 1);
  assert.equal(visual.items[2].value, null, "unknown methods cannot become an exact WebSocket total");
  assert.equal(serviceCatalogInsights(rows.slice(0, 4), []).items[2].value, 2, "disabled but configured Accessa sockets remain in configuration coverage");
  assert.equal(serviceCatalogInsights(rows, [{}]).items[1].value, null);
}

{
  const providers = [
    { enabled: true, credential_configured: true },
    { enabled: true, credential_configured: false },
    { enabled: false, credential_configured: true },
    { provider: "azure-foundry", enabled: true, credential_configured: false, foundry: { method: "workload_identity" } },
    { enabled: true },
    { credential_configured: true },
  ];
  const mappings = [
    { scope: "key", enabled: true, credential_configured: true },
    { scope: "key", enabled: false, credential_configured: true },
    { scope: "project", enabled: true, credential_configured: true },
    { scope: "project", enabled: true, credential_configured: false },
  ];
  const visual = providerCatalogInsights(providers, mappings, { enabled: true, authentication_mode: "litellm_bearer" });
  assert.deepEqual(values(visual), [2, 1, 1, 2], "provider states form disjoint configuration categories");
  assert.equal(values(visual).reduce((sum, value) => sum + value, 0), providers.length);
  assert.deepEqual(visual.items.map(item => item.value), [1, 1, "Enabled"]);
  assert.match(visual.items[2].detail, /Caller LiteLLM key/);
  assert.equal(providerCatalogInsights([], [], null).items[2].value, null);
  assert.equal(providerCatalogInsights([], [{ scope: "key", enabled: true }], null).items[0].value, null, "missing credential flags cannot imply zero configured mappings");
  assert.equal(catalogCredentialState({ provider: "azure-foundry", foundry: {} }).configured, null, "unknown Azure identity methods cannot appear configured");
  assert.equal(catalogCredentialState(providers[3]).tone, "neutral", "saved managed identity is not a verified connection");
}

{
  const routes = [
    { enabled: true, mode: "managed_by_gateway" },
    { enabled: true, mode: "direct_litellm_passthrough" },
    { enabled: false, mode: "managed_by_gateway" },
    { enabled: true, mode: "future-mode" },
    {},
  ];
  const visual = routeCatalogInsights(routes.slice(0, 3), routes.slice(3), [], [{ route: "/litellm/*", access: { websocket: {} } }]);
  assert.deepEqual(values(visual), [1, 1, 1, 2]);
  assert.equal(values(visual).reduce((sum, value) => sum + value, 0), routes.length);
  assert.equal(visual.items[2].value, 1);
  assert.deepEqual(visual.steps.map(step => step.label), ["Control plane", "Registered services", "Canonical APIs", "LiteLLM fallback"]);
  assert.match(visual.caption, /Aliases share/);
}

// Exercise table rendering around real forms and action attributes. The
// visual layer must not clone controls, expose input values, or remove columns.
{
  const badge = (label, tone = "neutral") => `<span class="badge ${esc(tone)}">${esc(label)}</span>`;
  const tableWrap = body => `<div class="table-wrap">${body}</div>`;
  const table = new Function("esc", "tableWrap", `${sourceFunction("table")}\nreturn table;`)(esc, tableWrap);
  const deps = {
    esc, attr: esc, badge, table, time: () => "Recorded timestamp", catalogCredentialState,
    projectServiceForm: row => `<form data-project-services-form data-project-id="${esc(row.id)}"><input name="service_names"><button>Save services</button></form>`,
    providerAuthSettingsForm: row => `<form data-provider-config-form data-provider-id="${esc(row.id)}"><input name="credential" type="password"><button>Update</button></form>`,
    mappingTargetName: row => row.target_id,
  };
  const render = name => new Function(...Object.keys(deps), `${sourceFunction(name)}\nreturn ${name};`)(...Object.values(deps));
  const hostile = '<img src=x onerror="bad()">';
  const dom = new JSDOM(render("projectTable")([{ id: "project-id", name: hostile, service_names: ["one"] }]));
  const document = dom.window.document;
  assert.equal(document.querySelectorAll("th").length, 5);
  assert.equal(document.querySelectorAll("details form[data-project-services-form]").length, 1);
  assert.equal(document.querySelector('[data-project-action="usage"]').dataset.projectId, "project-id");
  assert.equal(document.querySelector('[data-project-action="delete"]').dataset.projectId, "project-id");
  assert.equal(document.querySelectorAll("img").length, 0);
  assert.ok(document.querySelector(".table-wrap"));
  document.body.innerHTML = render("providerTable")([{ id: "provider-id", name: hostile, provider: "litellm", enabled: true, credential_configured: true }]);
  assert.equal(document.querySelectorAll("th").length, 7);
  assert.equal(document.querySelectorAll("details form[data-provider-config-form]").length, 1);
  assert.equal(document.querySelector('[name="credential"]').getAttribute("value"), null);
  assert.equal(document.querySelector('[data-provider-action="disable"]').dataset.providerId, "provider-id");
  assert.equal(document.querySelector('[data-provider-action="delete"]').dataset.providerId, "provider-id");
  assert.equal(document.querySelectorAll("img").length, 0);
  document.body.innerHTML = render("litellmCredentialMappingTable")([{ id: "mapping-id", scope: "project", target_id: "project-id", target_label: hostile, enabled: true, credential_configured: true }]);
  assert.equal(document.querySelectorAll("th").length, 6);
  assert.equal(document.querySelector('[data-litellm-mapping-action="disable"]').dataset.mappingId, "mapping-id");
  assert.equal(document.querySelector('[data-litellm-mapping-action="delete"]').dataset.mappingId, "mapping-id");
  assert.equal(document.querySelectorAll("img").length, 0);
  dom.window.close();
}

console.log("ok - catalog configuration coverage, scoped relationships, protocol contracts, unknown states, original forms, escaping and row actions");
