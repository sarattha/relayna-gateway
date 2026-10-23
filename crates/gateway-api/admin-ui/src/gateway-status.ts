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

function capacityText(capacity, kind) {
  if (!capacity) return "Unknown";
  const format = kind === "bytes" ? bytes : count;
  return `${format(capacity[`used_${kind}`])} occupied · ${format(capacity[`free_${kind}`])} free / ${format(capacity[`max_${kind}`])}`;
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

export function gatewayStatusView(selection, { esc, badge, table }, { compact = false, windowSeconds = 60 } = {}) {
  const runtime = selection.runtime;
  const active = runtime?.active;
  const latency = runtime?.latency;
  const capacity = runtime?.body_capacity;
  const status = badge(selection.label, selection.tone);
  const notice = selection.message ? `<p class="notice inline" data-kind="${selection.tone}" role="status">${esc(selection.message)}</p>` : "";
  const stat = (label, value) => `<div class="stat"><span>${esc(label)}</span><strong>${esc(value)}</strong></div>`;
  const metricStrip = `<div class="grid metric-strip">
    ${stat("Active HTTP / SSE / WS", `${count(active?.http)} / ${count(active?.sse)} / ${count(active?.websocket)}`)}
    ${stat("Arrivals / second", number(selection.arrivalsPerSecond) ? selection.arrivalsPerSecond.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "Unknown")}
    ${stat(`Completed · ${windowLabel(windowSeconds)}`, count(runtime?.counters?.completed))}
    ${stat("Body slots", capacityText(capacity, "requests"))}
    ${stat("Body bytes", capacityText(capacity, "bytes"))}
    ${stat("HTTP success p95", gatewayQuantile(latency?.http_success_duration, 95))}
  </div>`;
  if (compact) return `<div class="panel-heading"><h3>Gateway status</h3>${status}</div>${notice}${metricStrip}`;
  const latencyRows = [
    ["HTTP + SSE · all outcomes", "http_duration"],
    ["HTTP + SSE · successes", "http_success_duration"],
    ["HTTP + SSE · upstream duration", "upstream_duration"],
    ["HTTP + SSE · first body byte", "ttfb"],
    ["SSE · first content token", "ttft"],
    ["WebSocket · handshake", "websocket_handshake"],
    ["WebSocket · session only", "websocket_session"],
    ["Body admission · decision", "admission_decision"],
    ["Body admission · rejected decision", "admission_rejection"],
    ["Body lease · retention", "body_retention"],
  ].map(([label, key]) => [esc(label), count(latency?.[key]?.sample_count), ...[50, 95, 99].map((percentile) => esc(gatewayQuantile(latency?.[key], percentile)))]);
  const process = runtime?.process;
  const processRows = [
    ["CPU since preceding sample", number(selection.cpuUtilization) ? `${selection.cpuUtilization.toLocaleString(undefined, { maximumFractionDigits: 1 })}% of cgroup CPU limit` : "Unknown"],
    ["Process RSS", bytes(process?.rss_bytes)],
    ["Cgroup memory in use / limit", `${bytes(process?.cgroup_memory_current_bytes)} / ${bytes(process?.cgroup_memory_limit_bytes)}`],
    ["Cgroup memory headroom", number(process?.cgroup_memory_limit_bytes) && number(process?.cgroup_memory_current_bytes) ? bytes(Math.max(0, process.cgroup_memory_limit_bytes - process.cgroup_memory_current_bytes)) : "Unknown"],
    ["Cgroup CPU limit", number(process?.cgroup_cpu_limit_cores) ? `${count(process.cgroup_cpu_limit_cores)} cores` : "Unknown"],
    ["Cgroup CPU used", number(process?.cgroup_cpu_usage_usec) ? `${(process.cgroup_cpu_usage_usec / 1_000_000).toLocaleString()} s` : "Unknown"],
    ["Cgroup throttled periods / periods", `${count(process?.cgroup_cpu_throttled_periods)} / ${count(process?.cgroup_cpu_periods)}`],
    ["Cgroup throttled time", number(process?.cgroup_cpu_throttled_usec) ? `${(process.cgroup_cpu_throttled_usec / 1_000_000).toLocaleString()} s` : "Unknown"],
    ["Cgroup OOM events / kills", `${count(process?.cgroup_oom_events)} / ${count(process?.cgroup_oom_kills)}`],
    ["Restarts", count(process?.restarts)],
  ].map(([label, value]) => [esc(label), esc(value)]);
  const denials = runtime?.admission_denials;
  const recording = runtime?.recording_failures;
  const readiness = (value, latency) => `${value === true ? "Ready" : value === false ? "Unavailable" : "Unknown"}${number(latency) ? ` · ${latency.toLocaleString()} ms probe` : ""}`;
  const dependencyRows = (selection.instances || []).map(row => {
    const dependency = row.dependencies;
    return [esc(row.instance_id), esc(staleSample(row, Date.now()) ? "Stale" : "Reporting"),
      esc(readiness(dependency?.postgres_ready, dependency?.postgres_readiness_latency_ms)),
      esc(`${count(dependency?.postgres_pool_connections)} open · ${count(dependency?.postgres_pool_idle)} idle`),
      esc(readiness(dependency?.redis_ready, dependency?.redis_readiness_latency_ms)),
      esc(dependency?.publication_failed === true ? "Failed" : dependency?.publication_failed === false ? "Reporting" : "Unknown")];
  });
  return `<div class="panel-heading"><h3>Gateway</h3>${status}</div>${notice}${metricStrip}
    <p class="help">${esc(count(runtime?.instance_count))} reporting instances · ${esc(count(runtime?.observed_seconds))} s observed of ${esc(windowLabel(windowSeconds))}.
      ${runtime?.uptime_seconds == null ? "Process uptime is unavailable for a cluster aggregate." : `Process uptime: ${esc(count(runtime.uptime_seconds))} s.`}
      Counts include rejected and unattributed proxy traffic; these measurements do not depend on Usage writes.</p>
    <p class="help">Admission is fail-fast. Rejections in this window: slots ${esc(count(denials?.concurrency))} · bytes ${esc(count(denials?.bytes))} · other ${esc(count(denials?.other))}. No admission queue is used.</p>
    <p class="help">Body capacity describes retained managed buffers. Use container memory headroom below to assess total memory.</p>
    <p class="help">Recording failures in this window: Usage ${esc(count(recording?.usage_events))} · debug bundles ${esc(count(recording?.debug_bundles))} · request traffic ${esc(count(recording?.request_traffic))}.</p>
    <div class="panel-heading"><h4>Latency distribution</h4></div>
    <p class="help">Quantiles are histogram bucket upper bounds, merged across reporting instances. Sample counts apply to this window. HTTP success includes completed SSE; WebSocket sessions are separate. First-token samples require observed SSE content.</p>
    <div class="table-wrap">${table(["Measurement", "Samples", "p50", "p95", "p99"], latencyRows)}</div>
    <div class="panel-heading"><h4>Process and container</h4></div>
    <p class="help">Cgroup counters are cumulative and may include other processes in the same cgroup. Unsupported or missing platform measurements are unknown. An unlimited memory limit is also shown as unknown.</p>
    <div class="table-wrap">${table(["Measurement", "Value"], processRows)}</div>
    <div class="panel-heading"><h4>Dependencies and recording pipeline</h4></div>
    <p class="help">Readiness probes run every five seconds. Probe latency is separate from application query latency. PostgreSQL connections here belong to the primary pool; Usage has a separate four-connection pool.</p>
    <div class="table-wrap">${table(["Instance", "Source", "PostgreSQL", "Primary pool", "Redis", "Observation publisher"], dependencyRows)}</div>`;
}

export function mountGatewayStatus({ content, api, esc, attr = esc, badge, table, compact = false, refreshMs = 2_000, initialWindowSeconds = 60, initialInstanceId = "all" }) {
  let windowSeconds = WINDOWS.includes(Number(initialWindowSeconds)) ? Number(initialWindowSeconds) : 60;
  let instanceId = String(initialInstanceId), response = null, previousResponse = null, failed = false, disposed = false;
  let generation = 0, controller = null;
  content.innerHTML = `<section class="panel">
    <div class="actions">
      <label>Window<select data-gateway-window>${WINDOWS.map((seconds) => `<option value="${seconds}"${seconds === windowSeconds ? " selected" : ""}>${windowLabel(seconds)}</option>`).join("")}</select></label>
      <label>Instance<select data-gateway-instance><option value="all">All reporting instances</option></select></label>
      <button type="button" data-gateway-refresh>Refresh</button>
    </div>
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
    statusHost.innerHTML = gatewayStatusView(selection, { esc, badge, table }, { compact, windowSeconds });
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
  const changeWindow = () => { windowSeconds = Number(windowControl.value); render(); void refresh(); };
  const changeInstance = () => { instanceId = instanceControl.value; render(); void refresh(); };
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
