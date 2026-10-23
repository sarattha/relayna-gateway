// Inventory summaries describe saved configuration, never live availability.
export function catalogCredentialState(row) {
  if (row.provider === "azure-foundry" && ["workload_identity", "managed_identity"].includes(row.foundry?.method)) {
    return { label: "Azure identity", tone: "neutral", configured: true };
  }
  if (row.credential_configured === true) return { label: "configured", tone: "good", configured: true };
  if (row.credential_configured === false) return { label: "missing", tone: "bad", configured: false };
  return { label: "unknown", tone: "neutral", configured: null };
}

// Match the registered-service transport contract, including Accessa's exact
// run path. Disabled registrations can still have WebSocket configured.
export function catalogServiceTransport(service) {
  if (!Array.isArray(service.allowed_methods)) return "unknown";
  if (service.foundry || !service.allowed_methods.includes("GET")) return "http";
  const accessa = service.access?.accessa;
  if (accessa) {
    return accessa.app && service.route_pattern === `/app/${accessa.app}/channel/${accessa.channel}/v1/*`
      ? "accessa-websocket" : "http";
  }
  return service.access?.websocket ? "websocket" : "http";
}

export function projectCatalogInsights(projects, services) {
  const unknownLinks = projects.some(project => !Array.isArray(project.service_names));
  const links = new Set(projects.flatMap(project => Array.isArray(project.service_names) ? project.service_names : []));
  const registered = new Map(services.map(service => [service.name, service]));
  const linkedServices = [...links].map(name => registered.get(name));
  const unknownEnablement = linkedServices.some(service => !service || typeof service.enabled !== "boolean");
  const unresolved = linkedServices.filter(service => !service).length;
  return {
    items: [
      { label: "Distinct linked services", value: unknownLinks ? null : links.size, icon: "apps", detail: unknownLinks ? "Some project service selections are unavailable" : "Saved project service selections" },
      { label: "Enabled linked services", value: unknownLinks || unknownEnablement ? null : linkedServices.filter(service => service.enabled).length, icon: "circle-check", detail: unknownLinks || unknownEnablement ? "Some links or registrations are unavailable" : "Configuration state; no live probe" },
      { label: "Links outside inventory", value: unknownLinks ? null : unresolved, icon: "link", tone: unresolved ? "warn" : "neutral", detail: "Distinct names absent from the loaded service list" },
    ],
    distribution: projects.length ? [
      { label: "With service links", value: projects.filter(project => Array.isArray(project.service_names) && project.service_names.length > 0).length, tone: "good" },
      { label: "Without service links", value: projects.filter(project => Array.isArray(project.service_names) && project.service_names.length === 0).length, tone: "neutral" },
      { label: "Links unknown", value: projects.filter(project => !Array.isArray(project.service_names)).length, tone: "neutral" },
    ] : [],
    caption: "Projects in the current scope. Service links govern project-owned keys; they do not measure traffic.",
  };
}

export function providerCatalogInsights(providers, mappings, passthrough) {
  const configuredMappings = scope => {
    const scoped = mappings.filter(mapping => mapping.scope === scope);
    return scoped.some(mapping => typeof mapping.enabled !== "boolean" || (mapping.enabled && typeof mapping.credential_configured !== "boolean"))
      ? null : scoped.filter(mapping => mapping.enabled && mapping.credential_configured).length;
  };
  const classified = providers.map(provider => ({ enabled: provider.enabled, credential: catalogCredentialState(provider).configured }));
  return {
    items: [
      { label: "Key credential mappings", value: configuredMappings("key"), icon: "key", detail: "Enabled mappings with a stored credential" },
      { label: "Project credential mappings", value: configuredMappings("project"), icon: "folder", detail: "Enabled mappings with a stored credential" },
      { label: "Wildcard ingress", value: passthrough?.enabled === true ? "Enabled" : passthrough?.enabled === false ? "Disabled" : null, icon: "arrows-split", tone: passthrough?.enabled === true ? "warn" : "neutral", detail: passthrough?.enabled === true ? (passthrough.authentication_mode === "litellm_bearer" ? "Caller LiteLLM key · allowlists still apply" : "Gateway authentication · allowlists still apply") : "Fallback forwarding configuration" },
    ],
    distribution: providers.length ? [
      { label: "Enabled · auth configured", value: classified.filter(row => row.enabled === true && row.credential === true).length, tone: "good" },
      { label: "Enabled · credential missing", value: classified.filter(row => row.enabled === true && row.credential === false).length, tone: "bad" },
      { label: "Disabled", value: classified.filter(row => row.enabled === false).length, tone: "bad" },
      { label: "Configuration unknown", value: classified.filter(row => typeof row.enabled !== "boolean" || (row.enabled === true && row.credential === null)).length, tone: "neutral" },
    ] : [],
    caption: "Saved provider setup. Azure identity configuration and stored credentials do not confirm a working connection.",
  };
}

