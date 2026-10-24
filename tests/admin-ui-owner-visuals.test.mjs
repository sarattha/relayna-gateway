import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const moduleUrl = source => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const insights = moduleUrl(readFileSync(new URL("../crates/gateway-api/admin-ui/src/design-system/insights.ts", import.meta.url), "utf8"));
const catalog = moduleUrl(readFileSync(new URL("../crates/gateway-api/admin-ui/src/catalog-visuals.ts", import.meta.url), "utf8"));
const source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/owner-visuals.ts", import.meta.url), "utf8").replace('"./design-system/insights"', JSON.stringify(insights)).replace('"./catalog-visuals"', JSON.stringify(catalog));
const { ownerCatalogInsights, ownerDashboardInsights, ownerUsageDistribution, ownerRequestProfile, ownerIncidentSeries, ownerIncidentDescription } = await import(moduleUrl(source));
const { catalogServiceTransport } = await import(catalog);
const money = value => `$${value.toFixed(2)}`;
function inspect(html, check) {
  const dom = new JSDOM(html);
  try { check(dom.window.document); } finally { dom.window.close(); }
}
const metric = (document, label) => [...document.querySelectorAll(".insight-metric")].find(item => item.querySelector(".insight-label").textContent === label).querySelector("strong").textContent;
const bars = document => [...document.querySelectorAll(".insight-bar")].map(bar => [bar.querySelector("span").textContent, bar.querySelector("strong").textContent]);

{
  const main = readFileSync(new URL("../crates/gateway-api/admin-ui/src/main.ts", import.meta.url), "utf8");
  const start = main.indexOf("function ownerServiceCard(");
  const cardSource = main.slice(start, main.indexOf("\n}\n", start) + 2);
  const esc = value => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  const card = new Function("catalogServiceTransport", "esc", "badge", "attr", `${cardSource}; return ownerServiceCard;`)(catalogServiceTransport, esc, label => esc(label), esc);
  const registrations = [
    [{ allowed_methods: ["GET"], access: { websocket: {} } }, true],
    [{ allowed_methods: ["POST"], access: { websocket: {} } }, false],
    [{ allowed_methods: ["GET"], route_pattern: "/app/chat/channel/web/v1/*", access: { accessa: { app: "chat", channel: "web" } } }, true],
    [{ allowed_methods: ["POST"], route_pattern: "/app/chat/channel/web/v1/*", access: { accessa: { app: "chat", channel: "web" } } }, false],
    [{ allowed_methods: ["GET"], route_pattern: "/services/alias/*", access: { accessa: { app: "chat", channel: "web" } } }, false],
    [{ access: { websocket: {} } }, null],
  ];
  for (const [service, socket] of registrations) {
    inspect(ownerCatalogInsights([{ role: "owner", service }], "services"), document => {
      assert.equal(metric(document, "WebSocket configured"), socket === null ? "—" : `${socket ? 1 : 0} / 1`, "owner and admin summaries share GET and exact-path transport requirements");
    });
    inspect(card({ role: "owner", service }), document => {
      assert.equal(document.body.textContent.includes("WebSocket configured"), socket === true);
      if (socket === null) assert.match(document.body.textContent, /Transport not recorded/);
    });
  }
}

{
  inspect(ownerDashboardInsights({ summary: { request_count: 0, failure_count: 0, p95_latency_ms: 0, estimated_cost_usd: null } }, { money, range: "6h" }), document => {
    assert.equal(metric(document, "Requests"), "0");
    assert.equal(metric(document, "Recorded failure rate"), "—", "no samples do not imply a perfect failure rate");
    assert.equal(metric(document, "P95 latency"), "—");
    assert.equal(metric(document, "Estimated cost"), "—");
    assert.equal(document.querySelectorAll(".insight-bars progress").length, 0, "absent control counts do not become zeroes");
    assert.match(document.body.textContent, /Last 6 hours/);
    assert.equal(document.querySelectorAll(".page-insights").length, 1);
    assert.equal(document.querySelectorAll(".panel").length, 0, "the leading summary has no nested cards");
  });
  inspect(ownerDashboardInsights({ summary: { request_count: 4, failure_count: 1, p95_latency_ms: 0, estimated_cost_usd: 0, policy_denial_count: 0 } }, { money, range: "24h" }), document => {
    assert.equal(metric(document, "Recorded failure rate"), "25.0%");
    assert.equal(metric(document, "P95 latency"), "0 ms", "a recorded zero latency is retained");
    assert.equal(metric(document, "Estimated cost"), "$0.00");
    assert.equal(document.querySelector("progress").value, 0);
    assert.match(document.body.textContent, /categories may overlap/);
  });
}

