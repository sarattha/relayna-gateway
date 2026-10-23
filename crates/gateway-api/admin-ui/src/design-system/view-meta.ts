export type ViewId =
  | "overview"
  | "health"
  | "traffic"
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
    summary: "See gateway demand, operational health, and what needs attention.",
  },
  traffic: {
    title: "Traffic",
    domain: "Monitor",
    summary: "Inspect live sessions and recent requests across your gateway.",
  },
  health: {
    title: "Health",
    domain: "Monitor",
    summary: "Understand gateway demand, capacity, latency, and service health.",
  },
  usage: {
    title: "Usage & cost",
    domain: "Monitor",
    summary: "Explore request trends, costs, and the projects driving them.",
  },
  providers: {
    title: "Providers",
    domain: "Discover",
    summary: "Review provider connections and authentication coverage.",
  },
  services: {
    title: "Services",
    domain: "Discover",
    summary: "Explore registered services, transport options, and project coverage.",
  },
  routes: {
    title: "Routes",
    domain: "Discover",
    summary: "Understand route families, forwarding modes, and service exposure.",
  },
  projects: {
    title: "Projects",
    domain: "Discover",
    summary: "Manage project boundaries and the services they can access.",
  },
  keys: {
    title: "Virtual keys",
    domain: "Govern",
    summary: "Review key lifecycle, access policies, and safeguards.",
  },
  guardrails: {
    title: "Policies & guardrails",
    domain: "Govern",
    summary: "Understand safeguard coverage and recent guardrail decisions.",
  },
  audit: {
    title: "Audit log",
    domain: "Govern",
    summary: "Trace changes, actors, and related requests.",
  },
  settings: {
    title: "Settings",
    domain: "Govern",
    summary: "Review integrations, identity trust, and gateway configuration.",
  },
  members: {
    title: "People & identities",
    domain: "Govern",
    summary: "Review approvals and manage exact resource access.",
  },
  "managed-identities": {
    title: "Managed identities",
    domain: "Govern",
    summary: "Review workload principals and their exact resource bindings.",
  },
  "my-services": {
    title: "My services",
    domain: "Discover",
    summary: "Explore the services assigned to you and their configurations.",
  },
  "service-dashboard": {
    title: "Service dashboard",
    domain: "Monitor",
    summary: "Understand demand, incidents, and requests for this service.",
  },
  "my-projects": {
    title: "My projects",
    domain: "Discover",
    summary: "Explore your project assignments and service relationships.",
  },
  "project-dashboard": {
    title: "Project dashboard",
    domain: "Monitor",
    summary: "Understand demand, costs, and requests recorded for this project.",
  },
};

export function metaForView(view: string): ViewMeta {
  return viewMeta[view as ViewId] ?? {
    title: view ? view[0].toUpperCase() + view.slice(1) : "Overview",
    domain: "Monitor",
    summary: "Gateway operator workflow.",
  };
}
