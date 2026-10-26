import { appendGatewayObservation, capacityMeter, chartColor, gatewayTrendConfig, measured, metricNumber, monitorChartOptions, monitorKpi, monitorPalette, mountMonitorChart, protocolMix } from "./monitoring-visuals";

// Parent supplies an isolated content host and existing api/esc/attr/badge/table
// helpers. Both mounts return a disposer; invoke it on navigation.
// GET /admin-ui/admin/gateway/status?instance_id=all|UUID&window_seconds=60|300|900
// {scope, sampled_at, partial, unavailable,
//  instances:[{instance_id,sampled_at,stale,runtime}], aggregate:RuntimeSnapshot|null}
// RuntimeSnapshot is gateway_telemetry::runtime::RuntimeSnapshot. Histograms use
// fixed microsecond buckets and bucket-upper-bound p50_ms/p95_ms/p99_ms values.

const WINDOWS = [60, 300, 900];
const STALE_AFTER_MS = 5_000;
const number = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
const count = (value) => number(value) ? value.toLocaleString() : "Unknown";
const windowLabel = (seconds) => `${seconds / 60} min`;

export function gatewayStatusPath(instanceId = "all", windowSeconds = 60) {
  const query = new URLSearchParams({
    instance_id: String(instanceId),
    window_seconds: String(WINDOWS.includes(Number(windowSeconds)) ? windowSeconds : 60),
  });
  return `/admin-ui/admin/gateway/status?${query}`;
}

function staleSample(sample, now) {
  const sampled = Date.parse(sample?.sampled_at);
  return sample?.stale === true || !Number.isFinite(sampled)
    || Math.abs(now - sampled) > STALE_AFTER_MS;
}

export function selectGatewayStatus(response, { instanceId = "all", windowSeconds = 60, now = Date.now(), failed = false } = {}) {
  const instances = Array.isArray(response?.instances) ? response.instances.slice(0, 128) : [];
  const selected = instances.find((row) => row.instance_id === instanceId);
  const fresh = instances.filter((row) => !staleSample(row, now));
  let runtime = instanceId === "all" ? response?.aggregate : selected?.runtime;
  if (runtime?.window_seconds !== windowSeconds || runtime?.schema_version !== 1) runtime = null;
  const stale = instanceId === "all"
    ? instances.length > 0 && fresh.length === 0
    : Boolean(selected && staleSample(selected, now));
  const rateRows = instanceId === "all" ? fresh : selected && !stale ? [selected] : [];
  const compatibleRates = rateRows.filter(row => row.runtime?.schema_version === 1 && row.runtime?.window_seconds === windowSeconds);
  const arrivalsPerSecond = compatibleRates.length > 0 && compatibleRates.every(row => number(row.runtime?.counters?.started) && row.runtime?.observed_seconds > 0)
    ? compatibleRates.reduce((sum, row) => sum + row.runtime.counters.started / row.runtime.observed_seconds, 0) : null;
  let label = "Current snapshot", tone = "neutral", message = "";
  if (failed) {
    label = "Refresh failed";
    tone = "bad";
    message = "Gateway status could not be refreshed. Values shown are the last received snapshot.";
  } else if (response?.unavailable) {
    label = "Unavailable";
    tone = "bad";
    message = runtime
      ? "Cluster collection is unavailable. Only reporting instances are included; missing measurements are unknown."
      : "Gateway status is unavailable. Missing measurements are unknown.";
  } else if (stale) {
    label = "Stale snapshot";
    tone = "warn";
    message = "These measurements are stale. Current traffic and capacity are unknown.";
  } else if (response?.partial || fresh.length !== instances.length || runtime?.incompatible_instances > 0) {
    label = "Partial snapshot";
    tone = "warn";
    message = "Only reporting instances are included. Missing or stale instances do not count as zero activity.";
  } else if (!runtime) {
    label = "Unknown";
    message = response ? "No compatible runtime snapshot is available for this selection." : "Waiting for a gateway runtime snapshot.";
  }
  return { runtime, instances, freshCount: fresh.length, arrivalsPerSecond, stale, failed, label, tone, message };
}

