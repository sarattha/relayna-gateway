import { mountProviderCreate } from "./react/provider-create";
import { mountFoundryCheck } from "./react/foundry-check";
import { mountFoundryEditor } from "./react/foundry-editor";
import { mountRouteConfiguration } from "./react/route-configuration";
import { mountKeyRouteBindings } from "./react/key-route-bindings";
import { mountIdentityEditor } from "./react/mount-identity-editor";
import { mountUserTokenForwarding } from "./react/user-token-forwarding";
import { profileFields, profilesFromForm, showProfileError, profileKeyOptions } from "./auth-profiles";
import { keyLifecycle, reliability, fetchComplete } from "./monitoring";
import { mountTraffic, trafficDuration } from "./traffic";
import { mountHealthGatewayStatus, mountTrafficGatewayStatus } from "./gateway-status";
import { chartColor, installChartTheme, mountMonitorChart, usageChartsShell, usageRanking, usageSummaryView, usageTrendConfigs } from "./monitoring-visuals";
import { catalogCredentialState, catalogServiceTransport, projectCatalogInsights, providerCatalogInsights, serviceCatalogInsights, routeCatalogInsights } from "./catalog-visuals";
import { keyGovernanceSnapshot, guardrailGovernanceSnapshot, memberAccessSnapshot, managedIdentitySnapshot, auditSampleSnapshot, settingsGovernanceSnapshot } from "./governance-visuals";
import { ownerCatalogInsights, ownerDashboardInsights, ownerUsageDistribution, ownerRequestProfile, ownerIncidentSeries, ownerIncidentDescription } from "./owner-visuals";
import { installComponentGuidance } from "./design-system/guidance";
import { refreshWebSocketMetrics, routingModeLabel, usageValue, requestInvestigationView, bindInvestigationActions, matchTrafficRecord, investigationUsageSnapshot } from "./investigation";
import "@tabler/icons-webfont/dist/tabler-icons.min.css";
import Chart from "chart.js/auto";

import {
  actionGroup,
  applyViewChrome,
  auditLogTemplate,
  badge,
  emptyState,
  importDiffTemplate,
  jsonBlock,
  metricTile,
  panel,
  insightStrip,
  distributionBars,
  flowSummary,
  tableWrap,
  viewMeta,
} from "./design-system";

const tokenKey = "relayna_gateway_operator_token";
installChartTheme(Chart);
installComponentGuidance();
const viewIds = new Set(Object.keys(viewMeta));
const ownerViewIds = new Set(["my-services", "service-dashboard", "my-projects", "project-dashboard"]);
const state = {
  view: "overview",
  keys: [],
  projects: [],
  providers: [],
  litellmCredentialMappings: [],
  litellmPassthroughSettings: null,
  openaiRoutes: [],
  anthropicRoutes: [],
  services: [],
  guardrails: [],
  guardrailExecutions: [],
  guardrailSummary: [],
  studioServices: [],
  studioConnection: null,
  authSettings: null,
  policySimulation: null,
  policyLayers: [],
  providerHealthState: [],
  serviceImportVersions: [],
  auditEvents: [],
  debugBundle: null,
  editingKeyId: null,
  editingServiceName: null,
  openapiPreviews: {},
  editingGuardrailName: null,
  usagePagination: {
    eventsOffset: 0,
    timeseriesOffset: 0,
    serviceTimeseriesOffset: 0,
  },
  overviewWindow: "7d",
  projectScope: "",
  usageFilters: {},
  usageTab: "Recent requests",
  trafficFilters: {},
  session: null,
  authConfig: null,
  workspace: "admin",
  ownerServices: [],
  selectedOwnerService: null,
  ownerProjects: [],
  selectedOwnerProject: null,
  ownerDashboardFilters: {
    range: "7d",
    outcome: "all",
    statusCode: "",
    offset: 0,
    limit: 20,
  },
  members: [],
  managedIdentities: [],
  managedIdentityProjects: [],
};

const login = document.querySelector("#login");
const app = document.querySelector("#app");
const content = document.querySelector("#content");
const requestTimeoutMs = 8000;
let noticeTimer: ReturnType<typeof setTimeout> | null = null;
let dialogCounter = 0;
const dialogInertRoots = new Map();
let overviewChart: Chart | null = null;
let ownerDashboardChart: Chart | null = null;
let ownerDashboardGeneration = 0;
let stopTraffic = null;
let stopGatewayStatus = null;
let viewGeneration = 0;
let viewController = new AbortController();
let usageGeneration = 0;
let usageLoad = null;
let usageLabelCache = null;
let renderGeneration = 0;
let pendingWrites = 0;

function token() {
  return sessionStorage.getItem(tokenKey);
}

function setNotice(message, kind = "error") {
  document.querySelector(".message-box")?.remove();
  if (noticeTimer) {
    clearTimeout(noticeTimer);
    noticeTimer = null;
  }
  if (!message) return;

  const tone = kind === "success" ? "success" : "error";
  const delay = tone === "success" ? 4000 : 9000;
  const box = document.createElement("section");
  box.className = "message-box";
  box.dataset.kind = tone;
  box.setAttribute("role", "alert");
  box.setAttribute("aria-live", "polite");
  box.innerHTML = `
    <div>
      <h3>${tone === "success" ? "Success" : "Message"}</h3>
      <p>${esc(message)}</p>
    </div>
    <button type="button" data-close-message>Close</button>
  `;
  const dismiss = () => {
    if (noticeTimer) {
      clearTimeout(noticeTimer);
      noticeTimer = null;
    }
    box.remove();
  };
  const schedule = () => {
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(dismiss, delay);
  };
  box.querySelector("[data-close-message]").addEventListener("click", dismiss);
  box.addEventListener("mouseenter", () => {
    if (noticeTimer) clearTimeout(noticeTimer);
  });
  box.addEventListener("mouseleave", schedule);
  box.addEventListener("focusin", () => {
    if (noticeTimer) clearTimeout(noticeTimer);
  });
  box.addEventListener("focusout", schedule);
  document.body.appendChild(box);
  schedule();
}

function handleAsync(handler) {
  return async (event) => {
    const generation = viewGeneration;
    const pendingRoot = event.currentTarget || event.target;
    const pendingControls = setPending(pendingRoot, true);
    try {
      await handler(event);
    } catch (error) {
      if (generation !== viewGeneration || error.name === "AbortError") return;
      if (!showProfileError(pendingRoot, error.message)) setNotice(error.message);
    } finally {
      setPending(pendingRoot, false, pendingControls);
    }
  };
}

function setPending(root, pending, controls = null) {
  if (!(root instanceof HTMLElement)) return [];
  const targets = controls || (root.matches("button") ? [root] : [...root.querySelectorAll("button")]);
  root.setAttribute("aria-busy", String(pending));
  root.classList.toggle("is-pending", pending);
  targets.forEach((button) => {
    if (!(button instanceof HTMLButtonElement)) return;
    if (pending) {
      button.dataset.pendingDisabled = String(button.disabled);
      button.disabled = true;
    } else {
      button.disabled = button.dataset.pendingDisabled === "true";
      delete button.dataset.pendingDisabled;
    }
  });
  return targets;
}

async function api(path, options = {}) {
  const headers = {
    "content-type": "application/json",
    ...(token() ? { authorization: `Bearer ${token()}` } : {}),
    ...(!token() && state.session?.csrf_token ? { "x-csrf-token": state.session.csrf_token } : {}),
    ...(options.headers || {}),
  };
  const writing = options.method && options.method !== "GET";
  if (writing) pendingWrites += 1;
  let response;
  try { response = await fetchWithTimeout(path, { ...options, headers }); }
  finally { if (writing) pendingWrites -= 1; }
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const body = await response.json();
      const code = body.error?.code;
      const detail = body.error?.message || body.error?.detail;
      message = [code, detail].filter(Boolean).join(": ") || message;
    } catch (_) {}
    throw new Error(message);
  }
  if (options.method && options.method !== "GET") clearDirtyForms();
  if (response.status === 204) return null;
  return response.json();
}

async function json(path, options = {}) {
  const response = await fetchWithTimeout(path, options);
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return response.json();
}

async function fetchWithTimeout(path, options = {}) {
  const generation = viewGeneration;
  const response = await fetchComplete(path, options, {
    timeoutMs: /\/admin-ui\/admin\/(usage\/|provider-health(?:\?|$))/.test(path) ? 30000 : requestTimeoutMs,
    signal: !options.method || options.method === "GET" ? viewController.signal : undefined,
  });
  if (generation !== viewGeneration) throw new DOMException("Request superseded", "AbortError");
  return response;
}

const monitoringCache = new Map();
async function monitoringPanel(path, fallback, issues) {
  const url = new URL(path, location.origin);
  url.searchParams.delete("from"); url.searchParams.delete("to");
  const cacheKey = `${url.pathname}?${url.searchParams}|${state.overviewWindow}`;
  try {
    const data = await api(path);
    monitoringCache.set(cacheKey, { data, at: new Date().toLocaleTimeString() });
    return data;
  } catch (error) {
    if (error.name === "AbortError") throw error;
    const previous = monitoringCache.get(cacheKey);
    issues.push(`${url.pathname.split("/").at(-1)}: ${error.message}${previous ? ` · stale data from ${previous.at}` : " · unavailable"}`);
    return previous?.data ?? fallback;
  }
}
function monitoringIssues(issues) {
  return issues.length ? `<div class="notice" role="status"><strong>Some sources could not refresh.</strong><ul>${issues.map((issue) => `<li>${esc(issue)}</li>`).join("")}</ul><button type="button" data-retry-monitoring>Retry sources</button></div>` : "";
}

async function readiness() {
  try {
    const response = await fetchWithTimeout("/admin-ui/readyz");
    const body = await response.json();
    if (response.ok || (response.status === 503 && body.status === "not_ready")) return body;
    throw new Error(`Readiness HTTP ${response.status}`);
  } catch (error) {
    if (error.name === "AbortError") throw error;
    return { status: "unavailable", detail: error.message };
  }
}

function showRawToken(rawToken, label = "Token shown once") {
  const template = document.querySelector("#raw-token-template");
  const node = template.content.cloneNode(true);
  node.querySelector("h3").textContent = label;
  node.querySelector("textarea").value = rawToken;
  document.body.appendChild(node);
  const backdrop = document.body.lastElementChild;
  const close = mountDialog(backdrop, { initialFocus: "[data-copy-token]", dismissible: false });
  backdrop.querySelector("[data-copy-token]").addEventListener("click", async () => {
    await navigator.clipboard.writeText(rawToken);
    setNotice("Token copied. Store it in your secret manager now.", "success");
  });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
}

function showTextModal(titleText, value) {
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `
    <div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">${esc(titleText)}</h3>
      <textarea readonly rows="18" aria-label="${attr(titleText)}">${esc(value)}</textarea>
      ${actionGroup('<button type="button" data-close-modal>Close</button>')}
    </div>
  `;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
}

function confirmAction(titleText, bodyText) {
  return new Promise((resolve) => {
    const backdrop = document.createElement("section");
    backdrop.className = "modal-backdrop";
    const titleId = `dialog-title-${++dialogCounter}`;
    backdrop.innerHTML = `
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
        <h3 id="${titleId}">${esc(titleText)}</h3>
        <p>${esc(bodyText)}</p>
        <div class="form-actions">
          <button type="button" class="danger" data-confirm-yes>Confirm</button>
          <button type="button" data-confirm-no>Cancel</button>
        </div>
      </div>
    `;
    document.body.appendChild(backdrop);
    const close = mountDialog(backdrop, { initialFocus: "[data-confirm-no]", onClose: resolve });
    backdrop.querySelector("[data-confirm-yes]").addEventListener("click", () => close(true));
    backdrop.querySelector("[data-confirm-no]").addEventListener("click", () => close(false));
  });
}

function mountDialog(backdrop, { initialFocus = "button", onClose = () => {}, restoreFocus = null, dismissible = true } = {}) {
  const dialog = backdrop?.querySelector('[role="dialog"]');
  if (!(backdrop instanceof HTMLElement) || !(dialog instanceof HTMLElement)) {
    return () => {};
  }
  const previousFocus = restoreFocus instanceof HTMLElement
    ? restoreFocus
    : document.activeElement instanceof HTMLElement ? document.activeElement : null;
  for (const node of document.body.children) {
    if (!(node instanceof HTMLElement) || node === backdrop) continue;
    if (!node.matches(".modal-backdrop") && !dialogInertRoots.has(node)) dialogInertRoots.set(node, node.inert);
    node.inert = true;
  }
  let closed = false;
  const focusableSelector = [
    "button:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    'a[href]',
    "summary",
    '[tabindex]:not([tabindex="-1"])',
  ].join(",");
  const close = (value) => {
    if (closed) return;
    closed = true;
    backdrop.removeEventListener("keydown", onKeyDown);
    backdrop.remove();
    const top = [...document.querySelectorAll(".modal-backdrop")].at(-1);
    for (const node of document.body.children) {
      if (!(node instanceof HTMLElement)) continue;
      if (top) node.inert = node !== top;
      else if (dialogInertRoots.has(node)) node.inert = dialogInertRoots.get(node);
    }
    if (!top) dialogInertRoots.clear();
    const focusTarget = typeof restoreFocus === "function" ? restoreFocus() : previousFocus;
    if (focusTarget?.isConnected) focusTarget.focus();
    onClose(value);
  };
  const onKeyDown = (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (dismissible) close(false);
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [...dialog.querySelectorAll(focusableSelector)].filter((item) => item instanceof HTMLElement && item.getClientRects().length > 0);
    if (!focusable.length) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };
  backdrop.addEventListener("keydown", onKeyDown);
  backdrop.addEventListener("click", (event) => {
    if (dismissible && event.target === backdrop) close(false);
  });
  // Generic async workflows must not dismiss a newer show-once secret.
  backdrop.closeDialog = (value) => { if (dismissible) close(value); };
  queueMicrotask(() => {
    const target = dialog.querySelector(initialFocus) || dialog.querySelector(focusableSelector) || dialog;
    if (target instanceof HTMLElement) {
      if (target === dialog) target.tabIndex = -1;
      target.focus();
    }
  });
  return close;
}

function closeTopDialog(value = false) {
  const backdrops = document.querySelectorAll(".modal-backdrop");
  const backdrop = backdrops[backdrops.length - 1];
  if (typeof backdrop?.closeDialog === "function") backdrop.closeDialog(value);
  else backdrop?.remove();
}

function signedIn() {
  login.classList.add("hidden");
  app.classList.remove("hidden");
  configurePortalShell();
  restoreMonitoringHash();
  state.view = viewFromHash();
  syncNavigation();
  refresh();
}

function viewFromHash() {
  const value = location.hash.replace(/^#\/?/, "").split("?")[0];
  if (viewIds?.has(value) && viewAllowed(value)) return value;
  return state.workspace === "owner" ? ownerLandingView() : "overview";
}

function isAdmin() {
  return Boolean(token() || state.session?.member?.roles?.includes("admin"));
}

function hasOwnerAccess() {
  return Boolean(state.session?.service_memberships?.length || state.session?.project_memberships?.length);
}

function ownerLandingView() {
  return state.session?.service_memberships?.length ? "my-services" : "my-projects";
}

function viewAllowed(view) {
  if (ownerViewIds.has(view)) return hasOwnerAccess();
  return isAdmin();
}

function commandViewsForWorkspace() {
  return Object.entries(viewMeta).filter(([view]) => {
    if (state.workspace === "owner") return ownerViewIds.has(view) && hasOwnerAccess();
    return !ownerViewIds.has(view) && isAdmin();
  });
}

function configurePortalShell() {
  const admin = isAdmin();
  const owner = hasOwnerAccess();
  document.querySelectorAll("[data-admin-nav]").forEach((item) => item.classList.toggle("hidden", !admin || state.workspace !== "admin"));
  document.querySelectorAll("[data-owner-nav]").forEach((item) => item.classList.toggle("hidden", !owner || state.workspace !== "owner"));
  document.querySelector("[data-break-glass-only]")?.classList.toggle("hidden", !token());
  document.querySelector("#governed-change-trigger")?.classList.toggle("hidden", !admin || state.workspace !== "admin");
  const workspaceControl = document.querySelector("#workspace-control");
  workspaceControl?.classList.toggle("hidden", !(admin && owner));
  const workspaceSelect = document.querySelector("#workspace-select");
  if (workspaceSelect) workspaceSelect.value = state.workspace;
  const member = state.session?.member;
  document.querySelector("#session-name").textContent = member?.display_name || member?.email || (token() ? "Break-glass operator" : "Portal member");
  document.querySelector("#session-role").textContent = token() ? "Operator token" : admin ? "Admin member" : "Scoped member";
}

async function navigateToView(view, { replace = false, focus = true, workspace = state.workspace, monitoring = null } = {}) {
  if (pendingWrites) { setNotice("Wait for the current change to finish before leaving this view."); return; }
  if (hasDirtyForms() && !(await confirmAction("Discard unsaved changes?", "Leaving this view replaces its forms. Choose Cancel to keep editing."))) return;
  clearDirtyForms();
  if (!viewIds.has(view) || !viewAllowed(view)) return;
  state.workspace = workspace;
  if (monitoring) {
    synchronizeProjectScope(monitoring.projectScope);
    state.usageFilters = monitoring.usageFilters;
    resetUsagePagination();
  }
  configurePortalShell();
  const nextHash = `#/${view}${monitoringHash()}`;
  const changed = location.hash !== nextHash;
  if (replace) history.replaceState(null, "", nextHash);
  else if (changed) location.hash = nextHash;
  state.view = view;
  state.editingKeyId = null;
  state.editingServiceName = null;
  state.editingGuardrailName = null;
  syncNavigation();
  closeNavigation();
  closeGovernedMenu();
  window.scrollTo({ top: 0, behavior: "auto" });
  if (!changed || replace) refresh({ focus });
}

function syncNavigation() {
  document.querySelectorAll(".nav").forEach((item) => {
    const active = item.dataset.view === state.view || (item.dataset.view === "members" && state.view === "managed-identities");
    item.classList.toggle("active", active);
    if (active) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  });
}

function openNavigation() {
  document.querySelector("#sidebar").inert = false;
  document.querySelector(".workspace").inert = true;
  document.body.classList.add("nav-open");
  document.querySelector("#nav-backdrop")?.classList.remove("hidden");
  document.querySelector("#nav-toggle")?.setAttribute("aria-expanded", "true");
  document.querySelector("#nav-close")?.focus();
}

function closeNavigation() {
  const wasOpen = document.body.classList.contains("nav-open");
  document.querySelector(".workspace").inert = false;
  document.querySelector("#sidebar").inert = window.matchMedia("(max-width: 920px)").matches;
  if (wasOpen) document.querySelector("#nav-toggle")?.focus();
  document.body.classList.remove("nav-open");
  document.querySelector("#nav-backdrop")?.classList.add("hidden");
  document.querySelector("#nav-toggle")?.setAttribute("aria-expanded", "false");
}

function toggleGovernedMenu() {
  const trigger = document.querySelector("#governed-change-trigger");
  const menu = document.querySelector("#governed-change-menu");
  const opening = menu.classList.contains("hidden");
  menu.classList.toggle("hidden", !opening);
  trigger.setAttribute("aria-expanded", String(opening));
  if (opening) menu.querySelector("[role='menuitem']")?.focus();
}

function closeGovernedMenu() {
  document.querySelector("#governed-change-menu")?.classList.add("hidden");
  document.querySelector("#governed-change-trigger")?.setAttribute("aria-expanded", "false");
}

function showCommandPalette() {
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  const commands = commandViewsForWorkspace()
    .map(([view, meta]) => `<button type="button" class="command-item" data-command-view="${attr(view)}">
      <span><strong>${esc(meta.title)}</strong><small>${esc(meta.domain)} · ${esc(meta.summary)}</small></span>
      <kbd>↵</kbd>
    </button>`)
    .join("");
  const workspaceLabel = state.workspace === "owner" ? "Owner" : "Admin";
  backdrop.innerHTML = `
    <div class="modal command-palette" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">Go to ${workspaceLabel} view</h3>
      <label class="command-search"><span>Filter views</span><input type="search" placeholder="Search views…" autocomplete="off" data-command-search></label>
      <div class="command-list" role="list">${commands}</div>
      <div class="command-footer"><span>Enter to open</span><span>Esc to close</span></div>
    </div>`;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-command-search]" });
  const input = backdrop.querySelector("[data-command-search]");
  const filter = () => {
    const query = input.value.trim().toLowerCase();
    backdrop.querySelectorAll("[data-command-view]").forEach((button) => {
      button.hidden = query && !button.textContent.toLowerCase().includes(query);
    });
  };
  input.addEventListener("input", filter);
  input.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    const visible = [...backdrop.querySelectorAll("[data-command-view]")].find((button) => !button.hidden);
    if (visible) visible.click();
  });
  backdrop.querySelectorAll("[data-command-view]").forEach((button) => {
    button.addEventListener("click", () => {
      const view = button.dataset.commandView;
      close();
      navigateToView(view);
    });
  });
}

document.querySelector("#login-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const value = document.querySelector("#operator-token").value.trim();
  sessionStorage.setItem(tokenKey, value);
  try {
    await api("/admin-ui/admin/usage/summary");
    state.session = null;
    state.workspace = "admin";
    signedIn();
  } catch (error) {
    sessionStorage.removeItem(tokenKey);
    document.querySelector("#login-error").textContent = error.message;
  }
});

async function signOut() {
  stopGatewayStatus?.();
  stopGatewayStatus = null;
  stopTraffic?.();
  stopTraffic = null;
  let logoutUrl = null;
  if (!token() && state.session) {
    try {
      const result = await api("/admin-ui/auth/logout", { method: "POST", body: "{}" });
      logoutUrl = result?.logout_url ?? null;
    } catch (_) {}
  }
  sessionStorage.removeItem(tokenKey);
  if (logoutUrl) location.assign(logoutUrl);
  else location.reload();
}

document.querySelector("#sign-out").addEventListener("click", signOut);
document.querySelector("#access-state-sign-out").addEventListener("click", signOut);

document.querySelector("#workspace-select").addEventListener("change", (event) => {
  const workspace = event.target.value;
  // Keep the visible selection and shell unchanged until navigation is accepted.
  event.target.value = state.workspace;
  navigateToView(workspace === "owner" ? ownerLandingView() : "overview", { workspace });
});

document.querySelector("#refresh").addEventListener("click", refresh);
document.querySelector("#nav-toggle").addEventListener("click", openNavigation);
document.querySelector("#nav-close").addEventListener("click", closeNavigation);
document.querySelector("#nav-backdrop").addEventListener("click", closeNavigation);
document.querySelector("#command-trigger").addEventListener("click", showCommandPalette);
document.querySelector("#governed-change-trigger").addEventListener("click", toggleGovernedMenu);
document.querySelectorAll("[data-governed-view]").forEach((button) => {
  button.addEventListener("click", () => navigateToView(button.dataset.governedView));
});
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    showCommandPalette();
  }
});
document.addEventListener("click", (event) => {
  if (!(event.target instanceof Element) || !event.target.closest(".governed-change")) closeGovernedMenu();
});

document.querySelector("#rotate-token").addEventListener("click", async () => {
  if (!(await confirmAction("Rotate operator token", "The current token stops working."))) return;
  try {
    const body = await api("/admin-ui/admin/operator-token/rotate", { method: "POST", body: "{}" });
    sessionStorage.setItem(tokenKey, body.raw_token);
    showRawToken(body.raw_token, "Operator token shown once");
    setNotice("Operator token rotated. Store the new token now.", "success");
  } catch (error) {
    setNotice(error.message);
  }
});

document.querySelectorAll(".nav").forEach((button) => {
  button.addEventListener("click", () => navigateToView(button.dataset.view));
});

window.addEventListener("hashchange", async () => {
  if (!token() && !state.session) return;
  if (pendingWrites) { persistMonitoringHash(); return; }
  if (hasDirtyForms() && !(await confirmAction("Discard unsaved changes?", "Leaving this view replaces its forms."))) { persistMonitoringHash(); return; }
  clearDirtyForms();
  restoreMonitoringHash();
  state.view = viewFromHash();
  syncNavigation();
  closeNavigation();
  window.scrollTo({ top: 0, behavior: "auto" });
  refresh({ focus: true });
});

async function refresh({ focus = false } = {}) {
  if (pendingWrites) { setNotice("Wait for the current change to finish before refreshing."); return; }
  if (hasDirtyForms() && !(await confirmAction("Discard unsaved changes?", "Refreshing replaces the current forms. Choose Cancel to keep editing."))) return;
  clearDirtyForms();
  renderGeneration += 1;
  const generation = ++viewGeneration;
  const view = state.view;
  stopGatewayStatus?.();
  stopGatewayStatus = null;
  closeContentDrawer?.();
  viewController.abort();
  viewController = new AbortController();
  stopTraffic?.();
  stopTraffic = null;
  document.querySelector("#last-refreshed").textContent = "Loading current view…";
  setNotice("");
  destroyOverviewChart();
  destroyOwnerDashboardChart();
  usageLoad?.charts?.forEach(chart => chart?.destroy());
  if (state.view !== "service-dashboard" && state.view !== "project-dashboard") ownerDashboardGeneration += 1;
  applyViewChrome(view);
  document.querySelector("#page-actions").replaceChildren();
  syncProjectScope();
  const meta = viewMeta[state.view] || viewMeta.overview;
  document.querySelector("#breadcrumb-domain").textContent = meta.domain;
  document.querySelector("#breadcrumb-title").textContent = meta.title;
  content.innerHTML = panel("", emptyState("Loading..."));
  content.setAttribute("aria-busy", "true");
  try {
    if (view === "overview") await overview();
    if (view === "projects") await projects();
    if (view === "keys") await keys();
    if (view === "guardrails") await guardrails();
    if (view === "audit") await audit();
    if (view === "providers") await providers();
    if (view === "routes") await routes();
    if (view === "services") await services();
    if (view === "traffic") {
      const projects = await api("/admin-ui/admin/projects");
      if (generation !== viewGeneration) return;
      state.projects = projects;
      syncProjectScope();
      stopTraffic = mountTraffic({ content, api, headers: usageExportHeaders, esc, attr, table, badge, time, routingModeLabel, mountDialog, investigationView, bindInvestigationActions, refreshWebSocketMetrics, initialFilters: { ...state.trafficFilters, project_id: state.projectScope }, onFilters: applyTrafficFilters });
      const gatewayHost = document.createElement("div");
      gatewayHost.style.minWidth = "0";
      content.prepend(gatewayHost);
      stopGatewayStatus = mountTrafficGatewayStatus({ content: gatewayHost, api, esc, attr, badge, table, Chart });
    }
    if (view === "usage") await usage();
    if (view === "health") await health();
    if (view === "settings") await settings();
    if (view === "members") await members();
    if (view === "managed-identities") await managedIdentities();
    if (view === "my-services") await myServices();
    if (view === "service-dashboard") await serviceDashboard();
    if (view === "my-projects") await myProjects();
    if (view === "project-dashboard") await projectDashboard();
    if (generation !== viewGeneration) return;
    document.querySelector("#last-refreshed").textContent = `Updated ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
    if (focus) content.focus({ preventScroll: true });
  } catch (error) {
    if (generation !== viewGeneration || error.name === "AbortError") return;
    document.querySelector("#last-refreshed").textContent = "Refresh failed";
    setNotice(error.message);
    content.innerHTML = `<section class="panel"><div class="empty-state"><p>${esc(error.message)}</p></div></section>`;
  } finally {
    if (generation === viewGeneration) content.setAttribute("aria-busy", "false");
  }
}

async function overview() {
  const renderId = ++renderGeneration;
  const issues = [];
  const usageQuery = overviewUsageQuery();
  const [summary, timeseries, projectUsage, healthRows, ready, keysRows, openaiRoutes, anthropicRoutes, servicesRows, projectsRows, auditEvents, policyLayers] = await Promise.all([
    monitoringPanel(`/admin-ui/admin/usage/summary?${usageQuery}`, {}, issues),
    monitoringPanel(`/admin-ui/admin/usage/timeseries?${usageQuery}`, [], issues),
    monitoringPanel(`/admin-ui/admin/usage/by-project?${usageQuery}`, [], issues),
    monitoringPanel(`/admin-ui/admin/provider-health?${usageQuery}`, [], issues),
    readiness(),
    monitoringPanel("/admin-ui/admin/keys", [], issues),
    monitoringPanel("/admin-ui/admin/openai-routes", [], issues),
    monitoringPanel("/admin-ui/admin/anthropic-routes", [], issues),
    monitoringPanel("/admin-ui/admin/services", [], issues),
    monitoringPanel("/admin-ui/admin/projects", [], issues),
    monitoringPanel("/admin-ui/admin/audit-events?limit=8", [], issues),
    monitoringPanel("/admin-ui/admin/policy-layers", [], issues),
  ]);
  if (renderId !== renderGeneration) return;
  state.projects = projectsRows;
  syncProjectScope();
  const scopedKeys = state.projectScope ? keysRows.filter((key) => key.project_id === state.projectScope) : keysRows;
  const activeKeys = scopedKeys.filter((key) => ["active", "non-expiring"].includes(keyLifecycle(key))).length;
  const enabledRoutes =
    openaiRoutes.filter((route) => route.enabled).length + anthropicRoutes.filter((route) => route.enabled).length;
  const totalRoutes = openaiRoutes.length + anthropicRoutes.length;
  const enabledServices = servicesRows.filter((service) => service.enabled).length;
  const risks = overviewRisks(healthRows);
  const readyState = ready.status === "ready";
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    ${monitoringIssues(issues)}
    <div class="readiness-banner ${readyState ? "good" : "bad"}" role="status"><i class="ti ${readyState ? "ti-circle-check" : "ti-alert-triangle"}" aria-hidden="true"></i><span><strong>Gateway ${esc(ready.status.replaceAll("_", " "))}.</strong> ${ready.detail ? esc(ready.detail) : "Current readiness · gateway-wide"}</span><button type="button" class="link-button" data-overview-nav="health">View health →</button></div>
    <section class="page-insights" aria-label="Gateway activity summary">
      ${insightStrip([
        { label: "Requests", value: summary.request_count, icon: "arrows-exchange" },
        { label: "Success rate", value: summary.request_count > 0 && typeof summary.failure_count === "number" ? percent(1 - summary.failure_count / summary.request_count) : null, icon: "circle-check", detail: `${summary.failure_count ?? "Unknown"} recorded failures` },
        { label: "Average latency", value: summary.average_latency_ms == null ? null : `${Math.round(summary.average_latency_ms)} ms`, icon: "clock" },
        { label: "Estimated cost", value: summary.estimated_cost_usd == null ? null : money(summary.estimated_cost_usd), icon: "coin", detail: "Recorded reporting estimate · USD" },
      ], { label: `${scopeLabel()} · ${overviewWindowLabel()}` })}
    </section>
    <div class="overview-insights">
      <section class="panel chart-panel">
        <div class="panel-heading chart-heading"><div><h3>Request volume</h3><span class="subtle">${esc(scopeLabel())} · ${esc(overviewWindowLabel())}</span></div></div>
        ${overviewRequestVolume(timeseries, issues)}
        <button type="button" class="link-button" data-overview-nav="usage">Explore requests →</button>
      </section>
      <section class="panel attention-panel">
        <div class="panel-heading"><h3>Gateway attention</h3><span class="badge warn">${issues.some((issue) => issue.startsWith("provider-health:") && issue.endsWith("unavailable")) ? "Unavailable" : `${risks.length} elevated rates`}</span></div>
        <p class="help">${esc(overviewWindowLabel())} · minimum 20 requests · elevated at 5%</p>
        <div class="attention-list">${issues.some((issue) => issue.startsWith("provider-health:") && issue.endsWith("unavailable")) ? emptyState("Reliability observations are unavailable.") : risks.length ? risks.slice(0, 3).map(overviewRiskRow).join("") : emptyState("No elevated rates in the recorded sample.")}</div>
        ${distributionBars("Key lifecycle", [
          { label: "Active", value: issues.some(issue => issue.startsWith("keys:") && issue.endsWith("unavailable")) ? null : activeKeys, tone: "good" },
          { label: "Expired", value: issues.some(issue => issue.startsWith("keys:") && issue.endsWith("unavailable")) ? null : scopedKeys.filter(key => keyLifecycle(key) === "expired").length, tone: "bad" },
        ], { caption: `Saved key lifecycle in ${scopeLabel()}; an active key still requires policy approval.` })}
        <button type="button" class="link-button" data-overview-nav="keys">Review virtual keys →</button>
      </section>
    </div>
    ${overviewProjectActivity(projectUsage, projectsRows, issues)}
    <details class="workflow-disclosure"><summary>Gateway inventory · ${enabledRoutes}/${totalRoutes} routes · ${enabledServices} services</summary><section class="panel">${overviewOperationsTable(healthRows, scopedKeys)}</section></details>
  `;
  document.querySelector("#page-actions").innerHTML = `<label class="compact-field"><span>Overview time range</span><select id="overview-window">${option("24h", state.overviewWindow, "Last 24 hours")}${option("7d", state.overviewWindow, "Last 7 days")}${option("30d", state.overviewWindow, "Last 30 days")}</select></label>`;
  content.querySelectorAll("[data-overview-project]").forEach((button) => button.addEventListener("click", () => {
    navigateToView("usage", { monitoring: { projectScope: button.dataset.overviewProject, usageFilters: { time_preset: state.overviewWindow === "30d" ? "last_30d" : state.overviewWindow === "24h" ? "last_24h" : "last_7d" } } });
  }));
  content.querySelector("[data-retry-monitoring]")?.addEventListener("click", () => refresh());
  renderOverviewChart(timeseries);
  content.insertAdjacentHTML("beforeend", panel("Latest changes", `<p class="help">Latest sample of up to 8 audit entries · gateway-wide</p>${auditEventTable(auditEvents)}`));
  document.querySelector("#overview-window")?.addEventListener("change", (event) => {
    state.overviewWindow = event.currentTarget.value;
    persistMonitoringHash();
    refresh();
  });
  document.querySelectorAll("[data-overview-nav]").forEach((button) => {
    button.addEventListener("click", () => navigateToView(button.dataset.overviewNav));
  });
}

function overviewProjectActivity(projectUsage, projectsRows, issues) {
  const unavailable = issues.some((issue) => issue.startsWith("by-project:") && issue.endsWith("unavailable"));
  const maximum = Math.max(1, ...projectUsage.map(row => Number.isFinite(row.summary.request_count) ? row.summary.request_count : 0));
  return `    <section class="panel">
      <div class="panel-heading"><div><h3>Project activity</h3><span class="subtle">${esc(overviewWindowLabel())} · ${unavailable ? "Activity unavailable" : `top ${projectUsage.length} recorded projects`} · ${esc(scopeLabel())}</span></div></div>
      ${unavailable ? emptyState("Project activity is unavailable. Retry sources to load it.") : table(["Project", "Requests", "Failures", "Estimated cost", "Explore"], projectUsage.map((row) => [esc(projectsRows.find((project) => project.id === row.name)?.name || row.name), !Number.isFinite(row.summary.request_count) ? "Unknown" : `<div class="request-rank"><strong>${esc(row.summary.request_count)}</strong><progress max="${maximum}" value="${attr(row.summary.request_count)}" aria-label="${attr(`${projectsRows.find(project => project.id === row.name)?.name || row.name}: ${row.summary.request_count} requests`)}"></progress></div>`, esc(row.summary.failure_count), money(row.summary.estimated_cost_usd), `<button type="button" data-overview-project="${attr(row.name)}">View usage</button>`]))}
    </section>`;
}

function overviewRequestVolume(timeseries, issues) {
  if (issues.some((issue) => issue.startsWith("timeseries:") && issue.endsWith("unavailable"))) {
    return emptyState("Request volume is unavailable. Retry sources to load it.");
  }
  return `<div class="overview-chart-wrap"><canvas id="overview-chart" role="img" aria-label="Requests and failures over ${overviewWindowLabel().toLowerCase()}"></canvas></div>
    <p class="sr-only">${esc(overviewChartSummary(timeseries))}</p>`;
}

function overviewUsageQuery() {
  const now = new Date();
  const start = new Date(now);
  const days = state.overviewWindow === "30d" ? 30 : state.overviewWindow === "7d" ? 7 : 1;
  start.setDate(start.getDate() - days);
  const query = new URLSearchParams({
    from: start.toISOString(),
    to: now.toISOString(),
    interval: state.overviewWindow === "30d" ? "day" : "hour",
    breakdown_limit: "20",
    timeseries_limit: state.overviewWindow === "30d" ? "31" : state.overviewWindow === "7d" ? "169" : "25",
  });
  if (state.projectScope) query.set("project_id", state.projectScope);
  return query.toString();
}

function overviewWindowLabel() {
  if (state.overviewWindow === "30d") return "Last 30 days";
  if (state.overviewWindow === "24h") return "Last 24 hours";
  return "Last 7 days";
}

function overviewRisks(rows) {
  return rows.map((row) => ({ ...row, ...reliability(row) }))
    .filter((row) => row.score > 0).sort((a, b) => b.score - a.score);
}

function overviewRiskRow(row) {
  return `<div class="attention-row">
    ${badge(row.tone === "bad" ? "High" : "Review", row.tone)}
    <span><strong>${esc(row.label)}</strong><small>${esc(row.name)} · ${percent(row.signal)}</small></span>
    <button type="button" data-overview-nav="health">View reliability</button>
  </div>`;
}

function overviewOperationsTable(healthRows, keysRows) {
  const health = healthRows.slice(0, 5).map((row) => [
    `<strong>${esc(row.name)}</strong>`,
    esc(row.provider || row.provider_kind || "Service"),
    healthBadge(row),
    `<strong>${esc(row.error_count || 0)} errors · ${esc(row.fallback_count || 0)} fallbacks</strong><div class="subtle">${averageLatency(row)}</div>`,
    "Recorded usage",
    `<button type="button" class="icon-button" data-overview-nav="health" aria-label="Inspect health for ${attr(row.name)}"><i class="ti ti-dots" aria-hidden="true"></i></button>`,
  ]);
  const key = keysRows.find((row) => !row.revoked_at) || keysRows[0];
  if (key) {
    health.push([
      `<strong>${esc(key.key_prefix)}</strong>`,
      "Virtual key",
      keyStatus(key),
      keyPolicySummary(key),
      key.updated_at ? time(key.updated_at) : time(key.created_at),
      `<button type="button" class="icon-button" data-overview-nav="keys" aria-label="Inspect key ${attr(key.key_prefix)}"><i class="ti ti-dots" aria-hidden="true"></i></button>`,
    ]);
  }
  return table(["Item", "Type", "Status", "Key signal", "Last change", "Action"], health);
}

function overviewChartSummary(rows) {
  if (!rows.length) return "No usage timeseries data is available for this period.";
  const latest = rows[rows.length - 1];
  const summary = latest.summary || latest;
  return `Latest bucket: ${summary.request_count || 0} requests, ${summary.failure_count || 0} failures, and ${summary.average_latency_ms == null ? "no latency data" : `${Math.round(summary.average_latency_ms)} milliseconds average latency`}.`;
}

function renderOverviewChart(rows) {
  const canvas = document.querySelector("#overview-chart");
  if (!(canvas instanceof HTMLCanvasElement)) return;
  const labels = rows.map((row) => new Date(row.bucket_start || row.bucket || row.name).toLocaleString([], { month: "short", day: "numeric", ...(state.overviewWindow === "24h" ? { hour: "2-digit" } : {}) }));
  const values = (key) => rows.map((row) => Number(row.summary?.[key] ?? row[key] ?? 0));
  overviewChart = new Chart(canvas, {
    type: "line",
    data: {
      labels,
      datasets: [
        { label: "Requests", order: 1, data: values("request_count"), borderColor: chartColor("#087b60", "teal"), backgroundColor: chartColor("#e9f3ed", "fill"), fill: true, yAxisID: "y", tension: 0.32, pointRadius: 0, borderWidth: 2 },
        { label: "Failures", data: values("failure_count"), borderColor: chartColor("#d9474f", "red"), backgroundColor: chartColor("#d9474f", "red"), yAxisID: "y", tension: 0.32, pointRadius: 0, borderWidth: 2 },
      ],
    },
    options: {
      animation: false,
      maintainAspectRatio: false,
      responsive: true,
      interaction: { mode: "index", intersect: false },
      plugins: {
        legend: { position: "top", align: "start", labels: { boxWidth: 18, boxHeight: 2, usePointStyle: false, color: chartColor("#536276", "text"), font: { size: 11, weight: 600 } } },
        tooltip: { backgroundColor: chartColor("#062f36", "tooltip-bg"), titleColor: chartColor("#fff", "tooltip-text"), bodyColor: chartColor("#fff", "tooltip-text"), footerColor: chartColor("#fff", "tooltip-text"), padding: 10, titleFont: { size: 12 }, bodyFont: { size: 12 } },
      },
      scales: {
        x: { grid: { display: false }, border: { color: chartColor("rgba(0,0,0,0.1)", "grid") }, ticks: { color: chartColor("#58736e", "text"), maxTicksLimit: 7, font: { size: 10 } } },
        y: { beginAtZero: true, grid: { color: chartColor("#dcebe7", "grid") }, border: { color: chartColor("rgba(0,0,0,0.1)", "grid") }, ticks: { color: chartColor("#58736e", "text"), precision: 0, font: { size: 10 } } },
      },
    },
  });
}

function destroyOverviewChart() {
  if (!overviewChart) return;
  overviewChart.destroy();
  overviewChart = null;
}

function destroyOwnerDashboardChart() {
  if (!ownerDashboardChart) return;
  ownerDashboardChart.destroy();
  ownerDashboardChart = null;
}

function stat(label, value) {
  return metricTile(label, value);
}

async function projects() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([api("/admin-ui/admin/projects"), api("/admin-ui/admin/services")]);
  if (renderId !== renderGeneration) return;
  [state.projects, state.services] = loaded;
  if (renderId !== renderGeneration) return;
  const scopedProjects = state.projectScope ? state.projects.filter(project => project.id === state.projectScope) : state.projects;
  const insights = projectCatalogInsights(scopedProjects, state.services);
  content.innerHTML = `
    <section class="page-insights" aria-label="Project service coverage">
      <div class="page-insights-grid">
        ${insightStrip(insights.items, { label: "Service relationships" })}
        ${distributionBars("Project link coverage", insights.distribution, { caption: insights.caption, empty: "No projects in this scope." })}
      </div>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Create project</h3></div>
      <form id="project-form" class="form-grid">
        <label>Name<input name="name" required maxlength="120"></label>
        <div class="form-actions"><button class="primary">Create project</button></div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Projects</h3><span class="subtle">${state.projects.length} total</span></div>
      ${projectTable(state.projects)}
    </section>
  `;
  document.querySelector("#project-form").addEventListener("submit", handleAsync(createProject));
  document.querySelectorAll("[data-project-services-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(patchProjectServices));
  });
  bindServicePickerButtons();
  document.querySelectorAll("[data-project-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(projectAction));
  });
  organizeView("projects");
  content.prepend(content.querySelector(".page-insights"));
}