{
  const services = [
    { role: "owner", service: { allowed_methods: ["GET"], access: { websocket: {} }, openapi_endpoints: [{}] } },
    { role: "viewer", service: { allowed_methods: ["GET"], route_pattern: "/app/assistant/channel/web/v1/*", access: { accessa: { app: "assistant", channel: "web" } }, openapi_endpoints: [] } },
    { role: "viewer", service: { allowed_methods: ["GET"], access: { accessa: { app: null } }, openapi_endpoints: [] } },
    { role: "viewer", service: {} },
  ];
  inspect(ownerCatalogInsights(services, "services"), document => {
    assert.equal(metric(document, "OpenAPI coverage"), "1 / 3", "only loaded catalogs contribute to coverage");
    assert.equal(metric(document, "WebSocket configured"), "2 / 3", "Accessa discovery does not imply socket capability");
    assert.deepEqual(bars(document), [["Owner", "1"], ["Viewer", "3"]]);
    assert.match(document.body.textContent, /not availability/);
  });
  const projects = [
    { project: { name: "One", service_names: ["shared", "only-one"] } },
    { project: { name: "Two", service_names: ["shared"] } },
    { project: { name: "Empty", service_names: [] } },
    { project: { name: "Legacy" } },
  ];
  inspect(ownerCatalogInsights(projects, "projects"), document => {
    assert.equal(metric(document, "Known linked services"), "2", "shared service links are deduplicated across loaded projects");
    assert.equal(metric(document, "Known unlinked projects"), "1", "missing link metadata is not an empty link list");
    assert.match(document.body.textContent, /3 of 4 project link lists loaded/);
    assert.match(document.body.textContent, /links, not request attribution/);
    assert.deepEqual(bars(document), [["One", "2"], ["Two", "1"], ["Empty", "0"], ["Legacy", "Unknown"]]);
  });
}

{
  const malicious = '<img src=x onerror="bad()">';
  const rows = [1, 4, 0, 10, 6, 8].map((value, index) => ({ name: index === 3 ? malicious : `group-${index}`, summary: { request_count: value, failure_count: 0 } }));
  const original = JSON.stringify(rows);
  inspect(ownerUsageDistribution("Endpoints", rows), document => {
    assert.deepEqual(bars(document).map(([, value]) => value), ["10", "8", "6", "4", "1"]);
    assert.equal(document.querySelector("img"), null, "group names are escaped");
    assert.equal(bars(document)[0][0], malicious);
    assert.match(document.body.textContent, /top 5 of 6 returned groups/);
  });
  assert.equal(JSON.stringify(rows), original, "rankings do not reorder data used by the existing tables");
  inspect(ownerUsageDistribution("Endpoints", [{ name: "Legacy", summary: { request_count: null } }]), document => {
    assert.deepEqual(bars(document), [["Legacy", "Unknown"]]);
    assert.equal(document.querySelector("progress"), null);
  });
  inspect(ownerRequestProfile([
    { diagnostics: { protocol: "http" } }, { diagnostics: { protocol: "http" } },
    { diagnostics: { protocol: "websocket" } }, { diagnostics: { websocket: { state: "closed" } } },
    {}, { diagnostics: {} },
  ], true), document => {
    assert.deepEqual(bars(document), [["HTTP / SSE", "2"], ["WebSocket", "2"], ["Not recorded", "2"]]);
    assert.match(document.body.textContent, /6 request rows · current log filters · more pages available · not live connections/);
  });
  assert.match(ownerRequestProfile([]), /No request rows loaded/);
}

{
  const rows = [
    { bucket: "2026-10-23T06:00:00Z", summary: { request_count: 10, failure_count: 2, p95_latency_ms: null } },
    { bucket: "2026-10-23T00:00:00Z", summary: { request_count: 0, failure_count: 0, p95_latency_ms: 0 } },
    { bucket: "2026-10-23T01:00:00Z", summary: { request_count: 4, failure_count: 0, p95_latency_ms: 0 } },
    { bucket: "invalid", summary: { request_count: 1, failure_count: 1, p95_latency_ms: 10 } },
  ];
  const markers = [
    { first_observed_at: "2026-10-23T06:30:00Z", service_version: "rollback" },
    { first_observed_at: "2026-10-22T23:59:59Z", service_version: "before-window" },
    { first_observed_at: "2026-10-23T05:00:00Z", service_version: "missing-bucket" },
    { first_observed_at: "2026-10-23T01:30:00Z", service_version: "new" },
    { first_observed_at: "2026-10-23T07:00:00Z", service_version: "after-bucket" },
  ];
  const original = JSON.stringify({ rows, markers });
  const series = ownerIncidentSeries(rows, markers, "hour");
  assert.deepEqual(series.points.map(point => point.errorRate), [null, 0, null, 20]);
  assert.deepEqual(series.points.map(point => point.p95), [null, 0, null, null], "empty buckets and absent latency remain unmeasured");
  assert.equal(series.points[2].at, Date.parse("2026-10-23T02:00:00Z"), "sparse data introduces a gap at real interval spacing");
  assert.deepEqual(series.observations.map(marker => marker.version), ["new", "rollback"], "markers retain first observation times and cannot be clamped into another bucket");
  assert.equal(series.observations[1].at, Date.parse("2026-10-23T06:30:00Z"));
  assert.equal(JSON.stringify({ rows, markers }), original);
  assert.match(ownerIncidentDescription(rows, markers), /Latest recorded bucket: 20\.0% errors; P95 latency unavailable/);
  assert.match(ownerIncidentDescription(rows, markers), /not deployment time/);
  assert.match(ownerIncidentDescription([rows[1]], []), /no requests; error rate unavailable/);
  assert.match(ownerIncidentDescription([], []), /No recorded usage buckets/);
}

console.log("ok - owner visuals preserve scopes, configuration coverage, loaded-page protocols, measured zeroes, unknowns, sparse incident series and escaped labels");
