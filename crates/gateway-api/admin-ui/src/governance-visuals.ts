import { keyLifecycle } from "./monitoring";

// Configuration and returned samples only. These summaries do not infer request
// availability, effective policy, or activity outside the responses on the page.
export function keyGovernanceSnapshot(keys, layers, now = Date.now()) {
  const states = keys.map(key => keyLifecycle(key, now));
  const active = keys.filter((key, index) => ["active", "non-expiring"].includes(states[index]));
  const rotationDue = active.filter(key => key.rotation_due_at && Date.parse(key.rotation_due_at) <= now).length;
  const explicitGuardrails = keys.filter(key => ["mandatory_guardrails", "optional_guardrails", "forbidden_guardrails"].some(field => key.guardrail_policy?.[field]?.length)).length;
  return {
    insights: [
      { label: "Active lifecycle", value: active.length, icon: "key", tone: "good", detail: "Enabled, unrevoked and unexpired; policy still applies" },
      { label: "Rotation due", value: rotationDue, icon: "rotate-clockwise", tone: rotationDue ? "warn" : "neutral", detail: "Active keys past their configured rotation date" },
      { label: "Explicit guardrail rules", value: `${explicitGuardrails} / ${keys.length}`, icon: "shield-check", detail: "Direct key rules; inherited rules may add safeguards" },
    ],
    lifecycle: [
      ["active", "Expiring active", "good"], ["non-expiring", "Non-expiring", "good"],
      ["expired", "Expired", "bad"], ["disabled", "Disabled", "bad"], ["revoked", "Revoked", "bad"],
    ].map(([state, label, tone]) => ({ label, value: states.filter(value => value === state).length, tone })),
    layers: ["global", "project", "team", "route", "model"].map(kind => ({
      label: `${kind[0].toUpperCase()}${kind.slice(1)}`, icon: kind === "global" ? "world" : kind === "project" ? "folder" : kind === "team" ? "users" : kind === "route" ? "route" : "cpu",
      value: layers.filter(layer => layer.kind === kind).length,
    })),
  };
}

export function guardrailGovernanceSnapshot(catalog, executions) {
  const enabled = catalog.filter(row => row.enabled);
  return {
    insights: [
      { label: "Enabled safeguards", value: `${enabled.length} / ${catalog.length}`, icon: "shield-check", detail: "Catalog configuration; key policy determines application" },
      { label: "Enabled defaults", value: enabled.filter(row => row.default_on).length, icon: "adjustments", detail: "Default-on definitions, when allowed by key policy" },
      { label: "Streaming capable", value: enabled.filter(row => row.modes?.includes("during_call")).length, icon: "wave-sine", detail: "Enabled definitions supporting during-call checks" },
    ],
    actions: ["allow", "modify", "warn", "block"].map(action => ({
      label: action[0].toUpperCase() + action.slice(1), value: executions.filter(row => row.action === action).length,
      tone: action === "allow" ? "good" : action === "block" || action === "warn" ? "warn" : "neutral",
    })),
    stages: [
      { label: "Before provider", icon: "arrow-right", value: enabled.filter(row => row.modes?.includes("pre_call")).length },
      { label: "After provider", icon: "arrow-back", value: enabled.filter(row => row.modes?.includes("post_call")).length },
      { label: "During stream", icon: "wave-sine", value: enabled.filter(row => row.modes?.includes("during_call")).length },
    ],
  };
}

