// Shared visual primitives. Chart.js is supplied by the page controller so every
// chart has an explicit owner and can be destroyed on navigation.
export const monitorPalette = { teal: "#653D60", blue: "#426B9B", violet: "#7F5CA5", red: "#A53D64", amber: "#A45B28", muted: "#EADAE5", text: "#6E5B6A" };
const darkChartPalette = { teal: "#E7BEDA", blue: "#9EBFED", violet: "#B9A5EB", red: "#F09AB8", amber: "#E6AC7A", muted: "#402D42", text: "#C3ADBF", grid: "#5A4259", fill: "#3B2A3D", "tooltip-bg": "#F7F0E8", "tooltip-text": "#2C1F2E" };
const chartThemeInstallations = new WeakMap();

// Chart.js resolves scriptable colors again on update, preserving the original
// light colors without mutating datasets, replacing options or rebuilding views.
export function chartColor(lightColor, darkToken) {
  return context => {
    const document = context?.chart?.canvas?.ownerDocument ?? globalThis.document;
    if (document?.documentElement?.dataset?.theme !== "dark") return lightColor;
    const style = document.defaultView?.getComputedStyle(document.documentElement);
    return style?.getPropertyValue(`--rg-chart-${darkToken}`).trim() || darkChartPalette[darkToken];
  };
}

export function installChartTheme(Chart, document = globalThis.document) {
  if (!Chart || !document?.addEventListener) return () => {};
  let documents = chartThemeInstallations.get(Chart);
  if (!documents) { documents = new WeakMap(); chartThemeInstallations.set(Chart, documents); }
  if (documents.has(document)) return documents.get(document);
  if (Chart.defaults?.font) Chart.defaults.font.family = '"Relayna Round", ui-rounded, system-ui, sans-serif';
  let active = true;
  const refresh = () => {
    // Chart.js removes destroyed charts from this registry. Keeping no chart
    // collection here also avoids retaining canvases after navigation.
    for (const id of Object.keys(Chart.instances || {})) {
      const chart = Chart.instances[id];
      if (chart.canvas?.ownerDocument === document) chart.update?.("none");
    }
  };
  document.addEventListener("relayna:theme-change", refresh);
  document.fonts?.ready.then(() => { if (active) refresh(); });
  const dispose = () => {
    if (documents.get(document) !== dispose) return;
    active = false;
    document.removeEventListener("relayna:theme-change", refresh);
    documents.delete(document);
  };
  documents.set(document, dispose);
  return dispose;
}

export const measured = value => typeof value === "number" && Number.isFinite(value) && value >= 0;
export const metricNumber = value => measured(value) ? value.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "—";

export function monitorKpi(label, value, { esc, icon = "activity", detail = "", tone = "neutral" } = {}) {
  return `<div class="monitor-kpi ${esc(tone)}"><div class="monitor-kpi-label"><i class="ti ti-${esc(icon)}" aria-hidden="true"></i><span>${esc(label)}</span></div><strong>${esc(value)}</strong>${detail ? `<small>${esc(detail)}</small>` : ""}</div>`;
}

export function capacityMeter(label, used, free, maximum, format, esc) {
  const known = [used, free, maximum].every(measured) && maximum > 0;
  const percent = known ? Math.min(100, used / maximum * 100) : null;
  return `<div class="monitor-capacity"><div class="monitor-capacity-heading"><span>${esc(label)}</span><strong>${known ? esc(`${format(used)} / ${format(maximum)}`) : "Unknown"}</strong></div>
    ${known ? `<progress max="${maximum}" value="${Math.min(used, maximum)}" aria-label="${esc(`${label}: ${format(used)} occupied of ${format(maximum)}`)}" class="${percent >= 90 ? "is-high" : percent >= 70 ? "is-elevated" : ""}"></progress>` : `<div class="monitor-meter-unknown" aria-hidden="true"></div>`}
    <div class="monitor-capacity-foot"><span>${known ? `${esc(format(free))} free / ${esc(format(maximum))}` : "Capacity unavailable"}</span><span>${known ? `${percent.toFixed(0)}% used` : "—"}</span></div></div>`;
}

export function protocolMix(active, esc) {
  const values = [active?.http, active?.sse, active?.websocket];
  const known = values.every(measured), total = known ? values.reduce((a, b) => a + b, 0) : 0;
  const labels = ["HTTP", "SSE", "WebSocket"], colors = ["teal", "blue", "violet"];
  return `<div class="monitor-protocol-mix"><div class="monitor-protocol-bar" role="img" aria-label="${esc(labels.map((label, i) => `${label}: ${metricNumber(values[i])}`).join(", "))}">${total ? values.map((value, i) => `<span class="${colors[i]}" style="width:${100 * value / total}%"></span>`).join("") : `<span class="idle" style="width:100%"></span>`}</div><div class="monitor-legend">${labels.map((label, i) => `<span><i class="monitor-dot ${colors[i]}" aria-hidden="true"></i>${label}<strong>${esc(metricNumber(values[i]))}</strong></span>`).join("")}</div></div>`;
}

