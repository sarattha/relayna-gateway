import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import "./admin-ui-monitoring-visuals.test.mjs";

let source = readFileSync(new URL("../crates/gateway-api/admin-ui/src/gateway-status.ts", import.meta.url), "utf8");
const visualsSource = readFileSync(new URL("../crates/gateway-api/admin-ui/src/monitoring-visuals.ts", import.meta.url), "utf8");
const visualsUrl = `data:text/javascript;base64,${Buffer.from(visualsSource).toString("base64")}`;
source = source.replace('"./monitoring-visuals"', JSON.stringify(visualsUrl));
const { gatewayStatusPath, selectGatewayStatus, gatewayQuantile, gatewayStatusView, gatewayCpuUtilization, mountHealthGatewayStatus, mountTrafficGatewayStatus } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const now = Date.now();
const sampled_at = new Date(now).toISOString();
const cpuSample = (usage, timestamp) => ({ instances: [{ instance_id: "cpu-fixture", sampled_at, runtime: { sampled_at_unix_ms: timestamp, process: { cgroup_cpu_usage_usec: usage, cgroup_cpu_limit_cores: 2 } } }] });
assert.equal(gatewayCpuUtilization(cpuSample(1_500_000, now), cpuSample(1_000_000, now - 1000)), 25);
assert.equal(gatewayCpuUtilization(cpuSample(1_500_000, now), null), null);
assert.equal(gatewayCpuUtilization(cpuSample(500_000, now), cpuSample(1_000_000, now - 1000)), null);
assert.equal(gatewayCpuUtilization(cpuSample(1_500_000, now), cpuSample(1_000_000, now)), null);
assert.equal(gatewayCpuUtilization(cpuSample(null, now), cpuSample(null, now - 1000)), null);
assert.equal(gatewayCpuUtilization(cpuSample(3_600_000_000, now), cpuSample(null, now - 1000)), null);
assert.equal(gatewayCpuUtilization(cpuSample(null, now), cpuSample(1_000_000, now - 1000)), null);
assert.equal(gatewayCpuUtilization(cpuSample(0, now), cpuSample(0, now - 1000)), 0);
const histogram = { sample_count: 20, p50_ms: 1, p95_ms: 25, p99_ms: 50 };
const runtime = {
  schema_version: 1, window_seconds: 60, observed_seconds: 60, instance_count: 1,
  uptime_seconds: 500, incompatible_instances: 0,
  counters: { completed: 0 }, active: { http: 0, sse: 2, websocket: 1 },
  body_capacity: { max_requests: 8, used_requests: 3, free_requests: 5, max_bytes: 100, used_bytes: 25, free_bytes: 75 },
  latency: { http_success_duration: histogram },
  admission_denials: { concurrency: 0, bytes: 2, other: 0 },
  process: { rss_bytes: 0, cgroup_memory_current_bytes: null, restarts: null },
};
const envelope = { scope: "cluster", sampled_at, partial: false, unavailable: false, instances: [{ instance_id: "one", sampled_at, stale: false, runtime }], aggregate: runtime };
const restartedFleet = { ...envelope, instances: [
  { instance_id: "old", sampled_at, runtime: { ...runtime, counters: { started: 6000 }, observed_seconds: 60 } },
  { instance_id: "new", sampled_at, runtime: { ...runtime, counters: { started: 100 }, observed_seconds: 1 } },
] };
assert.equal(selectGatewayStatus(restartedFleet, { now }).arrivalsPerSecond, 200, "fleet rate sums per-instance rates across unequal uptimes");
assert.equal(selectGatewayStatus(restartedFleet, { now, instanceId: "new" }).arrivalsPerSecond, 100);
assert.equal(selectGatewayStatus(restartedFleet, { now: now + 6000 }).arrivalsPerSecond, null);
const esc = (value) => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const helpers = { esc, attr: esc, badge: (value, tone) => `<span class="badge ${esc(tone)}">${esc(value)}</span>`, table: (headers, rows) => `<table><thead><tr>${headers.map((value) => `<th>${esc(value)}</th>`).join("")}</tr></thead><tbody>${rows.map((row) => `<tr>${row.map((value) => `<td>${value}</td>`).join("")}</tr>`).join("")}</tbody></table>` };