export function gatewayQuantile(histogram, percentile) {
  if (!histogram || !number(histogram.sample_count)) return "Unknown";
  if (histogram.sample_count === 0) return "No samples";
  const value = histogram[`p${percentile}_ms`];
  return number(value) ? `≤ ${value.toLocaleString(undefined, { maximumFractionDigits: 3 })} ms` : "Above histogram range";
}

function bytes(value) {
  if (!number(value)) return "Unknown";
  if (value < 1024) return `${value.toLocaleString()} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toLocaleString(undefined, { maximumFractionDigits: 1 })} KiB`;
  return `${(value / (1024 * 1024)).toLocaleString(undefined, { maximumFractionDigits: 1 })} MiB`;
}

export function gatewayCpuUtilization(current, previous, instanceId = "all") {
  const before = new Map((previous?.instances || []).map(row => [row.instance_id, row]));
  const selected = (current?.instances || []).filter(row => instanceId === "all" || row.instance_id === instanceId);
  if (!selected.length) return null;
  let usedCores = 0, limitCores = 0;
  for (const row of selected) {
    const old = before.get(row.instance_id);
    const process = row.runtime?.process, prior = old?.runtime?.process;
    const elapsed = row.runtime?.sampled_at_unix_ms - old?.runtime?.sampled_at_unix_ms;
    if (!number(process?.cgroup_cpu_usage_usec) || !number(prior?.cgroup_cpu_usage_usec)) return null;
    const delta = process.cgroup_cpu_usage_usec - prior.cgroup_cpu_usage_usec;
    if (staleSample(row, Date.now()) || !number(delta) || !(elapsed > 0)
      || !number(process?.cgroup_cpu_limit_cores) || process.cgroup_cpu_limit_cores === 0) return null;
    usedCores += delta / (elapsed * 1000);
    limitCores += process.cgroup_cpu_limit_cores;
  }
  return limitCores > 0 ? 100 * usedCores / limitCores : null;
}

