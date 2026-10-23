import { distributionBars, insightStrip } from "./design-system/insights";
import { catalogServiceTransport } from "./catalog-visuals";

const measured = value => typeof value === "number" && Number.isFinite(value) && value >= 0;
const count = value => measured(value) ? value : null;
const totalLinks = project => Array.isArray(project?.service_names) ? project.service_names.length : null;
const windowLabel = range => range === "6h" ? "Last 6 hours" : range === "24h" ? "Last 24 hours" : "Last 7 days";

export function ownerCatalogInsights(items, kind) {
  const projects = kind === "projects";
  let metrics;
  let distribution;
  if (projects) {
    const known = items.filter(item => totalLinks(item.project) !== null);
    const services = new Set(known.flatMap(item => item.project.service_names));
    const unlinked = known.filter(item => totalLinks(item.project) === 0).length;
    metrics = [
      { label: "Assigned projects", value: items.length, icon: "folders", detail: "Exact project memberships" },
      { label: "Known linked services", value: known.length ? services.size : items.length ? null : 0, icon: "affiliate", detail: `${known.length} of ${items.length} project link lists loaded · distinct service names` },
      { label: "Known unlinked projects", value: known.length ? unlinked : items.length ? null : 0, icon: "link-off", tone: unlinked ? "warn" : "neutral", detail: `${known.length} of ${items.length} project link lists loaded. Link configuration does not define recorded usage scope.` },
    ];
    const ranked = [...items].sort((a, b) => (totalLinks(b.project) ?? -1) - (totalLinks(a.project) ?? -1)).slice(0, 5);
    distribution = distributionBars("Configured service links", ranked.map(item => ({ label: item.project.name, value: totalLinks(item.project) })), {
      caption: `Top ${ranked.length} of ${items.length} assigned projects · links, not request attribution`,
      empty: "No project assignments",
    });
  } else {
    const catalogs = items.filter(item => Array.isArray(item.service?.openapi_endpoints));
    const catalogued = catalogs.filter(item => item.service.openapi_endpoints.length > 0).length;
    const transports = items.map(item => catalogServiceTransport(item.service || {}));
    const knownTransports = transports.filter(transport => transport !== "unknown");
    const sockets = transports.filter(transport => transport === "websocket" || transport === "accessa-websocket").length;
    metrics = [
      { label: "Assigned services", value: items.length, icon: "server", detail: "Exact service memberships" },
      { label: "OpenAPI coverage", value: catalogs.length ? `${catalogued} / ${catalogs.length}` : null, icon: "api", detail: "Services with imported operations · loaded catalogs" },
      { label: "WebSocket configured", value: knownTransports.length ? `${sockets} / ${knownTransports.length}` : null, icon: "plug-connected", detail: `${knownTransports.length} of ${items.length} transport definitions loaded · GET and the exact route must support the opt-in; not availability` },
    ];
    const roles = new Map();
    for (const item of items) {
      const label = item.role === "owner" ? "Owner" : item.role === "viewer" ? "Viewer" : "Other / not recorded";
      roles.set(label, (roles.get(label) || 0) + 1);
    }
    distribution = distributionBars("Assignment roles", [...roles].map(([label, value]) => ({ label, value })), {
      caption: "Read-only dashboards · assigned services only",
      empty: "No service assignments",
    });
  }
  return `<section class="page-insights page-insights-grid" aria-label="${projects ? "Project" : "Service"} access summary">${insightStrip(metrics, { label: "Assigned resource coverage" })}${distribution}</section>`;
}

export function ownerDashboardInsights(dashboard, { money, range }) {
  const summary = dashboard.summary || {};
  const requests = count(summary.request_count);
  const failures = count(summary.failure_count);
  const errorRate = requests > 0 && failures !== null && failures <= requests ? `${(100 * failures / requests).toFixed(1)}%` : null;
  const metrics = [
    { label: "Requests", value: requests, icon: "arrows-exchange", detail: windowLabel(range) },
    { label: "Recorded failure rate", value: errorRate, icon: "alert-triangle", tone: failures > 0 ? "bad" : "neutral", detail: requests === 0 ? "No requests in this window" : failures === null ? "Failure count unavailable" : `${failures.toLocaleString()} failed requests` },
    { label: "P95 latency", value: requests > 0 && measured(summary.p95_latency_ms) ? `${Math.round(summary.p95_latency_ms).toLocaleString()} ms` : null, icon: "stopwatch", detail: requests === 0 ? "No latency samples" : "Recorded request duration" },
    { label: "Estimated cost", value: measured(summary.estimated_cost_usd) ? money(summary.estimated_cost_usd) : null, icon: "currency-dollar", detail: "USD · recorded estimates" },
  ];
  const signals = [
    ["Policy denials", summary.policy_denial_count],
    ["Rate limits", summary.rate_limit_denial_count],
    ["Budget denials", summary.budget_denial_count],
    ["Guardrail blocks", summary.guardrail_block_count],
    ["Fallbacks", summary.fallback_count],
  ].map(([label, value]) => ({ label, value: count(value), tone: value > 0 ? "warn" : "neutral" }));
  return `<section class="page-insights page-insights-grid" aria-label="Scoped usage summary">${insightStrip(metrics, { label: "Selected window · all outcomes" })}${distributionBars("Governance signals", signals, { caption: "Selected window · categories may overlap · log filters apply below" })}</section>`;
}