function projectTable(rows) {
  return table(
    ["Name", "UUID", "Linked services", "Updated", "Actions"],
    rows.map((row) => [
      `<strong><i class="ti ti-folder" aria-hidden="true"></i> ${esc(row.name)}</strong>`,
      `<code>${esc(row.id)}</code>`,
      `<details class="catalog-disclosure"><summary aria-label="Manage service access for project ${attr(row.name)}"><i class="ti ti-link" aria-hidden="true"></i> ${Array.isArray(row.service_names) ? `${row.service_names.length} service${row.service_names.length === 1 ? "" : "s"}` : "Links unknown"}<i class="ti ti-chevron-down" aria-hidden="true"></i></summary>${projectServiceForm(row)}</details>`,
      time(row.updated_at),
      `<div class="actions">
        <button data-project-action="usage" data-project-id="${attr(row.id)}" aria-label="View usage for project ${attr(row.name)}">Usage</button>
        <button class="danger" data-project-action="delete" data-project-id="${attr(row.id)}" aria-label="Delete project ${attr(row.name)}">Delete</button>
      </div>`,
    ]),
  );
}

function projectServiceForm(project) {
  return `<form class="inline-service-form" data-project-services-form data-project-id="${attr(project.id)}">
    ${serviceSelectionControl(project.service_names || [], "service_names", "Project services")}
    <div class="form-actions"><button>Save services</button></div>
  </form>`;
}

async function createProject(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const project = await api("/admin-ui/admin/projects", { method: "POST", body: JSON.stringify({ name: form.get("name") }) });
  synchronizeProjectScope(project.id);
  persistMonitoringHash();
  setNotice("Project created.", "success");
  await projects();
}

async function projectAction(event) {
  const { projectAction: action, projectId } = event.currentTarget.dataset;
  if (action === "usage") {
    await navigateToView("usage", { monitoring: { projectScope: projectId, usageFilters: { ...state.usageFilters, project_id: projectId, key_id: "" } } });
    return;
  }
  if (!(await confirmAction("Delete project", "Projects with linked keys, services, or usage cannot be deleted."))) return;
  await api(`/admin-ui/admin/projects/${projectId}`, { method: "DELETE" });
  if (state.projectScope === projectId) {
    synchronizeProjectScope("");
    resetUsagePagination();
    persistMonitoringHash();
  }
  setNotice("Project deleted.", "success");
  await projects();
}

async function patchProjectServices(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  await api(`/admin-ui/admin/projects/${event.target.dataset.projectId}`, {
    method: "PATCH",
    body: JSON.stringify({ service_names: form.getAll("service_names") }),
  });
  setNotice("Project services updated.", "success");
  await projects();
}

async function keys() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/projects"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/guardrails"),
    api("/admin-ui/admin/policy-layers"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.keys, state.projects, state.services, state.guardrails, state.policyLayers] = loaded;
  const editing = state.keys.find((key) => key.id === state.editingKeyId);
  const scopedKeys = state.projectScope ? state.keys.filter(key => key.project_id === state.projectScope) : state.keys;
  const visual = keyGovernanceSnapshot(scopedKeys, state.policyLayers);
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    <section class="page-insights" aria-label="Virtual key governance summary">
      <div class="page-insights-grid">
        ${insightStrip(visual.insights, { label: "Key configuration in selected project scope" })}
        ${distributionBars("Key lifecycle", visual.lifecycle, { caption: "Loaded keys · selected project scope", empty: "No keys in this scope." })}
      </div>
      ${flowSummary("Configured policy layers", visual.layers, "Gateway-wide layer inventory by scope kind. Matching layers and key policy determine the effective decision; use the simulator to inspect a request.")}
    </section>
    <div class="split">
      <section class="panel">
        <div class="panel-heading">
          <h3>Create virtual key</h3>
        </div>
        <form id="key-form" class="form-grid">
          ${formSection("Identity and ownership", "Choose a safe preset, owner, and lifecycle.", `
            <label>Preset<select name="preset">
              <option value="">Custom</option>
              <option value="developer">Developer</option>
              <option value="production_worker">Production worker</option>
              <option value="read_only_service">Read-only service</option>
              <option value="external_partner">External partner</option>
              <option value="temporary_debugging">Temporary debugging</option>
            </select></label>
            ${keyOwnershipFields()}
            <label>Expires at<input name="expires_at" type="datetime-local"></label>
            <label>Rotation due<input name="rotation_due_at" type="datetime-local"></label>
            <label class="check"><input name="no_expires_at" type="checkbox"> No expiration</label>
          `, true)}
          ${formSection("Access and limits", "Restrict routes, models, providers, rate, budget, and payload size.", policyFields())}
          ${formSection("Guardrail policy", "Choose mandatory, optional, and forbidden safeguards.", guardrailPolicyFields())}
          <div class="form-actions sticky-form-actions wide-field">
            <button type="submit" class="primary">Create key</button>
          </div>
        </form>
      </section>
      <section class="panel ${editing ? "" : "muted-panel"}">
        ${editing ? keyEditForm(editing) : `<div class="empty-state"><h3>No key selected</h3></div>`}
      </section>
    </div>
    <section class="panel">
      <div class="panel-heading">
        <h3>Inherited policy layers</h3>
        <span class="subtle">${state.policyLayers.length} configured</span>
      </div>
      <form id="policy-layer-form" class="form-grid">
        ${formSection("Layer identity", "Set the inheritance level and exact scope.", `
          <label>Layer<select name="kind">
            <option value="global">Global</option>
            <option value="project">Project</option>
            <option value="team">Team</option>
            <option value="route">Route</option>
            <option value="model">Model</option>
          </select></label>
          <label>Scope<input name="scope_id" placeholder="project UUID, team, route, or model"></label>
        `, true)}
        ${formSection("Inherited access and limits", "Neutral fields inherit unless you set an explicit value.", policyFields(null, true))}
        ${formSection("Inherited guardrails", "Apply guardrail requirements at this policy layer.", guardrailPolicyFields())}
        <div class="form-actions wide-field">
          <button type="submit" class="primary">Save layer</button>
        </div>
      </form>
      ${policyLayerTable(state.policyLayers)}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>Virtual keys</h3>
        <span class="subtle">${state.keys.length} total</span>
      </div>
      ${keyTable(state.keys)}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>Policy simulator</h3>
        <span class="subtle">Dry-run key governance</span>
      </div>
      <form id="policy-sim-form" class="form-grid">
        <label>Key<select name="key_id"><option value="">Default policy</option>${state.keys.map((key) => `<option value="${attr(key.id)}">${esc(keyName(key.id))}</option>`).join("")}</select></label>
        <label>Team scope<input name="team_id" placeholder="team identifier"></label>
        <label>Path<input name="path" value="/v1/chat/completions" data-policy-sim-path></label>
        <label>Provider<select name="provider" data-policy-sim-provider>
          <option value="">Route default</option>
          <option value="litellm">LiteLLM</option>
          <option value="openai-compatible">OpenAI-compatible</option>
          <option value="internal-service">Internal service</option>
        </select></label>
        <label data-policy-sim-model>Model<input name="model" value="gpt-4.1-mini"></label>
        <label data-policy-sim-service>Service name<select name="service_name">
          <option value="">Route-derived service</option>
          ${state.services.map((service) => `<option value="${attr(service.name)}">${esc(service.name)}</option>`).join("")}
        </select></label>
        <div class="help wide-field" data-policy-sim-service-help>Use a concrete path such as /services/service-name/test. The simulator reports the matched policy route separately.</div>
        <label>Request bytes<input name="request_body_bytes" type="number" min="0"></label>
        <label>Response bytes<input name="response_body_bytes" type="number" min="0"></label>
        <label class="check"><input name="stream" type="checkbox"> Stream</label>
        <label class="check"><input name="tools" type="checkbox"> Tools</label>
        <div class="form-actions">
          <button type="submit" class="primary">Simulate</button>
        </div>
      </form>
      <div id="policy-sim-result">${policySimulationResult()}</div>
    </section>
  `;
  document.querySelector("#key-form").addEventListener("submit", handleAsync(createKey));
  document.querySelector("#key-edit-form")?.addEventListener("submit", handleAsync(patchKey));
  document.querySelector("#policy-sim-form").addEventListener("submit", handleAsync(simulatePolicy));
  document.querySelector("#policy-layer-form").addEventListener("submit", handleAsync(savePolicyLayer));
  document.querySelectorAll("[data-policy-layer-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(policyLayerAction));
  });
  bindKeyExpiryControls();
  bindKeyOwnerControls();
  bindServicePickerButtons();
  bindGuardrailPickerButtons();
  bindPolicySimulatorControls();
  document.querySelectorAll("[data-key-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(keyAction));
  });
  organizeView("keys");
}

function policyFields(key = null, neutral = false) {
  const policy = key?.policy || {};
  return `
    <label>Routes<input name="allowed_routes" value="${attr(listValue(policy.allowed_routes, neutral ? "" : "/v1/chat/completions,/v1/responses"))}"></label>
    <label>Models<input name="allowed_models" value="${attr(listValue(policy.allowed_models, ""))}" placeholder="gpt-4o-mini"></label>
    <div class="field"><span>Providers</span>${providerPolicySelect(policy.allowed_providers, neutral)}<small class="field-hint">Allowed provider adapters. No selection adds no restriction at this layer; inherited policies still apply.</small></div>
    <label>RPM limit<input name="rpm_limit" type="number" min="0" value="${attr(policy.rpm_limit ?? "")}"></label>
    <label>TPM limit<input name="tpm_limit" type="number" min="0" value="${attr(policy.tpm_limit ?? "")}"></label>
    <label>Daily budget<input name="daily_budget_usd" type="number" min="0" step="any" value="${attr(policy.daily_budget_usd ?? "")}"></label>
    <label>Monthly budget<input name="monthly_budget_usd" type="number" min="0" step="any" value="${attr(policy.monthly_budget_usd ?? "")}"></label>
    <label>Max daily requests<input name="max_requests_per_day" type="number" min="0" value="${attr(policy.max_requests_per_day ?? "")}"></label>
    <label>Max daily tokens<input name="max_tokens_per_day" type="number" min="0" value="${attr(policy.max_tokens_per_day ?? "")}"></label>
    <label>Max cost/request<input name="max_cost_per_request" type="number" min="0" step="any" value="${attr(policy.max_cost_per_request ?? "")}"></label>
    <label>Max input tokens<input name="max_input_tokens_per_request" type="number" min="0" value="${attr(policy.max_input_tokens_per_request ?? "")}"></label>
    <label>Max output tokens<input name="max_output_tokens_per_request" type="number" min="0" value="${attr(policy.max_output_tokens_per_request ?? "")}"></label>
    <label>Allowed UTC hours<input name="allowed_hours_utc" value="${attr(listValue(policy.allowed_hours_utc, ""))}" placeholder="0,8,17"></label>
    <label>Stale disable days<input name="unused_key_auto_disable_after_days" type="number" min="0" value="${attr(policy.unused_key_auto_disable_after_days ?? "")}"></label>
    <label>Max request bytes<input name="max_request_body_bytes" type="number" min="0" value="${attr(policy.max_request_body_bytes ?? "")}"></label>
    <label>Max response bytes<input name="max_response_body_bytes" type="number" min="0" value="${attr(policy.max_response_body_bytes ?? "")}"></label>
    <label class="check"><input name="allow_streaming" type="checkbox" ${policy.allow_streaming || neutral ? "checked" : ""}> Allow streaming</label>
    <label class="check"><input name="allow_tools" type="checkbox" ${policy.allow_tools || neutral ? "checked" : ""}> Allow tools</label>
  `;
}

function guardrailPolicyFields(key = null) {
  const policy = key?.guardrail_policy || {};
  return `
    <div class="field"><span>Mandatory guardrails</span>${guardrailSelectionControl(policy.mandatory_guardrails || [], "mandatory_guardrails", "Mandatory guardrails")}<small class="field-hint">Checks that this key must run. Callers cannot opt out of mandatory checks.</small></div>
    <div class="field"><span>Optional guardrails</span>${guardrailSelectionControl(policy.optional_guardrails || [], "optional_guardrails", "Optional guardrails")}<small class="field-hint">Checks permitted for this key when requested, in addition to mandatory checks.</small></div>
    <div class="field"><span>Forbidden guardrails</span>${guardrailSelectionControl(policy.forbidden_guardrails || [], "forbidden_guardrails", "Forbidden guardrails")}<small class="field-hint">Checks this key cannot request. Do not also select them as mandatory or optional.</small></div>
    <div class="wide-field field">
      <span>Guardrail config overrides</span>
      <div data-guardrail-overrides>${guardrailOverrideControls(policy.guardrail_config_overrides || {}, activeConfigurableGuardrails(policy))}</div>
    </div>
  `;
}

function activeConfigurableGuardrails(policy = {}) {
  return [...new Set([...(policy.mandatory_guardrails || []), ...(policy.optional_guardrails || [])])].filter(
    (name) => !(policy.forbidden_guardrails || []).includes(name),
  );
}

function guardrailOverrideControls(overrides = {}, selectedNames = []) {
  const selected = new Set(selectedNames);
  const rows = (state.guardrails?.guardrails || []).filter((guardrail) => selected.has(guardrail.name));
  if (!selectedNames.length) return '<div class="empty-inline">Select mandatory or optional guardrails before setting config overrides.</div>';
  if (!rows.length) return '<div class="empty-inline">Selected guardrails are not in the current catalog.</div>';
  return `<div class="guardrail-overrides" role="group" aria-label="Guardrail config overrides">
    ${rows
      .map((guardrail) => {
        const enabled = Object.hasOwn(overrides, guardrail.name);
        const value = JSON.stringify(enabled ? overrides[guardrail.name] : {}, null, 2);
        const schema = JSON.stringify(guardrail.config_schema || {});
        return `<section class="guardrail-override-row">
          <label class="check guardrail-override-toggle">
            <input name="guardrail_override_names" type="checkbox" value="${attr(guardrail.name)}" ${enabled ? "checked" : ""}>
            <span><strong>${esc(guardrail.name)}</strong><small>${esc(guardrail.description || "Custom runtime settings")}</small></span>
          </label>
          <label>Configuration for ${esc(guardrail.name)}<textarea name="guardrail_override_${attr(guardrail.name)}" rows="4">${esc(value)}</textarea></label>
          <details>
            <summary>Config schema</summary>
            <code>${esc(schema)}</code>
          </details>
        </section>`;
      })
      .join("")}
  </div>`;
}

function keyOwnershipFields(key = null) {
  const ownerType = key?.owner_type || "project";
  return `
    <label>Key name (optional)<input data-guidance-name="key_name" name="key_name" maxlength="120" value="${attr(key?.name || "")}" placeholder="Production automation"></label>
    <label>Owner<select name="owner_type">
      <option value="project" ${ownerType === "project" ? "selected" : ""}>Project</option>
      <option value="individual" ${ownerType === "individual" ? "selected" : ""}>Individual</option>
    </select></label>
    <label data-owner-project>Project<select name="project_id">${projectOptions(key?.project_id || "")}</select></label>
    <div class="field" data-owner-services><span>Services</span>${serviceSelectionControl(key?.service_names || [], "service_names", "Individual key services")}</div>
  `;
}

function keyEditForm(key) {
  return `
    <div class="panel-heading">
      <h3>Edit virtual key</h3>
      <span class="subtle">${esc(key.key_prefix)}</span>
    </div>
    <form id="key-edit-form" class="form-grid" data-key-id="${attr(key.id)}">
      ${formSection("Identity and lifecycle", "Update ownership, expiry, rotation, and availability.", `
        ${keyOwnershipFields(key)}
        <label>Expires at<input name="expires_at" type="datetime-local" value="${attr(toLocalInput(key.expires_at))}"></label>
        <label>Rotation due<input name="rotation_due_at" type="datetime-local" value="${attr(toLocalInput(key.rotation_due_at))}"></label>
        <label class="check"><input name="no_expires_at" type="checkbox" ${key.expires_at ? "" : "checked"}> No expiration</label>
        <label class="check"><input name="disabled" type="checkbox" ${key.disabled ? "checked" : ""}> Disabled</label>
      `, true)}
      <div class="wide-field"><button type="button" data-key-profile-bindings="${attr(key.id)}">Inspect and assign route authentication profiles</button><p class="help">Assignments are per route and do not grant additional route permissions. The raw key is never needed.</p></div>
      ${formSection("Access and limits", "Edit the effective key policy.", policyFields(key))}
      ${formSection("Guardrail policy", "Edit mandatory, optional, and forbidden safeguards.", guardrailPolicyFields(key))}
      <div class="form-actions sticky-form-actions wide-field">
        <button type="submit" class="primary">Save changes</button>
        <button type="button" data-key-action="cancel-edit">Cancel</button>
      </div>
    </form>
  `;
}

function keyTable(rows) {
  return table(
    ["Key", "Owner", "Services", "Status", "Expiry", "Policy", "Updated", "Actions"],
    rows.map((key) => [
      `<strong><i class="ti ti-key" aria-hidden="true"></i> ${key.name ? esc(key.name) : "Virtual key"}</strong><div class="subtle"><code>${esc(key.key_prefix)}</code></div>`,
      keyOwnerLabel(key),
      esc(listValue(key.service_names, "derived")),
      keyStatus(key),
      esc(keyExpiry(key)),
      keyPolicySummary(key),
      time(key.updated_at),
      keyLifecycleActions(key),
    ]),
  );
}

function policyLayerTable(rows) {
  return table(
    ["Layer", "Scope", "Version", "Policy", "Guardrails", "Updated", "Actions"],
    rows.map((layer) => [
      `<i class="ti ti-hierarchy" aria-hidden="true"></i> ${badge(layer.kind)}`,
      `<code>${esc(layer.scope_id || "all")}</code>`,
      esc(layer.policy?.policy_version ?? "1"),
      keyPolicySummary({ policy: layer.policy, rotation_due_at: null, last_used_at: null }),
      guardrailPolicySummary(layer.guardrail_policy),
      time(layer.updated_at),
      `<button type="button" class="danger" data-policy-layer-action="delete" data-layer-id="${attr(layer.id)}">Delete</button>`,
    ]),
  );
}

function keyLifecycleActions(key) {
  const keyLabel = attr(key.name ? `${key.name} (${key.key_prefix})` : key.key_prefix);
  const toggle = key.revoked_at
    ? ""
    : key.disabled
      ? `<button data-key-action="enable" data-key-id="${attr(key.id)}" aria-label="Enable virtual key ${keyLabel}">Enable</button>`
      : `<button data-key-action="disable" data-key-id="${attr(key.id)}" aria-label="Disable virtual key ${keyLabel}">Disable</button>`;
  return `<div class="actions">
        <button data-key-action="edit" data-key-id="${attr(key.id)}" aria-label="Edit virtual key ${keyLabel}">Edit</button>
        <button data-key-action="spend" data-key-id="${attr(key.id)}" aria-label="LiteLLM spend for virtual key ${keyLabel}">LiteLLM spend</button>
        <button data-key-action="usage" data-key-id="${attr(key.id)}" aria-label="View usage for virtual key ${keyLabel}">Usage</button>
        ${toggle}
        <button class="danger" data-key-action="revoke" data-key-id="${attr(key.id)}" aria-label="Revoke virtual key ${keyLabel}" ${key.revoked_at ? "disabled" : ""}>Revoke</button>
      </div>`;
}

async function createKey(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  let guardrailPolicy;
  try {
    guardrailPolicy = guardrailPolicyBody(form);
  } catch (error) {
    setNotice(error.message);
    return;
  }
  const body = {
    name: String(form.get("key_name") || "").trim() || null,
    owner_type: form.get("owner_type"),
    project_id: form.get("owner_type") === "project" ? form.get("project_id") : null,
    service_names: form.get("owner_type") === "individual" ? form.getAll("service_names") : [],
    preset: form.get("preset") || null,
    expires_at: form.has("no_expires_at") ? null : isoDate(form.get("expires_at")),
    rotation_due_at: isoDate(form.get("rotation_due_at")),
    policy: policyBody(form),
    guardrail_policy: guardrailPolicy,
  };
  if (!form.has("no_expires_at") && !body.expires_at) delete body.expires_at;
  if (!body.rotation_due_at) delete body.rotation_due_at;
  if (!(await confirmAction("Review virtual key", `Issue a ${body.preset || "custom"} key for ${body.project_id ? projectName(body.project_id) : "the selected individual services"}? Expiration: ${body.expires_at === null ? "none" : body.expires_at || "server default (may be non-expiring)"}. The credential will be shown once after creation.`))) return;
  const response = await api("/admin-ui/admin/keys", { method: "POST", body: JSON.stringify(body) });
  closeContentDrawer?.();
  state.editingKeyId = null;
  setNotice("Virtual key created.", "success");
  try { await keys(); } catch (error) { if (error.name !== "AbortError") setNotice(`Key created; inventory refresh failed: ${error.message}`); }
  showRawToken(response.raw_key, "Virtual key shown once");
}

async function patchKey(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const keyId = event.target.dataset.keyId;
  let guardrailPolicy;
  try {
    guardrailPolicy = guardrailPolicyBody(form);
  } catch (error) {
    setNotice(error.message);
    return;
  }
  const body = {
    name: String(form.get("key_name") || "").trim() || null,
    owner_type: form.get("owner_type"),
    project_id: form.get("owner_type") === "project" ? form.get("project_id") : null,
    service_names: form.get("owner_type") === "individual" ? form.getAll("service_names") : [],
    disabled: form.has("disabled"),
    rotation_due_at: form.get("rotation_due_at") ? isoDate(form.get("rotation_due_at")) : null,
    policy: policyBody(form),
    guardrail_policy: guardrailPolicy,
  };
  if (form.has("no_expires_at")) {
    body.expires_at = null;
  } else if (form.get("expires_at")) {
    body.expires_at = isoDate(form.get("expires_at"));
  }
  await api(`/admin-ui/admin/keys/${keyId}`, { method: "PATCH", body: JSON.stringify(body) });
  state.editingKeyId = null;
  setNotice("Virtual key updated.", "success");
  await keys();
}

async function simulatePolicy(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const path = String(form.get("path") || "");
  const provider = form.get("provider") || null;
  const serviceMode = provider === "internal-service" || path.startsWith("/services/");
  const serviceName = serviceMode ? form.get("service_name") || null : null;
  clearPolicySimulationResult();
  const servicePathError = validatePolicySimulationServicePath(path, serviceName);
  if (servicePathError) {
    setNotice(servicePathError);
    return;
  }
  const body = {
    key_id: form.get("key_id") || null,
    team_id: form.get("team_id") || null,
    path,
    provider,
    service_name: serviceName,
    request_body_bytes: nullableNumber(form.get("request_body_bytes")),
    response_body_bytes: nullableNumber(form.get("response_body_bytes")),
    body: {
      model: serviceName ? undefined : form.get("model") || undefined,
      stream: form.has("stream"),
      tools: form.has("tools") ? [{ type: "function" }] : undefined,
    },
  };
  if (!body.key_id) delete body.key_id;
  if (!body.team_id) delete body.team_id;
  if (!body.provider) delete body.provider;
  if (!body.service_name) delete body.service_name;
  if (body.request_body_bytes === null) delete body.request_body_bytes;
  if (body.response_body_bytes === null) delete body.response_body_bytes;
  clearPolicySimulationResult();
  state.policySimulation = await api("/admin-ui/admin/policy/simulate", { method: "POST", body: JSON.stringify(body) });
  document.querySelector("#policy-sim-result").innerHTML = policySimulationResult();
}

function clearPolicySimulationResult() {
  state.policySimulation = null;
  const result = document.querySelector("#policy-sim-result");
  if (result) result.innerHTML = policySimulationResult();
}

function validatePolicySimulationServicePath(path, serviceName) {
  const trimmedPath = path.trim();
  if (trimmedPath.includes("*")) {
    return "Choose a concrete service path such as /services/service-name/test.";
  }
  if (trimmedPath === "/services" || trimmedPath === "/services/") {
    return "Choose a concrete service path such as /services/service-name/test.";
  }
  if (!serviceName) return null;
  const service = state.services.find((item) => item.name === serviceName);
  if (service?.route_pattern && policySimulationPathMatchesRoutePattern(trimmedPath, service.route_pattern)) {
    return null;
  }
  const segments = trimmedPath.split("/").filter(Boolean);
  if (segments[0] !== "services" || !segments[1]) {
    const expectedRoute = service?.route_pattern || `/services/${serviceName}`;
    return `Use a concrete path matching ${expectedRoute} or /services/${serviceName}/... when simulating ${serviceName}.`;
  }
  if (segments[1] !== serviceName) {
    return `Path service ${segments[1]} does not match selected service ${serviceName}.`;
  }
  return null;
}

function policySimulationPathMatchesRoutePattern(path, routePattern) {
  const prefix = routePattern.endsWith("/*") ? routePattern.slice(0, -2) : null;
  if (prefix) {
    return path === prefix || path.startsWith(`${prefix}/`);
  }
  return path === routePattern;
}

async function savePolicyLayer(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  let guardrailPolicy;
  try {
    guardrailPolicy = guardrailPolicyBody(form);
  } catch (error) {
    setNotice(error.message);
    return;
  }
  const body = {
    kind: form.get("kind"),
    scope_id: form.get("kind") === "global" ? null : form.get("scope_id"),
    policy: policyBody(form),
    guardrail_policy: guardrailPolicy,
  };
  await api("/admin-ui/admin/policy-layers", { method: "POST", body: JSON.stringify(body) });
  setNotice("Policy layer saved.", "success");
  await keys();
}

async function policyLayerAction(event) {
  const { policyLayerAction: action, layerId } = event.currentTarget.dataset;
  if (action !== "delete") return;
  if (!(await confirmAction("Delete policy layer", "Keys will immediately fall back to lower-priority inherited policy."))) return;
  await api(`/admin-ui/admin/policy-layers/${layerId}`, { method: "DELETE" });
  setNotice("Policy layer deleted.", "success");
  await keys();
}

async function audit() {
  const renderId = ++renderGeneration;
  const formMarkup = `
    <div class="form-grid">
    <label>Action<input name="action" placeholder="operator_token.rotate"></label>
    <label>Target type<input name="target_type" placeholder="key, policy_layer, provider"></label>
    <label>Target ID<input name="target_id"></label>
    <label>Operator token ID<input name="actor_token_id"></label>
    <label>Member ID<input name="actor_member_id"></label>
    <label>Limit<input name="limit" type="number" min="1" max="500" value="100"></label>
    </div>
    <div class="form-actions"><button class="primary">Apply</button></div>
  `;
  if (renderId !== renderGeneration) return;
  content.innerHTML = `<section class="page-insights" id="audit-insights" aria-label="Audit sample analysis" aria-busy="true"><p class="subtle">Loading audit sample…</p></section>${auditLogTemplate(formMarkup, '<div id="audit-results"></div>')}`;
  document.querySelector("[data-filter-form]").addEventListener("submit", handleAsync(loadAuditEvents));
  await loadAuditEvents();
  organizeView("audit");
}

async function loadAuditEvents(event) {
  event?.preventDefault();
  const form = event ? new FormData(event.target) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["action", "target_type", "target_id", "actor_token_id", "actor_member_id", "limit"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  const insights = document.querySelector("#audit-insights");
  insights?.setAttribute("aria-busy", "true");
  try {
    state.auditEvents = await api(`/admin-ui/admin/audit-events?${query}`);
  } catch (error) {
    if (insights?.isConnected) {
      insights.innerHTML = '<p class="subtle" role="status">Audit sample unavailable. Any previous results below may reflect earlier filters.</p>';
      insights.setAttribute("aria-busy", "false");
    }
    throw error;
  }
  const results = document.querySelector("#audit-results");
  if (results) {
    const visual = auditSampleSnapshot(state.auditEvents);
    if (insights?.isConnected) {
      insights.innerHTML = `<div class="page-insights-grid">${insightStrip(visual.insights, { label: "Loaded audit sample" })}${distributionBars("Actions in sample", visual.actions, { caption: "Returned events only · filtered and limited, not a lifetime total", empty: "No events match this sample." })}</div>`;
      insights.setAttribute("aria-busy", "false");
    }
    results.innerHTML = auditEventTable(state.auditEvents);
  }
}

function auditEventTable(rows) {
  return table(
    ["Time", "Actor", "Action", "Target", "Request", "IP", "User agent", "Snapshots"],
    rows.map((row) => [
      time(row.created_at),
      `<code>${esc(row.actor_member_id ? `member:${row.actor_member_id}` : row.actor_token_id ? `token:${row.actor_token_id}` : "system")}</code>`,
      `<span class="audit-action"><i class="ti ti-list-details" aria-hidden="true"></i>${badge(row.action)}</span>`,
      `<strong>${esc(row.target_type)}</strong><div class="subtle"><code>${esc(row.target_id || "")}</code></div>`,
      `<code>${esc(row.request_id || "")}</code>`,
      esc(row.ip || ""),
      esc(row.user_agent || ""),
      `<details><summary><i class="ti ti-arrows-diff" aria-hidden="true"></i> Before/after</summary>${jsonBlock({ before: row.before, after: row.after })}</details>`,
    ]),
  );
}

function keyOwnerLabel(key) {
  if (key.owner_type === "individual") return '<span class="badge">individual</span>';
  return `<strong>${esc(projectName(key.project_id))}</strong><div class="subtle"><code>${esc(key.project_id || "")}</code></div>`;
}

async function keyAction(event) {
  const { keyAction: action, keyId } = event.currentTarget.dataset;
  if (action === "edit") {
    state.editingKeyId = keyId;
    await keys();
    return;
  }
  if (action === "cancel-edit") {
    state.editingKeyId = null;
    await keys();
    return;
  }
  if (action === "spend") {
    await showLiteLlmSpend(keyId);
    return;
  }
  if (action === "usage") {
    const projectScope = state.keys.find((key) => key.id === keyId)?.project_id || "";
    await navigateToView("usage", { monitoring: { projectScope, usageFilters: { ...state.usageFilters, project_id: projectScope, key_id: keyId } } });
    return;
  }
  if (!(await confirmAction(`${action} virtual key`, "This lifecycle change is written to the database."))) return;
  await api(`/admin-ui/admin/keys/${keyId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`Virtual key ${action}d.`, "success");
  await keys();
}