export function gatewayStatusView(selection, { esc, badge, table }, { compact = false, windowSeconds = 60, observations = [] } = {}) {
  const runtime = selection.runtime, active = runtime?.active, latency = runtime?.latency, capacity = runtime?.body_capacity;
  const notice = `<div data-gateway-part="notice">${selection.message ? `<p class="notice inline" data-kind="${selection.tone}" role="status">${esc(selection.message)}</p>` : ""}</div>`;
  const activeTotal = [active?.http, active?.sse, active?.websocket].every(number) ? active.http + active.sse + active.websocket : null;
  const completed = runtime?.counters?.completed, failures = runtime?.counters?.failed;
  const latencySeries = ["http_success_duration", "ttfb", "ttft", "websocket_handshake"].map(key => latency?.[key]);
  const hasLatency = latencySeries.some(histogram => histogram?.sample_count > 0 && [50, 95, 99].some(percentile => number(histogram[`p${percentile}_ms`])));
  const latencyEmpty = hasLatency ? "" : latencySeries.some(histogram => histogram?.sample_count > 0) ? "Samples exceed the chart range" : latencySeries.some(histogram => number(histogram?.sample_count)) ? "No latency samples in this window" : "Latency unavailable";
  const rate = number(selection.arrivalsPerSecond) ? selection.arrivalsPerSecond.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "—";
  const hasTrend = observations.some(point => measured(point.rate));
  const latestRate = observations.at(-1)?.rate;
  const trendValue = measured(latestRate) ? `Last observed ${metricNumber(latestRate)} req/s` : "No fresh rate sample";
  const observationTime = at => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const trendRange = hasTrend ? `${observationTime(observations[0].at)} – ${observationTime(observations.at(-1).at)}` : "since this page opened";
  const trendCaption = `Rolling ${windowLabel(windowSeconds)} rate · browser observations ${trendRange}`;
  const trendEmpty = hasTrend ? "" : observations.length ? "No fresh arrival-rate samples" : "Waiting for live observations";
  const trendChart = `<div class="${compact ? "monitor-trend-chart" : "monitor-chart"}"><canvas data-gateway-chart="trend" role="img" aria-label="Arrival rate in requests per second. ${esc(trendValue)}. ${esc(trendCaption)}."></canvas><div class="monitor-chart-empty" data-gateway-part="trend-empty">${trendEmpty ? `<i class="ti ti-chart-line" aria-hidden="true"></i><span>${esc(trendEmpty)}</span>` : ""}</div></div>`;
  const capacityNote = '<p class="monitor-capacity-note">Current reservations for managed request/response body buffers. Requests can remain in flight after buffers are released. Brief occupancy may fall between samples; these meters do not show cumulative traffic or total memory use.</p>';
  const scope = `<div class="monitor-scope-line" data-gateway-part="scope">${badge(selection.label, selection.tone)}<span><i class="ti ti-server" aria-hidden="true"></i> ${esc(count(runtime?.instance_count))} reporting instances</span><span>${esc(count(completed))} completed · ${esc(windowLabel(windowSeconds))}</span></div>`;
  const kpis = `<div class="monitor-kpis" data-gateway-part="kpis">
    ${monitorKpi("Arrivals / second", rate, { esc, icon: "activity", detail: `Rolling ${windowLabel(windowSeconds)}` })}
    ${monitorKpi("In flight", metricNumber(activeTotal), { esc, icon: "arrows-exchange", detail: "HTTP · SSE · WebSocket" })}
    ${monitorKpi("HTTP success p95", gatewayQuantile(latency?.http_success_duration, 95), { esc, icon: "clock", detail: "Completed HTTP + SSE" })}
    ${compact ? monitorKpi("Buffered body slots", capacity ? `${count(capacity.used_requests)} / ${count(capacity.max_requests)}` : "—", { esc, icon: "stack-2", detail: capacity ? `${bytes(capacity.free_bytes)} buffer space free` : "Capacity unavailable" }) : monitorKpi("Failed requests", metricNumber(failures), { esc, icon: "alert-circle", detail: number(completed) && completed > 0 && number(failures) ? `${(100 * failures / completed).toFixed(2)}% of completed requests` : "No completed samples", tone: failures > 0 ? "bad" : "neutral" })}
  </div>`;
  if (compact) return `${scope}${notice}${kpis}${capacityNote}<div class="monitor-compact-footer"><div><h4 class="monitor-protocol-title">In-flight requests by protocol</h4><div data-gateway-part="protocols">${protocolMix(active, esc)}</div></div><figure class="monitor-mini-trend"><figcaption><h4>Arrival rate</h4><span data-gateway-part="trend-value">${esc(trendValue)}</span></figcaption><p class="monitor-trend-caption" data-gateway-part="trend-range">${esc(trendCaption)}</p>${trendChart}</figure></div>`;
  const latencyRows = [
    ["HTTP + SSE · all outcomes", "http_duration"], ["HTTP + SSE · successes", "http_success_duration"],
    ["HTTP + SSE · upstream duration", "upstream_duration"], ["HTTP + SSE · first body byte", "ttfb"],
    ["SSE · first content token", "ttft"], ["WebSocket · handshake", "websocket_handshake"],
    ["WebSocket · session only", "websocket_session"], ["Body admission · decision", "admission_decision"],
    ["Body admission · rejected decision", "admission_rejection"], ["Body lease · retention", "body_retention"],
  ].map(([label, key]) => [esc(label), count(latency?.[key]?.sample_count), ...[50, 95, 99].map(percentile => esc(gatewayQuantile(latency?.[key], percentile)))]);
  const process = runtime?.process;
  const processRows = [
    ["CPU since preceding sample", number(selection.cpuUtilization) ? `${selection.cpuUtilization.toFixed(1)}% of cgroup CPU limit` : "Unknown"],
    ["Process RSS", bytes(process?.rss_bytes)], ["Cgroup memory in use / limit", `${bytes(process?.cgroup_memory_current_bytes)} / ${bytes(process?.cgroup_memory_limit_bytes)}`],
    ["Cgroup CPU limit", number(process?.cgroup_cpu_limit_cores) ? `${count(process.cgroup_cpu_limit_cores)} cores` : "Unknown"],
    ["Cgroup CPU used", number(process?.cgroup_cpu_usage_usec) ? `${(process.cgroup_cpu_usage_usec / 1_000_000).toLocaleString()} s` : "Unknown"],
    ["Cgroup throttled periods / periods", `${count(process?.cgroup_cpu_throttled_periods)} / ${count(process?.cgroup_cpu_periods)}`],
    ["Cgroup throttled time", number(process?.cgroup_cpu_throttled_usec) ? `${(process.cgroup_cpu_throttled_usec / 1_000_000).toLocaleString()} s` : "Unknown"],
    ["Cgroup OOM events / kills", `${count(process?.cgroup_oom_events)} / ${count(process?.cgroup_oom_kills)}`], ["Restarts", count(process?.restarts)],
  ].map(([label, value]) => [esc(label), esc(value)]);
  const dependency = (label, ready) => `<span class="monitor-dependency ${ready === true ? "good" : ready === false ? "bad" : "neutral"}"><i class="ti ti-${ready === true ? "circle-check" : ready === false ? "alert-circle" : "minus"}" aria-hidden="true"></i>${esc(label)}<small>${ready === true ? "Ready" : ready === false ? "Unavailable" : "Unknown"}</small></span>`;
  const instances = (selection.instances || []).map(row => {
    const stale = staleSample(row, Date.now()), d = stale ? {} : row.dependencies;
    return `<div class="monitor-instance"><div><i class="ti ti-server" aria-hidden="true"></i><code title="${esc(row.instance_id)}">${esc(String(row.instance_id).slice(0, 8))}</code>${badge(stale ? "Stale" : "Reporting", stale ? "warn" : "neutral")}</div><div class="monitor-dependencies">${dependency("PostgreSQL", d?.postgres_ready)}${dependency("Redis", d?.redis_ready)}${dependency("Publisher", d?.publication_failed === false ? true : d?.publication_failed === true ? false : null)}</div><small class="monitor-instance-meta">Primary pool: ${esc(count(d?.postgres_pool_idle))} idle / ${esc(count(d?.postgres_pool_connections))} open</small></div>`;
  }).join("");
  const recording = runtime?.recording_failures, denials = runtime?.admission_denials;
  const recordingSignals = [["Usage", recording?.usage_events], ["Debug bundles", recording?.debug_bundles], ["Traffic", recording?.request_traffic]].map(([label, value]) => `<span class="${value > 0 ? "bad" : ""}">${esc(label)}<strong>${esc(count(value))}</strong></span>`).join("");
  return `${scope}${notice}${kpis}
    <div class="monitor-dashboard-grid">
      <figure class="monitor-figure"><figcaption><h3>Live request rate</h3><span data-gateway-part="trend-value">${esc(trendValue)}</span></figcaption><p class="monitor-trend-caption" data-gateway-part="trend-range">${esc(trendCaption)}</p>${trendChart}</figure>
      <div class="monitor-resource-panel" data-gateway-part="capacity"><div class="monitor-section-heading"><h3>Buffer capacity</h3><i class="ti ti-stack-2" aria-hidden="true"></i></div>${capacityNote}${capacityMeter("Buffered body slots", capacity?.used_requests, capacity?.free_requests, capacity?.max_requests, count, esc)}${capacityMeter("Reserved body bytes", capacity?.used_bytes, capacity?.free_bytes, capacity?.max_bytes, bytes, esc)}<div class="monitor-section-heading"><h4>Active protocols</h4><span>${esc(metricNumber(activeTotal))} in flight</span></div>${protocolMix(active, esc)}</div>
      <figure class="monitor-figure"><figcaption><h3>Response latency</h3><span>Histogram upper bounds · milliseconds</span></figcaption><div class="monitor-chart monitor-latency-chart"><canvas data-gateway-chart="latency" role="img" aria-label="Compare p50, p95 and p99 for successful HTTP, first byte, first token and WebSocket handshake. Exact displayed values are in Latency distribution below."></canvas><div class="monitor-chart-empty" data-gateway-part="latency-empty">${latencyEmpty ? `<i class="ti ti-clock" aria-hidden="true"></i><span>${esc(latencyEmpty)}</span>` : ""}</div></div></figure>
      <div class="monitor-resource-panel" data-gateway-part="dependencies"><div class="monitor-section-heading"><h3>Fleet & dependencies</h3><i class="ti ti-topology-star-3" aria-hidden="true"></i></div>${instances || `<div class="monitor-empty"><i class="ti ti-server-off" aria-hidden="true"></i><span>No reporting instances</span></div>`}<div class="monitor-section-heading"><h4>Recording failures</h4><span>${esc(windowLabel(windowSeconds))}</span></div><div class="monitor-recording-signals">${recordingSignals}</div></div>
    </div>
    <details class="monitor-details" data-gateway-detail="latency"><summary><i class="ti ti-chart-bar" aria-hidden="true"></i>Latency distribution & metric definitions</summary>
      <p class="help">Quantiles are histogram bucket upper bounds, merged across reporting instances. HTTP success includes completed SSE. WebSocket session duration is separate from handshake latency.</p><div class="table-wrap">${table(["Measurement", "Samples", "p50", "p95", "p99"], latencyRows)}</div></details>
    <details class="monitor-details" data-gateway-detail="runtime"><summary><i class="ti ti-cpu" aria-hidden="true"></i>Process, container & admission diagnostics</summary>
      <p class="help">Unsupported platform measurements are Unknown. Cgroup counters can include other processes in the same container.</p><div class="table-wrap">${table(["Measurement", "Value"], processRows)}</div>
      <div class="monitor-signal-strip"><span>Slot denials <strong>${esc(count(denials?.concurrency))}</strong></span><span>Byte denials <strong>${esc(count(denials?.bytes))}</strong></span><span>Other denials <strong>${esc(count(denials?.other))}</strong></span></div></details>
    <details class="monitor-details" data-gateway-detail="definitions"><summary><i class="ti ti-info-circle" aria-hidden="true"></i>How to read this dashboard</summary><ul class="monitor-definitions"><li>Request rate uses each instance’s observed window. Counts include rejected and unattributed traffic.</li><li>Live trends are bounded observations collected while this page is open. Gaps mean missing observations.</li><li>Body meters measure retained managed buffers, not total container memory. No admission queue is used.</li><li>Process uptime: ${runtime?.uptime_seconds == null ? "unavailable for a cluster aggregate" : `${esc(count(runtime.uptime_seconds))} s`}. Observed window: ${esc(count(runtime?.observed_seconds))} s.</li><li>Dependency probes run every five seconds. PostgreSQL pool counts describe the primary pool; Usage has its own four-connection pool.</li></ul></details>`;
}

