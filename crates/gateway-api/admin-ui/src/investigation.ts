// Shared presentation for an exact Traffic record, a Usage row, or a legacy debug snapshot.
export function investigationUsageSnapshot(usage) {
  if (!usage) return null;
  const fields = ["request_id", "key_id", "project_id", "route", "model", "provider", "status", "status_code", "latency_ms", "input_tokens", "output_tokens", "total_tokens", "estimated_cost_usd", "cost_source", "cost_mode", "pricing_rule_name", "service_name", "service_version", "http_method", "endpoint_template", "task_id", "run_id", "trace_id", "fallback_count", "created_at", "diagnostics"];
  // Keep concrete endpoint paths and any future payload/header fields out of copied diagnostics.
  return Object.fromEntries(fields.filter(field => field in usage).map(field => [field, usage[field]]));
}

export function timingValue(attempt, field) {
  const value = attempt[field];
  if (field === "dns_us" && attempt.dns_status === "ip_literal") return "Not needed · IP address";
  if (["tcp_connect_us", "tls_handshake_us"].includes(field) && attempt.connection_reused === true) return "Reused connection";
  if (field === "tls_handshake_us" && attempt.tls === false) return "Not used · HTTP";
  if (value == null || !Number.isFinite(value) || value < 0) return "Not recorded";
  return `${field.endsWith("_us") ? (value / 1000).toLocaleString(undefined, { maximumFractionDigits: 3 }) : value.toLocaleString()} ms`;
}

export function matchTrafficRecord(rows, usage) {
  // Client request IDs can be reused. Never join legacy rows by request ID alone.
  const id = usage?.diagnostics?.traffic_id;
  if (!id) return null;
  return rows.find(row => row.id === id && row.request_id === usage.request_id && row.key_id === usage.key_id && row.project_id === usage.project_id) || null;
}