export function ownerUsageDistribution(title, rows) {
  const ranked = [...rows].sort((a, b) => (count(b.summary?.request_count) ?? -1) - (count(a.summary?.request_count) ?? -1)).slice(0, 5);
  return distributionBars(title, ranked.map(row => ({
    label: row.name || "Not recorded",
    value: count(row.summary?.request_count),
    detail: measured(row.summary?.failure_count) ? `${row.summary.failure_count.toLocaleString()} failures` : "Failure count unavailable",
    tone: row.summary?.failure_count > 0 ? "warn" : "neutral",
  })), { caption: `Requests · top ${ranked.length} of ${rows.length} returned groups`, empty: "No usage groups returned for this window" });
}

export function ownerRequestProfile(rows, hasMore = false) {
  const groups = new Map();
  for (const row of rows) {
    const protocol = row.diagnostics?.protocol || (row.diagnostics?.websocket ? "websocket" : "");
    const label = protocol === "http" ? "HTTP / SSE" : protocol === "websocket" ? "WebSocket" : protocol === "sse" ? "SSE" : protocol ? "Other recorded protocol" : "Not recorded";
    groups.set(label, (groups.get(label) || 0) + 1);
  }
  return distributionBars("Protocols in loaded page", [...groups].map(([label, value]) => ({ label, value })), {
    caption: `${rows.length} request rows · current log filters${hasMore ? " · more pages available" : ""} · not live connections`,
    empty: "No request rows loaded for these filters",
  });
}

export function ownerIncidentSeries(rows, markers, interval = "day") {
  const intervalMs = interval === "hour" ? 3600000 : 86400000;
  const sorted = rows.map(row => ({ at: Date.parse(row.bucket_start || row.bucket || row.name), summary: row.summary || {} }))
    .filter(row => Number.isFinite(row.at)).sort((a, b) => a.at - b.at);
  const points = [];
  for (const row of sorted) {
    if (points.length && row.at - points.at(-1).at > intervalMs * 1.5) points.push({ at: points.at(-1).at + intervalMs, errorRate: null, p95: null });
    const requests = count(row.summary.request_count);
    const failures = count(row.summary.failure_count);
    points.push({ at: row.at,
      errorRate: requests > 0 && failures !== null && failures <= requests ? failures / requests * 100 : null,
      p95: requests > 0 && measured(row.summary.p95_latency_ms) ? row.summary.p95_latency_ms : null });
  }
  const observations = markers.map(marker => ({ at: Date.parse(marker.first_observed_at), version: marker.service_version }))
    .filter(marker => Number.isFinite(marker.at) && sorted.some(row => marker.at >= row.at && marker.at < row.at + intervalMs))
    .sort((a, b) => a.at - b.at);
  return { points, observations };
}

export function ownerIncidentDescription(rows, markers) {
  const sorted = [...rows].filter(row => Number.isFinite(Date.parse(row.bucket_start || row.bucket || row.name)))
    .sort((a, b) => Date.parse(a.bucket_start || a.bucket || a.name) - Date.parse(b.bucket_start || b.bucket || b.name));
  if (!sorted.length) return "No recorded usage buckets for this window. Missing samples are not zero measurements.";
  const latest = sorted.at(-1).summary || {};
  const requests = count(latest.request_count);
  const failures = count(latest.failure_count);
  const errors = requests > 0 && failures !== null && failures <= requests ? `${(100 * failures / requests).toFixed(1)}% errors` : requests === 0 ? "no requests; error rate unavailable" : "error rate unavailable";
  const latency = requests > 0 && measured(latest.p95_latency_ms) ? `${Math.round(latest.p95_latency_ms)} ms P95 latency` : "P95 latency unavailable";
  return `Latest recorded bucket: ${errors}; ${latency}. ${markers.length ? `${markers.length} version observations; markers show first Gateway observation, not deployment time.` : "No version transitions observed."} Missing buckets remain gaps.`;
}