assert.equal(gatewayStatusPath("one&window_seconds=900", 300), "/admin-ui/admin/gateway/status?instance_id=one%26window_seconds%3D900&window_seconds=300");
assert.ok(gatewayStatusPath("all", 123).endsWith("window_seconds=60"));
assert.equal(gatewayQuantile(undefined, 95), "Unknown");
assert.equal(gatewayQuantile({ sample_count: 0, p95_ms: null }, 95), "No samples");
assert.equal(gatewayQuantile({ sample_count: 1, p95_ms: null }, 95), "Above histogram range");
assert.equal(gatewayQuantile({ sample_count: 1, p95_ms: 0 }, 95), "≤ 0 ms");
assert.equal(selectGatewayStatus(envelope, { now }).label, "Current snapshot");
assert.equal(selectGatewayStatus(envelope, { now: now + 5_000 }).label, "Current snapshot");
assert.equal(selectGatewayStatus(envelope, { now: now - 5_000 }).label, "Current snapshot");
assert.equal(selectGatewayStatus(envelope, { now: now + 6_000 }).label, "Stale snapshot");
assert.equal(selectGatewayStatus(envelope, { now: now - 6_000 }).label, "Stale snapshot");
assert.equal(selectGatewayStatus({ ...envelope, partial: true }, { now }).label, "Partial snapshot");
assert.equal(selectGatewayStatus({ ...envelope, unavailable: true }, { now }).label, "Unavailable");
assert.equal(selectGatewayStatus(envelope, { now, failed: true }).label, "Refresh failed");
assert.equal(selectGatewayStatus(envelope, { now, windowSeconds: 900 }).runtime, null);
assert.equal(selectGatewayStatus(envelope, { now, instanceId: "missing" }).runtime, null);
assert.equal(selectGatewayStatus({ ...envelope, aggregate: { ...runtime, incompatible_instances: 1 } }, { now }).label, "Partial snapshot");
assert.equal(selectGatewayStatus({ ...envelope, aggregate: { ...runtime, schema_version: 2 } }, { now }).label, "Unknown");
const localFallback = selectGatewayStatus({ ...envelope, unavailable: true }, { now });
assert.equal(localFallback.label, "Unavailable");
assert.equal(localFallback.runtime, runtime);
assert.ok(localFallback.message.includes("Cluster collection is unavailable"));
assert.ok(localFallback.message.includes("Only reporting instances are included"));
assert.ok(!localFallback.message.includes("Gateway status is unavailable"));
assert.ok(selectGatewayStatus({ ...envelope, unavailable: true, instances: [], aggregate: null }, { now }).message.includes("Gateway status is unavailable"));

const full = gatewayStatusView(selectGatewayStatus(envelope, { now }), helpers);
assert.ok(full.includes("5 free / 8"));
assert.ok(full.includes("75 B free / 100 B"));
assert.ok(full.includes("SSE<strong>2"));
assert.ok(full.includes("WebSocket<strong>1"));
assert.ok(full.includes("0 B"), "measured zero remains visible");
assert.ok(full.includes("Unknown"), "missing diagnostics stay unknown");
assert.ok(full.includes("WebSocket · session only"));
assert.ok(full.includes("No admission queue is used"));
assert.ok(gatewayStatusView(localFallback, helpers).includes("5 free / 8"), "local runtime measurements stay visible when cluster collection is unavailable");
assert.ok(!gatewayStatusView(selectGatewayStatus(envelope, { now }), helpers, { compact: true }).includes("Latency distribution"));
const unknown = gatewayStatusView(selectGatewayStatus(null, { now }), helpers);
assert.ok(unknown.includes("Waiting for a gateway runtime snapshot"));
assert.ok(!unknown.includes("0 free /"), "unknown is not represented as zero capacity");
const malicious = gatewayStatusView({ runtime: null, label: '<img src=x onerror="bad()">', tone: "neutral", message: "<script>bad()</script>" }, helpers);
assert.ok(!malicious.includes("<img"));
assert.ok(!malicious.includes("<script"));

