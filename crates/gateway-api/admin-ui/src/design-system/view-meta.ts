export type ViewId =
  | "overview"
  | "health"
  | "usage"
  | "providers"
  | "services"
  | "routes"
  | "projects"
  | "keys"
  | "guardrails"
  | "audit"
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
    summary: "Cost, tokens, denials, fallbacks, guardrail blocks, task drilldowns, and exports.",
  },
  providers: {
    title: "Providers",
    domain: "Discover",
    summary: "Upstream provider configuration, write-only credentials, and enabled state.",
  },
  services: {
    title: "Services",
    domain: "Discover",
    summary: "Relayna service catalog, route patterns, Studio imports, sync, and lifecycle controls.",
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
  audit: {
    title: "Audit",
    domain: "Govern",
    summary: "Operator actions, request metadata, targets, and redacted change snapshots.",
  },
  settings: {
    title: "Settings",
    domain: "Govern",
    summary: "Studio connection settings, integration token controls, and release posture references.",
  },
};

export function metaForView(view: string): ViewMeta {
  return viewMeta[view as ViewId] ?? {
    title: view ? view[0].toUpperCase() + view.slice(1) : "Overview",
    domain: "Monitor",
    summary: "Gateway operator workflow.",
  };
}
