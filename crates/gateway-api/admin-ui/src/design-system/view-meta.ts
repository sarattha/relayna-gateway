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
  | "settings"
  | "members"
  | "managed-identities"
  | "my-services"
  | "service-dashboard"
  | "my-projects"
  | "project-dashboard";

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
  members: {
    title: "Members",
    domain: "Govern",
    summary: "Approve portal members and assign exact Owner or Viewer service access.",
  },
  "managed-identities": {
    title: "Managed identities",
    domain: "Govern",
    summary: "Bind audience- and app-role-authorized workloads to exact registered services.",
  },
  "my-services": {
    title: "My services",
    domain: "Discover",
    summary: "Registered services for which you have Owner or Viewer access.",
  },
  "service-dashboard": {
    title: "Service dashboard",
    domain: "Monitor",
    summary: "Scoped usage, errors, request logs, endpoints, providers, and models for one service.",
  },
  "my-projects": {
    title: "My projects",
    domain: "Discover",
    summary: "Projects for which you have Owner or Viewer access.",
  },
  "project-dashboard": {
    title: "Project dashboard",
    domain: "Monitor",
    summary: "Scoped usage, errors, request logs, services, endpoints, and providers for one project.",
  },
};

export function metaForView(view: string): ViewMeta {
  return viewMeta[view as ViewId] ?? {
    title: view ? view[0].toUpperCase() + view.slice(1) : "Overview",
    domain: "Monitor",
    summary: "Gateway operator workflow.",
  };
}
