import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import Chart from "chart.js/auto";
import { BasicPlatform } from "chart.js";
import { JSDOM } from "jsdom";

const source = file => readFileSync(new URL(`../crates/gateway-api/admin-ui/src/${file}`, import.meta.url), "utf8");
const moduleUrl = text => `data:text/javascript;base64,${Buffer.from(text).toString("base64")}`;
const visualsUrl = moduleUrl(source("monitoring-visuals.ts"));
// Pure config helpers must stay importable without installing browser globals.
assert.equal(globalThis.document, undefined);
const { chartColor, installChartTheme, measured, mountMonitorChart, usageTrendConfigs } = await import(visualsUrl);
const { mountHealthGatewayStatus, mountTrafficGatewayStatus } = await import(moduleUrl(source("gateway-status.ts").replace('"./monitoring-visuals"', JSON.stringify(visualsUrl))));
assert.equal(chartColor("#177264", "teal")({}), "#177264");
assert.doesNotThrow(() => installChartTheme(null)());

const dom = new JSDOM(`<main>
  <input aria-label="Draft" value="unsaved draft"><select><option value="300" selected>Five minutes</option></select>
  <canvas id="overview-chart"></canvas><canvas id="owner-incident-chart"></canvas>
  <canvas id="volume"></canvas><canvas id="cost"></canvas><section id="health"></section><section id="traffic"></section>
</main>`, { pretendToBeVisual: true });
const { document, HTMLCanvasElement } = dom.window;
const contexts = new WeakMap();
HTMLCanvasElement.prototype.getContext = function () {
  if (!contexts.has(this)) {
    // Exercise real Chart.js option resolution and lifecycle; rasterization is
    // the only browser surface replaced by this headless drawing context.
    const context = new Proxy({
      canvas: this, strokeColors: [],
      measureText: text => ({ width: String(text).length * 6 }),
      getLineDash: () => [],
      stroke() { this.strokeColors.push(this.strokeStyle); },
    }, { get: (target, key) => key in target ? target[key] : () => {} });
    contexts.set(this, context);
    this.width = 800;
    this.height = 360;
  }
  return contexts.get(this);
};
class HeadlessChart extends Chart {
  constructor(canvas, config) { super(canvas, { ...config, platform: BasicPlatform }); }
}

function sourceFunction(text, name) {
  const start = text.indexOf(`function ${name}(`);
  assert.notEqual(start, -1);
  return text.slice(start, text.indexOf("\n}", start) + 2).replace(" as any", "");
}
const ownerIncidentSeries = new Function("measured", "count", `${sourceFunction(source("owner-visuals.ts"), "ownerIncidentSeries")}; return ownerIncidentSeries;`)(measured, value => measured(value) ? value : null);
const main = source("main.ts");
assert.match(main, /installChartTheme\(Chart\);/);
const pageCharts = new Function("Chart", "chartColor", "ownerIncidentSeries", "document", "HTMLCanvasElement", `
  const state = { overviewWindow: "24h", ownerDashboardFilters: { range: "24h" } };
  let overviewChart = null, ownerDashboardChart = null;
  ${["renderOverviewChart", "destroyOverviewChart", "renderOwnerIncidentChart", "destroyOwnerDashboardChart", "ownerChartLabel"].map(name => sourceFunction(main, name)).join("\n")}
  return {
    overview(rows) { renderOverviewChart(rows); return overviewChart; },
    owner(rows, markers) { renderOwnerIncidentChart(rows, markers); return ownerDashboardChart; },
    dispose() { destroyOverviewChart(); destroyOwnerDashboardChart(); },
  };
`)(HeadlessChart, chartColor, ownerIncidentSeries, document, HTMLCanvasElement);