export function gatewayLatencyConfig(runtime) {
  const keys = ["http_success_duration", "ttfb", "ttft", "websocket_handshake"];
  const options = monitorChartOptions({ horizontal: true });
  options.indexAxis = "y";
  options.scales.x.title = { display: true, text: "Milliseconds · upper bound", color: options.scales.x.ticks.color, font: { size: 10 } };
  options.plugins.tooltip.callbacks = { label: context => `${context.dataset.label}: ≤ ${context.raw} ms` };
  const hasSamples = keys.some(key => runtime?.latency?.[key]?.sample_count > 0 && [50, 95, 99].some(percentile => measured(runtime.latency[key][`p${percentile}_ms`])));
  if (!hasSamples) { options.scales.x.display = false; options.scales.y.display = false; options.plugins.legend.display = false; }
  return { type: "bar", data: { labels: ["HTTP success", "First body byte", "First content token", "WS handshake"], datasets: [50, 95, 99].map((percentile, index) => ({ label: `p${percentile}`, data: keys.map(key => runtime?.latency?.[key]?.sample_count > 0 && measured(runtime.latency[key][`p${percentile}_ms`]) ? runtime.latency[key][`p${percentile}_ms`] : null), backgroundColor: chartColor([monitorPalette.teal, monitorPalette.blue, monitorPalette.violet][index], ["teal", "blue", "violet"][index]), borderRadius: 3, maxBarThickness: 9 })) }, options };
}