export function serviceCatalogInsights(services, projects) {
  const linkedNames = new Set(projects.flatMap(project => Array.isArray(project.service_names) ? project.service_names : []));
  const transport = services.map(catalogServiceTransport);
  const unknownProjects = projects.some(project => !Array.isArray(project.service_names));
  const reviewStates = ["incomplete", "stale", "failed"];
  return {
    items: [
      { label: "Enabled registrations", value: services.some(service => typeof service.enabled !== "boolean") ? null : `${services.filter(service => service.enabled).length} / ${services.length}`, icon: "circle-check", detail: "Saved enablement; availability is shown in Health" },
      { label: "Linked from projects", value: unknownProjects ? null : services.filter(service => linkedNames.has(service.name)).length, icon: "folders", detail: unknownProjects ? "Some project service selections are unavailable" : "Services selected by at least one project" },
      { label: "WebSocket configured", value: transport.includes("unknown") ? null : transport.filter(protocol => protocol === "websocket" || protocol === "accessa-websocket").length, icon: "plug-connected", detail: "Includes Accessa run sockets; SSE uses HTTP" },
    ],
    distribution: services.length ? [
      { label: "Local", value: services.filter(service => service.sync_status === "local").length, tone: "neutral" },
      { label: "Synced", value: services.filter(service => service.sync_status === "synced").length, tone: "good" },
      { label: "Review sync", value: services.filter(service => reviewStates.includes(service.sync_status)).length, tone: "warn", detail: "Incomplete, stale or failed" },
      { label: "Sync unknown", value: services.filter(service => !["local", "synced", ...reviewStates].includes(service.sync_status)).length, tone: "neutral" },
    ] : [],
    caption: "Saved registration sync state, independent of upstream health and enabled state.",
  };
}

export function routeCatalogInsights(openaiRoutes, anthropicRoutes, services, identities) {
  const canonical = [...openaiRoutes, ...anthropicRoutes];
  const transport = services.map(catalogServiceTransport);
  return {
    items: [
      { label: "Registered service routes", value: services.length, icon: "apps", detail: "Matched before canonical provider APIs" },
      { label: "Endpoint identity settings", value: identities.length, icon: "shield-lock", detail: "Canonical and additional route identity records" },
      { label: "WebSocket configured", value: transport.includes("unknown") ? null : transport.filter(protocol => protocol === "websocket" || protocol === "accessa-websocket").length + identities.filter(identity => identity.access?.websocket).length, icon: "plug-connected", detail: "Saved opt-ins; methods, forwarding mode and identity still apply" },
    ],
    distribution: canonical.length ? [
      { label: "Enabled · Gateway managed", value: canonical.filter(route => route.enabled === true && route.mode === "managed_by_gateway").length, tone: "good" },
      { label: "Enabled · direct LiteLLM", value: canonical.filter(route => route.enabled === true && route.mode === "direct_litellm_passthrough").length, tone: "warn" },
      { label: "Disabled", value: canonical.filter(route => route.enabled === false).length, tone: "bad" },
      { label: "Configuration unknown", value: canonical.filter(route => typeof route.enabled !== "boolean" || (route.enabled === true && !["managed_by_gateway", "direct_litellm_passthrough"].includes(route.mode))).length, tone: "neutral" },
    ] : [],
    caption: "Canonical OpenAI-compatible and Anthropic route settings. Aliases share their canonical policy.",
    steps: [
      { label: "Control plane", icon: "lock" },
      { label: "Registered services", icon: "apps", value: services.length },
      { label: "Canonical APIs", icon: "route", value: canonical.length },
      { label: "LiteLLM fallback", icon: "arrows-split" },
    ],
  };
}