let listenerAdds = 0, listenerRemoves = 0;
const addEventListener = document.addEventListener.bind(document);
const removeEventListener = document.removeEventListener.bind(document);
document.addEventListener = (type, ...args) => { if (type === "relayna:theme-change") listenerAdds++; return addEventListener(type, ...args); };
document.removeEventListener = (type, ...args) => { if (type === "relayna:theme-change") listenerRemoves++; return removeEventListener(type, ...args); };
const disposeTheme = installChartTheme(HeadlessChart, document);
assert.equal(installChartTheme(HeadlessChart, document), disposeTheme);
assert.equal(listenerAdds, 1, "repeated installation adds no duplicate listeners");
const toggle = theme => {
  document.documentElement.dataset.theme = theme;
  document.dispatchEvent(new dom.window.CustomEvent("relayna:theme-change", { detail: { theme, preference: theme } }));
};
const rows = [0, 6].map((hour, index) => ({ bucket: `2026-10-23T0${hour}:00:00Z`, summary: {
  request_count: 10, success_count: 9, failure_count: index, estimated_cost_usd: index ? null : 0, p95_latency_ms: index ? null : 25,
} }));
const overview = pageCharts.overview(rows);
const owner = pageCharts.owner(rows, [{ first_observed_at: rows[0].bucket, service_version: "v1" }]);
const usage = usageTrendConfigs(rows).map((config, index) => mountMonitorChart(HeadlessChart, document.querySelector(index ? "#cost" : "#volume"), config));
let apiReads = 0;
const esc = value => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll('"', "&quot;");
const mountStatus = (mount, id) => mount({
  content: document.querySelector(id), Chart: HeadlessChart, refreshMs: 0, esc, badge: esc, table: () => "",
  api: async () => {
    apiReads++;
    return { sampled_at: new Date().toISOString(), instances: [], aggregate: { schema_version: 1, window_seconds: 60,
      counters: { started: 12 }, observed_seconds: 60, active: {}, body_capacity: {},
      latency: { http_success_duration: { sample_count: 5, p50_ms: 0, p95_ms: 25, p99_ms: null } },
    } };
  },
});
const disposeHealth = mountStatus(mountHealthGatewayStatus, "#health");
const disposeTraffic = mountStatus(mountTrafficGatewayStatus, "#traffic");
await new Promise(resolve => setTimeout(resolve, 0));
const charts = Object.values(Chart.instances);
assert.equal(charts.length, 7, "Overview, owner, both Usage, both Health and Traffic charts are live");
const latency = charts.find(chart => chart.canvas.dataset.gatewayChart === "latency");
assert.deepEqual(latency.data.datasets.map(dataset => dataset.label), ["p50", "p95", "p99"]);
assert.deepEqual(latency.data.datasets[0].data, [0, null, null, null]);
assert.deepEqual(latency.data.datasets[2].data, [null, null, null, null]);
assert.deepEqual(usage[0].data.datasets[0].data.map(point => point.y), [9, null, 9]);
assert.deepEqual(owner.data.datasets[1].data.map(point => point.y), [25, null, null]);
const details = document.querySelector('[data-gateway-detail="latency"]');
details.open = true;
document.querySelector("input").focus();
overview.canvas.style.cursor = "crosshair";
overview.setDatasetVisibility(1, false);
overview.setActiveElements([{ datasetIndex: 0, index: 0 }]);
overview.tooltip.setActiveElements([{ datasetIndex: 0, index: 0 }], { x: 100, y: 100 });
overview.update("none");
const active = overview.getActiveElements().map(({ datasetIndex, index }) => ({ datasetIndex, index }));
const data = charts.map(chart => ({ datasets: chart.data.datasets, values: JSON.stringify(chart.data), options: chart.config.options }));
const light = charts.map(chart => ({
  line: chart.getDatasetMeta(0).data[0].options.borderColor,
  background: chart.getDatasetMeta(0).data[0].options.backgroundColor,
  legend: chart.legend.options.labels.color,
  tooltip: chart.tooltip.options.backgroundColor,
  ticks: Object.values(chart.scales).filter(scale => scale.options.display).map(scale => scale.options.ticks.color),
}));
assert.equal(overview.getDatasetMeta(0).dataset.options.borderColor, "#653D60");
assert.equal(overview.getDatasetMeta(0).dataset.options.backgroundColor, "#EFE4E9");
assert.equal(owner.getDatasetMeta(0).dataset.options.borderColor, "#A53D64");
assert.equal(usage[1].getDatasetMeta(0).data[0].options.backgroundColor, "#426B9B");
const updates = new Map();
for (const chart of charts) {
  const update = chart.update.bind(chart);
  chart.update = mode => { assert.equal(mode, "none"); updates.set(chart, (updates.get(chart) || 0) + 1); return update(mode); };
}