export function memberAccessSnapshot(items) {
  const active = items.filter(item => item.member.status === "active");
  const serviceAssignments = items.reduce((total, item) => total + (item.service_memberships?.length || 0), 0);
  const projectAssignments = items.reduce((total, item) => total + (item.project_memberships?.length || 0), 0);
  return {
    insights: [
      { label: "Awaiting approval", value: items.filter(item => item.member.status === "pending").length, icon: "clock", tone: "warn", detail: "Sign-in alone does not grant portal access" },
      { label: "Active administrators", value: active.filter(item => item.member.roles?.includes("admin")).length, icon: "user-shield", detail: "Active members with the administrator role" },
      { label: "Exact assignments", value: serviceAssignments + projectAssignments, icon: "hierarchy", detail: `${serviceAssignments} service · ${projectAssignments} project memberships, including inactive members` },
    ],
    statuses: [
      { label: "Active", value: active.length, tone: "good" },
      { label: "Pending", value: items.filter(item => item.member.status === "pending").length, tone: "warn" },
      { label: "Blocked", value: items.filter(item => item.member.status === "blocked").length, tone: "bad" },
    ],
  };
}

export function managedIdentitySnapshot(services, projects) {
  const bindings = [...services, ...projects];
  const enabled = bindings.filter(row => row.enabled);
  return {
    insights: [
      { label: "Enabled bindings", value: `${enabled.length} / ${bindings.length}`, icon: "fingerprint", detail: "Exact resource bindings; token trust checks still apply" },
      { label: "Workload principals", value: new Set(bindings.map(row => JSON.stringify([row.tenant_id, row.client_id]))).size, icon: "users", detail: "Distinct tenant / client pairs across all bindings" },
      { label: "Object-restricted", value: bindings.filter(row => row.object_id).length, icon: "lock", detail: "Bindings requiring an additional exact object ID match" },
    ],
    resources: [
      { label: "Enabled service", value: services.filter(row => row.enabled).length, tone: "good" },
      { label: "Disabled service", value: services.filter(row => !row.enabled).length, tone: "bad" },
      { label: "Enabled project", value: projects.filter(row => row.enabled).length, tone: "good" },
      { label: "Disabled project", value: projects.filter(row => !row.enabled).length, tone: "bad" },
    ],
  };
}

export function auditSampleSnapshot(rows) {
  const actions = new Map();
  for (const row of rows) actions.set(row.action || "Unspecified", (actions.get(row.action || "Unspecified") || 0) + 1);
  const ranked = [...actions].sort(([leftLabel, left], [rightLabel, right]) => right - left || leftLabel.localeCompare(rightLabel));
  const visible = ranked.slice(0, 5).map(([label, value]) => ({ label, value }));
  if (ranked.length > 5) visible.push({ label: "Other actions", value: ranked.slice(5).reduce((total, [, value]) => total + value, 0) });
  return {
    insights: [
      { label: "Returned events", value: rows.length, icon: "list-details", detail: "Loaded sample; filters and limit apply" },
      { label: "Distinct actors", value: new Set(rows.map(row => row.actor_member_id ? `member:${row.actor_member_id}` : row.actor_token_id ? `token:${row.actor_token_id}` : "system")).size, icon: "users", detail: "Members, operator tokens and system actors in sample" },
      { label: "Request correlation", value: `${rows.filter(row => row.request_id).length} / ${rows.length}`, icon: "link", detail: "Returned events with a recorded request ID" },
    ],
    actions: visible,
  };
}

export function settingsGovernanceSnapshot(studio, auth) {
  return {
    insights: [
      { label: "Studio origin", value: studio.base_url ? "Configured" : "Unset", icon: "plug-connected", detail: `Source: ${studio.source || "unknown"} · use Test connection to verify` },
      { label: "Studio token", value: studio.token_configured ? "Configured" : "Not configured", icon: "lock", detail: "Optional write-only credential; value is never returned" },
      { label: "Entra verification", value: auth.unverified_bearer_enabled ? "Paused" : auth.entra.enabled ? "Enabled" : "Disabled", icon: "shield-check", tone: auth.unverified_bearer_enabled ? "warn" : "neutral", detail: `Configured state · source: ${auth.source || "unknown"}` },
      { label: "Apigee trust", value: auth.apigee.trusted_header_enabled ? "Enabled" : "Disabled", icon: "route", detail: auth.apigee.secret_configured ? "Shared secret configured" : "Shared secret not configured" },
    ],
  };
}