class Control {
  innerHTML = "";
  value = "all";
  disabled = false;
  listeners = new Map();
  attrs = new Map();
  addEventListener(type, listener) { this.listeners.set(type, listener); }
  removeEventListener(type) { this.listeners.delete(type); }
  setAttribute(name, value) { this.attrs.set(name, value); }
  change(value) { this.value = value; this.listeners.get("change")?.(); }
}
function host() {
  const controls = new Map(["window", "instance", "refresh", "status"].map((key) => [key, new Control()]));
  return { innerHTML: "", controls, querySelector(selector) { return controls.get(selector.match(/data-gateway-(\w+)/)[1]); } };
}
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const content = host();
let fail = false;
const paths = [];
const dispose = mountHealthGatewayStatus({ content, ...helpers, refreshMs: 0, api: async (path) => { paths.push(path); if (fail) throw new Error("secret must never appear"); return envelope; } });
await tick();
assert.equal(paths.length, 1);
assert.ok(content.controls.get("status").innerHTML.includes("Latency distribution"));
fail = true;
content.controls.get("refresh").listeners.get("click")();
await tick();
assert.ok(content.controls.get("status").innerHTML.includes("Refresh failed"));
assert.ok(content.controls.get("status").innerHTML.includes("5 free / 8"), "last received capacity is retained with explicit failure");
assert.ok(!content.controls.get("status").innerHTML.includes("secret must never appear"));
content.controls.get("window").change("900");
await tick();
assert.ok(paths.at(-1).endsWith("window_seconds=900"));
dispose();
assert.equal(content.controls.get("window").listeners.size, 0);

const deferred = [];
const compactContent = host();
const compactDispose = mountTrafficGatewayStatus({ content: compactContent, ...helpers, refreshMs: 0, api: (path, options) => new Promise((resolve) => deferred.push({ path, options, resolve })) });
compactContent.controls.get("instance").change("one");
assert.equal(deferred[0].options.signal.aborted, true, "new selection aborts obsolete request");
assert.ok(deferred[1].path.includes("instance_id=one"));
deferred[1].resolve(envelope);
await tick();
assert.ok(compactContent.controls.get("status").innerHTML.includes("3 / 8"));
assert.ok(!compactContent.controls.get("status").innerHTML.includes("Latency distribution"));
deferred[0].resolve({ ...envelope, aggregate: { ...runtime, body_capacity: null } });
await tick();
assert.ok(compactContent.controls.get("status").innerHTML.includes("3 / 8"), "late responses cannot overwrite the new selection");
compactContent.controls.get("refresh").listeners.get("click")();
compactDispose();
assert.equal(deferred.at(-1).options.signal.aborted, true);
const afterDispose = compactContent.controls.get("status").innerHTML;
deferred.at(-1).resolve(envelope);
await tick();
assert.equal(compactContent.controls.get("status").innerHTML, afterDispose);

const originalSetInterval = globalThis.setInterval;
const originalClearInterval = globalThis.clearInterval;
const timer = {};
let defaultRefreshMs, clearedTimer;
globalThis.setInterval = (_callback, milliseconds) => { defaultRefreshMs = milliseconds; return timer; };
globalThis.clearInterval = (value) => { clearedTimer = value; };
try {
  const defaultDispose = mountHealthGatewayStatus({ content: host(), ...helpers, api: async () => envelope });
  assert.equal(defaultRefreshMs, 2_000, "default refresh stays within the server's five-second freshness window");
  defaultDispose();
  assert.equal(clearedTimer, timer);
  await tick();
} finally {
  globalThis.setInterval = originalSetInterval;
  globalThis.clearInterval = originalClearInterval;
}
console.log("ok - gateway snapshot windows, stale/partial/unknown states, quantile samples, escaping, cancellation and teardown");
