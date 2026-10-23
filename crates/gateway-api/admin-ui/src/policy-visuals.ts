import { html } from "./design-system/components";

const kinds = { global: ["world", "Global"], project: ["folder", "Project"], team: ["users", "Team"], route: ["route", "Route"], model: ["sparkles", "Model"] };
const fields = {
  allowed_routes: "Route patterns", allowed_models: "Models", allowed_providers: "Providers", allowed_services: "Services",
  rpm_limit: "Requests / minute", tpm_limit: "Estimated tokens / minute", daily_budget_usd: "Daily budget · USD", monthly_budget_usd: "Monthly budget · USD",
  max_requests_per_day: "Requests / UTC day", max_tokens_per_day: "Daily token threshold", max_cost_per_request: "Estimated USD / request",
  max_input_tokens_per_request: "Input tokens / request", max_output_tokens_per_request: "Output tokens / request",
  allowed_hours_utc: "Allowed UTC hours", unused_key_auto_disable_after_days: "Inactive days before disable",
  max_request_body_bytes: "Request body · bytes", max_response_body_bytes: "Response body · bytes", allow_streaming: "Streaming", allow_tools: "Tool calls",
};
const value = item => Array.isArray(item) ? item.length ? item.join(", ") : "No added restriction" : typeof item === "boolean" ? item ? "Permitted at this layer" : "Denied at this layer" : String(item);

export function policyLayerCards(rows, guardrailSummary, formatTime = value => String(value || "Not recorded")) {
  if (!rows.length) return '<div class="monitor-empty"><i class="ti ti-stack-2" aria-hidden="true"></i><span>No inherited layers configured. Direct key policy still applies.</span></div>';
  return `<div class="policy-layer-list" role="list" aria-label="Saved policy layers">${rows.map(layer => {
    const policy = layer.policy || {}, [icon, label] = Object.hasOwn(kinds, layer.kind) ? kinds[layer.kind] : ["stack-2", String(layer.kind || "Unknown")];
    const explicit = Object.entries(fields).filter(([name]) => policy[name] != null);
    const limits = explicit.filter(([name]) => typeof policy[name] === "number").length;
    const guardrails = layer.guardrail_policy || {};
    const guardrailNames = [["Mandatory", "mandatory_guardrails"], ["Optional", "optional_guardrails"], ["Forbidden", "forbidden_guardrails"]].filter(([, key]) => Array.isArray(guardrails[key]) && guardrails[key].length).map(([title, key]) => `<div><span>${title}</span>${guardrails[key].map(name => `<code class="badge neutral">${html(name)}</code>`).join("")}</div>`).join("");
    return `<article class="policy-layer-card" role="listitem"><div class="policy-layer-heading"><span class="drawer-mark" aria-hidden="true"><i class="ti ti-${icon}"></i></span><div><h4>${html(label)} <span class="badge neutral">v${html(policy.policy_version ?? 1)}</span></h4><code>${html(layer.scope_id || (layer.kind === "global" ? "Gateway-wide" : "Scope not recorded"))}</code></div><button type="button" class="danger" data-policy-layer-action="delete" data-layer-id="${html(layer.id)}" aria-label="Delete ${html(label)} policy layer">Delete</button></div>
      <div class="policy-layer-signals"><span><i class="ti ti-route" aria-hidden="true"></i>${Array.isArray(policy.allowed_routes) && policy.allowed_routes.length ? `${policy.allowed_routes.length} route patterns` : "No added route restriction"}</span><span><i class="ti ti-adjustments" aria-hidden="true"></i>${limits} numeric limits</span></div>
      <details class="catalog-disclosure"><summary><i class="ti ti-list-details" aria-hidden="true"></i> Rules & safeguards <i class="ti ti-chevron-down" aria-hidden="true"></i></summary><dl class="policy-rule-list">${explicit.map(([name, title]) => `<div><dt>${html(title)}</dt><dd>${html(value(policy[name]))}</dd></div>`).join("") || '<div><dt>Policy</dt><dd>No explicit constraints recorded</dd></div>'}</dl><div class="policy-layer-guardrails"><h5>Guardrails</h5>${guardrailSummary(layer.guardrail_policy)}${guardrailNames}</div><p class="subtle">Updated ${html(formatTime(layer.updated_at))}</p></details></article>`;
  }).join("")}</div>`;
}
