import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const visualSource = readFileSync(new URL("../crates/gateway-api/admin-ui/src/monitoring-visuals.ts", import.meta.url), "utf8");
const visualUrl = `data:text/javascript;base64,${Buffer.from(visualSource).toString("base64")}`;
const { appendGatewayObservation, capacityMeter, gatewayTrendConfig, protocolMix, usageRanking, usageSummaryView, usageTrendConfigs } = await import(visualUrl);
const gatewaySource = readFileSync(new URL("../crates/gateway-api/admin-ui/src/gateway-status.ts", import.meta.url), "utf8").replace('"./monitoring-visuals"', JSON.stringify(visualUrl));
const { gatewayLatencyConfig, mountHealthGatewayStatus, mountTrafficGatewayStatus } = await import(`data:text/javascript;base64,${Buffer.from(gatewaySource).toString("base64")}`);
const esc = value => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");

{
  const dom = new JSDOM(capacityMeter("Body slots", 0, 8, 8, String, esc));
  assert.equal(dom.window.document.querySelector("progress").value, 0, "a measured idle buffer remains a real zero");
  assert.match(dom.window.document.body.textContent, /8 free \/ 8/);
  assert.match(dom.window.document.body.textContent, /0% used/);
  for (const values of [[null, 8, 8], [0, null, 8], [0, 0, null], [0, 0, 0]]) {
    const html = capacityMeter("Body slots", ...values, String, esc);
    assert.doesNotMatch(html, /<progress/, "unknown capacity cannot be drawn as an empty measured buffer");
    assert.match(html, /Capacity unavailable/);
  }
  assert.doesNotMatch(capacityMeter('<img src=x onerror="bad()">', 1, 7, 8, String, esc), /<img/);
  const protocols = protocolMix({ http: 6, sse: 2, websocket: 2 }, esc);
  assert.match(protocols, /width:60%/);
  assert.match(protocols, /HTTP: 6, SSE: 2, WebSocket: 2/);
  assert.match(protocolMix(null, esc), /HTTP: —/);
  dom.window.close();
}

{
  const selection = rate => ({ arrivalsPerSecond: rate, runtime: { latency: { http_success_duration: { sample_count: 1, p95_ms: 25 } } } });
  let history = [];
  for (let index = 1; index <= 120; index++) history = appendGatewayObservation(history, selection(index === 120 ? 0 : index), index * 2000);
  assert.equal(history.length, 90, "long-open viewers use a bounded observation series");
  assert.equal(history.at(-1).rate, 0);
  assert.equal(history.at(-1).p95, 25);
  assert.equal(appendGatewayObservation(history, selection(500), 240000), history, "duplicate server observations do not create fabricated points");
  const gap = appendGatewayObservation(history, { ...selection(3), failed: true }, 260000);
  assert.equal(gap.at(-2).rate, null, "an observation gap does not become a continuous line");
  assert.equal(gap.at(-1).rate, null, "failed reads cannot become zero or fresh traffic");
  assert.equal(gap.at(-1).p95, null);
  assert.equal(appendGatewayObservation([], selection(9), 300000).length, 1, "new scopes can begin without carrying another scope’s history");
  const compact = gatewayTrendConfig([{ at: 100000, rate: 0.25 }, { at: 103000, rate: null }, { at: 120000, rate: 0 }], { compact: true });
  assert.equal(compact.options.scales.x.display, true, "compact trends retain visible time labels");
  assert.equal(compact.options.scales.y.display, true);
  assert.equal(compact.options.scales.y.title.text, "req/s");
  assert.equal(compact.options.scales.y.ticks.callback(0.25), "0.25", "fractional request rates stay readable");
  assert.equal(compact.options.plugins.tooltip.callbacks.label({ parsed: { y: 0 } }), "0 req/s");
  assert.deepEqual(compact.data.datasets[0].data, [{ x: 100000, y: 0.25 }, { x: 103000, y: null }, { x: 120000, y: 0 }], "time spacing, gaps and measured zero are preserved");
  assert.equal(compact.data.datasets[0].spanGaps, false);
  const single = gatewayTrendConfig([{ at: 100000, rate: 0 }], { compact: true });
  assert.equal(single.data.datasets[0].pointRadius, 3);
  assert.equal(single.options.scales.x.max - single.options.scales.x.min, 2000, "one observation gets a bounded time scale");
  for (const history of [[], [{ at: 100000, rate: null }]]) {
    const empty = gatewayTrendConfig(history, { compact: true });
    assert.equal(empty.options.scales.x.display, false);
    assert.equal(empty.options.scales.y.display, false);
  }
}

