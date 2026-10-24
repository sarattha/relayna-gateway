// Read-only presentation of measurements already loaded by a page controller.
const escape = value => String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const known = value => typeof value === "number" && Number.isFinite(value) && value >= 0;
const tone = value => ["good", "bad", "warn", "neutral"].includes(value) ? value : "neutral";
const icon = value => /^[a-z0-9-]+$/.test(value || "") ? value : "chart-dots";
const valueText = value => value == null || typeof value === "number" && !Number.isFinite(value) ? "—" : typeof value === "number" ? value.toLocaleString(undefined, { maximumFractionDigits: 2 }) : String(value);
const help = (label, detail) => detail ? `<button type="button" class="help-trigger insight-help" aria-label="About ${escape(label)}" data-help-tooltip="${escape(detail)}"><i class="ti ti-info-circle" aria-hidden="true"></i></button>` : "";

export function insightStrip(items, { label = "At a glance" } = {}) {
  return `<div class="insight-strip" role="list" aria-label="${escape(label)}"><span class="insight-eyebrow" aria-hidden="true">${escape(label)}</span>${items.map(item => `<div class="insight-metric ${tone(item.tone)}" role="listitem"><span class="insight-icon" aria-hidden="true"><i class="ti ti-${icon(item.icon)}"></i></span><div><span class="insight-label"><span>${escape(item.label)}</span>${help(item.label, item.detail)}</span><strong>${escape(valueText(item.value))}</strong>${item.detail ? `<small class="sr-only">${escape(item.detail)}</small>` : ""}</div></div>`).join("")}</div>`;
}

export function distributionBars(title, items, { caption = "", empty = "No observations available" } = {}) {
  const maximum = Math.max(1, ...items.map(item => known(item.value) ? item.value : 0));
  return `<figure class="insight-distribution"><figcaption><h3>${escape(title)}</h3>${help(title, caption)}${caption ? `<span class="sr-only">${escape(caption)}</span>` : ""}</figcaption>${items.length ? `<div class="insight-bars">${items.map(item => `<div class="insight-bar ${tone(item.tone)}"><div><span>${escape(item.label)}${help(item.label, item.detail)}</span><strong>${known(item.value) ? escape(valueText(item.value)) : "Unknown"}</strong></div>${known(item.value) ? `<progress max="${maximum}" value="${item.value}" aria-label="${escape(`${item.label}: ${valueText(item.value)}`)}"></progress>` : '<span class="insight-unknown" aria-label="Unknown measurement"></span>'}${item.detail ? `<small class="sr-only">${escape(item.detail)}</small>` : ""}</div>`).join("")}</div>` : `<p class="insight-empty"><i class="ti ti-chart-bar-off" aria-hidden="true"></i>${escape(empty)}</p>`}</figure>`;
}

export function flowSummary(title, steps, caption = "") {
  return `<div class="insight-flow"><div class="insight-flow-heading"><h4>${escape(title)}</h4>${help(title, caption)}${caption ? `<span class="sr-only">${escape(caption)}</span>` : ""}</div><ol>${steps.map((step, index) => `<li><span class="insight-flow-symbol" aria-hidden="true"><i class="ti ti-${icon(step.icon)}"></i></span><span><strong>${escape(step.label)}</strong>${step.value != null ? `<small>${escape(valueText(step.value))}</small>` : ""}</span>${index < steps.length - 1 ? '<i class="ti ti-chevron-right insight-flow-arrow" aria-hidden="true"></i>' : ""}</li>`).join("")}</ol></div>`;
}
