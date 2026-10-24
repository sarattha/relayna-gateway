import assert from "node:assert/strict";
import { request } from "node:http";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { before, after, test } from "node:test";
import { createDesignDataset, createDesignPreviewServer } from "./fixtures/admin-ui-design-preview.mjs";

const now = Date.now();
const data = createDesignDataset(now);
const server = createDesignPreviewServer({ now });
let origin;
before(async () => {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
});
after(async () => {
  server.closeAllConnections();
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
});
const get = (path, mode = "both", options = {}) => fetch(`${origin}${path}`, { headers: { cookie: `design_preview_mode=${mode}` }, ...options });
async function json(path, mode = "both") {
  const response = await get(path, mode);
  assert.equal(response.status, 200, path);
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.match(response.headers.get("x-design-preview"), /synthetic/);
  return response.json();
}
const admin = (path) => json(`/admin-ui/admin/${path}`);

test("deterministic synthetic data covers people, transport, credentials and lifecycles", () => {
  assert.deepEqual(createDesignDataset(now), data);
  assert.deepEqual(new Set(data.members.map((row) => row.member.status)), new Set(["active", "pending", "blocked"]));
  assert.deepEqual(new Set(data.providers.map((row) => row.provider)), new Set(["litellm", "internal-service", "azure-foundry"]));
  assert.ok(data.providers.some((row) => row.credential_configured === true));
  assert.ok(data.providers.some((row) => row.credential_configured === false));
  assert.ok(data.providers.some((row) => row.credential_configured === undefined));
  assert.ok(data.services.some((row) => row.access.websocket));
  assert.ok(data.services.some((row) => row.access.accessa?.app && row.allowed_methods.includes("GET")));
  assert.ok(data.services.some((row) => !row.access.websocket && !row.access.accessa));
  assert.ok(data.keys.some((row) => row.disabled));
  assert.ok(data.keys.some((row) => row.revoked_at));
  assert.ok(data.keys.some((row) => row.expires_at && Date.parse(row.expires_at) < now));
  assert.ok(data.keys.some((row) => row.expires_at === null));
  assert.ok(data.projects[0].service_names.includes("invoice-parser") && data.projects[1].service_names.includes("invoice-parser"));
  assert.equal(data.events.length, 420);
  assert.deepEqual(new Set(data.events.map((row) => row.diagnostics.routing_mode)), new Set(["managed_by_gateway", "litellm_passthrough"]));
  assert.ok(data.events.filter((row) => row.diagnostics.routing_mode === "litellm_passthrough").every((row) => row.estimated_cost_usd === null && row.total_tokens === 0));
  const prohibited = new Set(["credential", "raw_key", "raw_token", "token", "bearer_token", "client_secret", "prompt", "messages"]);
  const walk = (value) => {
    if (!value || typeof value !== "object") return;
    for (const [key, item] of Object.entries(value)) {
      assert.ok(!prohibited.has(key), `Unexpected sensitive DTO field ${key}`);
      walk(item);
    }
  };
  walk(data);
});