async function showLiteLlmSpend(keyId) {
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
    <h3 id="${titleId}">LiteLLM reported spend</h3>
    <p>Virtual key <code>${esc(keyId)}</code></p>
    <div data-spend-result role="status" aria-live="polite"></div>
    <p class="help">Current LiteLLM counter in USD, since its last reset. Includes all traffic using the mapped LiteLLM key, including outside Gateway. Date filters do not apply. Shared credentials cannot provide an exclusive per-key cost. Gateway usage estimates and budgets remain separate.</p>
    <div class="form-actions"><button type="button" data-refresh-spend>Refresh spend</button><button type="button" data-close-modal>Close</button></div>
  </div>`;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
  const result = backdrop.querySelector("[data-spend-result]");
  const refresh = backdrop.querySelector("[data-refresh-spend]");
  const load = async () => {
    refresh.disabled = true;
    result.textContent = "Loading reported spend…";
    try {
      const snapshot = await api(`/admin-ui/admin/keys/${encodeURIComponent(keyId)}/litellm-spend`);
      if (!backdrop.isConnected) return;
      result.innerHTML = liteLlmSpendContent(snapshot);
    } catch (error) {
      if (backdrop.isConnected) result.textContent = "Spend unavailable. Refresh to retry.";
    } finally {
      refresh.disabled = false;
    }
  };
  refresh.addEventListener("click", load);
  await load();
}

function liteLlmSpendContent(snapshot) {
  if (snapshot.status === "not_mapped") return emptyState("No enabled LiteLLM credential mapping. Configure a key or project mapping in Providers for gateway-managed endpoints.");
  const scope = snapshot.mapping_scope === "project" ? "Shared project mapping" : "Key mapping";
  if (snapshot.status !== "available" || !Number.isFinite(snapshot.spend_usd) || snapshot.spend_usd < 0) return `<p>${esc(scope)}</p><p>Spend unavailable. Check the LiteLLM connection and permission to read key information, then refresh.</p>`;
  return `<div class="stat"><span>LiteLLM reported spend · USD</span><strong>${money(snapshot.spend_usd)}</strong></div><p>${esc(scope)} · Retrieved ${time(snapshot.fetched_at)}</p>`;
}

async function openFoundryEditor(kind, record, trigger) {
  if (kind === "service") state.providers = await api("/admin-ui/admin/providers");
  mountFoundryEditor({ kind, record, restoreFocus: trigger,
    providers: (state.providers || []).filter(p => p.provider === "azure-foundry"), projects: state.projects || [],
    onSave: async body => {
      const collection = kind === "provider" ? "providers" : "services";
      const id = record && (kind === "provider" ? record.id : record.name);
      await api(`/admin-ui/admin/${collection}${id ? `/${encodeURIComponent(id)}` : ""}`, { method: record ? "PATCH" : "POST", body: JSON.stringify(body) });
      setNotice(kind === "provider" ? "Foundry connection saved." : "Foundry service saved. Configure caller profiles and service limits with Edit.", "success");
      if (kind === "provider") await providers(); else await services();
    },
  });
}

async function providers() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/providers"),
    api("/admin-ui/admin/providers/litellm-credentials"),
    api("/admin-ui/admin/providers/litellm-passthrough"),
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/projects"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.providers, state.litellmCredentialMappings, state.litellmPassthroughSettings, state.keys, state.projects] = loaded;
  if (renderId !== renderGeneration) return;
  const insights = providerCatalogInsights(state.providers, state.litellmCredentialMappings, state.litellmPassthroughSettings);
  content.innerHTML = `
    <section class="page-insights" aria-label="Provider authentication coverage">
      <div class="page-insights-grid">
        ${insightStrip(insights.items, { label: "Credential routing" })}
        ${distributionBars("Provider setup", insights.distribution, { caption: insights.caption, empty: "No provider connections registered." })}
      </div>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Provider configuration</h3><span class="subtle">${state.providers.length} total</span></div>
      ${providerTable(state.providers)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>LiteLLM credential mappings</h3><span class="subtle">${state.litellmCredentialMappings.length} total</span></div>
      ${litellmCredentialMappingTable(state.litellmCredentialMappings)}
      <details class="catalog-disclosure"><summary><i class="ti ti-key" aria-hidden="true"></i> Create or replace a credential mapping</summary>
      <form id="litellm-credential-form" class="form-grid">
        <label>Scope<select name="scope" data-litellm-mapping-scope>${option("key", "key")}${option("project", "")}</select></label>
        <label>Key<select name="key_target_id" data-litellm-key-target>${keyOptions()}</select></label>
        <label>Project<select name="project_target_id" data-litellm-project-target>${projectOptions()}</select></label>
        <label>LiteLLM virtual key<input name="credential" type="password" autocomplete="new-password" required></label>
        <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
        <div class="form-actions"><button class="primary">Save mapping</button></div>
      </form>
      </details>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>LiteLLM passthrough</h3><span class="subtle">single ingress mode</span></div>
      <details class="catalog-disclosure"><summary><i class="ti ti-arrows-split" aria-hidden="true"></i> Configure wildcard ingress</summary>${litellmPassthroughForm(state.litellmPassthroughSettings)}</details>
    </section>
  `;
  document.querySelectorAll("[data-foundry-provider]").forEach(button => button.addEventListener("click", handleAsync(() => openFoundryEditor("provider", state.providers.find(p => p.id === button.dataset.foundryProvider), button))));
  document.querySelectorAll("[data-foundry-check]").forEach(button => button.addEventListener("click", () => {
    const provider = state.providers.find(p => p.id === button.dataset.foundryCheck);
    mountFoundryCheck({ name: provider.name, restoreFocus: button, onCheck: () => api(`/admin-ui/admin/providers/${provider.id}/verify-connection`, { method: "POST" }) });
  }));
  document.querySelector("#litellm-credential-form").addEventListener("submit", handleAsync(saveLiteLlmCredentialMapping));
  document.querySelector("#litellm-passthrough-form").addEventListener("submit", handleAsync(saveLiteLlmPassthroughSettings));
  document.querySelector("[data-litellm-mapping-scope]").addEventListener("change", updateLiteLlmMappingTargetVisibility);
  updateLiteLlmMappingTargetVisibility();
  document.querySelectorAll("[data-provider-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(providerAction));
  });
  document.querySelectorAll("[data-provider-config-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(updateProviderAuthSettings));
  });
  document.querySelectorAll("[data-litellm-mapping-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(liteLlmCredentialMappingAction));
  });
  organizeView("providers");
  content.prepend(content.querySelector(".page-insights"));
  const createButton = document.createElement("button");
  createButton.type = "button";
  createButton.className = "primary";
  createButton.textContent = "Create provider";
  createButton.addEventListener("click", () => mountProviderCreate({ restoreFocus: createButton, onSave: createProvider }));
  document.querySelector("#page-actions").appendChild(createButton);
}

function litellmPassthroughForm(settings) {
  const current = settings || {};
  return `<form id="litellm-passthrough-form" class="form-grid">
    ${formSection("Ingress allowlist", "Restrict wildcard forwarding by path, method, timeout, and payload size.", `
      <label class="check"><input name="enabled" type="checkbox" ${current.enabled ? "checked" : ""}> Enable wildcard passthrough</label>
      <label>Allowed paths<input name="allowed_paths" value="${attr(listValue(current.allowed_paths, "/v1/*"))}"></label>
      <label>Blocked paths<input name="blocked_paths" aria-describedby="litellm-blocked-help" value="${attr(listValue(current.blocked_paths, ""))}"></label>
      <p id="litellm-blocked-help" class="field-hint wide-field">Comma-separated paths. Deny overrides allow, including /*, and covers direct LiteLLM routes. Use /config and /config/* to block a subtree and its root. Blank means no explicit denies.</p>
      <label>Allowed methods<input name="allowed_methods" value="${attr(listValue(current.allowed_methods, "GET,POST"))}"></label>
      <label>API authentication<select name="authentication_mode" aria-describedby="litellm-auth-help">
        <option value="gateway" ${current.authentication_mode !== "litellm_bearer" ? "selected" : ""}>Gateway</option>
        <option value="litellm_bearer" ${current.authentication_mode === "litellm_bearer" ? "selected" : ""}>Caller LiteLLM key</option>
      </select></label>
      <p id="litellm-auth-help" class="field-hint wide-field">Caller-key mode forwards only the supplied LiteLLM key, without a Relayna key or stored-key fallback. UI exposure is independent. Mandatory route profiles or inherited identity requirements conflict and reject requests; operator-only paths remain closed.</p>
      <label>Timeout ms<input name="timeout_ms" type="number" min="1" max="600000" value="${attr(current.timeout_ms ?? 120000)}"></label>
      <label>Max request bytes<input name="max_request_body_bytes" type="number" min="1" max="104857600" value="${attr(current.max_request_body_bytes ?? 1048576)}"></label>
      <label>Max response bytes<input name="max_response_body_bytes" type="number" min="1" max="104857600" value="${attr(current.max_response_body_bytes ?? 1048576)}"></label>
    `, true)}
    ${formSection("Administrative exposure", "Keep LiteLLM UI and control endpoints closed unless explicitly required.", `
      <label>LiteLLM UI exposure<select name="ui_exposure">
        ${option("disabled", current.ui_exposure || "disabled")}
        ${option("operator_only", current.ui_exposure || "")}
        ${option("explicitly_exposed", current.ui_exposure || "")}
        ${option("trusted_ingress", current.ui_exposure || "")}
      </select></label>
      <label>LiteLLM admin API exposure<select name="admin_api_exposure">
        ${option("disabled", current.admin_api_exposure || "disabled")}
        ${option("operator_only", current.admin_api_exposure || "")}
        ${option("explicitly_exposed", current.admin_api_exposure || "")}
      </select></label>
      <div class="notice warn wide-field"><strong>Exposure risk</strong><span>/ui, key, config, user/team, spend, and other LiteLLM admin endpoints stay blocked unless explicitly exposed.</span></div>
    `)}
    <div class="form-actions sticky-form-actions wide-field"><button class="primary">Save passthrough settings</button></div>
  </form>`;
}

function providerTable(rows) {
  return table(
    ["Provider", "Endpoint", "State", "Credential", "Authentication", "Updated", "Actions"],
    rows.map((row) => [
      `<strong><i class="ti ti-cloud" aria-hidden="true"></i> ${esc(row.name)}</strong><div class="subtle">${esc(row.provider)}</div>`,
      `<code>${esc(row.base_url)}</code>`,
      badge(row.enabled === true ? "enabled" : row.enabled === false ? "disabled" : "unknown", row.enabled === true ? "good" : row.enabled === false ? "bad" : "neutral"),
      badge(catalogCredentialState(row).label, catalogCredentialState(row).tone),
      row.provider === "litellm" ? `<details class="catalog-disclosure"><summary><i class="ti ti-shield-lock" aria-hidden="true"></i> Credential settings</summary>${providerAuthSettingsForm(row)}</details>` : providerAuthSettingsForm(row),
      time(row.updated_at),
      `<div class="actions">
        <button data-provider-action="${row.enabled ? "disable" : "enable"}" data-provider-id="${attr(row.id)}" aria-label="${row.enabled ? "Disable" : "Enable"} provider ${attr(row.name)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-provider-action="delete" data-provider-id="${attr(row.id)}" aria-label="Delete provider ${attr(row.name)}">Delete</button>
      </div>`,
    ]),
  );
}

function providerAuthSettingsForm(row) {
  if (row.provider === "azure-foundry") return `<div class="actions"><button type="button" data-foundry-provider="${attr(row.id)}">Configure Azure identity</button><button type="button" data-foundry-check="${attr(row.id)}">Verify connection</button></div>`;
  if (row.provider !== "litellm") {
    return '<span class="subtle">not applicable</span>';
  }
  return `<form class="inline-form" data-provider-config-form data-provider-id="${attr(row.id)}" aria-label="Authentication settings for provider ${attr(row.name)}">
    <label>Credential mode<select name="credential_header_mode">
      ${option("authorization_bearer", row.credential_header_mode || "authorization_bearer")}
      ${option("custom_header", row.credential_header_mode || "")}
    </select></label>
    <label>Custom header<input name="credential_header_name" placeholder="x-litellm-api-key" value="${attr(row.credential_header_name || "")}"></label>
    <label>Header value<select name="credential_header_value_format">
      ${option("raw", row.credential_header_value_format || "raw")}
      ${option("bearer", row.credential_header_value_format || "")}
    </select></label>
    <label>Replace credential<input name="credential" type="password" autocomplete="new-password" placeholder="Leave blank to keep current credential"></label>
    <button type="submit" aria-label="Update authentication for provider ${attr(row.name)}">Update</button>
    <span class="subtle">x-litellm-key usually needs bearer.</span>
  </form>`;
}

function litellmCredentialMappingTable(rows) {
  return table(
    ["Scope", "Target", "State", "Credential", "Updated", "Actions"],
    rows.map((row) => [
      `<span class="badge neutral"><i class="ti ti-${row.scope === "project" ? "folder" : "key"}" aria-hidden="true"></i> ${esc(row.scope)}</span>`,
      `<strong>${esc(row.target_label || mappingTargetName(row))}</strong><div class="subtle"><code>${esc(row.target_id)}</code></div>`,
      badge(row.enabled === true ? "enabled" : row.enabled === false ? "disabled" : "unknown", row.enabled === true ? "good" : row.enabled === false ? "bad" : "neutral"),
      badge(catalogCredentialState(row).label, catalogCredentialState(row).tone),
      time(row.updated_at),
      `<div class="actions">
        <button data-litellm-mapping-action="${row.enabled ? "disable" : "enable"}" data-mapping-id="${attr(row.id)}" aria-label="${row.enabled ? "Disable" : "Enable"} LiteLLM mapping for ${attr(row.target_label || mappingTargetName(row))}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-litellm-mapping-action="delete" data-mapping-id="${attr(row.id)}" aria-label="Delete LiteLLM mapping for ${attr(row.target_label || mappingTargetName(row))}">Delete</button>
      </div>`,
    ]),
  );
}

async function createProvider(body) {
  await api("/admin-ui/admin/providers", { method: "POST", body: JSON.stringify(body) });
  setNotice("Provider saved.", "success");
  await providers();
}

async function updateProviderAuthSettings(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const providerId = formElement.dataset.providerId;
  const form = new FormData(formElement);
  const credentialHeaderMode = form.get("credential_header_mode");
  const body = {
    credential_header_mode: credentialHeaderMode,
    credential_header_name: credentialHeaderMode === "custom_header" ? nullableString(form.get("credential_header_name")) : null,
    credential_header_value_format: form.get("credential_header_value_format"),
  };
  const credential = blankToUndefined(form.get("credential"));
  if (credential) body.credential = credential;
  await api(`/admin-ui/admin/providers/${providerId}`, {
    method: "PATCH",
    body: JSON.stringify(body),
  });
  setNotice("Provider auth settings updated.", "success");
  await providers();
}

async function providerAction(event) {
  const { providerAction: action, providerId } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} provider`, "This provider configuration change is written to the database."))) return;
  if (action === "delete") {
    await api(`/admin-ui/admin/providers/${providerId}`, { method: "DELETE" });
  } else {
    await api(`/admin-ui/admin/providers/${providerId}/${action}`, { method: "POST", body: "{}" });
  }
  setNotice(`Provider ${action}d.`, "success");
  await providers();
}

async function saveLiteLlmCredentialMapping(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const scope = form.get("scope");
  const targetId = scope === "project" ? form.get("project_target_id") : form.get("key_target_id");
  await api("/admin-ui/admin/providers/litellm-credentials", {
    method: "POST",
    body: JSON.stringify({
      scope,
      target_id: targetId,
      credential: blankToUndefined(form.get("credential")),
      enabled: form.has("enabled"),
    }),
  });
  setNotice("LiteLLM credential mapping saved.", "success");
  await providers();
}