export function monitorChartOptions({ horizontal = false, money = false } = {}) {
  return {
    responsive: true, maintainAspectRatio: false, animation: false,
    interaction: { mode: "index", intersect: false },
    plugins: {
      legend: { display: true, position: "bottom", align: "start", labels: { color: chartColor(monitorPalette.text, "text"), boxWidth: 10, boxHeight: 10, usePointStyle: true, pointStyle: "circle", padding: 18, font: { size: 11 } } },
      tooltip: { backgroundColor: chartColor("#2C1F2E", "tooltip-bg"), titleColor: chartColor("#fff", "tooltip-text"), bodyColor: chartColor("#fff", "tooltip-text"), footerColor: chartColor("#fff", "tooltip-text"), padding: 12, cornerRadius: 7 },
    },
    scales: {
      x: { grid: { display: horizontal, color: chartColor("#DFD0D9", "grid") }, border: { display: false }, ticks: { color: chartColor(monitorPalette.text, "text"), maxTicksLimit: 6, font: { size: 10 } }, ...(horizontal ? { beginAtZero: true } : {}) },
      y: { beginAtZero: true, grid: { color: chartColor("#DFD0D9", "grid"), display: !horizontal }, border: { display: false }, ticks: { color: chartColor(monitorPalette.text, "text"), font: { size: 10 }, ...(money ? { callback: value => `$${Number(value).toLocaleString()}` } : { precision: 0 }) } },
    },
  };
}

export function mountMonitorChart(Chart, canvas, config) {
  return Chart && canvas?.getContext ? new Chart(canvas, config) : null;
}

// These are browser observations, never reconstructed historical samples. A
// missing observation is a gap and scope/window changes start a new series.
export function appendGatewayObservation(history, selection, sampledAt) {
  if (!Number.isFinite(sampledAt) || history.at(-1)?.at >= sampledAt) return history;
  const current = selection.runtime && !selection.stale && !selection.failed;
  const next = [...history];
  if (next.length && sampledAt - next.at(-1).at > 10_000) next.push({ at: sampledAt - 1, rate: null, p95: null });
  next.push({ at: sampledAt, rate: current && measured(selection.arrivalsPerSecond) ? selection.arrivalsPerSecond : null,
    p95: current && selection.runtime.latency?.http_success_duration?.sample_count > 0 && measured(selection.runtime.latency.http_success_duration.p95_ms) ? selection.runtime.latency.http_success_duration.p95_ms : null });
  return next.slice(-90);
}

export function gatewayTrendConfig(history, { compact = false } = {}) {
  const options = monitorChartOptions();
  options.plugins.legend.display = !compact;
  options.scales.x.ticks.maxTicksLimit = 5;
  if (compact || !history.some(point => point.rate !== null)) { options.scales.x.display = false; options.scales.y.display = false; }
  if (!history.some(point => point.rate !== null)) options.plugins.legend.display = false;
  return { type: "line", data: {
    labels: history.map(point => new Date(point.at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })),
    datasets: [{ label: "Arrivals / s", data: history.map(point => point.rate), borderColor: chartColor(monitorPalette.teal, "teal"), backgroundColor: chartColor("#EFE4E9", "fill"), fill: true, pointRadius: history.length === 1 ? 3 : 0, borderWidth: 2, tension: 0.2, spanGaps: false }],
  }, options };
}

export function usageSummaryView(summary, { esc, money }) {
  const known = measured(summary.request_count), success = known && summary.request_count > 0 && measured(summary.success_count) ? 100 * summary.success_count / summary.request_count : null;
  return `<div class="monitor-kpis usage-kpis">
    ${monitorKpi("Requests", metricNumber(summary.request_count), { esc, icon: "arrows-exchange", detail: "Recorded in this period" })}
    ${monitorKpi("Estimated cost", measured(summary.estimated_cost_usd) ? money(summary.estimated_cost_usd) : "—", { esc, icon: "coin", detail: "Reporting estimate · USD" })}
    ${monitorKpi("Success rate", success === null ? "—" : `${success.toFixed(2)}%`, { esc, icon: "circle-check", detail: `${metricNumber(summary.failure_count)} failures`, tone: summary.failure_count > 0 ? "warn" : "good" })}
    ${monitorKpi("Average latency", measured(summary.average_latency_ms) ? `${Math.round(summary.average_latency_ms).toLocaleString()} ms` : "—", { esc, icon: "clock", detail: "Recorded request duration" })}
  </div><div class="monitor-signal-strip"><span><i class="ti ti-alert-circle" aria-hidden="true"></i>Failures <strong>${esc(metricNumber(summary.failure_count))}</strong></span><span><i class="ti ti-stack-2" aria-hidden="true"></i>Tokens <strong>${esc(metricNumber(summary.total_tokens))}</strong></span><span><i class="ti ti-route-alt-left" aria-hidden="true"></i>Fallbacks <strong>${measured(summary.fallback_rate) ? `${(summary.fallback_rate * 100).toFixed(1)}%` : "—"}</strong></span><span><i class="ti ti-shield-check" aria-hidden="true"></i>Guardrail blocks <strong>${esc(metricNumber(summary.guardrail_block_count))}</strong></span></div>`;
}

