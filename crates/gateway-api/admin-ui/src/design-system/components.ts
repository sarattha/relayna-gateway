export type Tone = "good" | "bad" | "warn" | "neutral";

export function toneForStatus(value: unknown): Tone {
  const status = String(value ?? "").toLowerCase();
  if (["ready", "ok", "healthy", "enabled", "configured", "active", "success", "non-expiring", "closed"].includes(status)) {
    return "good";
  }
  if (["missing", "failed", "failure", "disabled", "revoked", "expired", "timeout", "unhealthy", "open"].includes(status)) {
    return "bad";
  }
  if (["unknown", "pending", "fallback", "opt-in", "default", "degraded", "half_open", "half-open"].includes(status)) {
    return "warn";
  }
  return "neutral";
}

export function html(value: unknown): string {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function badge(label: unknown, tone: Tone = toneForStatus(label)): string {
  return `<span class="badge ${tone}">${html(label ?? "unknown")}</span>`;
}

export function panel(title: string, body: string, meta = ""): string {
  const heading = title
    ? `<div class="panel-heading"><h3>${html(title)}</h3>${meta ? `<span class="subtle">${html(meta)}</span>` : ""}</div>`
    : "";
  return `<section class="panel">${heading}${body}</section>`;
}

export function metricTile(label: string, value: unknown, tone: Tone = "neutral"): string {
  return `<section class="panel stat ${tone}"><span>${html(label)}</span><strong>${html(value)}</strong></section>`;
}

export function emptyState(message: string): string {
  return `<div class="empty-state"><p>${html(message)}</p></div>`;
}

export function tableWrap(tableMarkup: string): string {
  return `<div class="table-wrap">${tableMarkup}</div>`;
}

export function actionGroup(actions: string): string {
  return `<div class="form-actions">${actions}</div>`;
}

export function jsonBlock(value: unknown): string {
  return `<pre>${html(JSON.stringify(value ?? null, null, 2))}</pre>`;
}

export function modalShell(title: string, body: string, actions: string): string {
  return `
    <section class="modal-backdrop">
      <div class="modal">
        <h3>${html(title)}</h3>
        ${body}
        ${actionGroup(actions)}
      </div>
    </section>
  `;
}

export function noticeText(message: string, tone: Tone = "neutral"): string {
  return `<div class="notice inline" data-kind="${tone}">${html(message)}</div>`;
}
