import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const sourceUrl = source => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const read = path => readFileSync(new URL(path, import.meta.url), "utf8");
const monitoringUrl = sourceUrl(read("../crates/gateway-api/admin-ui/src/monitoring.ts"));
const governanceSource = read("../crates/gateway-api/admin-ui/src/governance-visuals.ts").replace('"./monitoring"', JSON.stringify(monitoringUrl));
const { keyGovernanceSnapshot, guardrailGovernanceSnapshot, memberAccessSnapshot, managedIdentitySnapshot, auditSampleSnapshot, settingsGovernanceSnapshot } = await import(sourceUrl(governanceSource));
const { insightStrip, distributionBars, flowSummary } = await import(sourceUrl(read("../crates/gateway-api/admin-ui/src/design-system/insights.ts")));
const value = (items, label) => items.find(item => item.label === label)?.value;

{
  const now = Date.parse("2026-10-23T00:00:00Z");
  const keys = [
    { expires_at: "2026-11-01T00:00:00Z", rotation_due_at: "2026-10-01T00:00:00Z", guardrail_policy: { mandatory_guardrails: ["redact"] } },
    { expires_at: null, guardrail_policy: { forbidden_guardrails: ["debug"] } },
    { expires_at: "2026-10-01T00:00:00Z", rotation_due_at: "2026-10-01T00:00:00Z" },
    { disabled: true, expires_at: "2026-10-01T00:00:00Z", rotation_due_at: "2026-10-01T00:00:00Z" },
    { revoked_at: "2026-10-01T00:00:00Z", disabled: true },
  ];
  const before = structuredClone(keys);
  const result = keyGovernanceSnapshot(keys, [{ kind: "global" }, { kind: "project" }, { kind: "project" }], now);
  assert.equal(value(result.insights, "Active lifecycle"), 2, "disabled, revoked and expired keys cannot inflate active lifecycle");
  assert.equal(value(result.insights, "Rotation due"), 1, "inactive keys do not inflate the active rotation workload");
  assert.equal(value(result.insights, "Explicit guardrail rules"), "2 / 5");
  assert.deepEqual(result.lifecycle.map(row => row.value), [1, 1, 1, 1, 1], "lifecycle buckets are exclusive and use shared precedence");
  assert.equal(value(result.layers, "Project"), 2);
  assert.equal(value(keyGovernanceSnapshot(keys.slice(0, 1), [], now).insights, "Active lifecycle"), 1, "a scoped response is summarized without consulting other keys");
  assert.deepEqual(keys, before);
  assert.equal(value(keyGovernanceSnapshot([], [], now).insights, "Explicit guardrail rules"), "0 / 0", "an empty inventory does not imply complete policy coverage");
}

{
  const catalog = [
    { enabled: true, default_on: true, modes: ["pre_call", "during_call"] },
    { enabled: true, default_on: false, modes: ["post_call"] },
    { enabled: false, default_on: true, modes: ["pre_call", "post_call", "during_call"] },
  ];
  const executions = [{ action: "allow" }, { action: "block" }, { action: "modify" }, { action: "allow" }];
  const result = guardrailGovernanceSnapshot(catalog, executions);
  assert.equal(value(result.insights, "Enabled safeguards"), "2 / 3");
  assert.equal(value(result.insights, "Enabled defaults"), 1);
  assert.equal(value(result.insights, "Streaming capable"), 1, "disabled definitions cannot count as enabled streaming capability");
  assert.deepEqual(result.stages.map(stage => stage.value), [1, 1, 1]);
  assert.deepEqual(result.actions.map(action => action.value), [2, 1, 0, 1]);
  assert.equal(result.actions.reduce((sum, action) => sum + action.value, 0), executions.length, "action distribution describes only returned execution samples");
}

{
  const result = memberAccessSnapshot([
    { member: { status: "active", roles: ["admin"] }, service_memberships: [{ service_name: "one" }] },
    { member: { status: "pending", roles: ["admin"] }, project_memberships: [{ project_id: "one" }] },
    { member: { status: "blocked", roles: ["admin"] }, service_memberships: [{ service_name: "two" }] },
  ]);
  assert.equal(value(result.insights, "Active administrators"), 1, "a pending or blocked admin role is not active administrator authority");
  assert.equal(value(result.insights, "Awaiting approval"), 1);
  assert.equal(value(result.insights, "Exact assignments"), 3);
  assert.match(result.insights.find(item => item.label === "Exact assignments").detail, /including inactive members/);
  assert.deepEqual(result.statuses.map(status => status.value), [1, 1, 1]);
}