try {
  toggle("dark");
  for (const [index, chart] of charts.entries()) {
    assert.equal(updates.get(chart), 1, "each existing chart redraws once per toggle");
    assert.equal(chart.data.datasets, data[index].datasets);
    assert.equal(JSON.stringify(chart.data), data[index].values, "datasets, sparse gaps and observations remain untouched");
    assert.equal(chart.config.options, data[index].options, "theme updates keep option and callback identity");
    assert.equal(chart.legend.options.labels.color, "#C3ADBF");
    assert.equal(chart.tooltip.options.backgroundColor, "#F7F0E8");
    for (const key of ["titleColor", "bodyColor", "footerColor"]) assert.equal(chart.tooltip.options[key], "#2C1F2E");
    for (const scale of Object.values(chart.scales).filter(scale => scale.options.display)) {
      assert.ok(["#C3ADBF", "#F09AB8", "#E7BEDA"].includes(scale.options.ticks.color));
      if (scale.options.grid.display) assert.equal(scale.options.grid.color, "#5A4259");
      if (scale.options.border.display) assert.equal(scale.options.border.color, "#5A4259");
    }
  }
  assert.equal(overview.getDatasetMeta(0).dataset.options.backgroundColor, "#3B2A3D");
  assert.equal(overview.getDatasetMeta(0).dataset.options.borderColor, "#E7BEDA");
  assert.equal(overview.getDatasetMeta(1).dataset.options.borderColor, "#F09AB8");
  assert.deepEqual(latency.data.datasets.map((_, index) => latency.getDatasetMeta(index).data[0].options.backgroundColor), ["#E7BEDA", "#9EBFED", "#B9A5EB"]);
  assert.equal(latency.scales.x.options.title.color, "#C3ADBF");
  assert.equal(usage[1].getDatasetMeta(0).data[0].options.backgroundColor, "#9EBFED");
  assert.equal(owner.getDatasetMeta(0).dataset.options.borderColor, "#F09AB8");
  assert.equal(owner.getDatasetMeta(1).dataset.options.borderColor, "#E7BEDA");
  assert.ok(contexts.get(owner.canvas).strokeColors.includes("#C3ADBF"), "service version markers recolor with their points");
  assert.equal(overview.isDatasetVisible(1), false, "legend visibility survives theme changes");
  assert.deepEqual(overview.getActiveElements().map(({ datasetIndex, index }) => ({ datasetIndex, index })), active);
  assert.equal(overview.tooltip.getActiveElements()[0].datasetIndex, 0);
  assert.equal(overview.canvas.style.cursor, "crosshair");
  assert.equal(document.querySelector("input").value, "unsaved draft");
  assert.equal(document.activeElement, document.querySelector("input"));
  assert.equal(document.querySelector("select").value, "300");
  assert.equal(details.open, true);
  assert.equal(apiReads, 2, "theme changes do not refetch Health or Traffic");

  // CSS owns the palette; new charts resolve current tokens on their first draw.
  document.documentElement.style.setProperty("--rg-chart-blue", "#abcdef");
  const canvas = document.createElement("canvas");
  document.body.append(canvas);
  const fresh = mountMonitorChart(HeadlessChart, canvas, usageTrendConfigs(rows)[1]);
  assert.equal(fresh.getDatasetMeta(0).data[0].options.backgroundColor, "#abcdef");
  fresh.destroy();
  fresh.update = () => assert.fail("destroyed charts must leave the live registry");
  document.documentElement.style.removeProperty("--rg-chart-blue");

  toggle("light");
  for (const [index, chart] of charts.entries()) {
    assert.equal(chart.getDatasetMeta(0).data[0].options.borderColor, light[index].line);
    assert.equal(chart.getDatasetMeta(0).data[0].options.backgroundColor, light[index].background);
    assert.equal(chart.legend.options.labels.color, light[index].legend);
    assert.equal(chart.tooltip.options.backgroundColor, light[index].tooltip);
    assert.deepEqual(Object.values(chart.scales).filter(scale => scale.options.display).map(scale => scale.options.ticks.color), light[index].ticks);
    assert.equal(updates.get(chart), 2);
  }
  disposeTheme();
  disposeTheme();
  assert.equal(listenerRemoves, 1, "listener disposal is idempotent");
  const stopAgain = installChartTheme(HeadlessChart, document);
  disposeTheme();
  assert.equal(installChartTheme(HeadlessChart, document), stopAgain, "old cleanup cannot remove a newer installation");
  stopAgain();
  class FakeChart { static instances = { withoutUpdate: { canvas: document.createElement("canvas") } }; }
  const fakeDispose = installChartTheme(FakeChart, document);
  assert.doesNotThrow(() => toggle("dark"), "test doubles need neither register nor an instance registry");
  fakeDispose();
} finally {
  disposeTheme();
  disposeHealth();
  disposeTraffic();
  pageCharts.dispose();
  usage.forEach(chart => chart.destroy());
  dom.window.close();
}
assert.equal(Object.keys(Chart.instances).length, 0, "every chart owner releases its instances");
console.log("ok - chart themes redraw every live monitor and owner chart, preserve light colors and state, and release listeners and charts");