export function mountGatewayStatus({ content, api, esc, attr = esc, badge, table, compact = false, refreshMs = 2_000, initialWindowSeconds = 60, initialInstanceId = "all", Chart = null }) {
  let windowSeconds = WINDOWS.includes(Number(initialWindowSeconds)) ? Number(initialWindowSeconds) : 60;
  let instanceId = String(initialInstanceId), response = null, previousResponse = null, failed = false, disposed = false;
  let generation = 0, controller = null;
  let observations = [], charts = [];
  content.innerHTML = `<section class="panel monitor-runtime${compact ? " is-compact" : ""}">
    <div class="monitor-toolbar"><h3><i class="ti ti-activity-heartbeat" aria-hidden="true"></i>Gateway pulse</h3><div class="actions gateway-status-controls">
      <label>Window<select data-gateway-window>${WINDOWS.map((seconds) => `<option value="${seconds}"${seconds === windowSeconds ? " selected" : ""}>${windowLabel(seconds)}</option>`).join("")}</select></label>
      <label>Instance<select data-gateway-instance><option value="all">All reporting instances</option></select></label>
      <button type="button" data-gateway-refresh><i class="ti ti-refresh" aria-hidden="true"></i><span>Refresh</span></button>
    </div></div>
    <div data-gateway-status role="region" aria-label="Gateway runtime status"></div>
  </section>`;
  const windowControl = content.querySelector("[data-gateway-window]");
  const instanceControl = content.querySelector("[data-gateway-instance]");
  const refreshControl = content.querySelector("[data-gateway-refresh]");
  const statusHost = content.querySelector("[data-gateway-status]");
  // Preserve known instance choices when an instance-scoped response contains
  // only that instance or a failed refresh returns no new inventory.
  const knownInstances = new Map();
  function render() {
    if (disposed) return;
    const selection = selectGatewayStatus(response, { instanceId, windowSeconds, failed });
    selection.cpuUtilization = gatewayCpuUtilization(response, previousResponse, instanceId);
    for (const row of selection.instances) {
      if (typeof row.instance_id === "string" && row.instance_id.length <= 128) {
        knownInstances.set(row.instance_id, row);
      }
    }
    if (knownInstances.size > 128) {
      for (const id of knownInstances.keys()) {
        if (knownInstances.size <= 128) break;
        if (id !== instanceId) knownInstances.delete(id);
      }
    }
    if (instanceId !== "all" && !knownInstances.has(instanceId)) {
      knownInstances.set(instanceId, { instance_id: instanceId, stale: true });
    }
    const options = `<option value="all">All reporting instances</option>` + [...knownInstances.values()].map((row) => `<option value="${attr(row.instance_id)}">${esc(row.instance_id)}${staleSample(row, Date.now()) ? " · stale" : ""}</option>`).join("");
    if (instanceControl.innerHTML !== options) instanceControl.innerHTML = options;
    instanceControl.value = instanceId;
    const openDetails = [...(statusHost.querySelectorAll?.("details[open][data-gateway-detail]") || [])].map(node => node.dataset.gatewayDetail);
    const focusedDetail = statusHost.querySelector?.("details[data-gateway-detail] > summary:focus")?.parentElement.dataset.gatewayDetail;
    const previousCharts = new Map(charts.map(chart => [chart.canvas.dataset.gatewayChart, chart]));
    charts = [];
    const markup = gatewayStatusView(selection, { esc, badge, table }, { compact, windowSeconds, observations });
    const nextFrame = statusHost.ownerDocument?.createElement("div");
    if (nextFrame) nextFrame.innerHTML = markup;
    if (nextFrame && statusHost.querySelector('[data-gateway-part="scope"]')) {
      // Update signals without detaching chart ancestors. Chart.js keeps its
      // resize observers and hover state attached to the original containers.
      for (const part of nextFrame.querySelectorAll("[data-gateway-part]")) {
        const current = statusHost.querySelector(`[data-gateway-part="${part.dataset.gatewayPart}"]`);
        if (current) current.innerHTML = part.innerHTML;
      }
      for (const detail of nextFrame.querySelectorAll("[data-gateway-detail]")) {
        const current = statusHost.querySelector(`[data-gateway-detail="${detail.dataset.gatewayDetail}"]`);
        if (current) current.innerHTML = detail.innerHTML;
      }
    } else statusHost.innerHTML = markup;
    for (const name of openDetails) { const node = statusHost.querySelector?.(`[data-gateway-detail="${name}"]`); if (node) node.open = true; }
    if (focusedDetail) statusHost.querySelector?.(`[data-gateway-detail="${focusedDetail}"] > summary`)?.focus({ preventScroll: true });
    const configs = [["trend", gatewayTrendConfig(observations, { compact })]];
    if (!compact) configs.push(["latency", gatewayLatencyConfig(selection.runtime)]);
    for (const [key, config] of configs) {
      const canvas = statusHost.querySelector?.(`[data-gateway-chart="${key}"]`);
      const previous = previousCharts.get(key);
      if (previous && canvas) {
        const nextLabel = nextFrame?.querySelector(`[data-gateway-chart="${key}"]`)?.getAttribute("aria-label");
        if (nextLabel) previous.canvas.setAttribute("aria-label", nextLabel);
        previous.data = config.data;
        previous.options = config.options;
        previous.update("none");
        charts.push(previous);
        previousCharts.delete(key);
      } else {
        const chart = mountMonitorChart(Chart, canvas, config);
        if (chart) charts.push(chart);
      }
    }
    previousCharts.forEach(chart => chart.destroy());
  }
  async function refresh() {
    if (disposed) return;
    const current = ++generation;
    controller?.abort();
    controller = new AbortController();
    refreshControl.disabled = true;
    statusHost.setAttribute("aria-busy", "true");
    try {
      const next = await api(gatewayStatusPath(instanceId, windowSeconds), { signal: controller.signal });
      if (disposed || current !== generation) return;
      previousResponse = response;
      response = next;
      failed = false;
      observations = appendGatewayObservation(observations, selectGatewayStatus(response, { instanceId, windowSeconds }), Date.parse(response.sampled_at));
    } catch (error) {
      if (disposed || current !== generation) return;
      failed = true;
    } finally {
      if (!disposed && current === generation) {
        refreshControl.disabled = false;
        statusHost.setAttribute("aria-busy", "false");
        render();
      }
    }
  }
  const changeWindow = () => { windowSeconds = Number(windowControl.value); observations = []; render(); void refresh(); };
  const changeInstance = () => { instanceId = instanceControl.value; observations = []; render(); void refresh(); };
  const clickRefresh = () => { void refresh(); };
  windowControl.addEventListener("change", changeWindow);
  instanceControl.addEventListener("change", changeInstance);
  refreshControl.addEventListener("click", clickRefresh);
  render();
  void refresh();
  const interval = refreshMs > 0 ? setInterval(() => { render(); void refresh(); }, Math.max(1_000, refreshMs)) : null;
  const dispose = () => {
    disposed = true;
    ++generation;
    controller?.abort();
    charts.forEach(chart => chart?.destroy());
    charts = [];
    if (interval !== null) clearInterval(interval);
    windowControl.removeEventListener("change", changeWindow);
    instanceControl.removeEventListener("change", changeInstance);
    refreshControl.removeEventListener("click", clickRefresh);
  };
  return dispose;
}

export function mountHealthGatewayStatus(options) {
  return mountGatewayStatus({ ...options, compact: false });
}

export function mountTrafficGatewayStatus(options) {
  return mountGatewayStatus({ ...options, compact: true });
}