export function usageChartsShell() {
  return `<div class="monitor-chart-grid usage-chart-grid" data-usage-visuals aria-busy="true">
    <figure class="monitor-figure"><figcaption><div><h3>Request volume</h3><span>Successes and failures over time</span></div><button type="button" class="monitor-data-link" data-usage-chart-data aria-label="View request trend data"><i class="ti ti-table" aria-hidden="true"></i>Data</button></figcaption><div class="monitor-chart" data-usage-volume-host><p class="monitor-chart-placeholder">Loading request trend…</p></div></figure>
    <figure class="monitor-figure"><figcaption><div><h3>Cost over time</h3><span>Estimated spend · USD</span></div><button type="button" class="monitor-data-link" data-usage-chart-data aria-label="View cost trend data"><i class="ti ti-table" aria-hidden="true"></i>Data</button></figcaption><div class="monitor-chart" data-usage-cost-host><p class="monitor-chart-placeholder">Loading cost trend…</p></div></figure>
  </div>`;
}

export function usageTrendConfigs(rows, interval = "hour") {
  const sorted = [...rows].map(row => ({ at: Date.parse(row.bucket_start || row.bucket || row.name), summary: row.summary || row })).filter(row => Number.isFinite(row.at)).sort((a, b) => a.at - b.at);
  const intervalMs = interval === "day" ? 86400000 : 3600000;
  const points = [];
  for (const row of sorted) {
    if (points.length && row.at - points.at(-1).at > intervalMs * 1.5) points.push({ at: row.at - 1, summary: {} });
    points.push(row);
  }
  const values = (key, series = points) => series.map(row => ({ x: row.at, y: measured(row.summary[key]) ? row.summary[key] : null }));
  const options = money => {
    const result = monitorChartOptions({ money });
    result.scales.x.type = "linear";
    result.scales.x.ticks.callback = value => new Date(value).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit" });
    result.plugins.tooltip.callbacks = { title: items => items.length ? new Date(items[0].parsed.x).toLocaleString() : "" };
    return result;
  };
  return [
    { type: "line", data: { datasets: [
      { label: "Successes", data: values("success_count"), borderColor: chartColor(monitorPalette.teal, "teal"), backgroundColor: chartColor("#EFE4E9", "fill"), fill: true, pointRadius: 0, tension: 0.2, borderWidth: 2, spanGaps: false },
      { label: "Failures", data: values("failure_count"), borderColor: chartColor(monitorPalette.red, "red"), backgroundColor: chartColor(monitorPalette.red, "red"), pointRadius: 0, tension: 0.2, borderWidth: 2, spanGaps: false },
    ] }, options: options(false) },
    // Artificial line-gap markers would reduce the bar width to one millisecond.
    { type: "bar", data: { datasets: [{ label: "Estimated cost (USD)", data: values("estimated_cost_usd", sorted), backgroundColor: chartColor(monitorPalette.blue, "blue"), borderRadius: 3, maxBarThickness: 24 }] }, options: options(true) },
  ];
}

export function usageRanking(rows, label, esc) {
  const ranked = rows.filter(row => measured(row.summary?.request_count)).sort((a, b) => b.summary.request_count - a.summary.request_count).slice(0, 8);
  const maximum = ranked[0]?.summary.request_count;
  if (!maximum) return "";
  return `<div class="monitor-ranking" aria-label="Top returned consumers by request count"><div class="monitor-section-heading"><h4>Top consumers</h4><span>Requests · returned groups</span></div>${ranked.map(row => `<div class="monitor-ranking-row"><span title="${esc(label(row.name))}">${esc(label(row.name))}</span><div class="monitor-ranking-track" aria-hidden="true"><span style="width:${100 * row.summary.request_count / maximum}%"></span></div><strong>${esc(metricNumber(row.summary.request_count))}</strong></div>`).join("")}</div>`;
}
