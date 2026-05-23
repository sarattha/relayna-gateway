export type ViewId =
  | "overview"
  | "providers"
  | "services"
  | "routes"
  | "projects"
  | "keys"
  | "guardrails"
  | "usage"
  | "health"
  | "settings";

export type ViewMeta = {
  title: string;
  domain: "Monitor" | "Discover" | "Govern";
  summary: string;
};

export const viewMeta: Record<ViewId, ViewMeta> = {
  overview: {
    title: "Overview",
    domain: "Monitor",
    summary: "Gateway posture, traffic, and service availability.",
  },
  health: {
    title: "Health",
    domain: "Monitor",
    summary: "Provider checks, circuit state, import versions, and debug bundles.",
  },
  usage: {
    title: "Usage",
    domain: "Monitor",
    summary: "Cost, tokens, denials, fallbacks, guardrail blocks, and exportable usage rows.",
  },
  providers: {
    title: "Providers",
    domain: "Discover",
    summary: "Upstream provider configuration, write-only credentials, and enabled state.",
  },
  services: {
    title: "Services",
    domain: "Discover",
    summary: "Relayna service catalog, route patterns, Studio imports, and lifecycle controls.",
  },
  routes: {
    title: "Routes",
    domain: "Discover",
    summary: "OpenAI-compatible routes and registered service route exposure.",
  },
  projects: {
    title: "Projects",
    domain: "Discover",
    summary: "Project ownership and service access boundaries for virtual keys.",
  },
  keys: {
    title: "Keys",
    domain: "Govern",
    summary: "Virtual key lifecycle, policy layers, simulations, scopes, and guardrail policy.",
  },
  guardrails: {
    title: "Guardrails",
    domain: "Govern",
    summary: "Catalog controls, execution summaries, and sanitized guardrail events.",
  },
  settings: {
    title: "Settings",
    domain: "Govern",
    summary: "Studio connection settings and write-only integration token controls.",
  },
};

export function metaForView(view: string): ViewMeta {
  return viewMeta[view as ViewId] ?? {
    title: view ? view[0].toUpperCase() + view.slice(1) : "Overview",
    domain: "Monitor",
    summary: "Gateway operator workflow.",
  };
}

export function toneForStatus(value: unknown): "good" | "bad" | "warn" | "neutral" {
  const status = String(value ?? "").toLowerCase();
  if (["ready", "ok", "healthy", "enabled", "configured", "active", "success", "non-expiring"].includes(status)) {
    return "good";
  }
  if (["missing", "failed", "failure", "disabled", "revoked", "expired", "timeout", "degraded"].includes(status)) {
    return "bad";
  }
  if (["unknown", "pending", "fallback", "opt-in", "default"].includes(status)) {
    return "warn";
  }
  return "neutral";
}

export function applyViewChrome(view: string): void {
  const meta = metaForView(view);
  document.documentElement.dataset.viewDomain = meta.domain.toLowerCase();
  document.querySelector("#view-title")!.textContent = meta.title;
  const domain = document.querySelector("#view-domain");
  if (domain) domain.textContent = meta.domain;
  const summary = document.querySelector("#view-summary");
  if (summary) summary.textContent = meta.summary;
}