export function requestInvestigationView({ traffic = null, usage = null, bundle = null, notice = "" }, { esc, table, time, projects = [] }) {
  usage = investigationUsageSnapshot(traffic?.usage || usage);
  bundle = traffic?.debug_bundle || bundle;
  const d = traffic?.diagnostics || usage?.diagnostics || {};
  const requestId = traffic?.request_id || usage?.request_id || bundle?.request_id || "Unknown request";
  const status = traffic?.client_status ?? usage?.status_code;
  const failed = Boolean(d.failure_code) || status >= 400 || usage?.status === "failure";
  const outcome = failed ? "Failed" : traffic?.completed === false ? "In progress" : status != null ? "Completed" : "Outcome not recorded";
  const text = value => esc(value == null || value === "" ? "Not recorded" : String(value));
  const label = value => value ? String(value).replaceAll("_", " ") : "Not recorded";
  const facts = rows => `<dl class="investigation-facts">${rows.map(([name, value]) => `<div><dt>${esc(name)}</dt><dd>${text(value)}</dd></div>`).join("")}</dl>`;
  const section = (title, content) => `<section class="investigation-section"><h4>${esc(title)}</h4>${content}</section>`;
  const projectId = traffic?.project_id ?? usage?.project_id ?? bundle?.project_id;
  const project = projects.find(p => p.id === projectId);
  const duration = traffic?.elapsed_ms ?? usage?.latency_ms;
  const reasons = {
    upstream_connection_refused: "The upstream refused the TCP connection. Check that the service is listening and its endpoint is correct.",
    upstream_no_route: "The gateway could not route a connection to the upstream network.",
    upstream_certificate_invalid: "The upstream certificate could not be validated.",
    upstream_connection_closed: "The upstream connection closed before the response completed.",
    upstream_write_failed: "Writing the request to the upstream failed.",
    upstream_protocol_error: "The upstream returned an invalid HTTP response.",
    control_state_unavailable: "The gateway could not access control state for rate limits or budgets.",
    gateway_overloaded: "Gateway body-processing capacity was exhausted.",
    store_unavailable: "The gateway could not access required database state.",
    upstream_transport_error: "The upstream connection or transfer failed. Inspect the attempt timeline.",
    client_disconnected: "The client connection closed before the request finished.",
    response_not_delivered: "No response headers were confirmed as sent to the client.",
    invalid_virtual_key: "The virtual key could not be authenticated. Check that the client uses the intended key and that it is active and unexpired.",
    missing_authorization: "No supported authorization was supplied. Check the client's configured key header.",
    policy_denied: "Gateway policy denied this request. Inspect the recorded policy and the requested route or model.",
    rate_limited: "A configured request or token limit was reached. Check the key's limits before retrying.",
    budget_exceeded: "The request exceeded its configured budget. Check the key's budget and recorded usage.",
    upstream_timeout: "The upstream operation timed out. Compare DNS, connection, TLS and response timings with the configured timeout.",
    upstream_tls_handshake_failed: "The TLS handshake failed. Check the upstream hostname, certificate chain and TLS configuration.",
    upstream_connection_error: "The upstream connection failed. Check name resolution and network reachability.",
    upstream_http_error: "The upstream returned an error response. Inspect its HTTP status and the attempt timeline.",
  };
  const note = d.failure_code ? reasons[d.failure_code] || `Request failed: ${label(d.failure_code)}. Inspect the failure stage and attempt timeline.` : traffic?.completed === false ? "This request is still running. Measurements appear as stages complete." : "";
  const traceList = (entries, empty) => entries?.length ? `<ul class="investigation-trace">${entries.map(v => `<li>${text(v)}</li>`).join("")}</ul>` : `<p class="help">${esc(empty)}</p>`;
  const attempts = traffic?.upstream_timings || [];
  const raw = { traffic, usage, debug_bundle: bundle };
  return `<div class="request-investigation">
    <div class="investigation-identity"><span class="badge ${failed ? "bad" : status != null ? "good" : "neutral"}">${esc(outcome)}</span><strong>${esc(requestId)}</strong></div>
    <div class="actions investigation-actions"><button type="button" data-investigation-copy="id">Copy request ID</button><button type="button" data-investigation-copy="diagnostics">Copy sanitized diagnostics</button><span class="help" data-investigation-copy-status role="status"></span></div>
    ${notice ? `<p class="notice">${esc(notice)}</p>` : ""}
    ${note ? `<p class="notice ${failed ? "bad" : ""}">${esc(note)}</p>` : ""}
    <div class="investigation-metrics">${[["Client HTTP", status ?? (traffic ? "Not confirmed" : null)], ["Upstream HTTP", d.upstream_status ?? (traffic?.attempts === 0 ? "Not attempted" : null)], ["Total duration", duration == null ? null : `${duration.toLocaleString()} ms`], ["Upstream attempts", traffic?.attempts]].map(([name,value]) => `<div><span>${esc(name)}</span><strong>${text(value)}</strong></div>`).join("")}</div>
    ${section("Request context", facts([
      [traffic ? "Started" : "Recorded", (traffic?.started_at || usage?.created_at || bundle?.created_at) ? time(traffic?.started_at || usage?.created_at || bundle?.created_at) : null],
      ["Method / endpoint", [traffic?.method || usage?.http_method, traffic?.endpoint || usage?.endpoint_template || usage?.route || bundle?.route].filter(Boolean).join(" ")],
      ["Model", usage?.model], ["Provider", traffic ? traffic.provider || "Not selected" : usage?.provider || bundle?.provider],
      ["Project", project ? `${project.name} · ${project.id}` : projectId], ["Service", traffic?.service || usage?.service_name || bundle?.service_name],
      ["Key", traffic?.key_prefix ? `${traffic.key_prefix}… · ${traffic.key_id}` : traffic?.key_id || usage?.key_id || (traffic ? "Unauthenticated or passthrough" : null)],
      ["Stream / outcome", traffic ? `${traffic.streaming ? "Streaming" : "Not identified as a stream"} · ${label(d.outcome || (traffic.completed ? "completed" : "in progress"))}` : null],
      ["Failure source / stage", failed ? `${label(d.failure_source)} / ${label(d.failure_stage)}` : "No reported failure"],
      ["Gateway instance", traffic?.instance_id || d.instance_id], ["Service version", usage?.service_version],
      ["Trace ID", usage?.trace_id || bundle?.trace_id], ["Task / run", [usage?.task_id,usage?.run_id].filter(Boolean).join(" / ")],
    ]))}
    ${section("Network & response timing", attempts.length ? `<p class="help">DNS, TCP and TLS are phase durations. Headers, first body byte and first content token are measured from each attempt's start, including connection setup. Total duration above includes gateway and client delivery time.</p>${attempts.map(a => `<article class="investigation-attempt"><h5>Attempt ${text(a.attempt)} · ${text(a.provider)}${a.connection_reused === true ? " · Reused connection" : a.connection_reused === false ? " · New connection" : ""}</h5>${facts([
      ["DNS resolution", `${timingValue(a,"dns_us")}${["failed","timeout"].includes(a.dns_status) ? ` · ${a.dns_status}` : ""}`],
      ["TCP connect",timingValue(a,"tcp_connect_us")], ["TLS handshake",timingValue(a,"tls_handshake_us")],
      ["Response headers",timingValue(a,"response_headers_ms")], ["First body byte",timingValue(a,"first_body_byte_ms")],
      ["First content token", traffic?.streaming ? timingValue(a,"first_token_ms") : "Not applicable · non-streaming"],
      ["Attempt duration",timingValue(a,"total_ms")], ["Upstream status / failure",[a.upstream_status,a.failure_code].filter(v=>v!=null).join(" / ")],
    ])}</article>`).join("")}${traffic.attempts > attempts.length ? '<p class="notice">Earlier attempt timings were discarded at the retention limit.</p>' : ""}` : `<p class="help">${traffic?.attempts === 0 ? "No upstream connection was attempted." : "Network timings were not recorded for this request. Older records remain available without timing measurements."}</p>`)}
    ${section("Event timeline", traffic?.timeline?.length ? `${traffic.timeline_truncated ? '<p class="notice">Earlier timeline steps were discarded at the retention limit.</p>' : ""}${table(["Elapsed", "Attempt", "Stage", "Reason", "Upstream HTTP"],traffic.timeline.map(step=>[`${text(step.elapsed_ms)} ms`,text(step.attempt),text(label(step.stage)),text(step.code ?? "—"),text(step.upstream_status ?? "—")]))}` : '<p class="help">No event timeline was recorded.</p>')}
    ${section("Usage & cost", facts([["Input tokens",usage?.input_tokens],["Output tokens",usage?.output_tokens],["Total tokens",usage?.total_tokens],["Estimated cost · USD",usage?.estimated_cost_usd == null ? null : `$${Number(usage.estimated_cost_usd).toFixed(6)}`],["Pricing source",usage?.cost_source],["Pricing rule",usage?.pricing_rule_name]]))}
    ${section("Policy & guardrails", `<h5>Policy decisions</h5>${traceList(bundle?.policy_trace,"Policy decisions were not recorded.")}<h5>Guardrail executions</h5>${traceList(bundle?.guardrail_trace,bundle ? "No guardrail executions were recorded in this snapshot." : "Guardrail execution details were not captured.")}`)}
    ${section("Routing decisions", `${traceList(bundle?.selection_trace,"Routing decisions were not recorded.")}${bundle?.fallback_history?.length ? table(["From","To","Reason"],bundle.fallback_history.map(a=>[text(a.from_provider),text(a.to_provider),text(a.reason)])) : '<p class="help">No fallback history was recorded.</p>'}`)}
    ${traffic?.recording_failures?.length ? `<p class="notice">Recording gaps: ${esc(traffic.recording_failures.join(", "))}. This investigation may be incomplete.</p>` : ""}
    <details class="investigation-raw" data-investigation-section="raw"><summary>Raw diagnostics & hashes</summary>${facts([["Internal request ID",traffic?.id],["Request hash",bundle?.request_hash],["Response hash",bundle?.response_hash],["Redaction version",bundle?.redaction_version]])}<pre data-investigation-raw>${esc(JSON.stringify(raw,null,2))}</pre></details>
  </div>`;
}

export function bindInvestigationActions(root, requestId) {
  root.querySelectorAll("[data-investigation-copy]").forEach(button => button.addEventListener("click", async () => {
    const status = root.querySelector("[data-investigation-copy-status]");
    try {
      await navigator.clipboard.writeText(button.dataset.investigationCopy === "id" ? requestId : root.querySelector("[data-investigation-raw]").textContent);
      status.textContent = "Copied";
    } catch { status.textContent = "Clipboard unavailable. Select and copy from Raw diagnostics."; }
  }));
}