test("checked-in assets, synthetic disclosure and help work without starting CLI server", async () => {
  const help = spawnSync(process.execPath, ["tests/fixtures/admin-ui-design-preview.mjs", "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0);
  assert.match(help.stdout, /18464/);
  assert.match(help.stdout, /NOT health\/performance evidence/);
  const result = spawnSync(process.execPath, ["tests/fixtures/admin-ui-design-preview.mjs", "--port", "0"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  const html = await (await get("/admin-ui")).text();
  assert.match(html, /Design preview · synthetic data only/);
  const currentJs = await readFile(new URL("../crates/gateway-api/src/static/admin-ui/app.js", import.meta.url), "utf8");
  assert.equal(await (await get("/admin-ui/app.js")).text(), currentJs);
  const controls = await json("/fixture");
  assert.equal(controls.captured_at, new Date(now).toISOString());
  assert.ok(controls.limitations.some((line) => line.includes("not complete history")));
});

test("all mutations are 405, non-loopback Hosts fail, and traversal is unavailable", async () => {
  for (const method of ["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]) {
    for (const path of ["/admin-ui/admin/keys", "/fixture/owner", "/admin-ui/auth/logout"]) {
      const response = await get(path, "both", { method });
      assert.equal(response.status, 405, `${method} ${path}`);
      assert.equal(response.headers.get("allow"), "GET");
      assert.equal(response.headers.get("set-cookie"), null);
    }
  }
  for (const host of ["example.test", "127.0.0.1.example.test", "localhost.evil", "127.0.0.1@evil.test", "0.0.0.0"]) {
    const status = await new Promise((resolve, reject) => {
      const req = request(origin + "/admin-ui/auth/session", { headers: { host } }, (response) => { response.resume(); resolve(response.statusCode); });
      req.on("error", reject); req.end();
    });
    assert.equal(status, 403, host);
  }
  assert.equal((await get("/admin-ui/%2e%2e%2fpackage.json")).status, 404);
  assert.equal((await get("/admin-ui/admin/not-a-contract")).status, 404);
});

test("session controls distinguish both, owner, admin, login, pending and blocked", async () => {
  const both = await json("/admin-ui/auth/session");
  assert.equal(both.member.display_name, "Design preview");
  assert.deepEqual(both.member.roles, ["admin"]);
  assert.deepEqual(both.service_memberships, data.serviceMemberships);
  assert.deepEqual(both.project_memberships, data.projectMemberships);
  const owner = await json("/admin-ui/auth/session", "owner");
  assert.deepEqual(owner.member.roles, []);
  assert.equal((await get("/admin-ui/admin/projects", "owner")).status, 403);
  assert.equal((await json("/owner/v1/services", "admin")).length, 0);
  assert.equal((await json("/admin-ui/auth/session", "login")).authenticated, false);
  assert.equal((await get("/owner/v1/services", "login")).status, 401);
  for (const mode of ["pending", "blocked"]) {
    assert.equal((await json("/admin-ui/auth/session", mode)).member.status, mode);
    assert.equal((await get("/owner/v1/services", mode)).status, 403);
  }
  for (const mode of ["both", "owner", "admin", "login", "pending", "blocked", "active"]) {
    const response = await get(`/fixture/${mode}`, "both", { redirect: "manual" });
    assert.equal(response.status, 302);
    assert.match(response.headers.get("set-cookie"), /HttpOnly; SameSite=Strict/);
    assert.match(response.headers.get("location"), /^\/admin-ui#/);
  }
});

test("every admin view gets its expected array or wrapped DTO contract", async () => {
  const arrays = ["projects", "services", "keys", "providers", "providers/litellm-credentials", "openai-routes", "anthropic-routes", "route-identities", "policy-layers", "members", "managed-identities", "managed-identity-projects", "audit-events", "provider-health", "provider-health/state", "services/import/versions", "studio/services", "traffic/history", "usage/timeseries", "usage/by-project"];
  for (const path of arrays) assert.ok(Array.isArray(await admin(path)), path);
  for (const [path, field] of [["guardrails", "guardrails"], ["guardrails/executions", "executions"], ["guardrails/summary", "summary"], ["traffic/cluster", "rows"], ["usage/events", "rows"]]) assert.ok(Array.isArray((await admin(path))[field]), path);
  for (const path of ["providers/litellm-passthrough", "studio/connection", "auth/front-door", "gateway/status", "usage/summary"]) assert.equal(typeof await admin(path), "object");
  for (const section of ["projects", "keys", "services", "project-key-services", "endpoints", "providers", "models", "tasks", "timeseries", "service-timeseries", "unused-keys"]) {
    const value = await admin(`usage/sections/${section}`);
    assert.ok(Array.isArray(value.rows), section);
    if (section.endsWith("timeseries")) assert.equal(typeof value.page.has_more, "boolean");
  }
  assert.ok((await admin("usage/filter-values?field=route")).values.length);
  assert.equal((await admin("auth/front-door")).source, "persisted");
  assert.equal((await admin("studio/connection")).source, "persisted");
  assert.equal((await admin("services/invoice-parser/sync-status")).sync_status, "synced");
  assert.equal((await admin(`keys/${data.keys[0].id}/litellm-spend`)).spend_usd, null);
  assert.equal((await admin("gateway/status")).aggregate.process.rss_bytes, null);
});

test("usage filters, cursor pagination, exports and chart summaries share one finite dataset", async () => {
  const from = new Date(now - 7 * 86400000).toISOString();
  const query = new URLSearchParams({ project_id: data.projects[0].id, from, interval: "hour", timeseries_limit: "500" });
  const summary = await admin(`usage/summary?${query}`);
  const chart = await admin(`usage/sections/timeseries?${query}`);
  assert.equal(chart.rows.reduce((sum, row) => sum + row.summary.request_count, 0), summary.request_count);
  assert.ok(summary.request_count > 20);
  assert.ok(chart.rows.length > 5);
  const first = await admin(`usage/events?${query}&limit=7&cursor=start`);
  const second = await admin(`usage/events?${query}&limit=7&cursor=${first.next_cursor}`);
  assert.equal(first.rows.length, 7); assert.equal(second.offset, 7);
  assert.ok(second.rows.every((row) => !first.rows.some((other) => other.request_id === row.request_id)));
  assert.ok(first.rows.every((row) => row.project_id === data.projects[0].id));
  const failures = await admin(`usage/events?${query}&status=failure`);
  assert.ok(failures.rows.length && failures.rows.every((row) => row.status === "failure"));
  const exported = await admin(`usage/export.json?${query}&stream=true`);
  assert.equal(exported.rows.length, summary.request_count);
  assert.equal(exported.summary.request_count, summary.request_count);
  const csv = await get(`/admin-ui/admin/usage/export.csv?${query}&stream=true`);
  assert.match(csv.headers.get("content-type"), /text\/csv/);
  assert.equal((await csv.text()).split("\r\n").length, summary.request_count + 1);
  assert.ok((await admin(`tasks/design-task-1/usage?${query}`)).request_count < summary.request_count);
});

test("owner inventories, charts, markers and details enforce exact memberships", async () => {
  const services = await json("/owner/v1/services", "owner");
  const projects = await json("/owner/v1/projects", "owner");
  assert.deepEqual(services.map((row) => row.service.name), data.serviceMemberships.map((row) => row.service_name));
  assert.deepEqual(projects.map((row) => row.project.id), data.projectMemberships.map((row) => row.project_id));
  for (const path of [`services/${services[0].service.name}`, `projects/${projects[0].project.id}`]) {
    const dashboard = await json(`/owner/v1/${path}/dashboard`, "owner");
    assert.ok(dashboard.summary.request_count > 20);
    assert.ok(dashboard.timeseries.length > 5);
    assert.equal(dashboard.version_markers.length, 2);
    const events = await json(`/owner/v1/${path}/events?limit=3`, "owner");
    assert.equal(events.rows.length, 3); assert.equal(events.has_more, true);
    const details = await json(`/owner/v1/${path}/requests/${events.rows[0].request_id}`, "owner");
    assert.deepEqual(details.request, events.rows[0]); assert.equal(details.debug_bundle, null);
  }
  // Foundry is linked to an assigned project, but has no exact service membership.
  assert.equal((await get("/owner/v1/services/foundry-assistant/dashboard", "owner")).status, 404);
  const foreign = data.events.find((row) => row.service_name === "foundry-assistant");
  const denied = await get(`/owner/v1/services/invoice-parser/requests/${foreign.request_id}`, "owner");
  const absent = await get("/owner/v1/services/invoice-parser/requests/absent", "owner");
  assert.equal(denied.status, 404); assert.equal(absent.status, 404);
  assert.deepEqual(await denied.json(), await absent.json());
  const narrowed = await json(`/owner/v1/projects/${projects[0].project.id}/events?project_id=${projects[1].project.id}`, "owner");
  assert.deepEqual(narrowed.rows, []);
});

test("Traffic exposes finite SSE and correlated saved metadata without live claims", async () => {
  const response = await get("/admin-ui/admin/traffic/live");
  assert.match(response.headers.get("content-type"), /text\/event-stream/);
  assert.match(response.headers.get("x-design-preview-history"), /finite-snapshot/);
  const frame = await response.text();
  const batch = JSON.parse(frame.split("\n").find((line) => line.startsWith("data: ")).slice(6));
  assert.ok(batch.rows.some((row) => !row.completed));
  assert.ok(batch.rows.some((row) => row.completed));
  const event = data.events[0];
  const matching = await admin(`traffic/history?id=${event.diagnostics.traffic_id}`);
  assert.equal(matching.length, 1);
  assert.equal(matching[0].request_id, event.request_id);
  const ws = await admin("traffic/history?protocol=websocket&limit=3");
  assert.ok(ws.length && ws.every((row) => row.diagnostics.protocol === "websocket"));
});