async function liteLlmCredentialMappingAction(event) {
  const { litellmMappingAction: action, mappingId } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} LiteLLM credential mapping`, "This changes upstream credential selection."))) return;
  if (action === "delete") {
    await api(`/admin-ui/admin/providers/litellm-credentials/${mappingId}`, { method: "DELETE" });
  } else {
    await api(`/admin-ui/admin/providers/litellm-credentials/${mappingId}/${action}`, { method: "POST", body: "{}" });
  }
  setNotice(`LiteLLM credential mapping ${action}d.`, "success");
  await providers();
}

async function saveLiteLlmPassthroughSettings(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  await api("/admin-ui/admin/providers/litellm-passthrough", {
    method: "PATCH",
    body: JSON.stringify({
      enabled: form.has("enabled"),
      authentication_mode: form.get("authentication_mode"),
      blocked_paths: csv(form.get("blocked_paths")),
      allowed_paths: csv(form.get("allowed_paths")),
      allowed_methods: csv(form.get("allowed_methods")).map((method) => method.toUpperCase()),
      ui_exposure: form.get("ui_exposure"),
      admin_api_exposure: form.get("admin_api_exposure"),
      timeout_ms: nullableNumber(form.get("timeout_ms")),
      max_request_body_bytes: nullableNumber(form.get("max_request_body_bytes")),
      max_response_body_bytes: nullableNumber(form.get("max_response_body_bytes")),
    }),
  });
  setNotice("LiteLLM passthrough settings updated.", "success");
  await providers();
}

function updateLiteLlmMappingTargetVisibility() {
  const scope = document.querySelector("[data-litellm-mapping-scope]")?.value || "key";
  document.querySelector("[data-litellm-key-target]")?.closest("label")?.toggleAttribute("hidden", scope !== "key");
  document.querySelector("[data-litellm-project-target]")?.closest("label")?.toggleAttribute("hidden", scope !== "project");
}

async function routes() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/openai-routes"),
    api("/admin-ui/admin/anthropic-routes"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/route-identities"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.openaiRoutes, state.anthropicRoutes, state.services, state.routeIdentities] = loaded;
  if (renderId !== renderGeneration) return;
  const insights = routeCatalogInsights(state.openaiRoutes, state.anthropicRoutes, state.services, state.routeIdentities);
  content.innerHTML = `
    <section class="page-insights" aria-label="Route configuration and precedence">
      <div class="page-insights-grid">
        ${insightStrip(insights.items, { label: "Routing configuration" })}
        ${distributionBars("Canonical forwarding modes", insights.distribution, { caption: insights.caption, empty: "No canonical route settings loaded." })}
      </div>
      ${flowSummary("Route precedence", insights.steps, "Control-plane routes are reserved. LiteLLM fallback applies only when configured and allowed.")}
    </section>
    <section class="panel"><div class="panel-heading"><h3>Additional endpoint identity</h3></div>
      <p class="field-hint">Per-endpoint requirements. Registered services use their own saved identity settings.</p>
      ${table(["Route", "Protocol", "Identity", "Actions"], state.routeIdentities.filter((row) => ![...state.openaiRoutes, ...state.anthropicRoutes].some((item) => item.route === row.route)).map((row) => [`<code>${esc(row.route)}</code>`, row.access?.websocket ? '<span class="badge">HTTP + WS</span><div class="subtle">Caller-owned LiteLLM keys only</div>' : '<span class="badge">HTTP</span>', endpointIdentityBadge(row.access), routeIdentityButton(row.route)]))}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>${routeFamilyLogo("openai")} OpenAI-compatible and rerank routes</h3>
        <span class="subtle">${state.openaiRoutes.length} total</span>
      </div>
      ${providerRouteTable(state.openaiRoutes, "openai")}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>${routeFamilyLogo("anthropic")} Anthropic Claude routes</h3>
        <span class="subtle">${state.anthropicRoutes.length} total</span>
      </div>
      ${providerRouteTable(state.anthropicRoutes, "anthropic")}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Registered service routes</h3><span class="subtle">${state.services.length} total</span></div>
      ${serviceRouteTable(state.services)}
    </section>
  `;
  document.querySelectorAll("[data-openai-route-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(openaiRouteAction));
  });
  document.querySelectorAll("[data-openai-route-mode-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(saveOpenAiRouteMode));
  });
  document.querySelectorAll("[data-anthropic-route-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(anthropicRouteAction));
  });
  document.querySelectorAll("[data-anthropic-route-mode-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(saveAnthropicRouteMode));
  });
  document.querySelectorAll("[data-service-route-timeout-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(saveServiceRouteTimeout));
  });
  document.querySelectorAll("[data-route-identity]").forEach((button) => button.addEventListener("click", editRouteIdentity));
  organizeView("routes");
  content.prepend(content.querySelector(".page-insights"));
}

function routeFamilyLogo(family) {
  const label = family === "anthropic" ? "A" : "OA";
  const title = family === "anthropic" ? "Anthropic" : "OpenAI";
  return `<span class="route-logo ${family}" title="${title}" aria-label="${title}">${label}</span>`;
}

function providerRouteTable(rows, family) {
  const actionAttr = family === "anthropic" ? "data-anthropic-route-action" : "data-openai-route-action";
  const modeForm = family === "anthropic" ? anthropicRouteModeForm : openaiRouteModeForm;
  return table(
    ["Route", "Protocol", "Identity", "State", "Configuration", "Updated", "Actions"],
    rows.map((row) => [
      `<strong><i class="ti ti-route" aria-hidden="true"></i> ${esc(row.route_id)}</strong><div class="subtle"><code>${esc(row.route)}</code></div>`,
      row.route === "/v1/responses" && row.mode === "direct_litellm_passthrough" && state.routeIdentities.some((setting) => setting.route === row.route && setting.access?.websocket) ? '<span class="badge">HTTP + WS</span><div class="subtle">Caller-owned LiteLLM keys only</div>' : '<span class="badge">HTTP</span>',
      routeIdentityControl(row.route),
      badge(row.enabled === true ? "enabled" : row.enabled === false ? "disabled" : "unknown", row.enabled === true ? "good" : row.enabled === false ? "bad" : "neutral"),
      modeForm(row),
      time(row.updated_at),
      `<div class="actions">
        <button ${actionAttr}="${row.enabled ? "disable" : "enable"}" data-route-id="${attr(row.route_id)}" aria-label="${row.enabled ? "Disable" : "Enable"} route ${attr(row.route)}">${row.enabled ? "Disable" : "Enable"}</button>
      </div>`,
    ]),
  );
}

function openaiRouteModeForm(row) {
  return routeConfigForm(row, "data-openai-route-mode-form");
}

function anthropicRouteModeForm(row) {
  return routeConfigForm(row, "data-anthropic-route-mode-form");
}

function routeConfigForm(row, dataAttrName) {
  return `<form class="inline-form route-config-form" ${dataAttrName} data-route-id="${attr(row.route_id)}">
    <label class="route-config-field">Mode<select name="mode">
      ${option("managed_by_gateway", row.mode || "managed_by_gateway")}
      ${option("direct_litellm_passthrough", row.mode || "")}
    </select></label>
    <label class="route-config-field">Timeout ms<input name="timeout_ms" type="number" min="1" max="600000" value="${attr(row.timeout_ms ?? 120000)}" title="Timeout ms"></label>
    <label class="route-config-field">Max request bytes<input name="max_request_body_bytes" type="number" min="1" max="104857600" value="${attr(row.max_request_body_bytes ?? 1048576)}" title="Max request bytes"></label>
    <label class="route-config-field">Max response bytes<input name="max_response_body_bytes" type="number" min="1" max="104857600" value="${attr(row.max_response_body_bytes ?? 1048576)}" title="Max response bytes"></label>
    <button type="submit">Save</button>
  </form>`;
}

function serviceRouteTable(rows) {
  return table(
    ["Service", "Route", "Protocol", "Identity", "State", "Methods", "Upstream", "Timeout", "Health check", "Credential"],
    rows.map((row) => [
      `<strong class="service-name"><i class="ti ti-apps" aria-hidden="true"></i><span>${esc(row.name)}</span></strong><div class="subtle">${esc(row.source)}</div>`,
      `<code>${esc(row.route_pattern)}</code>`,
      `<i class="ti ti-plug-connected" aria-hidden="true"></i> ${serviceProtocolSummary(row)}`,
      endpointIdentityBadge(row.access),
      serviceBadges(row),
      esc(listValue(row.allowed_methods, "none")),
      esc(row.foundry ? `Azure Foundry · ${row.foundry.mode === "registered_agent" ? row.foundry.agent_name : "Responses passthrough"}` : row.upstream_base_url || "missing"),
      serviceRouteTimeoutForm(row),
      esc(row.foundry ? "Not probed" : healthCheckLabel(row)),
      row.foundry ? '<span class="badge">Azure identity</span>' : row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
    ]),
  );
}

function serviceRouteTimeoutForm(row) {
  return `<form class="inline-form route-config-form" data-service-route-timeout-form data-service-name="${attr(row.name)}">
    <label class="route-config-field">Timeout ms<input name="timeout_ms" type="number" min="1" max="600000" step="1" required value="${attr(row.timeout_ms)}" title="Timeout ms"></label>
    <button type="submit">Save</button>
  </form>`;
}

async function openaiRouteAction(event) {
  const { routeId, openaiRouteAction: action } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} ${routeId}`, "This gateway route change is written to the database."))) return;
  await api(`/admin-ui/admin/openai-routes/${routeId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`OpenAI route ${action}d.`, "success");
  await routes();
}

async function anthropicRouteAction(event) {
  const { routeId, anthropicRouteAction: action } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} ${routeId}`, "This gateway route change is written to the database."))) return;
  await api(`/admin-ui/admin/anthropic-routes/${routeId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`Anthropic route ${action}d.`, "success");
  await routes();
}

async function saveOpenAiRouteMode(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const routeId = formElement.dataset.routeId;
  const form = new FormData(formElement);
  await api(`/admin-ui/admin/openai-routes/${routeId}/config`, {
    method: "PATCH",
    body: JSON.stringify(routeConfigPayload(form)),
  });
  setNotice("OpenAI route configuration updated.", "success");
  await routes();
}

async function saveAnthropicRouteMode(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const routeId = formElement.dataset.routeId;
  const form = new FormData(formElement);
  await api(`/admin-ui/admin/anthropic-routes/${routeId}/config`, {
    method: "PATCH",
    body: JSON.stringify(routeConfigPayload(form)),
  });
  setNotice("Anthropic route configuration updated.", "success");
  await routes();
}

async function saveServiceRouteTimeout(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const serviceName = formElement.dataset.serviceName;
  const form = new FormData(formElement);
  const timeoutMs = Number(form.get("timeout_ms"));
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 600000) {
    throw new Error("Timeout must be a whole number between 1 and 600000 ms.");
  }
  await api(`/admin-ui/admin/services/${serviceName}`, {
    method: "PATCH",
    body: JSON.stringify({ timeout_ms: timeoutMs }),
  });
  setNotice(`Service ${serviceName} timeout updated.`, "success");
  await routes();
}

function routeConfigPayload(form) {
  return {
    mode: form.get("mode"),
    timeout_ms: nullableNumber(form.get("timeout_ms")),
    max_request_body_bytes: nullableNumber(form.get("max_request_body_bytes")),
    max_response_body_bytes: nullableNumber(form.get("max_response_body_bytes")),
  };
}

function serviceTypePresets() {
  return [
    { id: "internal_http", label: "Internal HTTP service", description: "General HTTP API using the gateway identity setting. Adjust methods and Entra requirements for your service.", methods: ["GET", "POST"], mode: "inherit", timeout: 60000 },
    { id: "relayna_http", label: "Relayna HTTP service", description: "Relayna runtime exposed through its HTTP API. Point Upstream URL at the runtime; requests use /services/{name}/*. The runtime owns tasks and agent execution. Import from Studio if the service already exists there.", methods: ["GET", "POST"], mode: "inherit", timeout: 120000, health: "/health" },
    { id: "entra_http", label: "Entra-protected HTTP service", description: "HTTP API requiring its own Entra audience. Enter the audience and any required scopes, roles or groups below.", methods: ["GET", "POST"], mode: "required", timeout: 60000 },
    { id: "apigee_http", label: "Apigee-backed HTTP service", description: "HTTP API requiring Entra identity, also accepting signed Apigee claims. Enter this endpoint’s audience. Trusted Apigee verification must be configured in gateway settings.", methods: ["GET", "POST"], mode: "required", apigee: true, timeout: 60000 },
    { id: "accessa_channel", label: "Accessa app/channel (HTTP + WebSocket)", description: "BFF connects through the gateway to the channel adapter. App and channel generate the route; only GET /run supports WebSocket. PUT supports feedback and settings; PATCH supports metadata edits. Enter the Accessa Entra audience and bind a virtual key to this service.", methods: ["GET", "POST", "PUT", "PATCH", "DELETE"], mode: "required", accessa: true, app: "tara", channel: "web", timeout: 60000 },
    { id: "accessa_discovery", label: "Accessa channel discovery (HTTP)", description: "Channel identity discovery at /channel/{channel}/v1/me. App stays blank; enter the channel and its Entra audience. This endpoint does not open WebSockets.", methods: ["GET"], mode: "required", accessa: true, app: "", channel: "web", timeout: 60000 },
    { id: "custom", label: "Custom configuration", description: "Configure HTTP, opt-in WebSocket transport, or an Accessa route directly. Keeps current values so you can customize a preset." },
  ];
}

function suggestedServiceRoute(type, name, app, channel) {
  if (type === "accessa_channel") return app && channel ? `/app/${app}/channel/${channel}/v1/*` : "";
  if (type === "accessa_discovery") return channel ? `/channel/${channel}/v1/me` : "";
  return type && type !== "custom" && name ? `/services/${name}/*` : "";
}

function applyServiceTypePreset(form, type) {
  const preset = serviceTypePresets().find((item) => item.id === type);
  if (!preset) return;
  form.querySelector("#service-type-help").textContent = preset.description;
  if (type === "custom") return;
  const set = (name, value) => { form.elements.namedItem(name).value = value; };
  const check = (name, value) => { form.elements.namedItem(name).checked = value; };
  form.querySelectorAll('[name="allowed_methods"]').forEach((input) => { input.checked = preset.methods.includes(input.value); });
  set("timeout_ms", preset.timeout);
  set("health_check_path", preset.health || "");
  const identityMode = form.elements.namedItem("profiles_saved") ? "profiles" : preset.mode;
  set("endpoint_entra_mode", identityMode);
  for (const field of ["endpoint_audience", "endpoint_scopes", "endpoint_roles", "endpoint_groups"]) set(field, "");
  check("endpoint_apigee", !!preset.apigee);
  check("accessa_enabled", !!preset.accessa);
  check("websocket_enabled", false);
  set("accessa_app", preset.app || "");
  set("accessa_channel", preset.channel || "");
  set("socket_idle_ms", 60000);
  set("socket_connections", 100);
  set("socket_key_connections", 10);
  set("socket_frame_bytes", 1048576);
  set("route_pattern", "");
  form.dataset.suggestedRoute = "";
  updateServiceRouteSuggestion(form);
  const identity = form.elements.namedItem("endpoint_entra_mode").closest("details");
  if (identity) identity.open = identityMode !== "inherit";
  updateServiceTransportFields(form);
  form.elements.namedItem("endpoint_entra_mode").dispatchEvent(new Event("change", { bubbles: true }));
  form.elements.namedItem("endpoint_entra_mode").dispatchEvent(new CustomEvent("identity-preset", { bubbles: true, detail: identityMode }));
}

function updateServiceRouteSuggestion(form) {
  const value = (name) => String(form.elements.namedItem(name).value).trim();
  const input = form.elements.namedItem("route_pattern");
  if (input.value && input.value !== form.dataset.suggestedRoute) return;
  const suggested = suggestedServiceRoute(value("service_type"), value("name"), value("accessa_app"), value("accessa_channel"));
  input.value = suggested;
  form.dataset.suggestedRoute = suggested;
}

function updateServiceTransportFields(form) {
  const enabled = form.elements.namedItem("accessa_enabled").checked;
  const forwarding = form.querySelector("[data-user-token-forwarding]");
  if (forwarding) { forwarding.hidden = !enabled; forwarding.disabled = !enabled; }
  for (const name of ["accessa_app", "accessa_channel", "socket_idle_ms", "socket_connections", "socket_key_connections", "socket_frame_bytes"]) {
    form.elements.namedItem(name).closest("label").hidden = !enabled;
  }
}

function bindServiceTypePresets(form) {
  form.elements.namedItem("service_type").addEventListener("change", (event) => applyServiceTypePreset(form, event.target.value));
  for (const name of ["name", "accessa_app", "accessa_channel"]) {
    form.elements.namedItem(name).addEventListener("input", () => updateServiceRouteSuggestion(form));
  }
  bindServiceTransportFields(form);
}

function bindServiceTransportFields(form) {
  const binding = form?.elements.namedItem("accessa_enabled");
  if (!binding) return;
  binding.addEventListener("change", () => updateServiceTransportFields(form));
  updateServiceTransportFields(form);
}

async function services() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([api("/admin-ui/admin/services"), api("/admin-ui/admin/projects")]);
  if (renderId !== renderGeneration) return;
  [state.services, state.projects] = loaded;
  const editing = state.services.find((service) => service.name === state.editingServiceName);
  if (renderId !== renderGeneration) return;
  const insights = serviceCatalogInsights(state.services, state.projects);
  content.innerHTML = `
    <section class="page-insights" aria-label="Service registration coverage">
      <div class="page-insights-grid">
        ${insightStrip(insights.items, { label: "Service relationships and transport" })}
        ${distributionBars("Registration sync state", insights.distribution, { caption: insights.caption, empty: "No services registered." })}
      </div>
    </section>
    <div class="service-stack">
      <section class="panel">
        <div class="panel-heading">
          <h3>Create service</h3>
          <div class="actions"><button type="button" data-foundry-service>Register Foundry service</button><button type="button" data-service-action="studio-import">Import from Studio</button></div>
        </div>
        <form id="service-form" class="form-grid">
          <label class="wide-field">Service type<select name="service_type" required aria-describedby="service-type-help">
            <option value="">Choose a service type…</option>
            ${serviceTypePresets().map((preset) => `<option value="${attr(preset.id)}">${esc(preset.label)}</option>`).join("")}
          </select></label>
          <p id="service-type-help" class="field-hint wide-field" aria-live="polite">Choose a starting configuration, then adjust its settings. Changing type replaces route, methods, timeout and identity defaults; name, upstream, credentials and pricing are kept.</p>
          ${formSection("Identity and routing", "Name the service and define its public route and upstream.", `
            <label>Name<input name="name" required pattern="[a-z0-9]([a-z0-9\\x2d]{0,62}[a-z0-9])?" placeholder="temp-service-2" title="Use lowercase letters, numbers, and hyphens; start and end with a letter or number."></label>
            <label>Route pattern<input name="route_pattern" list="service-routes" placeholder="/services/name/*"></label>
            <label>Upstream URL<input name="upstream_base_url"></label>
            <div class="field"><span>Methods</span><small class="field-hint">HTTP methods clients may use on this service. Select only the methods its endpoints need.</small>${methodSelect(["POST"])}</div>
            <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
          `, true)}
          ${formSection("Reliability and credentials", "Configure health checks, write-only credentials, limits, and fallback.", `
            <label>Health path<input name="health_check_path" placeholder="/health"></label>
            <label>Health method<select name="health_check_method"><option value="GET">GET</option><option value="HEAD">HEAD</option></select></label>
            <label>Credential<input name="credential" type="password" autocomplete="new-password"></label>
            <label>Timeout ms<input name="timeout_ms" type="number" min="1" max="600000" step="1" value="60000"></label>
            <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="2097152"></label>
            <label>Fallback services<input name="fallback_services" placeholder="backup-a,backup-b"></label>
          `)}
          ${endpointAccessFields({})}
          ${formSection("Usage pricing", "Choose the cost source and optional request-matching rules.", `
            <label>Cost mode<select name="cost_mode"><option value="none">None</option><option value="fixed">Fixed</option><option value="passthrough">Passthrough</option></select></label>
            <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" max="999999999999.9999" step="0.00000001"></label>
            <div class="help wide-field">Fixed records the configured estimate per request. Passthrough records provider-reported response cost when the upstream returns one.</div>
            ${pricingRulesEditor([])}
          `)}
          <div class="form-actions sticky-form-actions wide-field">
            <button name="action" value="create" class="primary">Create</button>
          </div>
        </form>
      </section>
      <section class="panel ${editing ? "" : "muted-panel"}">
        ${editing ? serviceEditForm(editing) : `<div class="empty-state"><h3>No service selected</h3></div>`}
      </section>
    </div>
    <section class="panel">
      <div class="panel-heading"><h3>Registered services</h3><span class="subtle">${state.services.length} total</span></div>
      <p class="field-hint">Protocol shows configured transport. Accessa WebSockets use the GET run path; SSE uses HTTP.</p>
      ${serviceTable(state.services)}
    </section>
    <datalist id="service-routes">${serviceRouteOptions()}</datalist>
  `;
  document.querySelector("#service-form").addEventListener("submit", handleAsync(submitService));
  bindServiceTypePresets(document.querySelector("#service-form"));
  document.querySelector("#service-edit-form")?.addEventListener("submit", handleAsync(patchService));
  bindServiceTransportFields(document.querySelector("#service-edit-form"));
  bindServicePriceValidation(document.querySelector("#service-form"));
  bindServicePriceValidation(document.querySelector("#service-edit-form"));
  bindPricingRuleEditors();
  bindEndpointPricingEditors();
  document.querySelectorAll("[data-foundry-service]").forEach(button => button.addEventListener("click", handleAsync(() => openFoundryEditor("service", state.services.find(p => p.name === button.dataset.foundryService), button))));
  if (editing?.foundry) {
    const form = document.querySelector("#service-edit-form");
    for (const name of ["upstream_base_url", "credential", "clear_credential", "fallback_services", "health_check_path", "health_check_method", "studio_service_id", "sync_status"]) form.elements.namedItem(name).closest("label").hidden = true;
    form.querySelectorAll('[name="allowed_methods"]').forEach(input => { input.disabled = input.value !== "POST"; });
  }
  document.querySelectorAll("[data-service-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(serviceAction));
  });
  prepareProfileKeys(content);
  organizeView("services");
  content.prepend(content.querySelector(".page-insights"));
}

function normalizedPriceDecimal(value) {
  const [mantissa, exponent = "0"] = String(value).toLowerCase().split("e");
  const fraction = mantissa.split(".")[1]?.length || 0;
  const digits = mantissa.replace("-", "").replace(".", "").replace(/^0+/, "");
  const significant = digits.replace(/0+$/, "");
  if (!significant) return "0";
  return `${mantissa.startsWith("-") ? "-" : ""}${significant}e${Number(exponent) - fraction + digits.length - significant.length}`;
}

function validateServicePriceInput(input) {
  const raw = input.value.trim();
  const cost = Number(raw);
  const rounded = raw && (!Number.isFinite(cost) || normalizedPriceDecimal(raw) !== normalizedPriceDecimal(String(cost)));
  input.setCustomValidity(rounded ? "Use an amount that can be stored without rounding." : "");
}

function servicePriceInputs(form) {
  return form.querySelectorAll('[name="estimated_cost_usd"], [data-pricing-rule-field="estimated_cost_usd"], [data-endpoint-field="estimated_cost_usd"]');
}

function bindServicePriceValidation(form) {
  if (!form) return;
  servicePriceInputs(form).forEach(validateServicePriceInput);
  form.addEventListener("input", (event) => {
    if (event.target.matches('[name="estimated_cost_usd"], [data-pricing-rule-field="estimated_cost_usd"], [data-endpoint-field="estimated_cost_usd"]')) validateServicePriceInput(event.target);
  });
}

function validateServicePriceForm(form) {
  servicePriceInputs(form).forEach(validateServicePriceInput);
  return form.reportValidity();
}

function pricingRulesEditor(rules) {
  return `
    <div class="field wide-field pricing-rule-editor" data-pricing-rule-editor>
      <span>Pricing rules</span>
      <input type="hidden" name="pricing_rules" value="${attr(JSON.stringify(rules || []))}">
      <div class="pricing-rule-rows" data-pricing-rule-rows>
        ${(rules || []).map((rule) => pricingRuleRow(rule)).join("")}
        <div class="empty-state pricing-rules-empty" ${(rules || []).length ? "hidden" : ""}>
          <h3>No pricing rules</h3>
        </div>
      </div>
      <div class="actions">
        <button type="button" data-pricing-rule-action="add">Add rule</button>
      </div>
      <div class="help">Use JSON Pointer selectors such as /model or /payload/page_count. Request bodies still use normal key names.</div>
    </div>
  `;
}

function pricingRuleRow(rule = {}) {
  const costMode = rule.cost_mode || "fixed";
  return `
    <div class="pricing-rule-row" data-pricing-rule-row>
      <label>Name<input data-pricing-rule-field="name" value="${attr(rule.name ?? "")}" placeholder="ocr-doc-int"></label>
      <label>JSON pointer<input data-pricing-rule-field="json_pointer" value="${attr(rule.json_pointer ?? rule.path ?? "")}" placeholder="/model"></label>
      <label>Equals<input data-pricing-rule-field="equals" value="${attr(rule.equals ?? "")}" placeholder="doct-int"></label>
      <label>Cost mode<select data-pricing-rule-field="cost_mode">${option("fixed", costMode)}${option("passthrough", costMode)}${option("none", costMode)}</select></label>
      <label>Estimated cost<input data-pricing-rule-field="estimated_cost_usd" type="number" min="0" max="999999999999.9999" step="0.00000001" value="${attr(rule.estimated_cost_usd ?? "")}" placeholder="0.08"></label>
      <button type="button" class="danger" data-pricing-rule-action="remove">Remove</button>
    </div>
  `;
}

function bindPricingRuleEditors() {
  document.querySelectorAll("[data-pricing-rule-editor]").forEach((editor) => {
    syncPricingRuleEditor(editor);
    editor.addEventListener("input", () => syncPricingRuleEditor(editor));
    editor.addEventListener("change", () => syncPricingRuleEditor(editor));
    editor.addEventListener("click", (event) => {
      const button = event.target.closest("[data-pricing-rule-action]");
      if (!button) return;
      if (button.dataset.pricingRuleAction === "add") {
        editor.querySelector("[data-pricing-rule-rows]").insertAdjacentHTML("beforeend", pricingRuleRow());
      } else if (button.dataset.pricingRuleAction === "remove") {
        button.closest("[data-pricing-rule-row]")?.remove();
      }
      syncPricingRuleEditor(editor);
    });
  });
}

function syncPricingRuleEditor(editor) {
  const rows = Array.from(editor.querySelectorAll("[data-pricing-rule-row]"));
  const rules = rows.map(pricingRuleFromRow).filter(Boolean);
  editor.querySelector('input[name="pricing_rules"]').value = JSON.stringify(rules);
  editor.querySelector(".pricing-rules-empty").hidden = rows.length > 0;
}

function pricingRuleFromRow(row) {
  const value = (field) => String(row.querySelector(`[data-pricing-rule-field="${field}"]`)?.value || "").trim();
  const estimatedCost = value("estimated_cost_usd");
  const rule = {
    name: value("name"),
    json_pointer: value("json_pointer"),
    equals: value("equals"),
    cost_mode: value("cost_mode") || "fixed",
    estimated_cost_usd: estimatedCost === "" ? null : Number(estimatedCost),
  };
  if (!rule.name && !rule.json_pointer && !rule.equals && estimatedCost === "") return null;
  if (!rule.name) delete rule.name;
  return rule;
}

function openApiEndpointPricingEditor(service) {
  const pricingHelpId = `endpoint-pricing-help-${service.id || service.name}`;
  const endpoints = service.openapi_endpoints || [];
  const rules = service.endpoint_pricing_rules || [];
  const endpointKey = (method, path) => `${String(method).toUpperCase()} ${path}`;
  const endpointKeys = new Set(endpoints.map((endpoint) => endpointKey(endpoint.method, endpoint.path_template)));
  const rows = rules.map((rule) => {
    const endpoint = endpoints.find((candidate) => endpointKey(candidate.method, candidate.path_template) === endpointKey(rule.method, rule.path_template));
    const stale = !endpointKeys.has(endpointKey(rule.method, rule.path_template));
    const operationId = endpoint?.operation_id || rule.operation_id || "";
    return `
      <tr data-endpoint-pricing-row data-operation-id="${attr(operationId)}">
        <td><code data-endpoint-field="method">${esc(rule.method)}</code></td>
        <td><code data-endpoint-field="path_template">${esc(rule.path_template)}</code><div class="subtle">${esc(operationId)}</div></td>
        <td>${endpoint?.relayna_default ? '<span class="badge good">Relayna default</span>' : stale ? '<span class="badge warn">stale</span>' : '<span class="badge">service</span>'}</td>
        <td><select data-endpoint-field="cost_mode" aria-describedby="${attr(pricingHelpId)}" aria-label="Cost mode for ${attr(rule.method)} ${attr(rule.path_template)}">${option("none", rule.cost_mode)}${option("fixed", rule.cost_mode)}${option("passthrough", rule.cost_mode)}</select></td>
        <td><input data-endpoint-field="estimated_cost_usd" aria-describedby="${attr(pricingHelpId)}" type="number" min="0" max="999999999999.9999" step="0.00000001" value="${attr(rule.estimated_cost_usd ?? "")}" aria-label="Estimated cost for ${attr(rule.method)} ${attr(rule.path_template)}"></td>
      </tr>
    `;
  }).join("");
  return `
    <div class="field wide-field endpoint-pricing-editor" data-endpoint-pricing-editor data-service-name="${attr(service.name)}">
      <span>OpenAPI endpoint pricing</span>
      <input type="hidden" name="endpoint_pricing_rules" value="${attr(JSON.stringify(rules))}">
      <div class="form-grid compact-grid">
        <label>OpenAPI source path<input name="openapi_source_path" value="${attr(service.openapi_source_path || "/openapi.json")}" placeholder="/openapi.json"></label>
        <div class="field"><span>Discovery status</span><div>${service.openapi_synced_at ? '<span class="badge good">synced</span>' : '<span class="badge warn">not synced</span>'} <span class="subtle">${service.openapi_synced_at ? esc(time(service.openapi_synced_at)) : ""}</span></div></div>
      </div>
      <div class="actions">
        <button type="button" data-openapi-action="preview" data-service-name="${attr(service.name)}">Preview OpenAPI</button>
      </div>
      <div class="help">Discovery fetches JSON from the registered upstream origin only, does not forward service credentials, does not follow redirects, and never runs on the proxy request path.</div>
      <p id="${attr(pricingHelpId)}" class="field-hint">Endpoint prices are in USD per request. none records no charge; fixed uses the estimate (0 means no estimated charge); passthrough uses upstream-reported cost when available. Stale rules refer to endpoints absent from the last discovery.</p>
      ${rows ? tableWrap(`<table><thead><tr><th>Method</th><th>Endpoint</th><th>Class</th><th>Cost mode</th><th>Estimated cost</th></tr></thead><tbody>${rows}</tbody></table>`) : emptyState("No discovered endpoints. Preview and sync /openapi.json to create endpoint pricing rules.")}
    </div>
  `;
}

function bindEndpointPricingEditors() {
  document.querySelectorAll("[data-endpoint-pricing-editor]").forEach((editor) => {
    const sync = () => syncEndpointPricingEditor(editor);
    editor.addEventListener("input", sync);
    editor.addEventListener("change", sync);
  });
  document.querySelectorAll("[data-openapi-action='preview']").forEach((button) => {
    button.addEventListener("click", handleAsync(previewServiceOpenApi));
  });
}

function syncEndpointPricingEditor(editor) {
  const rules = Array.from(editor.querySelectorAll("[data-endpoint-pricing-row]")).map((row) => {
    const text = (name) => String(row.querySelector(`[data-endpoint-field="${name}"]`)?.textContent || "").trim();
    const value = (name) => String(row.querySelector(`[data-endpoint-field="${name}"]`)?.value || "").trim();
    const estimatedCost = value("estimated_cost_usd");
    const rule = {
      method: text("method"),
      path_template: text("path_template"),
      operation_id: row.dataset.operationId || undefined,
      cost_mode: value("cost_mode") || "none",
      estimated_cost_usd: estimatedCost === "" ? null : Number(estimatedCost),
    };
    if (!rule.operation_id) delete rule.operation_id;
    return rule;
  });
  editor.querySelector('input[name="endpoint_pricing_rules"]').value = JSON.stringify(rules);
}

async function previewServiceOpenApi(event) {
  const serviceName = event.currentTarget.dataset.serviceName;
  const form = document.querySelector("#service-edit-form");
  const sourcePath = String(new FormData(form).get("openapi_source_path") || "/openapi.json").trim();
  const preview = await api(`/admin-ui/admin/services/${serviceName}/openapi/preview`, {
    method: "POST",
    body: JSON.stringify({ source_path: sourcePath }),
  });
  state.openapiPreviews[serviceName] = preview;
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `
    <div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">OpenAPI preview · ${esc(serviceName)}</h3>
      <p>${esc(preview.title || "OpenAPI")} ${esc(preview.version || "")} · ${preview.endpoints.length} operations · +${preview.added.length} / −${preview.removed.length}</p>
      <div class="modal-scroll">${table(
        ["Method", "Endpoint", "Operation", "Default billing"],
        preview.endpoints.map((endpoint) => [
          `<code>${esc(endpoint.method)}</code>`,
          `<code>${esc(endpoint.path_template)}</code>`,
          esc(endpoint.operation_id || endpoint.summary || ""),
          endpoint.relayna_default ? '<span class="badge good">none</span>' : '<span class="badge warn">service default</span>',
        ]),
      )}</div>
      <div class="notice warn"><strong>Review before sync.</strong><span>Existing endpoint prices are preserved. New Relayna endpoints default to none; other new endpoints inherit the service price.</span></div>
      <div class="form-actions">
        <button type="button" class="primary" data-openapi-sync>Sync endpoint pricing</button>
        <button type="button" data-close-modal>Cancel</button>
      </div>
    </div>
  `;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
  backdrop.querySelector("[data-openapi-sync]").addEventListener("click", handleAsync(async () => {
    await api(`/admin-ui/admin/services/${serviceName}/openapi/sync`, {
      method: "POST",
      body: JSON.stringify({ source_path: preview.source_path, expected_schema_hash: preview.schema_hash }),
    });
    close();
    setNotice(`OpenAPI endpoint pricing synced for ${serviceName}.`, "success");
    await services();
  }));
}

function serviceEditForm(service) {
  return `
    <div class="panel-heading"><h3>Edit service</h3><span class="subtle">${esc(service.name)}</span></div>
    <form id="service-edit-form" class="form-grid" data-service-name="${attr(service.name)}">
      <div class="field wide-field"><span>Saved endpoint protocols</span><div>${serviceProtocolSummary(service)}</div><small class="field-hint">WS means WebSocket on the displayed GET route. SSE uses HTTP. Labels describe saved configuration, not current availability.</small></div>
      ${service.foundry ? `<div class="wide-field"><span class="subtle">Azure Foundry · ${service.foundry.mode === "registered_agent" ? esc(service.foundry.agent_name) : "Endpoint passthrough"}</span> <button type="button" data-foundry-service="${attr(service.name)}">Configure Foundry</button></div>` : ""}
      ${formSection("Identity and routing", "Update registry identity, route, upstream, and methods.", `
        <label>Studio service ID<input name="studio_service_id" value="${attr(service.studio_service_id ?? "")}"></label>
        <label>Route pattern<input name="route_pattern" list="service-routes" value="${attr(service.route_pattern)}"></label>
        <label>Upstream URL<input name="upstream_base_url" value="${attr(service.upstream_base_url ?? "")}"></label>
        <div class="field"><span>Methods</span><small class="field-hint">HTTP methods clients may use on this service. Select only the methods its endpoints need.</small>${methodSelect(service.allowed_methods)}</div>
        <label>Sync status<select name="sync_status">${["local", "synced", "incomplete", "stale", "failed"].map((value) => option(value, service.sync_status)).join("")}</select></label>
        <label class="check"><input name="enabled" type="checkbox" ${service.enabled ? "checked" : ""}> Enabled</label>
      `, true)}
      ${formSection("Reliability and credentials", "Update health checks, credentials, limits, and fallback.", `
        <label>Health path<input name="health_check_path" value="${attr(service.health_check_path ?? "")}" placeholder="/health"></label>
        <label>Health method<select name="health_check_method">${["GET", "HEAD"].map((value) => option(value, service.health_check_method || "GET")).join("")}</select></label>
        <label>Credential<input name="credential" type="password" autocomplete="new-password" placeholder="${service.credential_configured ? "configured" : "missing"}"></label>
        <label class="check"><input name="clear_credential" type="checkbox"> Clear credential</label>
        <label>Timeout ms<input name="timeout_ms" type="number" min="1" max="600000" step="1" value="${attr(service.timeout_ms)}"></label>
        <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="${attr(service.max_body_bytes)}"></label>
        <label>Fallback services<input name="fallback_services" value="${attr(listValue(service.fallback_services, ""))}"></label>
      `)}
      ${service.foundry ? formSection("Caller authentication", "Require a Relayna key and optionally an Entra identity for this service.", endpointIdentityFields(service.access || {}, service.project_id || "")) : endpointAccessFields(service.access || {}, service.project_id || "")}
      ${formSection("Usage pricing", "Update cost source and request-matching rules.", `
        <label>Apply pricing changes to<select name="pricing_update_scope" aria-describedby="pricing-update-help-${attr(service.name)}"><option value="new_requests">New requests only</option><option value="historical">Recorded costs and new requests</option></select></label>
        <p id="pricing-update-help-${attr(service.name)}" class="field-hint wide-field">Historical recalculation updates this service's reporting costs using saved endpoint and rule attribution. Budget enforcement keeps original charges. Upstream-reported costs and records without a safe pricing match stay unchanged; request bodies are not replayed.</p>
        <label>Cost mode<select name="cost_mode">${option("none", service.cost_mode)}${option("fixed", service.cost_mode)}${option("passthrough", service.cost_mode)}</select></label>
        <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" max="999999999999.9999" step="0.00000001" value="${attr(service.estimated_cost_usd ?? "")}"></label>
        <div class="help wide-field">Fixed uses the estimate configured here. Passthrough uses provider response cost fields such as usage.total_cost.</div>
        ${pricingRulesEditor(service.pricing_rules || [])}
        ${service.foundry ? "" : openApiEndpointPricingEditor(service)}
      `)}
      <div class="form-actions sticky-form-actions wide-field">
        <button type="submit" class="primary">Save service</button>
        <button type="button" data-service-action="cancel-edit">Cancel</button>
      </div>
    </form>
  `;
}

async function submitService(event) {
  event.preventDefault();
  if (!validateServicePriceForm(event.target)) return;
  const form = new FormData(event.target);
  const action = event.submitter.value;
  if (action === "import") {
    await api("/admin-ui/admin/services/import", {
      method: "POST",
      body: JSON.stringify({
        studio_service_id: form.get("studio_service_id"),
        name: form.get("name"),
        route_pattern: blankToUndefined(form.get("route_pattern")),
        default_pricing: form.get("estimated_cost_usd")
          ? { cost_mode: form.get("cost_mode"), estimated_cost_usd: Number(form.get("estimated_cost_usd")) }
          : undefined,
      }),
    });
  } else {
    await api("/admin-ui/admin/services", {
      method: "POST",
      body: JSON.stringify(serviceBody(form, false)),
    });
  }
  setNotice("Service saved.", "success");
  await services();
}

async function openStudioImportPicker() {
  try {
    state.studioServices = await api("/admin-ui/admin/studio/services");
    const backdrop = document.createElement("section");
    backdrop.className = "modal-backdrop";
    const titleId = `dialog-title-${++dialogCounter}`;
    backdrop.innerHTML = `
      <div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
        <h3 id="${titleId}">Import from Studio</h3>
        <form id="studio-import-form" class="modal-form">
          <div class="modal-scroll">${studioImportTable(state.studioServices)}</div>
          <div id="studio-import-preview"></div>
          <div class="form-actions">
            <button type="button" data-import-preview ${state.studioServices.length ? "" : "disabled"}>Preview selected</button>
            <button type="button" data-import-sync ${state.studioServices.length ? "" : "disabled"}>Sync selected</button>
            <button class="primary" ${state.studioServices.length ? "" : "disabled"}>Import selected</button>
            <button type="button" data-close-modal>Cancel</button>
          </div>
        </form>
      </div>
    `;
    document.body.appendChild(backdrop);
    const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
    backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
    backdrop.querySelector("#studio-import-form").addEventListener("submit", handleAsync(importSelectedStudioServices));
    backdrop.querySelector("[data-import-preview]").addEventListener("click", handleAsync(previewSelectedStudioServices));
    backdrop.querySelector("[data-import-sync]").addEventListener("click", handleAsync(syncSelectedStudioServices));
  } catch (error) {
    setNotice(`${error.message}. Check Settings for the Studio connection.`);
  }
}

async function settings() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/studio/connection"),
    api("/admin-ui/admin/auth/front-door"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.studioConnection, state.authSettings] = loaded;
  const visual = settingsGovernanceSnapshot(state.studioConnection, state.authSettings);
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    <section class="page-insights" aria-label="Saved integration and trust configuration">
      ${insightStrip(visual.insights, { label: "Saved configuration · connection health is not measured here" })}
      ${flowSummary("Identity configuration boundaries", [
        { label: "Shared trust settings", icon: "shield-check", value: state.authSettings.source || "Unknown source" },
        { label: "Endpoint identity policy", icon: "route", value: "Per route or service" },
        { label: "Virtual key governance", icon: "key", value: "Access and limits" },
      ], "Request identity depends on endpoint policy and forwarding mode. Shared configuration alone does not grant access.")}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Studio connection</h3><span class="subtle">${esc(state.studioConnection.updated_at ? time(state.studioConnection.updated_at) : "fallback or unset")}</span></div>
      <form id="studio-connection-form" class="form-grid">
        ${formSection("Connection", "Configure the Studio origin and write-only service token.", `
          <label>Base URL<input name="base_url" type="url" placeholder="http://127.0.0.1:8000" value="${attr(state.studioConnection.base_url || "")}"></label>
          <label>Bearer token<input name="token" type="password" autocomplete="new-password" placeholder="${state.studioConnection.token_configured ? "Leave blank to keep current token" : "Optional"}"></label>
        `, true)}
        <div class="form-actions sticky-form-actions wide-field">
          <button class="primary">Save connection</button>
          <button type="button" data-studio-action="test">Test connection</button>
          <button type="button" data-studio-action="clear-token">Clear token</button>
          <button type="button" class="danger" data-studio-action="clear-settings">Clear persisted settings</button>
        </div>
      </form>
    </section>
    <section class="panel">
      ${state.authSettings.unverified_bearer_enabled ? `<div class="notice" data-kind="warning" role="status"><span class="badge warn">Unverified bearer active</span> Legacy Entra verification is paused; endpoints configured to Require Entra still fail closed. Only the Relayna virtual key is authenticated.</div>` : ""}
      <div class="panel-heading"><h3>Shared Entra trust and legacy defaults</h3><span class="subtle">${esc(state.authSettings.updated_at ? time(state.authSettings.updated_at) : "environment or unset")}</span></div>
      <form id="auth-settings-form" class="form-grid">
        ${formSection("Gateway headers", "Keep native key and trusted-ingress behavior explicit.", `
          <label class="check wide-field"><input name="unverified_bearer_enabled" type="checkbox" ${state.authSettings.unverified_bearer_enabled ? "checked" : ""} aria-describedby="unverified-bearer-help"> Require unverified bearer (troubleshooting)</label>
          <p id="unverified-bearer-help" class="wide-field subtle">When enabled, gateway-managed requests require a nonempty Bearer token and a valid Relayna key in the header below. Entra verification is paused: no token identity, groups, roles or scopes are trusted. Disable Apigee trusted headers before enabling this mode. Keep Entra ID enabled and configured to restore verification by clearing this option. Direct LiteLLM passthrough is unchanged; use gateway-managed routes.</p>
          <label>Relayna key header<input name="relayna_key_header" value="${attr(state.authSettings.relayna_key_header || "X-Relayna-Key")}"></label>
          <label class="check"><input name="apigee_trusted_header_enabled" type="checkbox" ${state.authSettings.apigee.trusted_header_enabled ? "checked" : ""}> Enable Apigee trusted headers</label>
          <label>Apigee secret<input name="apigee_trusted_header_secret" type="password" autocomplete="new-password" placeholder="${apigeeSecretPlaceholder()}"></label>
        `, true)}
        ${formSection("Microsoft Entra ID", "Use one application ID for portal sign-in and API token validation.", `
          <label class="check"><input name="entra_enabled" type="checkbox" ${state.authSettings.entra.enabled ? "checked" : ""}> Enable Entra ID</label>
          <label>Tenant ID<input name="tenant_id" value="${attr(state.authSettings.entra.tenant_id || "")}"></label>
          <label>Application ID / token audience<input name="audience" value="${attr(state.authSettings.entra.audience || "")}" placeholder="00000000-0000-0000-0000-000000000000"></label>
          <label>Trusted issuer<input name="issuer" type="url" value="${attr(state.authSettings.entra.issuer || "")}"></label>
          <label class="wide-field">OIDC discovery URL<input name="oidc_discovery_url" type="url" value="${attr(state.authSettings.entra.oidc_discovery_url || "")}"></label>
          <label>Required scope<input name="required_scope" value="${attr(state.authSettings.entra.required_scope || "")}" placeholder="gateway.invoke"></label>
          <label>Required role<input name="required_role" value="${attr(state.authSettings.entra.required_role || "")}" placeholder="gateway.invoke"></label>
          <label>Allowed groups<input name="allowed_groups" value="${attr(listValue(state.authSettings.entra.allowed_groups, ""))}" placeholder="group-a,group-b"></label>
          <label>Accepted algorithms<input name="accepted_algorithms" value="${attr(listValue(state.authSettings.entra.accepted_algorithms, "RS256"))}"></label>
          <label>JWKS cache TTL<input name="jwks_cache_ttl_seconds" type="number" min="1" value="${attr(state.authSettings.entra.jwks_cache_ttl_seconds ?? 300)}"></label>
          <label>Clock skew seconds<input name="clock_skew_seconds" type="number" min="0" value="${attr(state.authSettings.entra.clock_skew_seconds ?? 60)}"></label>
        `)}
        <div class="form-actions sticky-form-actions wide-field">
          <button class="primary">Save auth settings</button>
          <button type="button" data-auth-action="clear-apigee-secret">Clear Apigee secret</button>
        </div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Security and release posture</h3><span class="subtle">Static operator references</span></div>
      <div class="kv">
        <div><strong>Release target</strong><span>${badge("v0.1.49")}</span></div>
        <div><strong>Admin contracts</strong><span>Preserve <code>/admin-ui</code> and <code>/admin-ui/admin/*</code> unless an implementation strategy changes the boundary.</span></div>
        <div><strong>Supply-chain exceptions</strong><span><a href="https://github.com/sarattha/relayna-gateway/blob/main/docs/security-exceptions.md" target="_blank" rel="noreferrer">docs/security-exceptions.md</a></span></div>
        <div><strong>Release metadata</strong><span><a href="https://github.com/sarattha/relayna-gateway/blob/main/scripts/validate-release-metadata.py" target="_blank" rel="noreferrer">validate-release-metadata.py</a></span></div>
      </div>
    </section>
  `;
  document.querySelector("#studio-connection-form").addEventListener("submit", handleAsync(saveStudioConnection));
  document.querySelector("#auth-settings-form").addEventListener("submit", handleAsync(saveAuthSettings));
  document.querySelectorAll("[data-studio-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(studioConnectionAction));
  });
  document.querySelectorAll("[data-auth-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(authSettingsAction));
  });
  organizeView("settings");
}

async function saveStudioConnection(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = { base_url: form.get("base_url")?.trim() || null };
  const tokenValue = form.get("token")?.trim();
  if (tokenValue) body.token = tokenValue;
  state.studioConnection = await api("/admin-ui/admin/studio/connection", {
    method: "PATCH",
    body: JSON.stringify(body),
  });
  setNotice("Studio connection saved.", "success");
  await settings();
}

async function studioConnectionAction(event) {
  const action = event.currentTarget.dataset.studioAction;
  if (action === "test") {
    const result = await api("/admin-ui/admin/studio/connection/test", { method: "POST", body: "{}" });
    setNotice(`Studio connection works. ${result.service_count} service${result.service_count === 1 ? "" : "s"} available.`, "success");
    return;
  }
  if (action === "clear-token") {
    state.studioConnection = await api("/admin-ui/admin/studio/connection", {
      method: "PATCH",
      body: JSON.stringify({ token: null }),
    });
    setNotice("Studio token cleared.", "success");
    await settings();
    return;
  }
  if (action === "clear-settings") {
    if (!(await confirmAction("Clear Studio settings", "Persisted Studio settings are removed and environment fallback may become active."))) return;
    state.studioConnection = await api("/admin-ui/admin/studio/connection", {
      method: "PATCH",
      body: JSON.stringify({ base_url: null }),
    });
    setNotice("Persisted Studio settings cleared.", "success");
    await settings();
  }
}

async function saveAuthSettings(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const secret = form.get("apigee_trusted_header_secret")?.trim();
  const apigeeEnabled = form.has("apigee_trusted_header_enabled");
  const envBackedApigeeSecret =
    state.authSettings.source === "environment" &&
    state.authSettings.apigee.trusted_header_enabled &&
    state.authSettings.apigee.secret_configured;
  if (apigeeEnabled && envBackedApigeeSecret && !secret) {
    setNotice("Re-enter the Apigee secret before saving environment-backed trusted-header settings.", "error");
    return;
  }
  if (apigeeEnabled && form.has("unverified_bearer_enabled")) {
    setNotice("Disable Apigee trusted headers before enabling unverified bearer mode.", "error");
    return;
  }
  const body = {
    unverified_bearer_enabled: form.has("unverified_bearer_enabled"),
    entra_enabled: form.has("entra_enabled"),
    apigee_trusted_header_enabled: apigeeEnabled,
    relayna_key_header: form.get("relayna_key_header")?.trim() || "X-Relayna-Key",
    tenant_id: nullableText(form.get("tenant_id")),
    audience: nullableText(form.get("audience")),
    issuer: nullableText(form.get("issuer")),
    oidc_discovery_url: nullableText(form.get("oidc_discovery_url")),
    required_scope: nullableText(form.get("required_scope")),
    required_role: nullableText(form.get("required_role")),
    allowed_groups: csv(form.get("allowed_groups")),
    accepted_algorithms: csv(form.get("accepted_algorithms")),
    jwks_cache_ttl_seconds: numberOrDefault(form.get("jwks_cache_ttl_seconds"), 300),
    clock_skew_seconds: numberOrDefault(form.get("clock_skew_seconds"), 60),
  };
  if (secret) body.apigee_trusted_header_secret = secret;
  state.authSettings = await api("/admin-ui/admin/auth/front-door", {
    method: "PATCH",
    body: JSON.stringify(body),
  });
  setNotice("Gateway auth settings saved.", "success");
  await settings();
}

function apigeeSecretPlaceholder() {
  if (
    state.authSettings.source === "environment" &&
    state.authSettings.apigee.trusted_header_enabled &&
    state.authSettings.apigee.secret_configured
  ) {
    return "Re-enter secret to persist environment settings";
  }
  return state.authSettings.apigee.secret_configured ? "Leave blank to keep current secret" : "Required when Apigee is enabled";
}

async function authSettingsAction(event) {
  const action = event.currentTarget.dataset.authAction;
  if (action === "clear-apigee-secret") {
    if (!(await confirmAction("Clear Apigee secret", "Apigee trusted-header mode cannot be enabled until a new secret is saved."))) return;
    await api("/admin-ui/admin/auth/front-door", {
      method: "PATCH",
      body: JSON.stringify({ apigee_trusted_header_enabled: false, apigee_trusted_header_secret: null }),
    });
    setNotice("Apigee secret cleared.", "success");
    await settings();
  }
}

function studioImportTable(rows) {
  if (!rows.length) return '<div class="empty-state"><p>No Studio services.</p></div>';
  return `<div class="table-wrap studio-import-table"><table><thead><tr>
    <th>Select</th><th>Service</th><th>Environment</th><th>Status</th><th>Base URL</th><th>Tags</th><th>Route</th>
  </tr></thead><tbody>${rows
    .map((row, index) => `<tr>
      <td><input name="studio_index" type="checkbox" value="${attr(index)}" aria-label="Select Studio service ${attr(row.display_name || row.name)}"></td>
      <td><strong>${esc(row.display_name || row.name)}</strong><div class="subtle">${esc(row.studio_service_id)}</div></td>
      <td>${esc(row.environment || "n/a")}</td>
      <td>${esc(row.status || "n/a")}</td>
      <td><code>${esc(row.base_url || "missing")}</code></td>
      <td>${esc(listValue(row.tags, "none"))}</td>
      <td><code>${esc(row.route_pattern)}</code></td>
    </tr>`)
    .join("")}</tbody></table></div>`;
}

function openServiceSelectionPicker(trigger) {
  const form = trigger.closest("form");
  const fieldName = trigger.dataset.servicePicker || "service_names";
  const selected = new Set(selectedServiceNames(form, fieldName));
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `
    <div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">${esc(trigger.dataset.servicePickerTitle || "Select services")}</h3>
      <form id="service-picker-form" class="modal-form">
        <div class="modal-scroll">${servicePickerTable(state.services, selected)}</div>
        <div class="form-actions">
          <button class="primary" ${state.services.length ? "" : "disabled"}>Apply selection</button>
          <button type="button" data-close-modal>Cancel</button>
        </div>
      </form>
    </div>
  `;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
  backdrop.querySelector("#service-picker-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const values = new FormData(event.target).getAll("service_name");
    setSelectedServiceNames(form, fieldName, values);
    close();
  });
}

function openGuardrailSelectionPicker(trigger) {
  const form = trigger.closest("form");
  const fieldName = trigger.dataset.guardrailPicker;
  const selected = new Set(selectedServiceNames(form, fieldName));
  const rows = state.guardrails?.guardrails || [];
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `
    <div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">${esc(trigger.dataset.guardrailPickerTitle || "Select guardrails")}</h3>
      <form id="guardrail-picker-form" class="modal-form">
        <div class="modal-scroll">${guardrailPickerTable(rows, selected)}</div>
        <div class="form-actions">
          <button class="primary" ${rows.length ? "" : "disabled"}>Apply selection</button>
          <button type="button" data-close-modal>Cancel</button>
        </div>
      </form>
    </div>
  `;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "[data-close-modal]" });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => close());
  backdrop.querySelector("#guardrail-picker-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const values = new FormData(event.target).getAll("guardrail_name");
    setSelectedServiceNames(form, fieldName, values);
    updateGuardrailOverrideControls(form);
    close();
  });
}

function servicePickerTable(rows, selected) {
  if (!rows.length) return '<div class="empty-state"><p>No services registered.</p></div>';
  return `<div class="table-wrap service-picker-table"><table><thead><tr>
    <th>Select</th><th>Service</th><th>Status</th><th>Route</th><th>Upstream</th>
  </tr></thead><tbody>${rows
    .map((row) => `<tr>
      <td><input name="service_name" type="checkbox" aria-label="Select service ${attr(row.name)}" value="${attr(row.name)}" ${selected.has(row.name) ? "checked" : ""}></td>
      <td><strong>${esc(row.name)}</strong><div class="subtle">${esc(row.studio_service_id || "local")}</div></td>
      <td>${esc(row.sync_status || (row.enabled ? "enabled" : "disabled"))}</td>
      <td><code>${esc(row.route_pattern)}</code></td>
      <td><code>${esc(row.foundry ? `Azure Foundry · ${row.foundry.mode === "registered_agent" ? row.foundry.agent_name : "Responses passthrough"}` : row.upstream_base_url || "missing")}</code></td>
    </tr>`)
    .join("")}</tbody></table></div>`;
}

function guardrailPickerTable(rows, selected) {
  if (!rows.length) return '<div class="empty-state"><p>No guardrails configured.</p></div>';
  return `<div class="table-wrap guardrail-picker-table"><table><thead><tr>
    <th>Select</th><th>Guardrail</th><th>Provider</th><th>Modes</th><th>Failure</th><th>Default</th>
  </tr></thead><tbody>${rows
    .map((row) => `<tr>
      <td><input name="guardrail_name" type="checkbox" aria-label="Select guardrail ${attr(row.name)}" value="${attr(row.name)}" ${selected.has(row.name) ? "checked" : ""}></td>
      <td><strong>${esc(row.name)}</strong><div class="subtle">${esc(row.description || "")}</div></td>
      <td>${esc(row.provider_kind)}</td>
      <td>${esc(listValue(row.modes, "none"))}</td>
      <td>${esc(row.failure_policy)}</td>
      <td>${row.default_on ? '<span class="badge good">default</span>' : '<span class="badge">opt-in</span>'}</td>
    </tr>`)
    .join("")}</tbody></table></div>`;
}

async function importSelectedStudioServices(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const selected = form.getAll("studio_index").map((value) => state.studioServices[Number(value)]).filter(Boolean);
  await api("/admin-ui/admin/services/import/activate", {
    method: "POST",
    body: JSON.stringify({ source: "studio", services: selected.map((service) => service.import_request) }),
  });
  closeTopDialog();
  setNotice(`${selected.length} Studio service${selected.length === 1 ? "" : "s"} imported.`, "success");
  await services();
}

async function previewSelectedStudioServices(event) {
  event.preventDefault();
  const form = new FormData(document.querySelector("#studio-import-form"));
  const selected = form.getAll("studio_index").map((value) => state.studioServices[Number(value)]).filter(Boolean);
  const preview = await api("/admin-ui/admin/services/import/preview", {
    method: "POST",
    body: JSON.stringify({ source: "studio", services: selected.map((service) => service.import_request) }),
  });
  const target = document.querySelector("#studio-import-preview");
  if (target) target.innerHTML = importDiffTemplate(preview.diff);
  setNotice(`Import preview: +${preview.diff.added.length} changed ${preview.diff.changed.length} removed ${preview.diff.removed.length} invalid ${preview.diff.invalid.length}.`, preview.diff.invalid.length ? "error" : "success");
}

async function syncSelectedStudioServices(event) {
  event.preventDefault();
  const form = new FormData(document.querySelector("#studio-import-form"));
  const selected = form.getAll("studio_index").map((value) => state.studioServices[Number(value)]).filter(Boolean);
  for (const service of selected) {
    await api("/admin-ui/admin/services/sync", {
      method: "POST",
      body: JSON.stringify(service.import_request),
    });
  }
  closeTopDialog();
  setNotice(`${selected.length} Studio service${selected.length === 1 ? "" : "s"} synced.`, "success");
  await services();
}

async function patchService(event) {
  event.preventDefault();
  if (!validateServicePriceForm(event.target)) return;
  const form = new FormData(event.target);
  const serviceName = event.target.dataset.serviceName;
  const body = serviceBody(form, true);
  if (body.reprice_existing_usage && !(await confirmAction("Recalculate recorded service costs", `Save the new pricing for ${serviceName} and recalculate its recorded reporting costs? Budget enforcement keeps original charges. Upstream-reported costs and records without a safe pricing match stay unchanged.`))) return;
  const service = await api(`/admin-ui/admin/services/${serviceName}`, {
    method: "PATCH",
    body: JSON.stringify(body),
  });
  state.editingServiceName = null;
  const repricing = service.historical_usage_repricing;
  setNotice(repricing ? `Service updated. Recalculated ${repricing.updated_requests} recorded costs; ${repricing.unchanged_requests} unchanged. Budget charges are unchanged.` : "Service updated.", "success");
  await services();
}

async function serviceAction(event) {
  const { serviceName, serviceAction: action } = event.currentTarget.dataset;
  if (action === "studio-import") {
    await openStudioImportPicker();
    return;
  }
  if (action === "edit") {
    state.editingServiceName = serviceName;
    await services();
    return;
  }
  if (action === "cancel-edit") {
    state.editingServiceName = null;
    await services();
    return;
  }
  if (action === "sync-status") {
    const body = await api(`/admin-ui/admin/services/${serviceName}/sync-status`);
    setNotice(
      `${body.name}: ${body.sync_status}${body.missing_runtime_fields.length ? `, missing ${body.missing_runtime_fields.join(", ")}` : ""}.`,
      body.sync_status === "synced" || body.sync_status === "local" ? "success" : "error",
    );
    return;
  }
  if (
    ["delete", "disable", "enable"].includes(action) &&
    !(await confirmAction(`${action} ${serviceName}`, "This service change is written to the database."))
  ) {
    return;
  }
  if (action === "delete") {
    await api(`/admin-ui/admin/services/${serviceName}`, { method: "DELETE" });
  } else {
    await api(`/admin-ui/admin/services/${serviceName}/${action}`, { method: "POST", body: "{}" });
  }
  setNotice(`Service ${action}d.`, "success");
  await services();
}

function serviceProtocolSummary(service) {
  if (service.access?.websocket && !service.access?.accessa && !service.foundry && (service.allowed_methods || []).includes("GET")) {
    return `<span class="badge">HTTP + WS</span><div class="subtle"><span class="badge">WS</span> GET <code>${esc(service.route_pattern)}</code></div>`;
  }
  const binding = service.access?.accessa;
  const prefix = binding?.app ? `/app/${binding.app}/channel/${binding.channel}/v1/` : null;
  const hasRunSocket = prefix && service.route_pattern === `${prefix}*` && (service.allowed_methods || []).includes("GET");
  if (!hasRunSocket) return '<span class="badge">HTTP</span>';
  return `<span class="badge">HTTP + WS</span>
    <div class="subtle">HTTP: registered methods</div>
    <div class="subtle"><span class="badge">WS</span> GET <code>${esc(`${prefix}run`)}</code></div>`;
}

function serviceTable(rows) {
  return table(
    ["Name", "State", "Route", "Protocol", "Identity", "Upstream", "Health check", "Credential", "Cost", "Actions"],
    rows.map((row) => [
      `<strong class="service-name"><i class="ti ti-apps" aria-hidden="true"></i><span>${esc(row.name)}</span></strong><div class="subtle">${esc(row.source)}</div>`,
      serviceBadges(row),
      `<code>${esc(row.route_pattern)}</code>`,
      `<i class="ti ti-plug-connected" aria-hidden="true"></i> ${serviceProtocolSummary(row)}`,
      endpointIdentityBadge(row.access),
      esc(row.foundry ? `Azure Foundry · ${row.foundry.mode === "registered_agent" ? row.foundry.agent_name : "Responses passthrough"}` : row.upstream_base_url || "missing"),
      esc(row.foundry ? "Not probed" : healthCheckLabel(row)),
      row.foundry ? '<span class="badge">Azure identity</span>' : row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
      `${esc(row.cost_mode)} ${row.estimated_cost_usd == null ? "" : money(row.estimated_cost_usd)}`,
      `<div class="actions">
        <button data-service-action="edit" data-service-name="${attr(row.name)}" aria-label="Edit service ${attr(row.name)}">Edit</button>
        <button data-service-action="sync-status" data-service-name="${attr(row.name)}" aria-label="View sync status for service ${attr(row.name)}">Status</button>
        <button data-service-action="${row.enabled ? "disable" : "enable"}" data-service-name="${attr(row.name)}" aria-label="${row.enabled ? "Disable" : "Enable"} service ${attr(row.name)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-service-action="delete" data-service-name="${attr(row.name)}" aria-label="Delete service ${attr(row.name)}">Delete</button>
      </div>`,
    ]),
  );
}

function serviceBadges(row) {
  const stateBadge = badge(row.enabled === true ? "enabled" : row.enabled === false ? "disabled" : "unknown", row.enabled === true ? "good" : row.enabled === false ? "bad" : "neutral");
  const syncTone = row.sync_status === "synced" || row.sync_status === "local" ? "good"
    : row.sync_status === "failed" ? "bad"
    : row.sync_status === "stale" || row.sync_status === "incomplete" ? "warn" : "neutral";
  const syncBadge = badge(row.sync_status || "sync unknown", syncTone);
  return `${stateBadge} ${syncBadge}`;
}

function healthCheckLabel(row) {
  return row.health_check_path ? `${row.health_check_method || "GET"} ${row.health_check_path}` : "upstream root";
}

async function usage() {
  const renderId = ++renderGeneration;
  // Inventories enrich labels and controls; they must not gate recorded usage.
  const inventories = Promise.allSettled([api("/admin-ui/admin/projects"), api("/admin-ui/admin/services"), api("/admin-ui/admin/keys")]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Usage filters</h3></div>
      <div data-usage-inventory-status role="status"></div>
      <form id="usage-form" class="form-grid">
        ${formSection("Primary filters", "Start with ownership, service, status, and time range.", `
        <label>Project<select name="project_id"><option value="">All</option>${projectOptions()}</select></label>
        <label>Virtual key<select name="key_id"><option value="">All</option>${keyOptions()}</select></label>
        <label>Service<select name="service"><option value="">All</option>${serviceOptions()}</select></label>
        <label>Provider<select name="provider"><option value="">All</option><option value="litellm">litellm</option><option value="openai-compatible">openai-compatible</option><option value="internal-service">internal-service</option></select></label>
        <label>Status<select name="status"><option value="">All</option><option value="success">Success</option><option value="failure">Failure</option></select></label>
        <label>Status code<input name="status_code" type="number" min="100" max="599" placeholder="500"></label>
        <label>Time range<select name="time_preset">
          <option value="all_time">All time</option>
          <option value="last_1h">Last 1h</option>
          <option value="last_6h">Last 6h</option>
          <option value="last_24h">Last 24h</option>
          <option value="last_7d">Last 7d</option>
          <option value="last_30d">Last 30d</option>
          <option value="today">Today</option>
          <option value="yesterday">Yesterday</option>
          <option value="this_week">This week</option>
          <option value="this_month">This month</option>
          <option value="custom">Custom</option>
        </select></label>
        <label>From<input name="from" type="datetime-local"></label>
        <label>To<input name="to" type="datetime-local"></label>
        `, true)}
        ${formSection("Advanced filters", "Narrow by route, model, execution identifiers, cost, and presentation.", `
        <label>Route<input name="route" list="usage-route-options" placeholder="/v1/chat/completions"></label>
        <label>Method<select name="method"><option value="">All</option><option value="GET">GET</option><option value="POST">POST</option><option value="PUT">PUT</option><option value="PATCH">PATCH</option><option value="DELETE">DELETE</option></select></label>
        <label>Endpoint<input name="endpoint" list="usage-endpoint-options" placeholder="/jobs/{job_id}"></label>
        <label>Model<input name="model" list="usage-model-options" placeholder="exact model"></label>
        <label>Task<input name="task_id" placeholder="exact task ID"></label>
        <label>Run<input name="run_id" placeholder="exact run ID"></label>
        <label>Trace<input name="trace_id"></label>
        <label>Interval<select name="interval"><option value="hour">Hour</option><option value="day">Day</option></select></label>
        <label>Min cost<input name="min_cost_usd" type="number" min="0" step="any"></label>
        <label>Show top<select name="breakdown_limit"><option value="20">20</option><option value="10">10</option><option value="50">50</option><option value="100">100</option></select></label>
        <label>Sort by<select name="sort_by"><option value="requests">Requests</option><option value="cost">Cost</option><option value="failures">Failures</option><option value="latency">Latency</option><option value="tokens">Tokens</option><option value="fallbacks">Fallbacks</option></select></label>
        <label>Rows per page<select name="limit"><option value="50">50</option><option value="20">20</option><option value="100">100</option></select></label>
        `)}
        <div class="form-actions sticky-form-actions wide-field">
          <button class="primary">Apply</button>
        </div>
        <div class="help wide-field">Text filters are exact-match. Use suggestions where available.</div>
      </form>
      <datalist id="usage-route-options"></datalist>
      <datalist id="usage-endpoint-options"></datalist>
      <datalist id="usage-model-options"></datalist>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Export options</h3></div>
      <form id="usage-export-form" class="usage-export-form">
        <div class="field">
          <label for="usage-export-format">Export format</label>
          <select id="usage-export-format" name="export_format" aria-describedby="usage-export-format-help"><option value="csv">CSV</option><option value="json">JSON</option></select>
          <small id="usage-export-format-help" class="field-hint">CSV for spreadsheets; JSON for scripts and integrations.</small>
        </div>
        <div class="field">
          <label for="usage-export-limit">Maximum rows</label>
          <select id="usage-export-limit" name="export_limit" aria-describedby="usage-export-limit-help usage-export-help"><option value="1000">1,000</option><option value="100">100</option><option value="5000">5,000</option><option value="10000">10,000</option><option value="all">All rows (download)</option></select>
          <small id="usage-export-limit-help" class="field-hint">Maximum matching records to include. All rows downloads every match in the selected time range.</small>
        </div>
        <div class="field">
          <label for="usage-export-from">Export from</label>
          <input id="usage-export-from" name="export_from" type="datetime-local" aria-describedby="usage-export-from-help usage-export-help">
          <small id="usage-export-from-help" class="field-hint">Include records at or after this time. Uses your local time.</small>
        </div>
        <div class="field">
          <label for="usage-export-to">Export to</label>
          <input id="usage-export-to" name="export_to" type="datetime-local" aria-describedby="usage-export-to-help usage-export-help">
          <small id="usage-export-to-help" class="field-hint">Include records before this time. Uses your local time.</small>
        </div>
        <div class="field">
          <label for="usage-export-offset">Rows to skip</label>
          <input id="usage-export-offset" name="export_offset" type="number" min="0" value="0" aria-describedby="usage-export-offset-help">
          <small id="usage-export-offset-help" class="field-hint">0 starts at the first matching row. For example, 100 skips the first 100 rows. Ignored for All rows.</small>
        </div>
        <div class="actions usage-export-actions">
          <button type="button" data-usage-export-action="preview">Preview</button>
          <button type="button" data-usage-export-action="download">Download</button>
          <button type="button" data-usage-export-action="copy-url">Copy URL</button>
          <button type="button" data-usage-export-action="copy-curl">Copy curl</button>
        </div>
        <div id="usage-export-help" class="help">Leave both dates blank to use the Usage time filter. Entering either date replaces that range; a blank start or end means no limit on that side. Other Usage filters still apply. All rows needs both a start and end (here or in Usage). Downloads stream to disk; Copy URL and Copy curl use the same streaming endpoint.</div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Task drilldown</h3></div>
      <form id="task-usage-form" class="inline-form">
        <div class="field">
          <label for="task-usage-id">Task ID</label>
          <input id="task-usage-id" name="task_lookup" placeholder="task ID" aria-describedby="task-usage-help" required>
          <small id="task-usage-help" class="field-hint">Enter an exact task ID to summarize its requests, failures and cost using the current Usage filters.</small>
        </div>
        <button>Load task usage</button>
      </form>
      <div id="task-usage-result"></div>
    </section>
    <section class="panel usage-analysis"><div class="panel-heading"><h3>Usage analysis</h3><span class="subtle">Recorded requests & cost</span></div><div id="usage-results"></div></section>
  `;
  const usageForm = document.querySelector("#usage-form");
  for (const [name, value] of Object.entries({ time_preset: "last_24h", ...state.usageFilters, project_id: state.projectScope })) {
    const field = usageForm.elements.namedItem(name);
    if (!field) continue;
    const selected = name === "time_preset" && value === "" ? "all_time" : value;
    if (field.tagName === "SELECT" && selected && ![...field.options].some((option) => option.value === selected)) field.add(new Option(selected, selected));
    field.value = selected;
  }
  document.querySelector("#usage-form").addEventListener("submit", handleAsync(applyUsageFilters));
  bindUsageSuggestions(usageForm);
  document.querySelector("#task-usage-form").addEventListener("submit", handleAsync(loadTaskUsage));
  document.querySelectorAll("[data-usage-export-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(usageExportAction));
  });
  document.querySelector("#usage-export-form [name=export_limit]")?.addEventListener("change", syncUsageExportControls);
  syncUsageExportControls();
  organizeView("usage");
  inventories.then((loaded) => {
    if (renderId !== renderGeneration || usageForm !== document.querySelector("#usage-form")) return;
    const fields = [["projects", "project_id", projectOptions], ["services", "service", serviceOptions], ["keys", "key_id", keyOptions]];
    const failed = [];
    loaded.forEach((result, index) => {
      const [inventory, name, options] = fields[index];
      if (result.status !== "fulfilled") { failed.push(inventory); return; }
      state[inventory] = result.value;
      const field = usageForm.elements.namedItem(name);
      const selected = field.value;
      field.innerHTML = `<option value="">All</option>${options()}`;
      if (selected && ![...field.options].some((option) => option.value === selected)) field.add(new Option(selected, selected));
      field.value = selected;
    });
    document.querySelector("[data-usage-inventory-status]").textContent = failed.length ? `Filter labels unavailable: ${failed.join(", ")}. Usage queries remain available.` : "";
  });
  await loadUsage();
}

async function loadUsage(event) {
  event?.preventDefault();
  usageLoad?.controllers.forEach((controller) => controller.abort());
  usageLoad?.charts?.forEach(chart => chart?.destroy());
  const generation = ++usageGeneration;
  const root = document.querySelector("#usage-results");
  if (!root) return;
  const query = usageQueryFromForm(event?.target);
  // Freeze this applied range for section requests and cursor navigation.
  const pageSize = usagePageSize();
  query.set("timeseries_limit", String(pageSize));
  query.set("service_timeseries_limit", String(pageSize));
  state.usagePagination.eventsOffset = 0;
  usageLoad = { generation, root, query, controllers: new Map(), loaded: new Set(), queue: [], active: 0, eventCursors: ["start"], eventIndex: 0, eventsPage: null, charts: [] };
  const sections = [
    ["project-key-services", "Project → Virtual key → Service"], ["projects", "Projects"], ["keys", "Keys"],
    ["services", "Services"], ["endpoints", "Endpoints"], ["providers", "Providers"], ["models", "Models"],
    ["tasks", "Tasks"], ["events", "Recent requests"], ["timeseries", "Timeseries"],
    ["service-timeseries", "Service timeseries"], ["unused-keys", "Unused keys"],
  ];
  root.innerHTML = `
    <div class="monitor-filter-chips" aria-label="Applied usage filters"><span><i class="ti ti-calendar" aria-hidden="true"></i>${query.get("from") && query.get("to") ? `${esc(new Date(query.get("from")).toLocaleDateString())} – ${esc(new Date(query.get("to")).toLocaleDateString())}` : "All time"}</span><span><i class="ti ti-folder" aria-hidden="true"></i>${esc(query.get("project_id") ? usageProjectName(query.get("project_id")) : "All projects")}</span>${[...query].filter(([key]) => !["from", "to", "project_id", "offset", "limit", "timeseries_limit", "service_timeseries_limit", "breakdown_limit", "sort_by", "interval"].includes(key)).map(([key, value]) => `<span>${esc(key.replaceAll("_", " "))}<strong>${esc(value)}</strong></span>`).join("")}</div>
    <div data-usage-summary aria-busy="true">${emptyState("Loading usage summary…")}</div>
    ${usageChartsShell()}
    ${query.get("key_id") ? `<p><button type="button" data-usage-litellm-spend>View LiteLLM reported spend</button> <span class="help">Current mapped-key counter · independent of date filters</span></p>` : ""}
    ${sections.map(([section, title]) => `<h4>${esc(title)}</h4><div data-usage-section="${attr(section)}">${emptyState("Select this section to load it.")}</div>`).join("")}
  `;
  root.querySelector("[data-usage-litellm-spend]")?.addEventListener("click", handleAsync(() => showLiteLlmSpend(query.get("key_id"))));
  tabulateUsage(root);
  // Summary and rows settle independently. Neither failure replaces the other.
  void loadUsageVisuals();
  await Promise.allSettled([loadUsageSummary(), loadUsageSection("events")]);
}

function usageRequestCurrent(load) {
  return usageLoad === load && load.root === document.querySelector("#usage-results");
}

function usageRequestController(load, section) {
  load.controllers.get(section)?.abort();
  const controller = new AbortController();
  load.controllers.set(section, controller);
  return controller;
}

async function loadUsageSummary() {
  const load = usageLoad;
  if (!load || !usageRequestCurrent(load)) return;
  const host = load.root.querySelector("[data-usage-summary]");
  const controller = usageRequestController(load, "summary");
  host.setAttribute("aria-busy", "true");
  try {
    const summary = await api(`/admin-ui/admin/usage/summary?${load.query}`, { signal: controller.signal });
    if (!usageRequestCurrent(load) || controller.signal.aborted) return;
    host.innerHTML = usageSummaryView(summary, { esc, money });
  } catch (error) {
    if (!usageRequestCurrent(load) || controller.signal.aborted || error.name === "AbortError") return;
    host.innerHTML = `<div class="notice" role="alert">Usage summary unavailable: ${esc(error.message)} <button type="button" data-retry-usage-summary>Retry summary</button></div>`;
    host.querySelector("[data-retry-usage-summary]").addEventListener("click", handleAsync(loadUsageSummary));
  } finally {
    if (usageRequestCurrent(load) && !controller.signal.aborted) host.setAttribute("aria-busy", "false");
  }
}

async function loadUsageVisuals() {
  const load = usageLoad;
  if (!load || !usageRequestCurrent(load)) return;
  const host = load.root.querySelector("[data-usage-visuals]");
  if (!host) return;
  const controller = usageRequestController(load, "visualization");
  host.querySelectorAll("[data-usage-chart-data]").forEach(button => {
    button.disabled = true;
    button.onclick = () => showContentDrawer(`Chart data · ${load.chartInterval === "day" ? "daily" : "hourly"} buckets`, usageTimeseriesTable(load.chartRows || []), () => {}, button);
  });
  const query = new URLSearchParams(load.query);
  const span = Date.parse(query.get("to")) - Date.parse(query.get("from"));
  if (!query.get("from") || !query.get("to") || !(span > 0)) {
    host.innerHTML = '<div class="monitor-empty"><i class="ti ti-calendar-stats" aria-hidden="true"></i><span>Choose a bounded time range to visualize trends.</span></div>';
    host.setAttribute("aria-busy", "false");
    return;
  }
  query.set("interval", span > 7 * 86400000 ? "day" : "hour");
  query.set("timeseries_limit", "168");
  query.set("timeseries_offset", "0");
  host.setAttribute("aria-busy", "true");
  load.visualizationActive = true;
  try {
    const data = await api(`/admin-ui/admin/usage/sections/timeseries?${query}`, { signal: controller.signal });
    if (!usageRequestCurrent(load) || controller.signal.aborted) return;
    const rows = data.rows || [];
    load.chartRows = rows;
    load.chartInterval = query.get("interval");
    host.querySelectorAll("[data-usage-chart-data]").forEach(button => { button.disabled = !rows.length; });
    const configs = usageTrendConfigs(rows, query.get("interval"));
    for (const [index, key] of ["volume", "cost"].entries()) {
      const target = host.querySelector(`[data-usage-${key}-host]`);
      if (!target) continue;
      if (!rows.length) { target.innerHTML = '<div class="monitor-empty"><i class="ti ti-chart-line" aria-hidden="true"></i><span>No recorded usage in this period</span></div>'; continue; }
      target.innerHTML = `<canvas role="img" aria-label="${key === "volume" ? "Successful and failed requests" : "Estimated cost in USD"} over ${rows.length} returned ${query.get("interval")} buckets"></canvas>`;
      const chart = mountMonitorChart(Chart, target.querySelector("canvas"), configs[index]);
      if (chart) load.charts.push(chart);
      const description = target.previousElementSibling.querySelector("span");
      description.textContent = `${query.get("interval") === "hour" ? "Hourly" : "Daily"} · ${rows.length} buckets${data.page?.has_more ? " · first page of this range" : ""}`;
    }
  } catch (error) {
    if (!usageRequestCurrent(load) || controller.signal.aborted || error.name === "AbortError") return;
    host.innerHTML = `<div class="notice" role="status">Charts unavailable: ${esc(error.message)} <button type="button" data-retry-usage-charts>Retry charts</button></div>`;
    host.querySelector("[data-retry-usage-charts]").addEventListener("click", handleAsync(() => { host.innerHTML = usageChartsShell().replace(/^<div[^>]*>|<\/div>$/g, ""); return loadUsageVisuals(); }));
  } finally {
    if (usageRequestCurrent(load)) {
      load.visualizationActive = false;
      if (!controller.signal.aborted) host.setAttribute("aria-busy", "false");
      drainUsageSections(load);
    }
  }
}

function loadVisibleUsageSections(root) {
  const load = usageLoad;
  if (!load || load.root !== root) return;
  root.querySelectorAll("[data-usage-section]").forEach((host) => {
    const section = host.dataset.usageSection;
    if (!host.closest(".breakdown-section")?.hidden && !load.loaded.has(section) && !load.controllers.has(section) && !load.queue.includes(section)) load.queue.push(section);
  });
  drainUsageSections(load);
}

function drainUsageSections(load) {
  if (!usageRequestCurrent(load)) return;
  while (load.active + (load.visualizationActive ? 1 : 0) < 3 && load.queue.length) {
    const section = load.queue.shift();
    if (load.loaded.has(section) || load.controllers.has(section)) continue;
    load.active += 1;
    loadUsageSection(section).finally(() => { load.active -= 1; drainUsageSections(load); });
  }
}

async function loadUsageSection(section, force = false) {
  const load = usageLoad;
  if (!load || !usageRequestCurrent(load) || (!force && load.controllers.has(section))) return;
  const host = load.root.querySelector(`[data-usage-section="${section}"]`);
  if (!host) return;
  const controller = usageRequestController(load, section);
  host.setAttribute("aria-busy", "true");
  host.innerHTML = emptyState("Loading this usage section…");
  const query = new URLSearchParams(load.query);
  query.set("timeseries_offset", String(state.usagePagination.timeseriesOffset));
  query.set("service_timeseries_offset", String(state.usagePagination.serviceTimeseriesOffset));
  if (section === "events") {
    const chronological = !["cost", "latency", "tokens", "status_code"].includes(query.get("sort_by"));
    if (chronological) { query.set("cursor", load.eventCursors[load.eventIndex] || "start"); query.set("offset", "0"); }
    else query.set("offset", String(state.usagePagination.eventsOffset));
  }
  try {
    const path = section === "events" ? "/admin-ui/admin/usage/events" : `/admin-ui/admin/usage/sections/${section}`;
    const data = await api(`${path}?${query}`, { signal: controller.signal });
    if (!usageRequestCurrent(load) || controller.signal.aborted) return;
    load.loaded.add(section);
    if (section === "events") {
      load.eventsPage = data;
      if (load.eventIndex === 0 && data.snapshot_cursor) load.eventCursors[0] = data.snapshot_cursor;
      host.innerHTML = usagePagedTable("Recent requests", section, usageEventsTable(data.rows || []), { ...data, offset: state.usagePagination.eventsOffset }, (data.rows || []).length);
    } else if (section === "timeseries" || section === "service-timeseries") {
      const rows = data.rows || [];
      host.innerHTML = usagePagedTable(section === "timeseries" ? "Timeseries" : "Service timeseries", section, section === "timeseries" ? usageTimeseriesTable(rows) : usageServiceTimeseriesTable(rows), data.page, rows.length);
    } else if (section === "project-key-services") host.innerHTML = usageProjectKeyServiceHierarchy(data.rows || []);
    else if (section === "unused-keys") host.innerHTML = unusedKeysTable(data.rows || []);
    else {
      const label = section === "projects" ? usageProjectName : section === "keys" ? usageKeyName : value => value;
      host.innerHTML = usageRanking(data.rows || [], label, esc) + usageBreakdownTable(data.rows || [], label);
    }
    if (["events", "timeseries", "service-timeseries"].includes(section)) host.previousElementSibling.hidden = true;
    host.querySelectorAll("[data-debug-request]").forEach((button) => button.addEventListener("click", handleAsync(openDebugRequest)));
    host.querySelectorAll("[data-usage-page-section]").forEach((button) => button.addEventListener("click", handleAsync(changeUsagePage)));
  } catch (error) {
    if (!usageRequestCurrent(load) || controller.signal.aborted || error.name === "AbortError") return;
    host.innerHTML = `<div class="notice" role="alert">This Usage section is unavailable: ${esc(error.message)} <button type="button" data-retry-usage-section>Retry section</button></div>`;
    host.querySelector("[data-retry-usage-section]").addEventListener("click", handleAsync(() => loadUsageSection(section, true)));
  } finally {
    if (usageRequestCurrent(load) && !controller.signal.aborted) host.setAttribute("aria-busy", "false");
  }
}

function bindUsageSuggestions(form) {
  for (const field of ["route", "endpoint", "model"]) {
    const input = form.elements.namedItem(field);
    let timer;
    let controller;
    const load = async () => {
      if (form !== document.querySelector("#usage-form")) return;
      controller?.abort();
      const current = controller = new AbortController();
      const status = input.parentElement.querySelector("[data-usage-suggestion-status]") || document.createElement("small");
      status.dataset.usageSuggestionStatus = field;
      status.setAttribute("role", "status");
      input.parentElement.append(status);
      try {
        const filterQuery = usageFilterValuesQueryFromForm(form);
        filterQuery.delete(field);
        filterQuery.set("field", field);
        filterQuery.set("q", input.value.trim());
        filterQuery.set("limit", "20");
        const data = await api(`/admin-ui/admin/usage/filter-values?${filterQuery}`, { signal: current.signal });
        if (current.signal.aborted || form !== document.querySelector("#usage-form")) return;
        document.querySelector(`#usage-${field}-options`).innerHTML = (data.values || []).map((value) => `<option value="${attr(value)}"></option>`).join("");
        status.textContent = "";
      } catch (error) {
        if (!current.signal.aborted && error.name !== "AbortError") status.textContent = "Suggestions unavailable. Exact values can still be entered.";
      }
    };
    input.addEventListener("focus", () => { clearTimeout(timer); load(); });
    input.addEventListener("input", () => { controller?.abort(); clearTimeout(timer); timer = setTimeout(load, 250); });
    form.addEventListener("change", () => { controller?.abort(); clearTimeout(timer); });
  }
}

function usageIdentifierLabels() {
  if (!usageLabelCache || usageLabelCache.projects !== state.projects || usageLabelCache.keys !== state.keys) {
    usageLabelCache = { projects: state.projects, keys: state.keys, projectNames: new Map(state.projects.map((project) => [project.id, project.name])), keyNames: new Map(state.keys.map((key) => [key.id, key.name ? `${key.name} (${key.key_prefix})` : key.key_prefix])) };
  }
  return usageLabelCache;
}

function usageProjectName(projectId) {
  return projectId && projectId !== "individual" ? usageIdentifierLabels().projectNames.get(projectId) || projectId : "Individual";
}

function usageKeyName(keyId) {
  return usageIdentifierLabels().keyNames.get(keyId) || keyId;
}

async function applyUsageFilters(event) {
  event.preventDefault();
  const filters = Object.fromEntries(new FormData(event.currentTarget));
  if (state.projectScope !== (filters.project_id || "")) {
    filters.key_id = "";
    event.currentTarget.elements.namedItem("key_id").value = "";
  }
  synchronizeProjectScope(filters.project_id || "");
  state.usageFilters = filters;
  persistMonitoringHash();
  syncProjectScope();
  resetUsagePagination();
  await loadUsage(event);
}

function resetUsagePagination() {
  state.usagePagination.eventsOffset = 0;
  state.usagePagination.timeseriesOffset = 0;
  state.usagePagination.serviceTimeseriesOffset = 0;
}

function usagePageSize() {
  const form = document.querySelector("#usage-form");
  const value = form ? Number(new FormData(form).get("limit")) : 50;
  if (!Number.isFinite(value)) return 50;
  return Math.min(Math.max(Math.trunc(value), 1), 500);
}

async function changeUsagePage(event) {
  const section = event.currentTarget.dataset.usagePageSection;
  const direction = event.currentTarget.dataset.usagePageDirection;
  const key = usagePaginationKey(section);
  if (!key || !usageLoad) return;
  const limitKey = section === "timeseries" ? "timeseries_limit" : section === "service-timeseries" ? "service_timeseries_limit" : "limit";
  const pageSize = Number((section === "events" && usageLoad.eventsPage?.limit) || usageLoad.query.get(limitKey) || 50);
  if (section === "events") {
    if (direction === "next") {
      usageLoad.eventCursors[usageLoad.eventIndex + 1] = usageLoad.eventsPage?.next_cursor;
      usageLoad.eventIndex += 1;
    } else usageLoad.eventIndex = Math.max(0, usageLoad.eventIndex - 1);
  }
  const current = state.usagePagination[key];
  state.usagePagination[key] = direction === "next" ? current + pageSize : Math.max(0, current - pageSize);
  await loadUsageSection(section, true);
}

function usagePaginationKey(section) {
  if (section === "events") return "eventsOffset";
  if (section === "timeseries") return "timeseriesOffset";
  if (section === "service-timeseries") return "serviceTimeseriesOffset";
  return "";
}

function usageQueryFromForm(formElement = document.querySelector("#usage-form"), { includeDateRange = true } = {}) {
  const form = formElement ? new FormData(formElement) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["project_id", "key_id", "service", "route", "method", "endpoint", "provider", "model", "task_id", "run_id", "trace_id", "status", "status_code", "min_cost_usd", "breakdown_limit", "sort_by", "limit"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  if (state.projectScope) query.set("project_id", state.projectScope);
  if (includeDateRange) {
    const range = usageDateRange(form);
    if (range.from) query.set("from", range.from);
    if (range.to) query.set("to", range.to);
  }
  const interval = form.get("interval");
  if (interval) query.set("interval", interval);
  return query;
}

function usageFilterValuesQueryFromForm(formElement = document.querySelector("#usage-form")) {
  const form = formElement ? new FormData(formElement) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["project_id", "key_id", "service", "route", "method", "endpoint", "provider", "model", "task_id", "run_id", "trace_id", "status", "status_code"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  const range = usageDateRange(form);
  if (range.from) query.set("from", range.from);
  if (range.to) query.set("to", range.to);
  return query;
}

function usageDateRange(form) {
  const preset = form.get("time_preset");
  if (!preset || preset === "all_time") return {};

  const now = new Date();
  if (preset === "custom") {
    const fromInput = form.get("from");
    const toInput = form.get("to");
    const from = fromInput ? new Date(fromInput) : null;
    const to = toInput ? new Date(toInput) : null;
    if ((from && Number.isNaN(from.getTime())) || (to && Number.isNaN(to.getTime()))) {
      throw new Error("Use valid custom usage dates.");
    }
    if (from && to && from > to) {
      throw new Error("Usage start time must be before the end time.");
    }
    return {
      from: from ? from.toISOString() : "",
      to: to ? to.toISOString() : "",
    };
  }

  const start = new Date(now);
  const end = new Date(now);
  if (preset === "last_1h") start.setHours(start.getHours() - 1);
  if (preset === "last_6h") start.setHours(start.getHours() - 6);
  if (preset === "last_24h") start.setHours(start.getHours() - 24);
  if (preset === "last_7d") start.setDate(start.getDate() - 7);
  if (preset === "last_30d") start.setDate(start.getDate() - 30);
  if (preset === "today") {
    start.setHours(0, 0, 0, 0);
  }
  if (preset === "yesterday") {
    start.setDate(start.getDate() - 1);
    start.setHours(0, 0, 0, 0);
    end.setHours(0, 0, 0, 0);
  }
  if (preset === "this_week") {
    const day = start.getDay();
    const daysSinceMonday = day === 0 ? 6 : day - 1;
    start.setDate(start.getDate() - daysSinceMonday);
    start.setHours(0, 0, 0, 0);
  }
  if (preset === "this_month") {
    start.setDate(1);
    start.setHours(0, 0, 0, 0);
  }
  return { from: start.toISOString(), to: end.toISOString() };
}

async function usageExportAction(event) {
  const action = event.currentTarget.dataset.usageExportAction;
  const exportForm = document.querySelector("#usage-export-form");
  const format = exportForm ? new FormData(exportForm).get("export_format") || "csv" : "csv";
  const { query, allRows } = usageExportQuery();
  if (allRows) {
    if (action === "preview") throw new Error("Preview is limited to one bounded page. Choose a maximum row count.");
    if (!query.get("from") || !query.get("to")) throw new Error("All rows requires a bounded start and end time.");
    query.set("stream", "true");
    if (action === "download") { await downloadAllUsageExport(format, query); return; }
  }
  const path = usageExportPath(format, query);
  if (action === "copy-url") {
    await navigator.clipboard.writeText(path);
    setNotice("Export URL copied.", "success");
    return;
  }
  if (action === "copy-curl") {
    await navigator.clipboard.writeText(`curl -sS -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" "${location.origin}${path}"`);
    setNotice("Export curl copied.", "success");
    return;
  }
  const response = await fetchWithTimeout(path, {
    headers: usageExportHeaders(),
  });
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  if (action === "download") {
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `relayna-usage-${new Date().toISOString()}.${format}`;
    anchor.click();
    URL.revokeObjectURL(url);
    return;
  }
  const text = format === "json" ? JSON.stringify(await response.json(), null, 2) : await response.text();
  showTextModal(`Usage export ${format.toUpperCase()}`, text);
}

function usageExportQuery() {
  const form = document.querySelector("#usage-export-form");
  const data = form ? new FormData(form) : null;
  const exportRange = data ? usageExportDateRange(data) : null;
  const query = usageQueryFromForm(undefined, { includeDateRange: !exportRange });
  let allRows = false;
  if (data) {
    const exportLimit = data.get("export_limit");
    allRows = exportLimit === "all";
    if (allRows) query.delete("limit");
    else query.set("limit", exportLimit || "1000");
    query.set("offset", allRows ? "0" : data.get("export_offset") || "0");

    if (exportRange) {
      query.delete("from");
      query.delete("to");
      if (exportRange.from) query.set("from", exportRange.from);
      if (exportRange.to) query.set("to", exportRange.to);
    }
  }
  return { query, allRows };
}

function usageExportDateRange(form) {
  const fromInput = String(form.get("export_from") || "").trim();
  const toInput = String(form.get("export_to") || "").trim();
  if (!fromInput && !toInput) return null;

  const from = fromInput ? new Date(fromInput) : null;
  const to = toInput ? new Date(toInput) : null;
  if ((from && Number.isNaN(from.getTime())) || (to && Number.isNaN(to.getTime()))) {
    throw new Error("Use valid export dates.");
  }
  if (from && to && from >= to) {
    throw new Error("Export start time must be before the end time.");
  }
  return {
    from: from ? from.toISOString() : "",
    to: to ? to.toISOString() : "",
  };
}

function usageExportPath(format, query = usageExportQuery().query) {
  return `/admin-ui/admin/usage/export.${format}?${query}`;
}

function usageExportHeaders() {
  if (token()) return { authorization: `Bearer ${token()}` };
  if (state.session?.csrf_token) return { "x-csrf-token": state.session.csrf_token };
  return {};
}

function syncUsageExportControls() {
  const form = document.querySelector("#usage-export-form");
  if (!form) return;
  const allRows = new FormData(form).get("export_limit") === "all";
  form.querySelector("[name=export_offset]").disabled = allRows;
  form.querySelectorAll("[data-usage-export-action]").forEach((button) => {
    button.disabled = allRows && button.dataset.usageExportAction === "preview";
  });
}

async function downloadAllUsageExport(format, baseQuery) {
  if (!baseQuery.get("from") || !baseQuery.get("to")) {
    throw new Error("All rows requires a bounded start and end time. Set Export from and Export to, or apply a bounded Usage time filter.");
  }
  const query = new URLSearchParams(baseQuery);
  query.set("stream", "true");
  query.delete("offset");
  const path = usageExportPath(format, query);
  const filename = `relayna-usage-${new Date().toISOString().replaceAll(":", "-")}.${format}`;
  if (!window.showSaveFilePicker) {
    // Both session and operator requests need protected headers. Native links
    // cannot carry those headers; do not weaken auth or buffer a huge Blob.
    throw new Error("This browser cannot stream downloads to disk. Use Copy curl for All rows, or choose a bounded row count.");
  }
  const file = await window.showSaveFilePicker({ suggestedName: filename });
  const writable = await file.createWritable();
  const controller = new AbortController();
  const abort = () => controller.abort();
  viewController.signal.addEventListener("abort", abort, { once: true });
  try {
    // fetchWithTimeout buffers finite responses; this path keeps backpressure
    // from the file sink through the browser to each bounded database batch.
    const response = await fetch(path, { headers: usageExportHeaders(), signal: controller.signal });
    if (!response.ok || !response.body) {
      await writable.abort();
      throw new Error(`Usage export unavailable: ${response.status} ${response.statusText}`);
    }
    await response.body.pipeTo(writable, { signal: controller.signal });
    setNotice("Usage export saved.", "success");
  } catch (error) {
    await writable.abort().catch(() => {});
    throw error;
  } finally {
    viewController.signal.removeEventListener("abort", abort);
  }
}

async function loadTaskUsage(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const taskId = form.get("task_lookup");
  const query = usageQueryFromForm();
  const summary = await api(`/admin-ui/admin/tasks/${encodeURIComponent(taskId)}/usage?${query}`);
  const target = document.querySelector("#task-usage-result");
  if (target) {
    target.innerHTML = `<div class="grid stats">
      ${stat("Requests", summary.request_count)}
      ${stat("Failures", summary.failure_count)}
      ${stat("Cost", money(summary.estimated_cost_usd))}
      ${stat("Fallback rate", percent(summary.fallback_rate))}
    </div>`;
  }
}

function usageBreakdownTable(rows, label = (value) => value) {
  return table(
    ["Name", "Requests", "Success", "Failure", "Avg latency", "Total latency", "Cost"],
    rows.map((row) => [
      esc(label(row.name)),
      row.summary.request_count,
      row.summary.success_count,
      row.summary.failure_count,
      row.summary.average_latency_ms == null ? "n/a" : `${Math.round(row.summary.average_latency_ms)} ms`,
      `${row.summary.total_latency_ms} ms`,
      money(row.summary.estimated_cost_usd),
    ]),
  );
}

function usageProjectKeyServiceHierarchy(rows) {
  if (!rows.length) return emptyState("No project, virtual key, and service usage combinations are available for these filters.");
  const projects = new Map();
  rows.forEach((row) => {
    const projectId = row.project_id || "";
    if (!projects.has(projectId)) projects.set(projectId, new Map());
    const keys = projects.get(projectId);
    if (!keys.has(row.key_id)) keys.set(row.key_id, []);
    keys.get(row.key_id).push(row);
  });
  return `<div class="usage-hierarchy">
    <div class="help">Showing ${rows.length} top project, key, and service combinations using the selected sort and limit.</div>
    ${[...projects.entries()].map(([projectId, keys], projectIndex) => `
      <details class="usage-hierarchy-project" data-usage-project="${attr(projectId || "individual")}" ${projectIndex === 0 ? "open" : ""}>
        <summary><span><strong>${esc(usageProjectName(projectId))}</strong><small>${keys.size} virtual ${keys.size === 1 ? "key" : "keys"}</small></span><span>${[...keys.values()].reduce((count, services) => count + services.length, 0)} combinations</span></summary>
        <div class="usage-hierarchy-keys">
          ${[...keys.entries()].map(([keyId, services], keyIndex) => `
            <details class="usage-hierarchy-key" data-usage-key="${attr(keyId)}" ${projectIndex === 0 && keyIndex === 0 ? "open" : ""}>
              <summary><span><strong><code>${esc(usageKeyName(keyId))}</code></strong><small>${services.length} ${services.length === 1 ? "service" : "services"}</small></span></summary>
              ${usageProjectKeyServiceTable(services)}
            </details>`).join("")}
        </div>
      </details>`).join("")}
  </div>`;
}

function usageProjectKeyServiceTable(rows) {
  return table(
    ["Service", "Requests", "Success", "Failure", "Tokens", "Avg latency", "Cost"],
    rows.map((row) => [
      esc(row.service_name == null ? "Unattributed" : row.service_name),
      row.summary.request_count,
      row.summary.success_count,
      row.summary.failure_count,
      row.summary.total_tokens,
      row.summary.average_latency_ms == null ? "n/a" : `${Math.round(row.summary.average_latency_ms)} ms`,
      money(row.summary.estimated_cost_usd),
    ]),
  );
}

function usagePagedTable(title, section, tableMarkup, page = {}, rowCount = 0) {
  const offset = Number(page.offset || 0);
  const limit = Number(page.limit || usagePageSize());
  const hasMore = Boolean(page.has_more);
  const start = rowCount ? offset + 1 : 0;
  const end = rowCount ? offset + rowCount : 0;
  return `
    <div class="usage-section-heading">
      <h4>${esc(title)}</h4>
      <div class="table-pager" aria-label="${attr(`${title} pagination`)}">
        <span>Showing ${start}-${end}</span>
        <button type="button" data-usage-page-section="${attr(section)}" data-usage-page-direction="previous" ${offset <= 0 ? "disabled" : ""}>Previous</button>
        <button type="button" data-usage-page-section="${attr(section)}" data-usage-page-direction="next" ${hasMore ? "" : "disabled"}>Next</button>
      </div>
    </div>
    ${tableMarkup}
  `;
}

function usageEventsTable(rows, { ownerService = null, ownerProject = null } = {}) {
  if (!ownerService && !ownerProject) return table(
    ["Created", "Request / service", "Model / provider", "Outcome", "Latency", "Tokens / cost", "Details"],
    rows.map(row => [
      time(row.created_at),
      `<div class="traffic-request-cell"><code title="${attr(row.request_id)}">${esc(row.request_id)}</code><small>${esc(row.service_name || row.route || "Not recorded")} ${row.diagnostics?.websocket ? badge("WS", "neutral") : row.diagnostics?.protocol === "http" ? badge("HTTP", "neutral") : ""}</small></div>`,
      `<span>${esc(row.model || "—")}</span><div class="subtle">${esc(row.provider || "Not recorded")}</div>`,
      `${badge(row.status, row.status === "success" ? "good" : "bad")}<div class="subtle">HTTP ${esc(row.status_code)}</div>`,
      esc(trafficDuration(row.latency_ms)),
      `${esc(usageValue(row.diagnostics, row.total_tokens))} tokens<div class="subtle">${esc(usageValue(row.diagnostics, row.estimated_cost_usd, money))}</div>`,
      `<button type="button" class="icon-button" aria-label="Inspect request ${attr(row.request_id)}" title="Inspect request" data-nav="health" data-debug-request="${attr(row.request_id)}" data-debug-usage="${attr(JSON.stringify(investigationUsageSnapshot(row)))}"><i class="ti ti-chevron-right" aria-hidden="true"></i></button>`,
    ]),
  );
  return table(
    ["Created", "Request", "Protocol / transfer", "Route", "Routing mode", "Service", "Method", "Endpoint", "Model", "Provider", "Status", "Latency", "Tokens", "Cost", "Cost source", "Pricing rule", "Trace", "Actions"],
    rows.map((row) => [
      time(row.created_at),
      `<code>${esc(row.request_id)}</code>`,
      row.diagnostics?.websocket ? `${badge("WS", "neutral")}<div class="subtle">↑ ${esc(row.diagnostics.websocket.client_bytes)} B · ↓ ${esc(row.diagnostics.websocket.upstream_bytes)} B</div>` : badge(row.diagnostics?.protocol === "http" ? "HTTP" : "Not recorded", "neutral"),
      esc(row.route),
      badge(routingModeLabel(row.diagnostics), "neutral"),
      esc(row.service_name || ""),
      esc(row.http_method || ""),
      `<code>${esc(row.endpoint_template || row.endpoint_path || "")}</code>`,
      esc(row.model || ""),
      esc(row.provider),
      `<span class="usage-status">${badge(row.status, row.status === "success" ? "good" : "bad")}<code>${esc(row.status_code)}</code></span>`,
      `${esc(row.latency_ms)} ms`,
      esc(usageValue(row.diagnostics, row.total_tokens)),
      esc(usageValue(row.diagnostics, row.estimated_cost_usd, money)),
      esc(row.cost_source || ""),
      esc(row.pricing_rule_name || ""),
      row.trace_id ? `<code>${esc(row.trace_id)}</code>` : "",
      ownerService || ownerProject
        ? `<button type="button" ${ownerService ? `data-owner-service="${attr(ownerService)}"` : `data-owner-project="${attr(ownerProject)}"`} data-owner-request="${attr(row.request_id)}">View details</button>`
        : `<button type="button" data-nav="health" data-debug-request="${attr(row.request_id)}" data-debug-usage="${attr(JSON.stringify(investigationUsageSnapshot(row)))}">Debug</button>`,
    ]),
  );
}

function investigationView(data) {
  return requestInvestigationView(data, { esc, table, time, projects: state.projects });
}

async function openDebugRequest(event) {
  const trigger = event.currentTarget;
  const requestId = trigger.dataset.debugRequest;
  const usage = trigger.dataset.debugUsage ? JSON.parse(trigger.dataset.debugUsage) : null;
  const body = document.createElement("div");
  let open = true;
  const render = (traffic, notice = "") => {
    if (!open) return;
    body.innerHTML = investigationView({ traffic, usage, notice });
    bindInvestigationActions(body, requestId);
  };
  render(null, usage?.diagnostics?.traffic_id ? "Loading the matching request timeline…" : "This older usage record has no internal Traffic ID. Request-ID-only debug snapshots are not combined with it because client request IDs can be reused.");
  showContentDrawer("Request investigation", body, () => { open = false; }, trigger);
  if (!usage?.diagnostics?.traffic_id) return;
  try {
    const query = new URLSearchParams({ id: usage.diagnostics.traffic_id, limit: "1" });
    const rows = await api(`/admin-ui/admin/traffic/history?${query}`);
    const traffic = matchTrafficRecord(rows, usage);
    render(traffic, traffic ? "" : "The matching Traffic record is unavailable or was not saved. Usage metadata is still available.");
  } catch {
    render(null, "Traffic details could not be loaded. Usage metadata is still available; close and reopen to retry.");
  }
}

function unusedKeysTable(rows) {
  return table(
    ["Key", "Project", "Created", "Last used"],
    rows.map((row) => [
      `<code>${esc(row.key_prefix)}</code>`,
      esc(usageProjectName(row.project_id || "")),
      time(row.created_at),
      row.last_used_at ? time(row.last_used_at) : "never",
    ]),
  );
}

function usageTimeseriesTable(rows) {
  return table(
    ["Bucket", "Requests", "Success", "Failure", "Error rate", "P95 latency", "Cost"],
    rows.map((row) => {
      const requests = row.summary?.request_count ?? row.request_count ?? 0;
      const failures = row.summary?.failure_count ?? row.failure_count ?? 0;
      const p95Latency = row.summary?.p95_latency_ms ?? row.p95_latency_ms;
      return [
        esc(row.bucket_start || row.bucket || row.name),
        requests,
        row.summary?.success_count ?? row.success_count ?? 0,
        failures,
        requests ? `${((failures / requests) * 100).toFixed(1)}%` : "0.0%",
        p95Latency == null ? "n/a" : `${Math.round(p95Latency)} ms`,
        money(row.summary?.estimated_cost_usd ?? row.estimated_cost_usd),
      ];
    }),
  );
}

function usageServiceTimeseriesTable(rows) {
  return table(
    ["Bucket", "Service", "Requests", "Success", "Failure", "Avg latency", "Cost"],
    rows.map((row) => [
      esc(row.bucket_start || row.bucket || row.name),
      esc(row.service_name || "none"),
      row.summary?.request_count ?? row.request_count ?? 0,
      row.summary?.success_count ?? row.success_count ?? 0,
      row.summary?.failure_count ?? row.failure_count ?? 0,
      row.summary?.average_latency_ms == null ? "n/a" : `${Math.round(row.summary.average_latency_ms)} ms`,
      money(row.summary?.estimated_cost_usd ?? row.estimated_cost_usd),
    ]),
  );
}

async function health() {
  const renderId = ++renderGeneration;
  const gatewayHost = document.createElement("div");
  gatewayHost.style.minWidth = "0";
  content.replaceChildren(gatewayHost);
  stopGatewayStatus = mountHealthGatewayStatus({ content: gatewayHost, api, esc, attr, badge, table, Chart });
  const issues = [];
  const recentWindow = new URLSearchParams({ from: new Date(Date.now() - 86400000).toISOString(), to: new Date().toISOString() });
  const [ready, rows, healthState, importVersions] = await Promise.all([
    readiness(),
    monitoringPanel(`/admin-ui/admin/provider-health?${recentWindow}`, [], issues),
    monitoringPanel("/admin-ui/admin/provider-health/state", [], issues),
    monitoringPanel("/admin-ui/admin/services/import/versions", [], issues),
  ]);
  state.providerHealthState = healthState;
  state.serviceImportVersions = importVersions;
  const requestCount = rows.reduce((sum, row) => sum + row.request_count, 0);
  const errorCount = rows.reduce((sum, row) => sum + row.error_count, 0);
  const fallbackCount = rows.reduce((sum, row) => sum + row.fallback_count, 0);
  const errorRate = requestCount ? `${((errorCount / requestCount) * 100).toFixed(1)}%` : "0.0%";
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    ${monitoringIssues(issues)}
    <section class="panel">
      <div class="panel-heading"><h3>Recorded reliability</h3><span class="subtle">Last 24 hours · minimum 20 requests · elevated at 5%</span></div>
      <div class="monitor-history-summary">${stat("Gateway", ready.status)}${stat("Routes observed", rows.length)}${stat("Error rate", errorRate)}${stat("Fallbacks", fallbackCount)}</div>
      ${healthTable(rows)}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>Current availability & circuit state</h3>
        <button type="button" data-health-action="check">Run checks</button>
      </div>
      ${healthStateTable(healthState)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Manage provider health state</h3><span class="subtle">Writes explicit provider intelligence state</span></div>
      <form id="provider-health-state-form" class="form-grid">
        <label>Name<input name="name" required placeholder="LiteLLM"></label>
        <label>Provider<select name="provider">
          <option value="LiteLlm">LiteLLM</option>
          <option value="OpenAiCompatible">OpenAI-compatible</option>
          <option value="InternalService">Internal service</option>
        </select></label>
        <label>Status<select name="status">
          <option value="healthy">Healthy</option>
          <option value="degraded">Degraded</option>
          <option value="unhealthy">Unhealthy</option>
          <option value="unknown">Unknown</option>
        </select></label>
        <label>Circuit<select name="circuit_state">
          <option value="closed">Closed</option>
          <option value="half_open">Half open</option>
          <option value="open">Open</option>
        </select></label>
        <label>Active check<select name="active_check_ok">
          <option value="">Unknown</option>
          <option value="true">OK</option>
          <option value="false">Failed</option>
        </select></label>
        <label>Passive success<input name="passive_success_count" type="number" min="0" value="0"></label>
        <label>Passive failure<input name="passive_failure_count" type="number" min="0" value="0"></label>
        <label>Consecutive failures<input name="consecutive_failures" type="number" min="0" value="0"></label>
        <label>Average latency ms<input name="average_latency_ms" type="number" min="0"></label>
        <label>Last error<input name="last_error_code"></label>
        <label>Cooldown until<input name="cooldown_until" type="datetime-local"></label>
        <div class="form-actions"><button class="primary">Save health state</button></div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Debug bundle</h3></div>
      <form id="debug-bundle-form" class="inline-form">
        <label>Request ID<input name="request_id" placeholder="request ID" required></label>
        <button>Load</button>
      </form>
      ${state.debugBundle ? debugBundleView(state.debugBundle) : ""}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Service import versions</h3></div>
      ${serviceImportVersionsTable(importVersions)}
    </section>
  `;
  document.querySelector("[data-health-action='check']").addEventListener("click", handleAsync(runHealthChecks));
  document.querySelector("#provider-health-state-form").addEventListener("submit", handleAsync(saveProviderHealthState));
  document.querySelectorAll("[data-health-state-edit]").forEach((button) => {
    button.addEventListener("click", () => fillProviderHealthStateForm(button.dataset.healthStateEdit));
  });
  document.querySelector("#debug-bundle-form").addEventListener("submit", handleAsync(loadDebugBundle));
  if (state.debugBundle) bindInvestigationActions(content, state.debugBundle.request_id);
  document.querySelectorAll("[data-import-rollback]").forEach((button) => {
    button.addEventListener("click", handleAsync(rollbackImportVersion));
  });
  content.prepend(gatewayHost);
  organizeView("health");
  content.querySelector("[data-retry-monitoring]")?.addEventListener("click", () => refresh());
}

function healthTable(rows) {
  return table(
    ["Name", "Status", "Requests", "Errors", "Timeouts", "Fallbacks", "Avg latency"],
    rows.map((row) => [
      esc(row.name),
      healthBadge(row),
      row.request_count,
      row.error_count,
      row.timeout_count,
      row.fallback_count ? badge(row.fallback_count, "warn") : "0",
      `${averageLatency(row)} ms`,
    ]),
  );
}

function healthBadge(row) {
  const signal = reliability(row);
  return badge(signal.label, signal.tone);
}

function averageLatency(row) {
  if (!row.request_count) return 0;
  return Math.round(row.total_latency_ms / row.request_count);
}

function healthStateTable(rows) {
  return table(
    ["Name", "Provider", "Status", "Circuit", "Active check", "Passive", "Latency", "Last error", "Cooldown", "Actions"],
    rows.map((row) => [
      esc(row.name),
      esc(row.provider),
      badge(row.status),
      badge(row.circuit_state),
      row.active_check_ok === true ? badge("ok", "good") : row.active_check_ok === false ? badge("failed", "bad") : badge("unknown", "warn"),
      `${badge(`${row.passive_success_count ?? 0} ok`, "good")} ${badge(`${row.passive_failure_count ?? 0} failed`, row.passive_failure_count ? "bad" : "neutral")}`,
      esc(row.average_latency_ms ?? ""),
      esc(row.last_error_code ?? ""),
      esc(row.cooldown_until ? time(row.cooldown_until) : ""),
      `<button type="button" data-health-state-edit="${attr(`${row.provider}|${row.name}`)}">Edit state</button>`,
    ]),
  );
}

function debugBundleView(bundle) {
  return investigationView({ bundle, notice: "Legacy debug snapshot, looked up by client request ID. Timings and usage are shown only when captured in the same request record." });
}

function serviceImportVersionsTable(rows) {
  return table(
    ["Version", "Source", "Activated", "Rollback", "Diff", "Actions"],
    rows.map((row) => [
      row.version,
      esc(row.source),
      esc(row.activated_at ? time(row.activated_at) : ""),
      esc(row.rolled_back_from_version ?? ""),
      esc(`+${row.diff.added.length} changed ${row.diff.changed.length} removed ${row.diff.removed.length}`),
      `<button type="button" data-import-rollback="${attr(row.version)}">Rollback</button>`,
    ]),
  );
}

async function runHealthChecks() {
  await api("/admin-ui/admin/provider-health/check", { method: "POST", body: "{}" });
  setNotice("Provider health checks completed.", "success");
  await health();
}

async function saveProviderHealthState(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const activeCheck = form.get("active_check_ok");
  const now = new Date().toISOString();
  const existing = state.providerHealthState.find((row) => row.provider === form.get("provider") && row.name === form.get("name"));
  const body = {
    name: form.get("name"),
    provider: form.get("provider"),
    status: form.get("status"),
    circuit_state: form.get("circuit_state"),
    active_check_ok: activeCheck === "" ? null : activeCheck === "true",
    passive_success_count: nullableNumber(form.get("passive_success_count")) ?? existing?.passive_success_count ?? 0,
    passive_failure_count: nullableNumber(form.get("passive_failure_count")) ?? existing?.passive_failure_count ?? 0,
    consecutive_failures: nullableNumber(form.get("consecutive_failures")) ?? existing?.consecutive_failures ?? 0,
    average_latency_ms: nullableNumber(form.get("average_latency_ms")),
    last_error_code: nullableString(form.get("last_error_code")),
    cooldown_until: isoDate(form.get("cooldown_until")),
    checked_at: existing?.checked_at ?? now,
    updated_at: now,
  };
  await api("/admin-ui/admin/provider-health/state", { method: "POST", body: JSON.stringify(body) });
  setNotice("Provider health state saved.", "success");
  await health();
}

function fillProviderHealthStateForm(key) {
  const [provider, name] = key.split("|");
  const row = state.providerHealthState.find((candidate) => candidate.provider === provider && candidate.name === name);
  const form = document.querySelector("#provider-health-state-form");
  if (!row || !form) return;
  for (const [field, value] of Object.entries({
    name: row.name,
    provider: row.provider,
    status: row.status,
    circuit_state: row.circuit_state,
    active_check_ok: row.active_check_ok == null ? "" : String(row.active_check_ok),
    passive_success_count: row.passive_success_count ?? 0,
    passive_failure_count: row.passive_failure_count ?? 0,
    consecutive_failures: row.consecutive_failures ?? 0,
    average_latency_ms: row.average_latency_ms ?? "",
    last_error_code: row.last_error_code ?? "",
    cooldown_until: toLocalInput(row.cooldown_until),
  })) {
    const input = form.elements.namedItem(field);
    if (input) input.value = value;
  }
}

async function loadDebugBundle(event) {
  event.preventDefault();
  const requestId = new FormData(event.target).get("request_id");
  try {
    state.debugBundle = await api(`/admin-ui/admin/debug-bundles/${encodeURIComponent(requestId)}`);
    await health();
  } catch (error) {
    state.debugBundle = null;
    if (error.message === "debug_bundle_not_found") {
      setNotice("No debug bundle was captured for this request. Service demo rows may have usage data without proxy debug traces.");
      await health();
      return;
    }
    throw error;
  }
}

async function rollbackImportVersion(event) {
  const version = event.currentTarget.dataset.importRollback;
  if (!(await confirmAction(`Rollback import ${version}`, "This activates the stored service registry snapshot."))) return;
  await api(`/admin-ui/admin/services/import/rollback/${version}`, { method: "POST", body: "{}" });
  setNotice(`Service registry rolled back to ${version}.`, "success");
  await health();
}

function formSection(title, description, body, open = false) {
  return `<details class="form-section wide-field" ${open ? "open" : ""}>
    <summary><span><strong>${esc(title)}</strong><small>${esc(description)}</small></span><i class="ti ti-chevron-down" aria-hidden="true"></i></summary>
    <div class="form-section-grid">${body}</div>
  </details>`;
}

function table(headers, rows) {
  if (!rows.length) return '<div class="empty-state"><p>No rows.</p></div>';
  return tableWrap(`<table><thead><tr>${headers.map((h) => `<th>${esc(h)}</th>`).join("")}</tr></thead><tbody>${rows
    .map((row) => `<tr>${row.map((cell) => `<td>${cell ?? ""}</td>`).join("")}</tr>`)
    .join("")}</tbody></table>`);
}

function policyBody(form) {
  const body = {
    allowed_routes: csv(form.get("allowed_routes")),
    allowed_models: csv(form.get("allowed_models")),
    allowed_providers: form.getAll("allowed_providers"),
    allowed_services: csv(form.get("allowed_services")),
    rpm_limit: nullableNumber(form.get("rpm_limit")),
    tpm_limit: nullableNumber(form.get("tpm_limit")),
    daily_budget_usd: nullableNumber(form.get("daily_budget_usd")),
    monthly_budget_usd: nullableNumber(form.get("monthly_budget_usd")),
    max_requests_per_day: nullableNumber(form.get("max_requests_per_day")),
    max_tokens_per_day: nullableNumber(form.get("max_tokens_per_day")),
    max_cost_per_request: nullableNumber(form.get("max_cost_per_request")),
    max_input_tokens_per_request: nullableNumber(form.get("max_input_tokens_per_request")),
    max_output_tokens_per_request: nullableNumber(form.get("max_output_tokens_per_request")),
    allowed_hours_utc: csv(form.get("allowed_hours_utc")).map((value) => Number(value)).filter((value) => Number.isInteger(value)),
    unused_key_auto_disable_after_days: nullableNumber(form.get("unused_key_auto_disable_after_days")),
    max_request_body_bytes: nullableNumber(form.get("max_request_body_bytes")),
    max_response_body_bytes: nullableNumber(form.get("max_response_body_bytes")),
    allow_streaming: form.has("allow_streaming"),
    allow_tools: form.has("allow_tools"),
  };
  return body;
}

function guardrailPolicyBody(form) {
  const forbidden = form.getAll("forbidden_guardrails");
  const configurable = new Set([...form.getAll("mandatory_guardrails"), ...form.getAll("optional_guardrails")]);
  const guardrailConfigOverrides = {};
  for (const name of form.getAll("guardrail_override_names")) {
    if (forbidden.includes(name)) throw new Error("guardrail_override_forbidden");
    if (!configurable.has(name)) continue;
    const value = JSON.parse(form.get(`guardrail_override_${name}`) || "{}");
    if (!value || Array.isArray(value) || typeof value !== "object") throw new Error("invalid_guardrail_override");
    guardrailConfigOverrides[name] = value;
  }
  return {
    mandatory_guardrails: form.getAll("mandatory_guardrails"),
    optional_guardrails: form.getAll("optional_guardrails"),
    forbidden_guardrails: forbidden,
    guardrail_config_overrides: guardrailConfigOverrides,
  };
}

async function guardrails() {
  const renderId = ++renderGeneration;
  [state.guardrails, state.guardrailExecutions, state.guardrailSummary] = await Promise.all([
    api("/admin-ui/admin/guardrails"),
    api("/admin-ui/admin/guardrails/executions?limit=50"),
    api("/admin-ui/admin/guardrails/summary"),
  ]);
  const selected = state.guardrails.guardrails.find((guardrail) => guardrail.name === state.editingGuardrailName);
  const visual = guardrailGovernanceSnapshot(state.guardrails.guardrails, state.guardrailExecutions.executions);
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    <section class="page-insights" aria-label="Guardrail configuration and execution sample">
      <div class="page-insights-grid">
        ${insightStrip(visual.insights, { label: "Catalog policy coverage" })}
        ${distributionBars("Recent execution actions", visual.actions, { caption: `${state.guardrailExecutions.executions.length} returned executions · maximum 50, not a lifetime total`, empty: "No execution samples returned." })}
      </div>
      ${flowSummary("Enabled definitions by execution stage", visual.stages, "A guardrail can support several stages. These counts describe catalog capability; applied key policy and request mode determine what runs.")}
    </section>
    <div class="split guardrail-workspace">
      <section class="panel">
        <div class="panel-heading">
          <h3>Catalog</h3>
          <div class="actions">
            <span class="subtle">${state.guardrails.guardrails.length} configured</span>
            <button type="button" data-guardrail-action="new">New guardrail</button>
          </div>
        </div>
        ${guardrailCatalogTable(state.guardrails.guardrails)}
      </section>
      <section class="panel ${state.editingGuardrailName === null ? "muted-panel" : ""}">
        ${guardrailDrawer(selected)}
      </section>
    </div>
    <section class="panel">
      <div class="panel-heading"><h3>Recorded execution summary</h3><span class="subtle">Returned aggregate groups</span></div>
      ${guardrailSummaryTable(state.guardrailSummary.summary)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Recent executions</h3><span class="subtle">Latest returned sample · up to 50 executions</span></div>
      ${guardrailExecutionTable(state.guardrailExecutions.executions)}
    </section>
  `;
  document.querySelector("[data-guardrail-action='new']")?.addEventListener("click", () => {
    state.editingGuardrailName = "";
    guardrails();
  });
  document.querySelector("#guardrail-form")?.addEventListener("submit", handleAsync(submitGuardrail));
  document.querySelector("[data-guardrail-action='cancel']")?.addEventListener("click", () => {
    state.editingGuardrailName = null;
    guardrails();
  });
  document.querySelector("[data-guardrail-action='delete']")?.addEventListener("click", handleAsync(deleteGuardrail));
  document.querySelectorAll("[data-guardrail-edit]").forEach((button) => {
    button.addEventListener("click", () => {
      state.editingGuardrailName = button.dataset.guardrailEdit;
      guardrails();
    });
  });
  organizeView("guardrails");
}

function guardrailCatalogTable(rows) {
  return table(
    ["Name", "Provider", "Modes", "Default", "Failure", "Enabled", "Endpoint", "Token", "Actions"],
    rows.map((row) => [
      `<i class="ti ti-shield-check" aria-hidden="true"></i> <code>${esc(row.name)}</code><div class="subtle">${esc(row.description)}</div>`,
      badge(row.provider_kind),
      (row.modes || []).map(mode => badge(mode === "pre_call" ? "Pre-call" : mode === "post_call" ? "Post-call" : mode === "during_call" ? "Streaming" : mode)).join(" "),
      row.default_on ? '<span class="badge good">default</span>' : '<span class="badge">opt-in</span>',
      badge(row.failure_policy, row.failure_policy === "fail_open" || row.failure_policy === "dry_run" ? "warn" : "neutral"),
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      row.endpoint_configured ? '<span class="badge good">configured</span>' : '<span class="badge">built-in</span>',
      row.token_configured ? '<span class="badge good">configured</span>' : '<span class="badge">none</span>',
      `<button type="button" data-guardrail-edit="${attr(row.name)}">Edit</button>`,
    ]),
  );
}

function guardrailDrawer(guardrail) {
  if (state.editingGuardrailName === null) {
    return '<div class="empty-state"><h3>No guardrail selected</h3></div>';
  }
  const creating = state.editingGuardrailName === "";
  const builtIn = !creating && guardrail?.provider_kind === "built_in";
  const titleText = creating ? "New guardrail" : `Edit ${guardrail ? guardrail.name : "guardrail"}`;
  const schemaValue = JSON.stringify(guardrail?.config_schema ?? {}, null, 2);
  const runtimeConfigValue = JSON.stringify(guardrail?.runtime_config ?? {}, null, 2);
  return `
    <div class="panel-heading">
      <h3>${esc(titleText)}</h3>
      ${builtIn ? '<span class="badge">built-in</span>' : '<span class="badge good">http</span>'}
    </div>
    <form id="guardrail-form" class="form-grid guardrail-form" data-mode="${creating ? "create" : "edit"}" data-guardrail-name="${attr(guardrail?.name || "")}" data-provider-kind="${attr(guardrail?.provider_kind || "http")}">
      <label>Name<input name="name" required ${creating ? "" : "readonly"} value="${attr(guardrail?.name || "")}" placeholder="custom-policy-check"></label>
      <label>Description<input name="description" ${builtIn ? "disabled" : "required"} value="${attr(guardrail?.description || "")}"></label>
      <div class="field"><span>Modes</span><small class="field-hint">Execution stages: pre_call before forwarding, post_call after a response, during_call during streaming.</small>${guardrailModeSelect(guardrail?.modes || ["pre_call"])}</div>
      <label>Failure policy<select name="failure_policy">${["fail_closed", "fail_open", "dry_run"].map((value) => option(value, guardrail?.failure_policy || "fail_closed")).join("")}</select></label>
      <label>Timeout ms<input name="timeout_ms" type="number" min="100" max="10000" value="${attr(guardrail?.timeout_ms ?? 1500)}" ${builtIn ? "disabled" : ""}></label>
      <label>Endpoint URL<input name="endpoint_url" type="url" ${creating ? "required" : ""} value="${attr(guardrail?.endpoint_url || "")}" placeholder="https://guardrail.example/check" ${builtIn ? "disabled" : ""}></label>
      <label>Bearer token<input name="bearer_token" type="password" autocomplete="new-password" placeholder="${guardrail?.token_configured ? "configured" : "optional"}" ${builtIn ? "disabled" : ""}></label>
      <label class="check"><input name="clear_token" type="checkbox" ${builtIn || creating ? "disabled" : ""}> Clear token</label>
      <label class="check"><input name="default_on" type="checkbox" ${guardrail?.default_on ? "checked" : ""}> Default on</label>
      <label class="check"><input name="enabled" type="checkbox" ${creating || guardrail?.enabled ? "checked" : ""}> Enabled</label>
      <label class="wide-field">Config schema JSON<textarea name="config_schema" rows="6">${esc(schemaValue)}</textarea></label>
      <label class="wide-field">Runtime config JSON<textarea name="runtime_config" rows="6">${esc(runtimeConfigValue)}</textarea></label>
      <div class="help">${builtIn ? "Built-in guardrails protect endpoint and token fields." : "Bearer tokens are write-only; leave blank to keep the current token."}</div>
      <div class="form-actions wide-field">
        <button class="primary">${creating ? "Create guardrail" : "Save guardrail"}</button>
        ${!creating && !builtIn ? '<button type="button" class="danger" data-guardrail-action="delete">Delete</button>' : ""}
        <button type="button" data-guardrail-action="cancel">Cancel</button>
      </div>
    </form>
  `;
}

function guardrailModeSelect(selected = []) {
  const values = new Set(Array.isArray(selected) && selected.length ? selected : ["pre_call"]);
  return `<div class="checkbox-group" role="group" aria-label="Guardrail modes">
    ${["pre_call", "post_call", "during_call"].map((value) => `<label><input name="modes" type="checkbox" value="${attr(value)}" ${values.has(value) ? "checked" : ""}> ${esc(value)}</label>`).join("")}
  </div>`;
}

function guardrailBody(form, creating, builtIn) {
  const configSchema = JSON.parse(form.get("config_schema") || "{}");
  const runtimeConfig = JSON.parse(form.get("runtime_config") || "{}");
  if (!runtimeConfig || Array.isArray(runtimeConfig) || typeof runtimeConfig !== "object") throw new Error("invalid_runtime_config");
  const body = {
    modes: form.getAll("modes"),
    default_on: form.has("default_on"),
    failure_policy: form.get("failure_policy"),
    config_schema: configSchema,
    runtime_config: runtimeConfig,
    enabled: form.has("enabled"),
  };
  if (creating || !builtIn) {
    body.description = form.get("description");
    body.endpoint_url = form.get("endpoint_url");
    body.timeout_ms = nullableNumber(form.get("timeout_ms"));
    const tokenValue = blankToUndefined(form.get("bearer_token"));
    if (tokenValue !== undefined) body.bearer_token = tokenValue;
    if (!creating && form.has("clear_token")) body.bearer_token = null;
  }
  if (creating) body.name = form.get("name");
  return body;
}

async function submitGuardrail(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const form = new FormData(formElement);
  const creating = formElement.dataset.mode === "create";
  const builtIn = formElement.dataset.providerKind === "built_in";
  let body;
  try {
    body = guardrailBody(form, creating, builtIn);
  } catch (error) {
    setNotice(error.message === "invalid_runtime_config" ? "invalid_runtime_config" : "invalid_config_json");
    return;
  }
  const path = creating ? "/admin-ui/admin/guardrails" : `/admin-ui/admin/guardrails/${encodeURIComponent(formElement.dataset.guardrailName)}`;
  await api(path, {
    method: creating ? "POST" : "PATCH",
    body: JSON.stringify(body),
  });
  state.editingGuardrailName = null;
  setNotice(`Guardrail ${creating ? "created" : "saved"}.`, "success");
  await guardrails();
}

async function deleteGuardrail(event) {
  const form = event.currentTarget.closest("form");
  const name = form.dataset.guardrailName;
  if (!(await confirmAction(`Delete ${name}`, "The guardrail is removed from key policies. Historical executions remain."))) return;
  await api(`/admin-ui/admin/guardrails/${encodeURIComponent(name)}`, { method: "DELETE" });
  state.editingGuardrailName = null;
  setNotice("Guardrail deleted.", "success");
  await guardrails();
}

function guardrailSummaryTable(rows) {
  return table(
    ["Guardrail", "Mode", "Action", "Failure policy", "Count", "Total latency"],
    rows.map((row) => [
      esc(row.guardrail_name),
      badge(row.mode),
      badge(row.action, row.action === "allow" ? "good" : row.action === "warn" || row.action === "block" ? "warn" : "neutral"),
      badge(row.failure_policy, row.failure_policy === "fail_open" || row.failure_policy === "dry_run" ? "warn" : "neutral"),
      row.count,
      `${esc(row.total_latency_ms)} ms`,
    ]),
  );
}

function guardrailExecutionTable(rows) {
  return table(
    ["Time", "Request", "Key", "Guardrail", "Mode", "Action", "Latency", "Reason"],
    rows.map((row) => [
      time(row.created_at),
      `<code>${esc(row.request_id)}</code>`,
      row.key_id ? `<code>${esc(row.key_id)}</code>` : "",
      esc(row.guardrail_name),
      badge(row.mode),
      badge(row.action, row.action === "allow" ? "good" : row.action === "warn" || row.action === "block" ? "warn" : "neutral"),
      `${esc(row.latency_ms)} ms`,
      esc(row.reason || ""),
    ]),
  );
}

function endpointIdentityFields(access = {}, projectId = "") {
  const entra = access.entra || {};
  const mode = access.authentication_profiles ? "profiles" : access.skip_entra ? "disabled" : access.entra ? "required" : "inherit";
  const savedProfiles = mode === "profiles" && access.authentication_profiles.revision > 0;
  return `<div class="wide-field form-grid" data-endpoint-identity data-key-project="${attr(projectId)}">${savedProfiles ? '<input type="hidden" name="profiles_saved" value="true">' : ""}<label class="wide-field">Route authentication mode<select name="endpoint_entra_mode">
    <option value="profiles" ${mode === "profiles" ? "selected" : ""}>Use authentication profiles</option>
    <option value="inherit" ${savedProfiles ? "disabled" : ""} ${mode === "inherit" ? "selected" : ""}>Use existing gateway setting</option>
    <option value="required" ${savedProfiles ? "disabled" : ""} ${mode === "required" ? "selected" : ""}>Require Entra</option>
    <option value="disabled" ${savedProfiles ? "disabled" : ""} ${mode === "disabled" ? "selected" : ""}>No Entra</option>
  </select></label>
    <div class="wide-field form-grid" data-endpoint-entra ${mode === "required" ? "" : "hidden"}>
    <label>Entra audience (required)<input ${mode === "required" ? "required" : "disabled"} name="endpoint_audience" value="${attr(entra.audience || "")}" placeholder="api://accessa"></label>
    <label>Scopes (optional)<input ${mode === "required" ? "" : "disabled"} name="endpoint_scopes" value="${attr(listValue(entra.required_scopes, ""))}"></label>
    <label>Roles (optional)<input ${mode === "required" ? "" : "disabled"} name="endpoint_roles" value="${attr(listValue(entra.required_roles, ""))}"></label>
    <label>Groups (optional)<input ${mode === "required" ? "" : "disabled"} name="endpoint_groups" value="${attr(listValue(entra.allowed_groups, ""))}"></label>
    <label class="check"><input ${mode === "required" ? "" : "disabled"} name="endpoint_apigee" type="checkbox" ${entra.allow_apigee ? "checked" : ""}> Accept signed Apigee identity</label></div>
    ${profileFields(access)}</div>`;
}

function endpointIdentityBadge(access = {}) {
  return access.authentication_profiles ? `<span class="badge good">${esc(access.authentication_profiles.profiles.length)} authentication profiles</span><div class="subtle">Revision ${esc(access.authentication_profiles.revision)} · explicit key bindings</div>` : access.entra ? `<span class="badge good">Entra required</span><div class="subtle">${esc(access.entra.audience)}</div>`
    : access.skip_entra ? '<span class="badge">No Entra</span>' : '<span class="badge warn">Gateway setting</span>';
}

function routeIdentityControl(route) {
  const setting = (state.routeIdentities || []).find((item) => item.route === route);
  if (!setting) return '<span class="subtle">Unavailable</span>';
  return `<div class="route-identity-control">${endpointIdentityBadge(setting.access)}${routeIdentityButton(route)}</div>`;
}

function routeIdentityButton(route) {
  return `<button type="button" data-route-identity="${attr(route)}" aria-label="Edit route authentication for ${attr(route)}">Edit identity</button>`;
}

function editRouteIdentity(event) {
  const route = event.currentTarget.dataset.routeIdentity;
  const setting = state.routeIdentities.find((item) => item.route === route);
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `<div class="modal wide" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
    <h3 id="${titleId}">Endpoint identity · ${esc(route)}</h3>
    <form class="modal-form"><div class="modal-scroll form-grid">${endpointIdentityFields(setting.access)}${["/litellm/*", "/v1/responses"].includes(route) ? websocketTransportFields(setting.access, true) : ""}</div>
      <div class="form-actions"><button class="primary">Save identity</button><button type="button" data-close-modal>Cancel</button></div>
    </form></div>`;
  document.body.appendChild(backdrop);
  let saving = false;
  prepareProfileKeys(backdrop);
  const close = mountDialog(backdrop, { initialFocus: "select", dismissible: false });
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => { if (!saving) close(); });
  backdrop.addEventListener("keydown", (keyEvent) => { if (keyEvent.key === "Escape" && !saving) close(); });
  backdrop.querySelector("form").addEventListener("submit", handleAsync(async (submit) => {
    submit.preventDefault();
    if (saving) return;
    const access = endpointAccessFromForm(new FormData(submit.currentTarget));
    saving = true;
    try {
      await api("/admin-ui/admin/route-identities", { method: "PUT", body: JSON.stringify({ route, access }) });
      close();
      setNotice(`Endpoint identity saved for ${route}.`, "success");
      await routes();
    } finally { saving = false; }
  }));
}

function endpointAccessFields(access, projectId = "") {
  const binding = access.accessa || {};
  return formSection("Endpoint identity and transport", "Apply identity requirements and choose WebSocket or Accessa transport for this service.", `
    ${endpointIdentityFields(access, projectId)}
    <label class="check"><input name="accessa_enabled" type="checkbox" ${access.accessa ? "checked" : ""}> Accessa channel binding</label>
    <label>Accessa app<input name="accessa_app" value="${attr(binding.app || "")}" placeholder="tara"></label>
    <label>Accessa channel<input name="accessa_channel" value="${attr(binding.channel || "")}" placeholder="tara-frontend"></label>
    <label>Socket idle timeout ms<input name="socket_idle_ms" type="number" min="100" max="600000" value="${attr(binding.idle_timeout_ms ?? 60000)}"></label>
    <label>Connections per route per instance<input name="socket_connections" type="number" min="1" max="10000" value="${attr(binding.max_connections ?? 100)}"></label>
    <label>Connections per key per instance<input name="socket_key_connections" type="number" min="1" max="10000" value="${attr(binding.max_connections_per_key ?? 10)}"></label>
    <label>Maximum socket message bytes<input name="socket_frame_bytes" type="number" min="125" max="16777216" value="${attr(binding.max_frame_bytes ?? 1048576)}"></label>
    <fieldset class="wide-field" style="border:0;margin:0;padding:0;min-width:0" data-user-token-forwarding="${attr(JSON.stringify(access.user_token_forwarding || null))}" ${access.accessa ? "" : "disabled hidden"}></fieldset>
    <div class="help wide-field">With Require Entra, all listed scopes and roles are required; any listed group may match. Apigee must sign audience and expiry claims. Accessa requires Require Entra and an audience; a blank app is only for /channel/{channel}/v1/me. Socket limits apply per gateway instance; the instance ceiling is 4,096. Accessa transport and admission carry no charge; downstream service calls remain metered.</div>
    ${websocketTransportFields(access)}
  `);
}

function websocketTransportFields(access = {}, litellm = false) {
  const socket = access.websocket || {};
  return `<details class="wide-field" ${access.websocket ? "open" : ""}><summary>WebSocket transport</summary><div class="form-grid">
    <label class="check wide-field"><input name="websocket_enabled" type="checkbox" ${access.websocket ? "checked" : ""}> Allow WebSocket alongside HTTP</label>
    <p class="field-hint wide-field">${litellm ? "Caller-owned LiteLLM keys only. Set the route to direct LiteLLM passthrough (or wildcard authentication to litellm_bearer). Path, method and exposure restrictions still apply. LiteLLM owns model policy and spend; Relayna-governed sessions are not supported." : "Requires GET in allowed methods and a WebSocket-capable upstream. Do not combine with Accessa. Relayna checks identity, streaming permission, request rate and budget eligibility when connecting. Socket messages have no HTTP pricing or content inspection; the service owns per-message authorization and charging."}</p>
    <label>WebSocket idle timeout ms<input name="websocket_idle_ms" type="number" min="100" max="600000" value="${attr(socket.idle_timeout_ms ?? 60000)}"></label>
    <label>WebSocket connections per route per instance<input name="websocket_connections" type="number" min="1" max="10000" value="${attr(socket.max_connections ?? 100)}"></label>
    <label>WebSocket connections per key per instance<input name="websocket_key_connections" type="number" min="1" max="10000" value="${attr(socket.max_connections_per_key ?? 10)}"></label>
    <label>WebSocket maximum message bytes<input name="websocket_frame_bytes" type="number" min="125" max="16777216" value="${attr(socket.max_frame_bytes ?? 1048576)}"></label>
    <label>WebSocket session duration seconds<input name="websocket_duration_seconds" type="number" min="1" max="86400" value="${attr(socket.max_session_duration_seconds ?? 3600)}"></label>
    <p class="field-hint wide-field">Limits apply per gateway instance; the shared connection ceiling is 4,096. Frames and fragmented messages share the size limit. Compression is not negotiated. Session expiry is checked on traffic; idle reads and writes use the idle timeout.</p>
  </div></details>`;
}

function endpointAccessFromForm(form) {
  const mode = String(form.get("endpoint_entra_mode") || "inherit");
  if (form.get("profiles_saved") === "true" && mode !== "profiles") throw new Error("This route has saved authentication profiles. Edit the profiles instead; switching identity modes would remove them and is not supported.");
  const audience = mode === "required" ? String(form.get("endpoint_audience") || "").trim() : "";
  const accessa = form.has("accessa_enabled");
  if (accessa && form.has("websocket_enabled")) throw new Error("Choose Accessa or general WebSocket transport, not both.");
  if (accessa && mode !== "profiles" && !audience) throw new Error("Accessa requires an endpoint Entra audience and Require Entra mode.");
  if (mode === "required" && !audience) throw new Error("Require Entra needs an endpoint audience.");
  if (!["inherit", "required", "disabled", "profiles"].includes(mode)) throw new Error("Choose a Route authentication mode.");
  return {
    ...(form.has("websocket_enabled") ? { websocket: {
      idle_timeout_ms: Number(form.get("websocket_idle_ms")), max_connections: Number(form.get("websocket_connections")),
      max_connections_per_key: Number(form.get("websocket_key_connections")), max_frame_bytes: Number(form.get("websocket_frame_bytes")),
      max_session_duration_seconds: Number(form.get("websocket_duration_seconds")),
    } } : {}),
    ...(accessa && (form.has("forward_user_token") || form.get("user_token_configured") === "true" || (form.get("user_token_header") && form.get("user_token_header") !== "x-access-token")) ? {
      user_token_forwarding: { enabled: form.has("forward_user_token"), header: String(form.get("user_token_header") || "x-access-token").trim().toLowerCase() },
    } : {}),
    ...(mode === "profiles" ? { authentication_profiles: profilesFromForm(form, accessa) } : {}),
    skip_entra: mode === "disabled",
    entra: audience ? {
      audience, required_scopes: csv(form.get("endpoint_scopes")), required_roles: csv(form.get("endpoint_roles")),
      allowed_groups: csv(form.get("endpoint_groups")), allow_apigee: form.has("endpoint_apigee"),
    } : null,
    accessa: accessa ? {
      app: nullableString(form.get("accessa_app")), channel: String(form.get("accessa_channel") || "").trim(),
      idle_timeout_ms: Number(form.get("socket_idle_ms")), max_connections: Number(form.get("socket_connections")),
      max_connections_per_key: Number(form.get("socket_key_connections")), max_frame_bytes: Number(form.get("socket_frame_bytes")),
    } : null,
  };
}

function serviceBody(form, patch) {
  const body = {
    ...(patch ? { reprice_existing_usage: form.get("pricing_update_scope") === "historical" } : {}),
    access: endpointAccessFromForm(form),
    project_id: form.has("project_id") ? nullableString(form.get("project_id")) : undefined,
    studio_service_id: patch ? nullableString(form.get("studio_service_id")) : blankToUndefined(form.get("studio_service_id")),
    route_pattern: form.get("route_pattern") || undefined,
    upstream_base_url: patch ? nullableString(form.get("upstream_base_url")) : blankToUndefined(form.get("upstream_base_url")),
    health_check_path: patch ? nullableString(form.get("health_check_path")) : blankToUndefined(form.get("health_check_path")),
    health_check_method: form.get("health_check_method") || "GET",
    enabled: form.has("enabled"),
    allowed_methods: form.getAll("allowed_methods"),
    timeout_ms: Number(form.get("timeout_ms")),
    max_body_bytes: Number(form.get("max_body_bytes")),
    cost_mode: form.get("cost_mode"),
    estimated_cost_usd: nullableNumber(form.get("estimated_cost_usd")),
    pricing_rules: pricingRulesFromForm(form),
    openapi_source_path: patch ? nullableString(form.get("openapi_source_path")) : undefined,
    endpoint_pricing_rules: endpointPricingRulesFromForm(form),
    fallback_services: csv(form.get("fallback_services")),
  };
  if (!patch) {
    body.name = form.get("name");
    body.credential = blankToUndefined(form.get("credential"));
  } else if (form.has("clear_credential")) {
    body.credential = null;
  } else if (form.get("credential")) {
    body.credential = form.get("credential");
  }
  if (patch) body.sync_status = form.get("sync_status");
  return body;
}

function pricingRulesFromForm(form) {
  const value = String(form.get("pricing_rules") || "").trim();
  if (!value) return undefined;
  const parsed = JSON.parse(value);
  if (!Array.isArray(parsed)) throw new Error("pricing_rules must be a JSON array");
  return parsed;
}

function endpointPricingRulesFromForm(form) {
  const value = String(form.get("endpoint_pricing_rules") || "").trim();
  if (!value) return undefined;
  const parsed = JSON.parse(value);
  if (!Array.isArray(parsed)) throw new Error("endpoint_pricing_rules must be a JSON array");
  return parsed;
}

function keyStatus(key) {
  const status = keyLifecycle(key);
  return badge(status, ["active", "non-expiring"].includes(status) ? "good" : "bad");
}

function keyExpiry(key) {
  return key.expires_at ? time(key.expires_at) : "No expiration";
}

function bindKeyExpiryControls() {
  document.querySelectorAll('form input[name="no_expires_at"]').forEach((checkbox) => {
    const form = checkbox.closest("form");
    const expiresAt = form?.querySelector('input[name="expires_at"]');
    const update = () => {
      if (!expiresAt) return;
      expiresAt.disabled = checkbox.checked;
      if (checkbox.checked) expiresAt.value = "";
    };
    checkbox.addEventListener("change", update);
    update();
  });
}

function bindKeyOwnerControls() {
  document.querySelectorAll('form select[name="owner_type"]').forEach((select) => {
    const form = select.closest("form");
    const projectField = form?.querySelector("[data-owner-project]");
    const serviceField = form?.querySelector("[data-owner-services]");
    const update = () => {
      const project = select.value === "project";
      projectField?.classList.toggle("hidden", !project);
      serviceField?.classList.toggle("hidden", project);
      const projectInput = projectField?.querySelector('select[name="project_id"]');
      if (projectInput) projectInput.required = project;
    };
    select.addEventListener("change", update);
    update();
  });
}

function bindServicePickerButtons() {
  document.querySelectorAll("[data-service-picker]").forEach((button) => {
    button.addEventListener("click", () => openServiceSelectionPicker(button));
  });
}

function bindGuardrailPickerButtons() {
  document.querySelectorAll("[data-guardrail-picker]").forEach((button) => {
    button.addEventListener("click", () => openGuardrailSelectionPicker(button));
  });
}

function bindPolicySimulatorControls() {
  const form = document.querySelector("#policy-sim-form");
  if (!form) return;
  const pathInput = form.querySelector("[data-policy-sim-path]");
  const providerSelect = form.querySelector("[data-policy-sim-provider]");
  const modelField = form.querySelector("[data-policy-sim-model]");
  const serviceField = form.querySelector("[data-policy-sim-service]");
  const serviceHelp = form.querySelector("[data-policy-sim-service-help]");
  const serviceSelect = form.querySelector('select[name="service_name"]');
  const update = () => {
    const path = pathInput?.value || "";
    const provider = providerSelect?.value || "";
    const serviceMode = provider === "internal-service" || path.startsWith("/services/");
    modelField?.classList.toggle("muted-field", serviceMode);
    serviceField?.classList.toggle("hidden", !serviceMode);
    serviceHelp?.classList.toggle("hidden", !serviceMode);
    if (!serviceMode && serviceSelect) serviceSelect.value = "";
  };
  pathInput?.addEventListener("input", update);
  providerSelect?.addEventListener("change", update);
  update();
}

function keyPolicySummary(key) {
  const policy = key.policy;
  return `<div>${esc((policy.allowed_routes || []).join(", ") || "no routes")}</div>
    <div class="subtle">${esc((policy.allowed_providers || []).join(", ") || "no providers")}</div>
    <div class="subtle">RPM ${esc(policy.rpm_limit ?? "none")} / daily ${esc(money(policy.daily_budget_usd))}</div>
    <div class="subtle">Req ${esc(policy.max_request_body_bytes ?? "route")} / Resp ${esc(policy.max_response_body_bytes ?? "route")}</div>
    <div class="subtle">Rotate ${esc(key.rotation_due_at ? time(key.rotation_due_at) : "none")} / Last used ${esc(key.last_used_at ? time(key.last_used_at) : "never")}</div>`;
}

function guardrailPolicySummary(policy = {}) {
  const mandatory = policy.mandatory_guardrails || [];
  const optional = policy.optional_guardrails || [];
  const forbidden = policy.forbidden_guardrails || [];
  return `<div>${badge(`${mandatory.length} mandatory`, mandatory.length ? "warn" : "neutral")} ${badge(`${optional.length} optional`)}</div>
    <div class="subtle">${esc(forbidden.length ? `${forbidden.length} forbidden` : "none forbidden")}</div>`;
}

function policySimulationResult() {
  const result = state.policySimulation;
  if (!result) return '<div class="empty-inline">No simulation run.</div>';
  const decision = result.final_decision || {};
  const warnings = result.warnings || [];
  const warningMarkup = warnings.length
    ? `<div class="notice warn wide-field"><strong>Policy warnings</strong><span>${warnings.map((warning) => esc(warning)).join("<br>")}</span></div>`
    : "";
  return `${warningMarkup}<div class="kv">
    <div><strong>Decision</strong><span>${badge(decision.allowed ? "allowed" : decision.error_code || "denied", decision.allowed ? "good" : "bad")}</span></div>
    <div><strong>Matched route</strong><span>${esc(result.route_match?.route || "")}</span></div>
    <div><strong>Provider</strong><span>${esc(result.route_match?.provider || "")}</span></div>
    <div><strong>Service</strong><span>${esc(result.route_match?.service_name || "none")}</span></div>
    <div><strong>Policy version</strong><span>${esc(result.policy_merge?.policy_version ?? "n/a")}</span></div>
    <div><strong>Applied layers</strong><span>${esc((result.policy_merge?.applied_layers || []).map((layer) => `${layer.kind}:${layer.scope_id || "all"}`).join(", ") || "none")}</span></div>
    <div><strong>Guardrails</strong><span>${esc((result.guardrail_plan || []).join(", ") || "none")}</span></div>
    <div><strong>Rate</strong><span>RPM ${esc(result.rate_limit_projection?.rpm_limit ?? "none")} / TPM ${esc(result.rate_limit_projection?.tpm_limit ?? "none")}</span></div>
    <div><strong>Budget</strong><span>${esc(money(result.budget_projection?.daily_budget_usd))} daily</span></div>
  </div>
  <details class="wide-field">
    <summary>Simulation trace</summary>
    ${jsonBlock({
      policy_merge: result.policy_merge,
      route_match: result.route_match,
      rate_limit_projection: result.rate_limit_projection,
      budget_projection: result.budget_projection,
      guardrail_plan: result.guardrail_plan,
      warnings: result.warnings,
      final_decision: result.final_decision,
    })}
  </details>`;
}

function csv(value) {
  return String(value || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function nullableNumber(value) {
  return value === null || value === "" ? null : Number(value);
}

function nullableString(value) {
  return value === null || String(value).trim() === "" ? null : String(value).trim();
}

function projectOptions(selected = "") {
  return state.projects
    .map((project) => `<option value="${attr(project.id)}" ${project.id === selected ? "selected" : ""}>${esc(project.name)} (${esc(project.id)})</option>`)
    .join("");
}

function serviceOptions(selected = "") {
  return state.services
    .map((service) => `<option value="${attr(service.name)}" ${service.name === selected ? "selected" : ""}>${esc(service.name)}</option>`)
    .join("");
}

function keyOptions(selected = "") {
  return state.keys
    .map((key) => `<option value="${attr(key.id)}" ${key.id === selected ? "selected" : ""}>${esc(keyName(key.id))} (${esc(key.owner_type || "project")})</option>`)
    .join("");
}

function serviceCheckboxes(selected = [], name = "service_names") {
  const values = new Set(Array.isArray(selected) ? selected : []);
  if (!state.services.length) return '<div class="empty-inline">No services registered.</div>';
  return `<div class="checkbox-group service-checkboxes" role="group" aria-label="Services">
    ${state.services.map((service) => `<label title="${attr(service.route_pattern)}"><input name="${attr(name)}" type="checkbox" value="${attr(service.name)}" ${values.has(service.name) ? "checked" : ""}> ${esc(service.name)}</label>`).join("")}
  </div>`;
}

function serviceSelectionControl(selected = [], name = "service_names", title = "Select services") {
  const values = Array.isArray(selected) ? selected : [];
  return `<div class="service-selection" data-service-selection data-field-name="${attr(name)}" data-selection-label="services">
    <div class="service-selection-values" data-field-name="${attr(name)}">${serviceHiddenInputs(values, name)}</div>
    <div class="service-selection-summary">${serviceSelectionSummary(values, "services")}</div>
    <button type="button" data-service-picker="${attr(name)}" data-service-picker-title="${attr(title)}">Select services</button>
  </div>`;
}

function guardrailSelectionControl(selected = [], name, title = "Select guardrails") {
  const values = Array.isArray(selected) ? selected : [];
  return `<div class="service-selection guardrail-selection" data-service-selection data-field-name="${attr(name)}" data-selection-label="guardrails">
    <div class="service-selection-values" data-field-name="${attr(name)}">${serviceHiddenInputs(values, name)}</div>
    <div class="service-selection-summary">${serviceSelectionSummary(values, "guardrails")}</div>
    <button type="button" data-guardrail-picker="${attr(name)}" data-guardrail-picker-title="${attr(title)}">Select guardrails</button>
  </div>`;
}

function serviceHiddenInputs(values, name) {
  return values.map((value) => `<input type="hidden" name="${attr(name)}" value="${attr(value)}">`).join("");
}

function selectedServiceNames(form, name) {
  return [...form.querySelectorAll(`input[type="hidden"][name="${CSS.escape(name)}"]`)].map((input) => input.value);
}

function setSelectedServiceNames(form, name, values) {
  const selection = form.querySelector(`[data-service-selection][data-field-name="${CSS.escape(name)}"]`);
  const hidden = selection?.querySelector(`[data-field-name="${CSS.escape(name)}"].service-selection-values`);
  const summary = selection?.querySelector(".service-selection-summary");
  if (!hidden || !summary) return;
  hidden.innerHTML = serviceHiddenInputs(values, name);
  summary.innerHTML = serviceSelectionSummary(values, selection.dataset.selectionLabel || "services");
}

function updateGuardrailOverrideControls(form) {
  const field = form.querySelector("[data-guardrail-overrides]");
  if (!field) return;
  const formData = new FormData(form);
  const overrides = {};
  for (const name of formData.getAll("guardrail_override_names")) {
    try {
      overrides[name] = JSON.parse(formData.get(`guardrail_override_${name}`) || "{}");
    } catch (_) {
      overrides[name] = {};
    }
  }
  field.innerHTML = guardrailOverrideControls(overrides, [
    ...selectedServiceNames(form, "mandatory_guardrails"),
    ...selectedServiceNames(form, "optional_guardrails"),
  ]);
}

function serviceSelectionSummary(values, label = "services") {
  if (!values.length) return `<span class="subtle">No ${esc(label)} selected.</span>`;
  return `<strong>${values.length} selected</strong><div class="service-selection-list">${esc(values.join(", "))}</div>`;
}

function projectName(projectId) {
  if (!projectId) return "Individual";
  return state.projects.find((project) => project.id === projectId)?.name || projectId;
}

function keyName(keyId) {
  const key = state.keys.find((key) => key.id === keyId);
  return key?.name ? `${key.name} (${key.key_prefix})` : key?.key_prefix || keyId;
}

function mappingTargetName(mapping) {
  return mapping.scope === "project" ? projectName(mapping.target_id) : keyName(mapping.target_id);
}

function providerPolicySelect(selected = [], neutral = false) {
  const values = new Set(Array.isArray(selected) && selected.length ? selected : neutral ? [] : ["litellm"]);
  return `<div class="checkbox-group" role="group" aria-label="Providers">
    ${["litellm", "internal-service"].map((value) => `<label><input name="allowed_providers" type="checkbox" value="${attr(value)}" ${values.has(value) ? "checked" : ""}> ${esc(value)}</label>`).join("")}
  </div>`;
}

function serviceRouteOptions() {
  const builtIns = ["/summary", "/translation", "/ocr", "/embeddings", "/services/name/*"];
  const routes = [...new Set([...builtIns, ...state.services.map((service) => service.route_pattern)])];
  return routes.map((route) => `<option value="${attr(route)}"></option>`).join("");
}

async function members() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/members"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/projects"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.members, state.services, state.projects] = loaded;
  const visual = memberAccessSnapshot(state.members);
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    <section class="page-insights" aria-label="Portal approval and access summary">
      <div class="page-insights-grid">
        ${insightStrip(visual.insights, { label: "Gateway-wide portal access configuration" })}
        ${distributionBars("Member approval state", visual.statuses, { caption: "Loaded members · assignments alone do not activate a pending or blocked member", empty: "No portal members yet." })}
      </div>
    </section>
    <section class="panel">
      <div class="panel-heading"><div><h3>Portal members</h3><p>Entra proves identity. Relayna approval and exact service membership grant access.</p></div><span class="badge">${state.members.length} members</span></div>
      <div class="member-grid">${state.members.map(memberAccessCard).join("") || emptyState("No one has signed in through Entra yet.")}</div>
    </section>`;
  content.querySelectorAll("[data-member-status]").forEach((button) => button.addEventListener("click", handleAsync(updateMemberStatus)));
  content.querySelectorAll("[data-member-admin]").forEach((button) => button.addEventListener("click", handleAsync(updateMemberAdmin)));
  content.querySelectorAll("[data-membership-form]").forEach((form) => form.addEventListener("submit", handleAsync(saveServiceMembership)));
  content.querySelectorAll("[data-membership-delete]").forEach((button) => button.addEventListener("click", handleAsync(deleteServiceMembership)));
  content.querySelectorAll("[data-project-membership-form]").forEach((form) => form.addEventListener("submit", handleAsync(saveProjectMembership)));
  content.querySelectorAll("[data-project-membership-delete]").forEach((button) => button.addEventListener("click", handleAsync(deleteProjectMembership)));
  organizeView("members");
}

function memberAccessCard(item) {
  const member = item.member;
  const admin = member.roles?.includes("admin");
  return `<article class="member-card">
    <div class="member-card-heading">
      <div><h4>${esc(member.display_name || member.email || member.object_id)}</h4><p>${esc(member.email || member.object_id)}</p></div>
      ${badge(member.status, member.status === "active" ? "good" : member.status === "blocked" ? "bad" : "warn")}
    </div>
    <div class="member-meta"><span><i class="ti ti-${admin ? "user-shield" : "user"}" aria-hidden="true"></i> ${admin ? "Admin role" : "Member role"}</span><span><i class="ti ti-clock" aria-hidden="true"></i> Last sign-in ${time(member.last_sign_in_at)}</span></div>
    <div class="member-meta"><span><i class="ti ti-hierarchy" aria-hidden="true"></i> ${item.service_memberships?.length || 0} service · ${item.project_memberships?.length || 0} project assignments</span></div>
    <div class="actions">
      <button data-member-status="active" data-member-id="${attr(member.id)}">Approve</button>
      <button data-member-status="blocked" data-member-id="${attr(member.id)}" class="danger">Block</button>
      <button data-member-admin="${admin ? "false" : "true"}" data-member-id="${attr(member.id)}">${admin ? "Remove admin" : "Make admin"}</button>
    </div>
    <details class="catalog-disclosure member-access-details">
    <summary><i class="ti ti-adjustments" aria-hidden="true"></i> Manage resource access <i class="ti ti-chevron-down" aria-hidden="true"></i></summary>
    <h5><i class="ti ti-server" aria-hidden="true"></i> Service access</h5>
    ${item.service_memberships?.length ? table(["Service", "Role", ""], item.service_memberships.map((membership) => [
      esc(membership.service_name),
      badge(membership.role),
      `<button class="danger" data-membership-delete data-member-id="${attr(member.id)}" data-service-name="${attr(membership.service_name)}">Remove</button>`,
    ])) : '<p class="subtle">No service access assigned.</p>'}
    <form data-membership-form data-member-id="${attr(member.id)}" class="inline-form membership-form">
      <label>Service<select name="service_name" required><option value="">Select service</option>${state.services.map((service) => option(service.name, "")).join("")}</select></label>
      <label>Role<select name="role">${option("viewer", "viewer")}${option("owner", "")}</select></label>
      <button type="submit">Assign</button>
    </form>
    <h5><i class="ti ti-folder" aria-hidden="true"></i> Project access</h5>
    ${item.project_memberships?.length ? table(["Project", "Role", ""], item.project_memberships.map((membership) => [
      esc(projectName(membership.project_id)),
      badge(membership.role),
      `<button class="danger" data-project-membership-delete data-member-id="${attr(member.id)}" data-project-id="${attr(membership.project_id)}">Remove</button>`,
    ])) : '<p class="subtle">No project access assigned.</p>'}
    <form data-project-membership-form data-member-id="${attr(member.id)}" class="inline-form membership-form">
      <label>Project<select name="project_id" required><option value="">Select project</option>${state.projects.map((project) => option(project.id, "", project.name)).join("")}</select></label>
      <label>Role<select name="role">${option("viewer", "viewer")}${option("owner", "")}</select></label>
      <button type="submit">Assign</button>
    </form>
    </details>
  </article>`;
}

async function updateMemberStatus(event) {
  await api(`/admin-ui/admin/members/${event.currentTarget.dataset.memberId}`, {
    method: "PATCH",
    body: JSON.stringify({ status: event.currentTarget.dataset.memberStatus }),
  });
  setNotice("Member status updated.", "success");
  await members();
}

async function updateMemberAdmin(event) {
  await api(`/admin-ui/admin/members/${event.currentTarget.dataset.memberId}`, {
    method: "PATCH",
    body: JSON.stringify({ admin: event.currentTarget.dataset.memberAdmin === "true" }),
  });
  setNotice("Member role updated.", "success");
  await members();
}

async function saveServiceMembership(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const serviceName = form.get("service_name");
  await api(`/admin-ui/admin/members/${event.currentTarget.dataset.memberId}/services/${encodeURIComponent(serviceName)}`, {
    method: "PUT",
    body: JSON.stringify({ role: form.get("role") }),
  });
  setNotice("Service access assigned.", "success");
  await members();
}

async function deleteServiceMembership(event) {
  const { memberId, serviceName } = event.currentTarget.dataset;
  if (!(await confirmAction("Remove service access", `This member will immediately lose access to ${serviceName}.`))) return;
  await api(`/admin-ui/admin/members/${memberId}/services/${encodeURIComponent(serviceName)}`, { method: "DELETE" });
  setNotice("Service access removed.", "success");
  await members();
}

async function saveProjectMembership(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const projectId = form.get("project_id");
  await api(`/admin-ui/admin/members/${event.currentTarget.dataset.memberId}/projects/${encodeURIComponent(projectId)}`, {
    method: "PUT",
    body: JSON.stringify({ role: form.get("role") }),
  });
  setNotice("Project access assigned.", "success");
  await members();
}

async function deleteProjectMembership(event) {
  const { memberId, projectId } = event.currentTarget.dataset;
  if (!(await confirmAction("Remove project access", `This member will immediately lose access to ${projectName(projectId)}.`))) return;
  await api(`/admin-ui/admin/members/${memberId}/projects/${encodeURIComponent(projectId)}`, { method: "DELETE" });
  setNotice("Project access removed.", "success");
  await members();
}

async function managedIdentities() {
  const renderId = ++renderGeneration;
  const loaded = await Promise.all([
    api("/admin-ui/admin/managed-identities"),
    api("/admin-ui/admin/managed-identity-projects"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/projects"),
  ]);
  if (renderId !== renderGeneration) return;
  [state.managedIdentities, state.managedIdentityProjects, state.services, state.projects] = loaded;
  const visual = managedIdentitySnapshot(state.managedIdentities, state.managedIdentityProjects);
  if (renderId !== renderGeneration) return;
  content.innerHTML = `
    <section class="page-insights" aria-label="Workload identity binding summary">
      <div class="page-insights-grid">
        ${insightStrip(visual.insights, { label: "Gateway-wide workload access configuration" })}
        ${distributionBars("Exact resource bindings", visual.resources, { caption: "Bindings can share a workload principal; enabled does not bypass token validation", empty: "No workload bindings registered." })}
      </div>
      ${flowSummary("Workload monitoring authorization", [
        { label: "Tenant and audience", icon: "fingerprint", value: "Verified token" },
        { label: "Required application role", icon: "user-shield", value: "Per binding" },
        { label: "Enabled exact binding", icon: "lock", value: "Service or project" },
      ], "All checks must pass. An optional object ID further narrows a binding; one binding never grants access to other resources.")}
    </section>
    <section class="panel">
      <div class="panel-heading"><div><h3>Register managed identity</h3><p>A token must have the monitoring audience, required app role, and this exact service binding.</p></div></div>
      <form id="managed-identity-form" class="form-grid">
        <label>Display name<input name="display_name" required placeholder="orders-api workload"></label>
        <label>Tenant ID<input name="tenant_id" required></label>
        <label>Client ID<input name="client_id" required></label>
        <label>Object ID <span class="subtle">optional exact match</span><input name="object_id"></label>
        <label>Service<select name="service_name" required><option value="">Select service</option>${state.services.map((service) => option(service.name, "")).join("")}</select></label>
        <label>Required app role<input name="required_role" value="gateway.monitor.read" required></label>
        <div class="form-actions wide-field"><button class="primary">Register identity</button></div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Service bindings</h3><span class="badge">${state.managedIdentities.length}</span></div>
      ${managedIdentityTable(state.managedIdentities)}
    </section>
    <section class="panel">
      <div class="panel-heading"><div><h3>Register project monitoring identity</h3><p>The same <code>gateway.monitor.read</code> app role is narrowed to one exact project by Relayna.</p></div></div>
      <form id="managed-identity-project-form" class="form-grid">
        <label>Display name<input name="display_name" required placeholder="payments project monitor"></label>
        <label>Tenant ID<input name="tenant_id" required></label>
        <label>Client ID<input name="client_id" required></label>
        <label>Object ID <span class="subtle">optional exact match</span><input name="object_id"></label>
        <label>Project<select name="project_id" required><option value="">Select project</option>${state.projects.map((project) => option(project.id, "", project.name)).join("")}</select></label>
        <label>Required app role<input name="required_role" value="gateway.monitor.read" required></label>
        <div class="form-actions wide-field"><button class="primary">Register identity</button></div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Project bindings</h3><span class="badge">${state.managedIdentityProjects.length}</span></div>
      ${managedIdentityProjectTable(state.managedIdentityProjects)}
    </section>`;
  document.querySelector("#managed-identity-form").addEventListener("submit", handleAsync(createManagedIdentity));
  content.querySelectorAll("[data-managed-toggle]").forEach((button) => button.addEventListener("click", handleAsync(toggleManagedIdentity)));
  content.querySelectorAll("[data-managed-delete]").forEach((button) => button.addEventListener("click", handleAsync(deleteManagedIdentity)));
  document.querySelector("#managed-identity-project-form").addEventListener("submit", handleAsync(createManagedIdentityProject));
  content.querySelectorAll("[data-managed-project-toggle]").forEach((button) => button.addEventListener("click", handleAsync(toggleManagedIdentityProject)));
  content.querySelectorAll("[data-managed-project-delete]").forEach((button) => button.addEventListener("click", handleAsync(deleteManagedIdentityProject)));
  organizeView("managedIdentities");
}

function managedIdentityProjectTable(rows) {
  if (!rows.length) return emptyState("No project managed identities registered.");
  return table(["Identity", "Tenant / client", "Project", "Role", "Status", "Actions"], rows.map((row) => [
    `<i class="ti ti-fingerprint" aria-hidden="true"></i> <strong>${esc(row.display_name)}</strong>${row.object_id ? `<div class="subtle">Exact object: <code>${esc(row.object_id)}</code></div>` : '<div class="subtle">Tenant and client match</div>'}`,
    `<code>${esc(row.tenant_id)}</code><div class="subtle"><code>${esc(row.client_id)}</code></div>`,
    `<i class="ti ti-folder" aria-hidden="true"></i> ${esc(projectName(row.project_id))}`,
    `<code>${esc(row.required_role)}</code>`,
    badge(row.enabled ? "enabled" : "disabled", row.enabled ? "good" : "bad"),
    `<div class="actions"><button data-managed-project-toggle data-identity-id="${attr(row.id)}" data-enabled="${row.enabled ? "false" : "true"}">${row.enabled ? "Disable" : "Enable"}</button><button class="danger" data-managed-project-delete data-identity-id="${attr(row.id)}">Delete</button></div>`,
  ]));
}

function managedIdentityTable(rows) {
  if (!rows.length) return emptyState("No managed identities registered.");
  return table(["Identity", "Tenant / client", "Service", "Role", "Status", "Actions"], rows.map((row) => [
    `<i class="ti ti-fingerprint" aria-hidden="true"></i> <strong>${esc(row.display_name)}</strong>${row.object_id ? `<div class="subtle">Exact object: <code>${esc(row.object_id)}</code></div>` : '<div class="subtle">Tenant and client match</div>'}`,
    `<code>${esc(row.tenant_id)}</code><div class="subtle"><code>${esc(row.client_id)}</code></div>`,
    `<i class="ti ti-server" aria-hidden="true"></i> ${esc(row.service_name)}`,
    `<code>${esc(row.required_role)}</code>`,
    badge(row.enabled ? "enabled" : "disabled", row.enabled ? "good" : "bad"),
    `<div class="actions"><button data-managed-toggle data-identity-id="${attr(row.id)}" data-enabled="${row.enabled ? "false" : "true"}">${row.enabled ? "Disable" : "Enable"}</button><button class="danger" data-managed-delete data-identity-id="${attr(row.id)}">Delete</button></div>`,
  ]));
}

async function createManagedIdentity(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  await api("/admin-ui/admin/managed-identities", {
    method: "POST",
    body: JSON.stringify({
      display_name: form.get("display_name"),
      tenant_id: form.get("tenant_id"),
      client_id: form.get("client_id"),
      object_id: nullableText(form.get("object_id")),
      service_name: form.get("service_name"),
      required_role: form.get("required_role"),
      enabled: true,
    }),
  });
  setNotice("Managed identity registered.", "success");
  await managedIdentities();
}

async function toggleManagedIdentity(event) {
  await api(`/admin-ui/admin/managed-identities/${event.currentTarget.dataset.identityId}`, {
    method: "PATCH",
    body: JSON.stringify({ enabled: event.currentTarget.dataset.enabled === "true" }),
  });
  await managedIdentities();
}

async function deleteManagedIdentity(event) {
  const identityId = event.currentTarget.dataset.identityId;
  if (!(await confirmAction("Delete managed identity binding", "The workload immediately loses monitoring API access."))) return;
  await api(`/admin-ui/admin/managed-identities/${identityId}`, { method: "DELETE" });
  await managedIdentities();
}

async function createManagedIdentityProject(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  await api("/admin-ui/admin/managed-identity-projects", {
    method: "POST",
    body: JSON.stringify({
      display_name: form.get("display_name"),
      tenant_id: form.get("tenant_id"),
      client_id: form.get("client_id"),
      object_id: nullableText(form.get("object_id")),
      project_id: form.get("project_id"),
      required_role: form.get("required_role"),
      enabled: true,
    }),
  });
  setNotice("Project monitoring identity registered.", "success");
  await managedIdentities();
}

async function toggleManagedIdentityProject(event) {
  await api(`/admin-ui/admin/managed-identity-projects/${event.currentTarget.dataset.identityId}`, {
    method: "PATCH",
    body: JSON.stringify({ enabled: event.currentTarget.dataset.enabled === "true" }),
  });
  await managedIdentities();
}

async function deleteManagedIdentityProject(event) {
  const identityId = event.currentTarget.dataset.identityId;
  if (!(await confirmAction("Delete project identity binding", "The workload immediately loses project monitoring API access."))) return;
  await api(`/admin-ui/admin/managed-identity-projects/${identityId}`, { method: "DELETE" });
  await managedIdentities();
}

async function myServices() {
  const renderId = ++renderGeneration;
  state.ownerServices = await api("/owner/v1/services");
  if (!state.selectedOwnerService && state.ownerServices.length) state.selectedOwnerService = state.ownerServices[0].service.name;
  if (renderId !== renderGeneration) return;
  content.innerHTML = `${ownerCatalogInsights(state.ownerServices, "services")}
    <section class="panel">
      <div class="panel-heading"><div><h3><i class="ti ti-server" aria-hidden="true"></i> My registered services</h3><p>Open a dashboard for an assigned service. Configuration does not indicate live health.</p></div><span class="badge">${state.ownerServices.length} assigned</span></div>
      <div class="owner-service-grid">${state.ownerServices.map(ownerServiceCard).join("") || emptyState("No service access has been assigned to this member.")}</div>
    </section>`;
  content.querySelectorAll("[data-open-owner-service]").forEach((button) => button.addEventListener("click", () => {
    state.selectedOwnerService = button.dataset.openOwnerService;
    navigateToView("service-dashboard");
  }));
  organizeView("myServices");
}

function ownerServiceCard(item) {
  const service = item.service;
  const operations = Array.isArray(service.openapi_endpoints) ? service.openapi_endpoints.length : null;
  const transport = catalogServiceTransport(service);
  const websocket = transport === "unknown" ? "Transport not recorded" : transport === "http" ? "HTTP transport" : "WebSocket configured";
  return `<article class="owner-service-card">
    <div class="member-card-heading"><i class="ti ti-box" aria-hidden="true"></i>${badge(item.role, "neutral")}</div>
    <div><h3>${esc(service.name)}</h3><p><code>${esc(service.route_pattern)}</code></p></div>
    <div class="member-meta">${badge(service.enabled === true ? "Enabled" : service.enabled === false ? "Disabled" : "Status unavailable", service.enabled === true ? "good" : service.enabled === false ? "bad" : "neutral")}<span><i class="ti ti-route" aria-hidden="true"></i> ${esc(service.allowed_methods?.join(", ") || "Methods not recorded")}</span></div>
    <div class="member-meta"><span><i class="ti ti-api" aria-hidden="true"></i> ${operations == null ? "Catalog unavailable" : `${operations} catalog operations`}</span><span><i class="ti ti-plug-connected" aria-hidden="true"></i> ${websocket}</span></div>
    <button class="primary" data-open-owner-service="${attr(service.name)}" aria-label="Open dashboard for ${attr(service.name)}">Open dashboard <i class="ti ti-arrow-up-right" aria-hidden="true"></i></button>
  </article>`;
}

async function myProjects() {
  const renderId = ++renderGeneration;
  state.ownerProjects = await api("/owner/v1/projects");
  if (!state.selectedOwnerProject && state.ownerProjects.length) state.selectedOwnerProject = state.ownerProjects[0].project.id;
  if (renderId !== renderGeneration) return;
  content.innerHTML = `${ownerCatalogInsights(state.ownerProjects, "projects")}
    <section class="panel">
      <div class="panel-heading"><div><h3><i class="ti ti-folders" aria-hidden="true"></i> My projects</h3><p>Monitoring follows each request's recorded project ID, independently of configured service links.</p></div><span class="badge">${state.ownerProjects.length} assigned</span></div>
      <div class="owner-service-grid">${state.ownerProjects.map(ownerProjectCard).join("") || emptyState("No project access has been assigned to this member.")}</div>
    </section>`;
  content.querySelectorAll("[data-open-owner-project]").forEach((button) => button.addEventListener("click", () => {
    state.selectedOwnerProject = button.dataset.openOwnerProject;
    navigateToView("project-dashboard");
  }));
  organizeView("myProjects");
}

function ownerProjectCard(item) {
  const project = item.project;
  const services = Array.isArray(project.service_names) ? project.service_names : null;
  return `<article class="owner-service-card">
    <div class="member-card-heading"><i class="ti ti-folder" aria-hidden="true"></i>${badge(item.role, "neutral")}</div>
    <div><h3>${esc(project.name)}</h3><p><code>${esc(project.id)}</code></p></div>
    <div class="member-meta"><span><i class="ti ti-affiliate" aria-hidden="true"></i> ${services ? `${services.length} configured service links` : "Service links unavailable"}</span></div>
    <div class="member-meta">${services?.length ? services.slice(0, 3).map(name => badge(name, "neutral")).join("") + (services.length > 3 ? `<span>+${services.length - 3} more</span>` : "") : `<span>${services ? "No service links configured" : "Link configuration not recorded"}</span>`}</div>
    <button class="primary" data-open-owner-project="${attr(project.id)}" aria-label="Open dashboard for ${attr(project.name)}">Open dashboard <i class="ti ti-arrow-up-right" aria-hidden="true"></i></button>
  </article>`;
}

function ownerDashboardRange() {
  const hours = state.ownerDashboardFilters.range === "6h" ? 6 : state.ownerDashboardFilters.range === "24h" ? 24 : 7 * 24;
  return {
    from: new Date(Date.now() - hours * 60 * 60 * 1000).toISOString(),
    interval: hours <= 24 ? "hour" : "day",
  };
}

function ownerIncidentSummary(rows, markers) {
  return ownerIncidentDescription(rows, markers);
}

function ownerChartLabel(row) {
  const hourly = ["6h", "24h"].includes(state.ownerDashboardFilters.range);
  return new Date(row.bucket_start || row.bucket || row.name).toLocaleString([], { month: "short", day: "numeric", ...(hourly ? { hour: "2-digit" } : {}) });
}

function renderOwnerIncidentChart(rows, markers) {
  const canvas = document.querySelector("#owner-incident-chart");
  if (!(canvas instanceof HTMLCanvasElement)) return;
  destroyOwnerDashboardChart();
  const { points, observations } = ownerIncidentSeries(rows, markers, ["6h", "24h"].includes(state.ownerDashboardFilters.range) ? "hour" : "day");
  if (!points.length) return;
  const markerPlugin = {
    id: "service-version-markers",
    afterDatasetsDraw(chart) {
      if (!chart.isDatasetVisible(2)) return;
      const { ctx, chartArea, scales } = chart;
      observations.forEach((observation) => {
        const x = scales.x.getPixelForValue(observation.at);
        if (x < chartArea.left || x > chartArea.right) return;
        ctx.save();
        ctx.strokeStyle = chartColor("#6b7280", "text")({ chart });
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.moveTo(x, chartArea.top);
        ctx.lineTo(x, chartArea.bottom);
        ctx.stroke();
        ctx.restore();
      });
    },
  };
  ownerDashboardChart = new Chart(canvas, {
    type: "line",
    plugins: [markerPlugin],
    data: {
      datasets: [
        {
          label: "Error rate (%)",
          data: points.map(point => ({ x: point.at, y: point.errorRate })),
          borderColor: chartColor("#c54c59", "red"),
          backgroundColor: chartColor("#c54c59", "red"),
          yAxisID: "errorRate",
          tension: 0.2,
          spanGaps: false,
          pointRadius: 2,
          borderWidth: 2,
        },
        {
          label: "P95 latency (ms)",
          data: points.map(point => ({ x: point.at, y: point.p95 })),
          borderColor: chartColor("#177264", "teal"),
          backgroundColor: chartColor("#177264", "teal"),
          yAxisID: "latency",
          tension: 0.2,
          spanGaps: false,
          pointRadius: 2,
          borderWidth: 2,
        },
        {
          label: "Service version",
          data: observations.map(observation => ({ x: observation.at, y: 1 })),
          borderColor: chartColor("#6b7280", "text"),
          backgroundColor: chartColor("#6b7280", "text"),
          yAxisID: "version",
          showLine: false,
          pointStyle: "triangle",
          pointRadius: 5,
          observations,
        },
      ],
    },
    options: {
      animation: false,
      maintainAspectRatio: false,
      responsive: true,
      interaction: { mode: "nearest", axis: "x", intersect: false },
      plugins: {
        legend: { position: "bottom", align: "start", labels: { boxWidth: 10, usePointStyle: true, color: chartColor("#63717a", "text"), font: { size: 11, weight: 600 } } },
        tooltip: {
          backgroundColor: chartColor("#062f36", "tooltip-bg"),
          titleColor: chartColor("#fff", "tooltip-text"),
          bodyColor: chartColor("#fff", "tooltip-text"),
          footerColor: chartColor("#fff", "tooltip-text"),
          callbacks: {
            title(items) { return items.length ? new Date(items[0].parsed.x).toLocaleString() : ""; },
            label(context) {
              if (context.dataset.label === "Service version") {
                const version = context.dataset.observations?.[context.dataIndex]?.version;
                return `Version ${version} first observed by Gateway`;
              }
              return `${context.dataset.label}: ${Number(context.parsed.y).toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
            },
          },
        },
      },
      scales: {
        x: { type: "linear", grid: { display: false }, border: { color: chartColor("rgba(0,0,0,0.1)", "grid") }, ticks: { color: chartColor("#63717a", "text"), maxTicksLimit: 8, callback: value => ownerChartLabel({ bucket: Number(value) }), font: { size: 10 } } },
        errorRate: { beginAtZero: true, suggestedMax: 100, grid: { color: chartColor("#edf0f2", "grid") }, border: { color: chartColor("rgba(0,0,0,0.1)", "grid") }, ticks: { color: chartColor("#8a3439", "red"), callback: (value) => `${value}%`, font: { size: 10 } } },
        latency: { beginAtZero: true, position: "right", grid: { drawOnChartArea: false, color: chartColor("rgba(0,0,0,0.1)", "grid") }, border: { color: chartColor("rgba(0,0,0,0.1)", "grid") }, ticks: { color: chartColor("#177264", "teal"), callback: (value) => `${value} ms`, font: { size: 10 } } },
        version: { display: false, min: 0, max: 1.08 },
      },
    },
  } as any);
}

function ownerRequestDetailsRows(request) {
  return [
    ["Request", `<code>${esc(request.request_id)}</code>`],
    ["Created", time(request.created_at)],
    ["Outcome", badge(request.status, request.status === "success" ? "good" : "bad")],
    ["HTTP status", `<code>${esc(request.status_code)}</code>`],
    ["Route", `<code>${esc(request.route)}</code>`],
    ["Endpoint", `<code>${esc(request.endpoint_template || request.endpoint_path || "")}</code>`],
    ["Provider", esc(request.provider)],
    ["Service version", esc(request.service_version || "Not observed")],
    ["Latency", `${esc(request.latency_ms)} ms`],
    ["Tokens", esc(request.total_tokens)],
    ["Estimated cost", money(request.estimated_cost_usd)],
    ["Trace", request.trace_id ? `<code>${esc(request.trace_id)}</code>` : "Not recorded"],
  ];
}

async function openOwnerRequestDetails(event) {
  const trigger = event.currentTarget;
  const serviceName = trigger.dataset.ownerService;
  const requestId = trigger.dataset.ownerRequest;
  const projectId = trigger.dataset.ownerProject;
  const details = await api(projectId
    ? `/owner/v1/projects/${encodeURIComponent(projectId)}/requests/${encodeURIComponent(requestId)}`
    : `/owner/v1/services/${encodeURIComponent(serviceName)}/requests/${encodeURIComponent(requestId)}`);
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop owner-request-backdrop";
  const titleId = `dialog-title-${++dialogCounter}`;
  backdrop.innerHTML = `
    <div class="modal owner-request-drawer" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <div class="owner-request-heading">
        <div><span class="eyebrow">Sanitized request details</span><h3 id="${titleId}">${esc(requestId)}</h3></div>
        <button type="button" data-close-owner-request aria-label="Close request details">Close</button>
      </div>
      <div class="modal-scroll owner-request-content">
        ${table(["Field", "Value"], ownerRequestDetailsRows(details.request))}
        <section class="owner-debug-section">
          <h4>Debug bundle</h4>
          ${details.debug_bundle ? jsonBlock(details.debug_bundle) : emptyState("Usage metadata is available, but no proxy debug bundle was captured for this request.")}
        </section>
      </div>
    </div>`;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, {
    initialFocus: "[data-close-owner-request]",
    restoreFocus: trigger,
  });
  backdrop.querySelector("[data-close-owner-request]").addEventListener("click", () => close(false));
}

async function serviceDashboard() {
  const generation = ++ownerDashboardGeneration;
  const isStale = () => generation !== ownerDashboardGeneration;
  destroyOwnerDashboardChart();
  if (!state.ownerServices.length) {
    try {
      const ownerServices = await api("/owner/v1/services");
      if (isStale()) return;
      state.ownerServices = ownerServices;
    } catch (error) {
      if (isStale()) return;
      throw error;
    }
  }
  if (isStale()) return;
  if (!state.selectedOwnerService) state.selectedOwnerService = state.ownerServices[0]?.service?.name;
  if (!state.selectedOwnerService) {
    content.innerHTML = panel("Service dashboard", emptyState("No service access has been assigned."));
    return;
  }
  const serviceName = state.selectedOwnerService;
  const range = ownerDashboardRange();
  const dashboardQuery = new URLSearchParams({ from: range.from, interval: range.interval });
  const eventsQuery = new URLSearchParams({
    from: range.from,
    limit: String(state.ownerDashboardFilters.limit),
    offset: String(state.ownerDashboardFilters.offset),
  });
  if (state.ownerDashboardFilters.outcome !== "all") eventsQuery.set("status", state.ownerDashboardFilters.outcome);
  if (state.ownerDashboardFilters.statusCode) eventsQuery.set("status_code", state.ownerDashboardFilters.statusCode);
  let dashboard;
  let events;
  try {
    [dashboard, events] = await Promise.all([
      api(`/owner/v1/services/${encodeURIComponent(serviceName)}/dashboard?${dashboardQuery}`),
      api(`/owner/v1/services/${encodeURIComponent(serviceName)}/events?${eventsQuery}`),
    ]);
  } catch (error) {
    if (isStale()) return;
    throw error;
  }
  if (isStale()) return;
  const statusCodes = (dashboard.status_codes || []).map(String);
  if (state.ownerDashboardFilters.statusCode && !statusCodes.includes(state.ownerDashboardFilters.statusCode)) {
    statusCodes.push(state.ownerDashboardFilters.statusCode);
  }
  const eventStart = events.rows?.length ? events.offset + 1 : 0;
  const eventEnd = events.rows?.length ? events.offset + events.rows.length : 0;
  content.innerHTML = `
    ${ownerDashboardInsights(dashboard, { money, range: state.ownerDashboardFilters.range })}
    <section class="owner-dashboard-toolbar">
      <label>Service<select id="owner-service-select">${state.ownerServices.map((item) => option(item.service.name, serviceName)).join("")}</select></label>
      <span class="badge">${esc(dashboard.role)}</span>
      <span class="subtle"><i class="ti ti-lock" aria-hidden="true"></i> Exact service scope · read only</span>
    </section>
    <section class="panel owner-incident-panel">
      <div class="panel-heading"><div><h3><i class="ti ti-chart-line" aria-hidden="true"></i> Incident signals</h3><p>Recorded error rate and P95 latency · selected window, all outcomes.</p></div></div>
      <p class="help" id="owner-incident-summary">${esc(ownerIncidentSummary(dashboard.timeseries || [], dashboard.version_markers || []))}</p>
      ${dashboard.timeseries?.length ? `<div class="owner-incident-chart-wrap"><canvas id="owner-incident-chart" role="img" aria-label="Service error rate and P95 latency over time with version transition markers" aria-describedby="owner-incident-summary"></canvas></div>` : emptyState("No recorded request buckets in this window.")}
      ${dashboard.version_markers?.length ? `<details class="monitor-details"><summary><i class="ti ti-flag" aria-hidden="true"></i> Version observations (${dashboard.version_markers.length})</summary>${table(["Version", "First Gateway observation"], dashboard.version_markers.map(marker => [esc(marker.service_version), time(marker.first_observed_at)]))}</details>` : ""}
    </section>
    <section class="owner-dashboard-grid">
      ${panel("", `${ownerUsageDistribution("Endpoint usage", dashboard.endpoints || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Endpoint usage table</summary>${usageBreakdownTable(dashboard.endpoints || [])}</details>`)}
      ${panel("", `${ownerUsageDistribution("Provider usage", dashboard.providers || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Provider usage table</summary>${usageBreakdownTable(dashboard.providers || [])}</details>`)}
      ${panel("", `${ownerUsageDistribution("Model usage", dashboard.models || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Model usage table</summary>${usageBreakdownTable(dashboard.models || [])}</details>`)}
    </section>
    <section class="panel owner-request-panel">
      <div class="panel-heading"><div><h3><i class="ti ti-list-search" aria-hidden="true"></i> Request logs</h3><p>Sanitized outcomes for this service. Outcome and status filters apply to this log only.</p></div></div>
      <div class="owner-request-filters" role="group" aria-label="Request log filters">
        <label>Time range<select id="owner-range-select">
          ${option("6h", state.ownerDashboardFilters.range, "Last 6 hours")}
          ${option("24h", state.ownerDashboardFilters.range, "Last 24 hours")}
          ${option("7d", state.ownerDashboardFilters.range, "Last 7 days")}
        </select></label>
        <label>Outcome<select id="owner-outcome-select">
          ${option("all", state.ownerDashboardFilters.outcome, "All outcomes")}
          ${option("success", state.ownerDashboardFilters.outcome, "Success")}
          ${option("failure", state.ownerDashboardFilters.outcome, "Failure")}
        </select></label>
        <label>Status code<select id="owner-status-select">
          ${option("", state.ownerDashboardFilters.statusCode, "All status codes")}
          ${statusCodes.map((code) => option(code, state.ownerDashboardFilters.statusCode, code)).join("")}
        </select></label>
      </div>
      <details class="monitor-details"><summary><i class="ti ti-chart-bar" aria-hidden="true"></i> Protocols in this request page</summary>${ownerRequestProfile(events.rows || [], events.has_more)}</details>
      <div class="table-pager owner-request-pager" aria-label="Request logs pagination">
        <span>Showing ${eventStart}-${eventEnd}</span>
        <button type="button" data-owner-page="previous" ${events.offset <= 0 ? "disabled" : ""}>Previous</button>
        <button type="button" data-owner-page="next" ${events.has_more ? "" : "disabled"}>Next</button>
      </div>
      ${usageEventsTable(events.rows || [], { ownerService: serviceName })}
    </section>
    ${panel("Usage over time", usageTimeseriesTable(dashboard.timeseries || []))}`;
  renderOwnerIncidentChart(dashboard.timeseries || [], dashboard.version_markers || []);
  document.querySelector("#owner-service-select").addEventListener("change", (event) => {
    state.selectedOwnerService = event.target.value;
    state.ownerDashboardFilters.offset = 0;
    serviceDashboard().catch((error) => setNotice(error.message));
  });
  for (const id of ["owner-range-select", "owner-outcome-select", "owner-status-select"]) {
    document.querySelector(`#${id}`).addEventListener("change", (event) => {
      if (id === "owner-range-select") state.ownerDashboardFilters.range = event.target.value;
      if (id === "owner-outcome-select") state.ownerDashboardFilters.outcome = event.target.value;
      if (id === "owner-status-select") state.ownerDashboardFilters.statusCode = event.target.value;
      state.ownerDashboardFilters.offset = 0;
      serviceDashboard().catch((error) => setNotice(error.message));
    });
  }
  content.querySelectorAll("[data-owner-page]").forEach((button) => button.addEventListener("click", () => {
    const delta = button.dataset.ownerPage === "next" ? state.ownerDashboardFilters.limit : -state.ownerDashboardFilters.limit;
    state.ownerDashboardFilters.offset = Math.max(0, state.ownerDashboardFilters.offset + delta);
    serviceDashboard().catch((error) => setNotice(error.message));
  }));
  content.querySelectorAll("[data-owner-request]").forEach((button) => button.addEventListener("click", handleAsync(openOwnerRequestDetails)));
  organizeView("serviceDashboard");
}

async function projectDashboard() {
  const generation = ++ownerDashboardGeneration;
  const isStale = () => generation !== ownerDashboardGeneration;
  destroyOwnerDashboardChart();
  if (!state.ownerProjects.length) {
    try {
      const ownerProjects = await api("/owner/v1/projects");
      if (isStale()) return;
      state.ownerProjects = ownerProjects;
    } catch (error) {
      if (isStale()) return;
      throw error;
    }
  }
  if (isStale()) return;
  if (!state.selectedOwnerProject) state.selectedOwnerProject = state.ownerProjects[0]?.project?.id;
  if (!state.selectedOwnerProject) {
    content.innerHTML = panel("Project dashboard", emptyState("No project access has been assigned."));
    return;
  }
  const projectId = state.selectedOwnerProject;
  const range = ownerDashboardRange();
  const dashboardQuery = new URLSearchParams({ from: range.from, interval: range.interval });
  const eventsQuery = new URLSearchParams({
    from: range.from,
    limit: String(state.ownerDashboardFilters.limit),
    offset: String(state.ownerDashboardFilters.offset),
  });
  if (state.ownerDashboardFilters.outcome !== "all") eventsQuery.set("status", state.ownerDashboardFilters.outcome);
  if (state.ownerDashboardFilters.statusCode) eventsQuery.set("status_code", state.ownerDashboardFilters.statusCode);
  let dashboard;
  let events;
  try {
    [dashboard, events] = await Promise.all([
      api(`/owner/v1/projects/${encodeURIComponent(projectId)}/dashboard?${dashboardQuery}`),
      api(`/owner/v1/projects/${encodeURIComponent(projectId)}/events?${eventsQuery}`),
    ]);
  } catch (error) {
    if (isStale()) return;
    throw error;
  }
  if (isStale()) return;
  const statusCodes = (dashboard.status_codes || []).map(String);
  if (state.ownerDashboardFilters.statusCode && !statusCodes.includes(state.ownerDashboardFilters.statusCode)) {
    statusCodes.push(state.ownerDashboardFilters.statusCode);
  }
  const eventStart = events.rows?.length ? events.offset + 1 : 0;
  const eventEnd = events.rows?.length ? events.offset + events.rows.length : 0;
  content.innerHTML = `
    ${ownerDashboardInsights(dashboard, { money, range: state.ownerDashboardFilters.range })}
    <section class="owner-dashboard-toolbar">
      <label>Project<select id="owner-project-select">${state.ownerProjects.map((item) => option(item.project.id, projectId, item.project.name)).join("")}</select></label>
      <span class="badge">${esc(dashboard.role)}</span>
      <span class="subtle"><i class="ti ti-lock" aria-hidden="true"></i> Exact project scope · read only</span>
    </section>
    <section class="panel owner-incident-panel">
      <div class="panel-heading"><div><h3><i class="ti ti-chart-line" aria-hidden="true"></i> Project incident signals</h3><p>Recorded project attribution · selected window, all outcomes.</p></div></div>
      <p class="help" id="owner-incident-summary">${esc(ownerIncidentSummary(dashboard.timeseries || [], dashboard.version_markers || []))}</p>
      ${dashboard.timeseries?.length ? `<div class="owner-incident-chart-wrap"><canvas id="owner-incident-chart" role="img" aria-label="Project error rate and P95 latency over time with version observations" aria-describedby="owner-incident-summary"></canvas></div>` : emptyState("No recorded request buckets in this window.")}
      ${dashboard.version_markers?.length ? `<details class="monitor-details"><summary><i class="ti ti-flag" aria-hidden="true"></i> Version observations (${dashboard.version_markers.length})</summary>${table(["Version", "First Gateway observation"], dashboard.version_markers.map(marker => [esc(marker.service_version), time(marker.first_observed_at)]))}</details>` : ""}
    </section>
    <section class="owner-dashboard-grid">
      ${panel("", `${ownerUsageDistribution("Service usage", dashboard.services || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Service usage table</summary>${usageBreakdownTable(dashboard.services || [])}</details>`)}
      ${panel("", `${ownerUsageDistribution("Endpoint usage", dashboard.endpoints || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Endpoint usage table</summary>${usageBreakdownTable(dashboard.endpoints || [])}</details>`)}
      ${panel("", `${ownerUsageDistribution("Provider usage", dashboard.providers || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Provider usage table</summary>${usageBreakdownTable(dashboard.providers || [])}</details>`)}
      ${panel("", `${ownerUsageDistribution("Model usage", dashboard.models || [])}<details class="monitor-details"><summary><i class="ti ti-table" aria-hidden="true"></i> Model usage table</summary>${usageBreakdownTable(dashboard.models || [])}</details>`)}
    </section>
    <section class="panel owner-request-panel">
      <div class="panel-heading"><div><h3><i class="ti ti-list-search" aria-hidden="true"></i> Project request logs</h3><p>Sanitized outcomes attributed to this project. Outcome and status filters apply to this log only.</p></div></div>
      <div class="owner-request-filters" role="group" aria-label="Project request log filters">
        <label>Time range<select id="owner-range-select">
          ${option("6h", state.ownerDashboardFilters.range, "Last 6 hours")}
          ${option("24h", state.ownerDashboardFilters.range, "Last 24 hours")}
          ${option("7d", state.ownerDashboardFilters.range, "Last 7 days")}
        </select></label>
        <label>Outcome<select id="owner-outcome-select">
          ${option("all", state.ownerDashboardFilters.outcome, "All outcomes")}
          ${option("success", state.ownerDashboardFilters.outcome, "Success")}
          ${option("failure", state.ownerDashboardFilters.outcome, "Failure")}
        </select></label>
        <label>Status code<select id="owner-status-select">
          ${option("", state.ownerDashboardFilters.statusCode, "All status codes")}
          ${statusCodes.map((code) => option(code, state.ownerDashboardFilters.statusCode, code)).join("")}
        </select></label>
      </div>
      <details class="monitor-details"><summary><i class="ti ti-chart-bar" aria-hidden="true"></i> Protocols in this request page</summary>${ownerRequestProfile(events.rows || [], events.has_more)}</details>
      <div class="table-pager owner-request-pager" aria-label="Project request logs pagination">
        <span>Showing ${eventStart}-${eventEnd}</span>
        <button type="button" data-owner-page="previous" ${events.offset <= 0 ? "disabled" : ""}>Previous</button>
        <button type="button" data-owner-page="next" ${events.has_more ? "" : "disabled"}>Next</button>
      </div>
      ${usageEventsTable(events.rows || [], { ownerProject: projectId })}
    </section>
    ${panel("Usage over time", usageTimeseriesTable(dashboard.timeseries || []))}`;
  renderOwnerIncidentChart(dashboard.timeseries || [], dashboard.version_markers || []);
  document.querySelector("#owner-project-select").addEventListener("change", (event) => {
    state.selectedOwnerProject = event.target.value;
    state.ownerDashboardFilters.offset = 0;
    projectDashboard().catch((error) => setNotice(error.message));
  });
  for (const id of ["owner-range-select", "owner-outcome-select", "owner-status-select"]) {
    document.querySelector(`#${id}`).addEventListener("change", (event) => {
      if (id === "owner-range-select") state.ownerDashboardFilters.range = event.target.value;
      if (id === "owner-outcome-select") state.ownerDashboardFilters.outcome = event.target.value;
      if (id === "owner-status-select") state.ownerDashboardFilters.statusCode = event.target.value;
      state.ownerDashboardFilters.offset = 0;
      projectDashboard().catch((error) => setNotice(error.message));
    });
  }
  content.querySelectorAll("[data-owner-page]").forEach((button) => button.addEventListener("click", () => {
    const delta = button.dataset.ownerPage === "next" ? state.ownerDashboardFilters.limit : -state.ownerDashboardFilters.limit;
    state.ownerDashboardFilters.offset = Math.max(0, state.ownerDashboardFilters.offset + delta);
    projectDashboard().catch((error) => setNotice(error.message));
  }));
  content.querySelectorAll("[data-owner-request]").forEach((button) => button.addEventListener("click", handleAsync(openOwnerRequestDetails)));
  organizeView("projectDashboard");
}

function blankToUndefined(value) {
  return value === null || String(value).trim() === "" ? undefined : String(value).trim();
}

function nullableText(value) {
  return value === null || String(value).trim() === "" ? null : String(value).trim();
}

function numberOrDefault(value, fallback) {
  const text = String(value || "").trim();
  return text === "" ? fallback : Number(text);
}

function isoDate(value) {
  return value ? new Date(value).toISOString() : null;
}

function toLocalInput(value) {
  if (!value) return "";
  const date = new Date(value);
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

function listValue(values, fallback) {
  return Array.isArray(values) && values.length ? values.join(",") : fallback;
}

function methodSelect(selected = []) {
  const selectedMethods = new Set(Array.isArray(selected) && selected.length ? selected : ["POST"]);
  return `<div class="checkbox-group" role="group" aria-label="Methods">
    ${["GET", "POST", "PUT", "PATCH", "DELETE"].map((value) => methodOption(value, selectedMethods)).join("")}
  </div>`;
}

function methodOption(value, selectedMethods) {
  return `<label><input name="allowed_methods" type="checkbox" value="${attr(value)}" ${selectedMethods.has(value) ? "checked" : ""}> ${esc(value)}</label>`;
}

function option(value, selected, label = value) {
  return `<option value="${attr(value)}" ${value === selected ? "selected" : ""}>${esc(label)}</option>`;
}

function time(value) {
  return value ? new Date(value).toLocaleString() : "n/a";
}

function money(value) {
  return value == null ? "n/a" : new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: Number(value) > 0 && Number(value) < 0.01 ? 20 : 2 }).format(Number(value));
}

function percent(value) {
  return value == null ? "0.0%" : `${(Number(value) * 100).toFixed(1)}%`;
}

function esc(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function attr(value) {
  return esc(value);
}

function hasDirtyForms() {
  return Boolean(document.querySelector('form[data-dirty="true"]'));
}
function clearDirtyForms() {
  document.querySelectorAll('form[data-dirty]').forEach((form) => { delete form.dataset.dirty; });
}
document.addEventListener("input", (event) => {
  const form = event.target.closest?.("form");
  if (!form || ["login-form", "usage-form", "usage-export-form", "traffic-filters", "task-usage-form", "debug-bundle-form", "audit-form", "policy-sim-form"].includes(form.id)) return;
  form.dataset.dirty = "true";
});
window.addEventListener("beforeunload", (event) => {
  if (hasDirtyForms()) { event.preventDefault(); event.returnValue = ""; }
});

let closeContentDrawer = null;
const scopedViews = new Set(["overview", "usage", "traffic", "keys", "projects"]);

function scopeLabel() {
  if (!state.projectScope) return "All projects";
  return state.projects.find((project) => project.id === state.projectScope)?.name || `Project ${state.projectScope}`;
}

function synchronizeProjectScope(projectScope) {
  const changedProject = state.projectScope !== projectScope;
  state.projectScope = projectScope;
  for (const field of ["usageFilters", "trafficFilters"]) {
    state[field] = { ...state[field], project_id: projectScope, ...(changedProject ? { key_id: "" } : {}) };
  }
}

function applyTrafficFilters(filters) {
  const projectScope = filters.project_id || "";
  const changedProject = state.projectScope !== projectScope;
  if (changedProject && filters.key_id === state.trafficFilters.key_id) filters.key_id = "";
  synchronizeProjectScope(projectScope);
  state.trafficFilters = filters;
  if (changedProject) resetUsagePagination();
  syncProjectScope();
  persistMonitoringHash();
}

function monitoringHash() {
  const query = new URLSearchParams();
  if (state.projectScope) query.set("project", state.projectScope);
  if (state.overviewWindow !== "7d") query.set("range", state.overviewWindow);
  for (const [name, value] of Object.entries(state.usageFilters)) {
    if (value && name !== "project_id") query.set(`usage.${name}`, String(value));
  }
  return query.size ? `?${query}` : "";
}

function persistMonitoringHash() {
  history.replaceState(null, "", `#/${state.view}${monitoringHash()}`);
}

function restoreMonitoringHash() {
  const query = new URLSearchParams(location.hash.split("?")[1] || "");
  synchronizeProjectScope(query.get("project") || "");
  state.overviewWindow = ["24h", "7d", "30d"].includes(query.get("range")) ? query.get("range") : "7d";
  state.usageFilters = Object.fromEntries([...query].filter(([key]) => key.startsWith("usage.")).map(([key, value]) => [key.slice(6), value]));
  resetUsagePagination();
}

function syncProjectScope() {
  const control = document.querySelector("#project-scope");
  const scoped = scopedViews.has(state.view) && state.workspace === "admin";
  control.closest("label").classList.toggle("hidden", !scoped);
  document.querySelector("#scope-context").textContent = state.workspace === "owner" ? "Membership scope" : scoped ? "Workspace" : "Gateway-wide";
  const projects = state.projects;
  // An unknown URL scope stays exact; never silently turn it into All projects.
  control.innerHTML = `<option value="">All projects</option>${projects.map((project) => option(project.id, state.projectScope, project.name)).join("")}${state.projectScope && !projects.some((project) => project.id === state.projectScope) ? `<option value="${attr(state.projectScope)}" selected>Unknown project · ${esc(state.projectScope)}</option>` : ""}`;
  control.value = state.projectScope;
}

function showContentDrawer(title, markupOrNode, onClose = () => {}, restoreFocus = null) {
  closeContentDrawer?.();
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop drawer-backdrop";
  const id = `drawer-title-${++dialogCounter}`;
  backdrop.innerHTML = `<div class="modal resource-drawer" role="dialog" aria-modal="true" aria-labelledby="${id}"><div class="drawer-heading"><h3 id="${id}">${esc(title)}</h3><button type="button" data-drawer-close aria-label="Close ${attr(title)}">Close</button></div><div class="drawer-body"></div></div>`;
  const body = backdrop.querySelector(".drawer-body");
  if (markupOrNode instanceof Node) body.appendChild(markupOrNode);
  else body.innerHTML = markupOrNode;
  document.body.appendChild(backdrop);
  const close = mountDialog(backdrop, { restoreFocus, onClose: () => { closeContentDrawer = null; onClose(); } });
  closeContentDrawer = close;
  backdrop.querySelector("[data-drawer-close]").addEventListener("click", () => close());
  return close;
}

function organizeView(view) {
  closeContentDrawer?.();
  const actions = document.querySelector("#page-actions");
  actions.replaceChildren();
  syncProjectScope();
  // Move existing, bound forms rather than cloning them: validation, handlers,
  // write-only credentials, imports and pricing editors remain functional.
  if (view === "routes") content.querySelectorAll('form.route-config-form').forEach(form => mountRouteConfiguration(form, showContentDrawer));
  const createForms = { projects: "project-form", keys: "key-form", services: "service-form", managedIdentities: "managed-identity-form" };
  const id = createForms[view];
  const create = id && content.querySelector(`#${id}`)?.closest(".panel");
  if (create) {
    const title = create.querySelector("h3")?.textContent || "Create";
    const placeholder = document.createElement("div");
    placeholder.hidden = true;
    create.before(placeholder);
    placeholder.appendChild(create);
    const project = create.querySelector("select[name=project_id]");
    if (project && state.projectScope && [...project.options].some((option) => option.value === state.projectScope)) project.value = state.projectScope;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "primary";
    button.textContent = title;
    actions.appendChild(button);
    const foundryAction = create.querySelector("[data-foundry-service]");
    if (foundryAction) actions.appendChild(foundryAction);
    button.addEventListener("click", () => showContentDrawer(title, create, () => placeholder.appendChild(create)));
  }
  content.querySelectorAll(".muted-panel").forEach((panel) => { panel.hidden = true; });
  content.querySelectorAll(".split, .service-stack").forEach((wrapper) => {
    const visible = [...wrapper.children].filter((child) => !child.hidden);
    if (!visible.length) wrapper.hidden = true;
    else wrapper.classList.add("contextual-stack");
  });
  const inventoryTitles = { projects: "Projects", keys: "Virtual keys", services: "Registered services", providers: "Provider configuration", managedIdentities: "Service bindings" };
  const inventory = inventoryTitles[view] && [...content.querySelectorAll(".panel")].find((panel) => panel.querySelector("h3")?.textContent === inventoryTitles[view]);
  if (inventory) {
    content.prepend(inventory);
    addInventorySearch(inventory);
  }
  if (view === "keys" || view === "projects") {
    const rows = view === "keys" ? state.keys : state.projects;
    const scoped = state.projectScope ? rows.filter((row) => (view === "keys" ? row.project_id : row.id) === state.projectScope) : rows;
    const tableRows = [...inventory.querySelectorAll("tbody > tr")];
    tableRows.forEach((row, index) => { row.dataset.outOfScope = String(!scoped.includes(rows[index])); row.hidden = row.dataset.outOfScope === "true"; });
    const count = inventory.querySelector(".panel-heading .subtle");
    if (count) count.textContent = `${scoped.length} in ${scopeLabel()}`;
    if (!scoped.length && state.projectScope) inventory.insertAdjacentHTML("beforeend", emptyState(`No ${view === "keys" ? "virtual keys" : "projects"} in ${scopeLabel()}.`));
  }
  if (view === "keys") {
    // Policy operations keep their complete real forms and simulator; expose
    // them on demand without putting them ahead of the inventory.
    for (const title of ["Inherited policy layers", "Policy simulator"]) {
      const panel = [...content.querySelectorAll(".panel")].find((item) => item.querySelector("h3")?.textContent === title);
      if (panel) disclosePanel(panel, title);
    }
  }
  if (view === "usage") {
    const results = content.querySelector("#usage-results")?.closest(".panel");
    if (results) content.prepend(results);
    const filters = content.querySelector("#usage-form")?.closest(".panel");
    if (filters) { content.prepend(filters); disclosePanel(filters, "Filter usage & cost"); }
    for (const formId of ["usage-export-form", "task-usage-form"]) {
      const panel = content.querySelector(`#${formId}`)?.closest(".panel");
      if (panel) disclosePanel(panel, panel.querySelector("h3").textContent);
    }
    const resultsBody = content.querySelector("#usage-results");
    if (resultsBody) tabulateUsage(resultsBody);
  }
  if (view === "members" || view === "managedIdentities") {
    const tabs = document.createElement("nav"); tabs.className = "result-tabs";
    tabs.setAttribute("aria-label", "People and identities pages");
    tabs.innerHTML = `<button type="button" data-identity-view="members" aria-current="${view === "members" ? "page" : "false"}">People</button><button type="button" data-identity-view="managed-identities" aria-current="${view === "managedIdentities" ? "page" : "false"}">Workload identities</button>`;
    content.prepend(tabs);
    tabs.querySelectorAll("button").forEach((button) => button.addEventListener("click", () => navigateToView(button.dataset.identityView)));
    const projectForm = content.querySelector("#managed-identity-project-form")?.closest(".panel");
    if (projectForm) disclosePanel(projectForm, "Register project monitoring identity");
  }
  if (view === "guardrails") {
    const button = document.createElement("button"); button.type = "button"; button.textContent = "Key policies & simulator";
    button.addEventListener("click", () => navigateToView("keys")); actions.appendChild(button);
  }
  if (view === "health") {
    for (const formId of ["provider-health-state-form", "debug-bundle-form"]) {
      const panel = content.querySelector(`#${formId}`)?.closest(".panel");
      if (panel) disclosePanel(panel, panel.querySelector("h3").textContent);
    }
  }
  const visualSummary = content.querySelector(".page-insights");
  if (visualSummary) {
    const identityTabs = content.querySelector(':scope > .result-tabs[aria-label="People and identities pages"]');
    if (identityTabs) identityTabs.after(visualSummary);
    else content.prepend(visualSummary);
  }
  const editForm = content.querySelector(view === "keys" ? "#key-edit-form" : view === "services" ? "#service-edit-form" : "#no-edit-form");
  if (editForm) {
    const panel = editForm.closest(".panel");
    const placeholder = document.createElement("div"); placeholder.hidden = true; panel.before(placeholder);
    showContentDrawer(view === "keys" ? "Edit virtual key" : "Edit service", panel, () => { placeholder.appendChild(panel); if (view === "keys") state.editingKeyId = null; else state.editingServiceName = null; });
  }
  content.querySelectorAll(".table-wrap").forEach((wrapper) => { wrapper.tabIndex = 0; wrapper.setAttribute("role", "region"); wrapper.setAttribute("aria-label", "Scrollable results table"); });
}

function disclosePanel(panel, title) {
  if (panel.parentElement?.matches("details")) return;
  const details = document.createElement("details");
  details.className = "workflow-disclosure";
  const summary = document.createElement("summary");
  summary.textContent = title;
  details.appendChild(summary);
  panel.before(details);
  details.appendChild(panel);
}

function addInventorySearch(panel) {
  if (panel.querySelector("[data-inventory-search]")) return;
  const label = document.createElement("label");
  label.className = "inventory-search";
  label.innerHTML = `<span>Search inventory</span><input type="search" aria-label="Search inventory" data-inventory-search placeholder="Search this inventory…"><span class="search-count" role="status"></span>`;
  panel.querySelector(".panel-heading").after(label);
  const rows = [...panel.querySelectorAll("tbody > tr")];
  label.querySelector("input").addEventListener("input", (event) => {
    const query = event.target.value.trim().toLowerCase();
    rows.forEach((row) => { row.hidden = row.dataset.outOfScope === "true" || !row.textContent.toLowerCase().includes(query); });
    label.querySelector(".search-count").textContent = `${rows.filter((row) => !row.hidden).length} matching rows`;
  });
}

function tabulateUsage(root) {
  if (root.querySelector(".result-tabs")) return;
  const headings = [...root.querySelectorAll(":scope > h4, :scope > .usage-section-heading")];
  if (!headings.length) return;
  const nav = document.createElement("div");
  nav.className = "result-tabs";
  nav.setAttribute("role", "group");
  nav.setAttribute("aria-label", "Usage breakdown");
  headings[0].before(nav);
  const hint = document.createElement("p");
  hint.className = "sr-only";
  nav.after(hint);
  const sections = headings.map((heading) => {
    const section = document.createElement("section");
    section.className = "breakdown-section";
    const nodes = [heading];
    let next = heading.nextElementSibling;
    while (next && !headings.includes(next)) { nodes.push(next); next = next.nextElementSibling; }
    heading.before(section);
    nodes.forEach((node) => section.appendChild(node));
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = heading.matches("h4") ? heading.textContent : heading.querySelector("h4").textContent;
    nav.appendChild(button);
    return { section, button };
  });
  const select = (chosen) => {
    state.usageTab = chosen < 0 ? null : sections[chosen].button.textContent;
    sections.forEach(({ section, button }, index) => {
      section.hidden = chosen >= 0 && index !== chosen;
      button.setAttribute("aria-pressed", String(index === chosen));
    });
    hint.textContent = chosen < 0 ? "Showing all breakdowns. Select a breakdown to focus on it." : "Click the selected breakdown again to show all breakdowns.";
    if (typeof loadVisibleUsageSections === "function") loadVisibleUsageSections(root);
  };
  sections.forEach(({ button }, index) => button.addEventListener("click", () => select(state.usageTab === button.textContent ? -1 : index)));
  select(state.usageTab === null ? -1 : Math.max(0, sections.findIndex(({ button }) => button.textContent === state.usageTab)));
}

window.matchMedia("(max-width: 920px)").addEventListener("change", closeNavigation);
document.querySelector("#sidebar").addEventListener("keydown", (event) => {
  if (!document.body.classList.contains("nav-open")) return;
  if (event.key === "Escape") { event.preventDefault(); closeNavigation(); }
  if (event.key !== "Tab") return;
  const buttons = [...document.querySelectorAll("#sidebar button")].filter((button) => button.getClientRects().length && !button.disabled);
  const first = buttons[0], last = buttons.at(-1);
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
  if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
});
document.querySelector("#project-scope").addEventListener("change", async (event) => {
  if (pendingWrites) { syncProjectScope(); return; }
  if (hasDirtyForms() && !(await confirmAction("Discard unsaved changes?", "Changing project scope replaces the current forms."))) { syncProjectScope(); return; }
  clearDirtyForms();
  synchronizeProjectScope(event.target.value);
  resetUsagePagination();
  persistMonitoringHash();
  refresh();
});
closeNavigation();

async function initializePortal() {
  if (token()) {
    state.workspace = "admin";
    signedIn();
    return;
  }
  try {
    state.authConfig = await json("/admin-ui/auth/config");
    const signIn = document.querySelector("#entra-sign-in");
    if (state.authConfig.enabled) {
      const returnTo = `${location.pathname}${location.hash}`;
      signIn.href = `/admin-ui/auth/login?return_to=${encodeURIComponent(returnTo)}`;
    } else {
      signIn.classList.add("hidden");
      document.querySelector("#oidc-unavailable").classList.remove("hidden");
    }
    const session = await json("/admin-ui/auth/session");
    if (!session.authenticated) return;
    state.session = session;
    if (session.member.status !== "active") {
      renderAccessState(session.member);
      return;
    }
    state.workspace = session.member.roles?.includes("admin") ? "admin" : "owner";
    signedIn();
  } catch (error) {
    document.querySelector("#login-error").textContent = error.message;
  }
}

function renderAccessState(member) {
  document.querySelector("#entra-sign-in").classList.add("hidden");
  document.querySelector(".break-glass-login").classList.remove("hidden");
  document.querySelector("#access-state-sign-out").classList.remove("hidden");
  document.querySelector("#login-title").textContent = member.status === "blocked" ? "Access blocked" : "Approval pending";
  document.querySelector(".login-copy").textContent = member.status === "blocked"
    ? "An administrator has blocked this portal membership. Contact the gateway operations team."
    : "Your Microsoft identity is verified. A gateway administrator must approve your membership and assign service access.";
  document.querySelector("#login-error").textContent = member.email || member.object_id;
}

initializePortal();

async function profileKeyCatalog() {
  const [keys, projects] = await Promise.all([api('/admin-ui/admin/keys'),api('/admin-ui/admin/projects')]);
  return {keys,projects};
}
function prepareProfileKeys(root) {
  root.querySelectorAll('[data-user-token-forwarding]').forEach(host => mountUserTokenForwarding(host));
  root.querySelectorAll('[data-endpoint-identity]').forEach(host => {
    const project = host.dataset.keyProject || '';
    mountIdentityEditor(host, async () => {
      const catalog = await profileKeyCatalog();
      return {all: profileKeyOptions(catalog.keys,catalog.projects), eligible: profileKeyOptions(catalog.keys,catalog.projects,'',project)};
    });
  });
}

document.addEventListener("click", handleAsync(async (event) => {
  const button = event.target.closest("[data-key-profile-bindings]");
  if (!button) return;
  const keyId = button.dataset.keyProfileBindings;
  const [routes, services] = await Promise.all([api("/admin-ui/admin/route-identities"), api("/admin-ui/admin/services")]);
  const entries = [...routes.map(row => ({...row,url:"/admin-ui/admin/route-identities",method:"PUT"})),
    ...services.map(row => ({route:row.route_pattern,access:row.access,url:`/admin-ui/admin/services/${encodeURIComponent(row.name)}`,method:"PATCH"}))];
  mountKeyRouteBindings({
    keyId, keyName: keyName(keyId), entries, trigger: button,
    save: async (entry, access) => {
      const result = await api(entry.url, {
        method: entry.method,
        body: JSON.stringify(entry.method === 'PUT' ? {route: entry.route, access} : {access}),
      });
      return result.access;
    },
  });
}));