{
  const result = managedIdentitySnapshot([
    { tenant_id: "tenant-a", client_id: "client", object_id: "object-a", enabled: true },
    { tenant_id: "tenant-a", client_id: "client", enabled: false },
    { tenant_id: "tenant-b", client_id: "client", enabled: true },
  ], [{ tenant_id: "tenant-a", client_id: "client", object_id: "object-a", enabled: true }]);
  assert.equal(value(result.insights, "Workload principals"), 2, "principals deduplicate across exact bindings while retaining tenant boundaries");
  assert.equal(value(result.insights, "Enabled bindings"), "3 / 4");
  assert.equal(value(result.insights, "Object-restricted"), 2);
  assert.deepEqual(result.resources.map(resource => resource.value), [2, 1, 1, 0]);
}

{
  const rows = [
    { action: "key:update", actor_member_id: "a", actor_token_id: "ignored", request_id: "request-one" },
    { action: "key:update", actor_member_id: "a" },
    { action: "key:create", actor_token_id: "a" },
    ...["provider:update", "service:update", "member:approve", "settings:update", '<img src=x onerror="bad()">'].map(action => ({ action })),
  ];
  const result = auditSampleSnapshot(rows);
  assert.equal(value(result.insights, "Returned events"), rows.length);
  assert.equal(value(result.insights, "Distinct actors"), 3, "member IDs and token IDs occupy distinct namespaces; system is one actor");
  assert.equal(value(result.insights, "Request correlation"), "1 / 8");
  assert.equal(result.actions.length, 6);
  assert.equal(value(result.actions, "Other actions"), 2);
  assert.equal(result.actions.reduce((sum, action) => sum + action.value, 0), rows.length, "ranking retains all remaining sampled actions in Other");
  const dom = new JSDOM(`<section>${insightStrip(result.insights)}${distributionBars("Actions", result.actions, { caption: "Returned sample only" })}</section>`);
  assert.equal(dom.window.document.querySelector("img"), null, "action labels remain escaped through the shared chart renderer");
  assert.ok(dom.window.document.querySelector("figure figcaption"));
  assert.ok([...dom.window.document.querySelectorAll("progress")].every(progress => progress.getAttribute("aria-label")));
  dom.window.close();
  assert.deepEqual(auditSampleSnapshot([]).actions, [], "no returned audit events do not create fabricated action observations");
}

{
  const studio = { base_url: "https://studio.invalid", source: "persisted", token_configured: true, token: "private-value-marker" };
  const auth = { source: "environment", unverified_bearer_enabled: true, entra: { enabled: true }, apigee: { trusted_header_enabled: false, secret_configured: false, secret: "private-value-marker" } };
  const result = settingsGovernanceSnapshot(studio, auth);
  assert.equal(value(result.insights, "Entra verification"), "Paused", "the troubleshooting bypass cannot be presented as verified identity");
  assert.equal(result.insights.find(item => item.label === "Entra verification").tone, "warn");
  assert.equal(value(result.insights, "Studio token"), "Configured");
  assert.doesNotMatch(JSON.stringify(result), /private-value-marker/, "configuration figures use presence flags without reading credential values");
  assert.doesNotMatch(insightStrip(result.insights), /private-value-marker/);
  const unknownDom = new JSDOM(`${insightStrip([{ label: "Unavailable", value: null, icon: "shield" }])}${flowSummary("Trust", [{ label: '<svg onload="bad()">', icon: "lock", value: 0 }])}`);
  assert.match(unknownDom.window.document.body.textContent, /—/);
  assert.equal(unknownDom.window.document.querySelector("svg"), null);
  assert.match(unknownDom.window.document.querySelector("ol").textContent, /0/, "a measured configuration count keeps its zero");
  unknownDom.window.close();
}

console.log("ok - governance figures preserve lifecycle, inactive access boundaries, exact bindings, sampled totals, escaping and write-only secrets");