{
  const rows = [
    { bucket: "2026-10-23T01:00:00Z", summary: { request_count: 12, success_count: 10, failure_count: 2, estimated_cost_usd: null } },
    { bucket: "2026-10-23T00:00:00Z", summary: { request_count: 5, success_count: 5, failure_count: 0, estimated_cost_usd: 0 } },
  ];
  const [volume, cost] = usageTrendConfigs(rows);
  assert.deepEqual(volume.data.datasets[0].data.map(point => point.y), [5, 10]);
  assert.deepEqual(volume.data.datasets[1].data.map(point => point.y), [0, 2]);
  assert.deepEqual(cost.data.datasets[0].data.map(point => point.y), [0, null], "missing cost stays a missing point, independently of measured zero cost");
  const [gapped, gappedCost] = usageTrendConfigs([rows[1], { ...rows[0], bucket: "2026-10-23T06:00:00Z" }]);
  assert.deepEqual(gapped.data.datasets[0].data.map(point => point.y), [5, null, 10], "missing buckets interrupt the line");
  assert.equal(gapped.options.scales.x.type, "linear", "real timestamps preserve the spacing between sparse buckets");
  assert.deepEqual(gappedCost.data.datasets[0].data.map(point => point.x), [Date.parse(rows[1].bucket), Date.parse("2026-10-23T06:00:00Z")], "bar widths use real bucket spacing, without artificial line-gap markers");
  const summary = usageSummaryView({ request_count: 0, failure_count: 0, success_count: 0, estimated_cost_usd: null }, { esc, money: String });
  assert.doesNotMatch(summary, /100\.00%/, "no samples cannot appear as perfect success");
  const ranking = usageRanking([{ name: '<img src=x onerror="bad()">', summary: { request_count: 12 } }, { name: "small", summary: { request_count: 3 } }], value => value, esc);
  assert.doesNotMatch(ranking, /<img/);
  assert.match(ranking, /width:25%/);
  const latency = gatewayLatencyConfig({ latency: { http_success_duration: { sample_count: 1, p50_ms: 0, p95_ms: 25, p99_ms: null } } });
  assert.deepEqual(latency.data.datasets[0].data, [0, null, null, null]);
  assert.deepEqual(latency.data.datasets[2].data, [null, null, null, null], "overflow or no samples must not be drawn as zero latency");
}

{
  const dom = new JSDOM('<main id="content"></main>', { pretendToBeVisual: true });
  const content = dom.window.document.querySelector("main");
  const charts = [];
  class Chart {
    constructor(canvas, config) { this.canvas = canvas; this.config = config; charts.push(this); }
    update() { this.updated = true; }
    destroy() { this.destroyed = true; }
  }
  const envelope = () => ({ sampled_at: new Date().toISOString(), instances: [], aggregate: { schema_version: 1, window_seconds: 60, body_capacity: { used_requests: 0, free_requests: 8, max_requests: 8 }, counters: {}, active: {}, latency: {} } });
  const dispose = mountHealthGatewayStatus({ content, Chart, refreshMs: 0, api: async () => envelope(), esc, badge: esc, table: () => "" });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(charts.filter(chart => !chart.destroyed).length, 2);
  const dashboard = content.querySelector(".monitor-dashboard-grid");
  const chartContainer = charts[0].canvas.parentElement;
  const details = content.querySelector('[data-gateway-detail="latency"]');
  details.open = true;
  details.querySelector("summary").focus();
  content.querySelector("[data-gateway-refresh]").click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(content.querySelector('[data-gateway-detail="latency"]').open, true, "auto-refresh retains expanded diagnostics");
  assert.equal(dom.window.document.activeElement.parentElement.dataset.gatewayDetail, "latency", "refresh preserves keyboard focus");
  assert.equal(charts.filter(chart => !chart.destroyed).length, 2, "refresh releases replaced charts");
  assert.equal(charts.length, 2, "refresh reuses both canvases for uninterrupted hover analysis");
  assert.equal(content.querySelector(".monitor-dashboard-grid"), dashboard, "refresh never detaches chart ancestors or resize observers");
  assert.equal(charts[0].canvas.parentElement, chartContainer);
  assert.ok(charts.every(chart => chart.updated));
  dispose();
  assert.equal(charts.filter(chart => !chart.destroyed).length, 0, "navigation releases every chart");
  dom.window.close();
}
{
  const dom = new JSDOM('<main></main>', { pretendToBeVisual: true });
  const content = dom.window.document.querySelector('main');
  const charts = [];
  class Chart {
    constructor(canvas, config) { this.canvas = canvas; this.data = config.data; charts.push(this); }
    update() {}
    destroy() {}
  }
  let reads = 0;
  const at = Date.now();
  const dispose = mountTrafficGatewayStatus({ content, Chart, refreshMs: 0, esc, badge: esc, table: () => '', api: async () => {
    const sampled_at = new Date(at + reads).toISOString();
    const runtime = { schema_version: 1, window_seconds: 60, observed_seconds: 60, counters: { started: reads++ ? 15 : 0 } };
    return { sampled_at, instances: [{ instance_id: 'trend-fixture', sampled_at, runtime }], aggregate: runtime };
  } });
  await new Promise(resolve => setTimeout(resolve, 0));
  const figure = content.querySelector('.monitor-mini-trend');
  const canvas = figure.querySelector('canvas');
  assert.equal(content.querySelector('[data-gateway-part="trend-value"]').textContent, 'Last observed 0 req/s');
  content.querySelector('[data-gateway-refresh]').click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(content.querySelector('[data-gateway-part="trend-value"]').textContent, 'Last observed 0.25 req/s');
  assert.equal(content.querySelector('.monitor-mini-trend'), figure, 'polling retains the chart figure');
  assert.equal(content.querySelector('.monitor-mini-trend canvas'), canvas, 'polling retains the chart canvas');
  assert.equal(charts.length, 1);
  assert.deepEqual(charts[0].data.datasets[0].data.map(point => point.y), [0, 0.25]);
  assert.match(canvas.getAttribute('aria-label'), /0\.25 req\/s/);
  dispose();
  dom.window.close();
}
console.log("ok - monitoring visuals preserve measured zeros, unknowns, labeled compact trends, live updates, gaps, scope, escaping, chart teardown and keyboard state");
