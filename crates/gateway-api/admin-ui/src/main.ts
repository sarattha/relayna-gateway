import "@tabler/icons-webfont/dist/tabler-icons.min.css";
import Chart from "chart.js/auto";
import "./app.css";
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
  tableWrap,
  viewMeta,
} from "./design-system";

const tokenKey = "relayna_gateway_operator_token";
const viewIds = new Set(Object.keys(viewMeta));
const state = {
  view: viewFromHash(),
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
};

const login = document.querySelector("#login");
const app = document.querySelector("#app");
const content = document.querySelector("#content");
const requestTimeoutMs = 8000;
let noticeTimer: ReturnType<typeof setTimeout> | null = null;
let dialogCounter = 0;
let overviewChart: Chart | null = null;

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
    const pendingRoot = event.currentTarget || event.target;
    const pendingControls = setPending(pendingRoot, true);
    try {
      await handler(event);
    } catch (error) {
      setNotice(error.message);
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
  const response = await fetchWithTimeout(path, {
    ...options,
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${token()}`,
      ...(options.headers || {}),
    },
  });
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
  if (response.status === 204) return null;
  return response.json();
}

async function json(path, options = {}) {
  const response = await fetchWithTimeout(path, options);
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  return response.json();
}

async function fetchWithTimeout(path, options = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), requestTimeoutMs);
  try {
    return await fetch(path, {
      ...options,
      signal: controller.signal,
    });
  } catch (error) {
    if (error.name === "AbortError") {
      throw new Error("request_timeout");
    }
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}

function showRawToken(rawToken, label = "Token shown once") {
  const template = document.querySelector("#raw-token-template");
  const node = template.content.cloneNode(true);
  node.querySelector("h3").textContent = label;
  node.querySelector("textarea").value = rawToken;
  document.body.appendChild(node);
  const backdrop = document.querySelector(".modal-backdrop:last-of-type");
  const close = mountDialog(backdrop, { initialFocus: "[data-copy-token]" });
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
      <textarea readonly rows="18">${esc(value)}</textarea>
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

function mountDialog(backdrop, { initialFocus = "button", onClose = () => {} } = {}) {
  const dialog = backdrop?.querySelector('[role="dialog"]');
  if (!(backdrop instanceof HTMLElement) || !(dialog instanceof HTMLElement)) {
    return () => {};
  }
  const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  let closed = false;
  const focusableSelector = [
    "button:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    'a[href]',
    '[tabindex]:not([tabindex="-1"])',
  ].join(",");
  const close = (value) => {
    if (closed) return;
    closed = true;
    backdrop.removeEventListener("keydown", onKeyDown);
    backdrop.remove();
    previousFocus?.focus();
    onClose(value);
  };
  const onKeyDown = (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close(false);
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [...dialog.querySelectorAll(focusableSelector)].filter((item) => item instanceof HTMLElement && !item.hidden);
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
    if (event.target === backdrop) close(false);
  });
  backdrop.closeDialog = close;
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
  state.view = viewFromHash();
  syncNavigation();
  refresh();
}

function viewFromHash() {
  const value = location.hash.replace(/^#\/?/, "");
  return viewIds?.has(value) ? value : "overview";
}

function navigateToView(view, { replace = false, focus = true } = {}) {
  if (!viewIds.has(view)) return;
  const nextHash = `#/${view}`;
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
    const active = item.dataset.view === state.view;
    item.classList.toggle("active", active);
    if (active) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  });
}

function openNavigation() {
  document.body.classList.add("nav-open");
  document.querySelector("#nav-backdrop")?.classList.remove("hidden");
  document.querySelector("#nav-toggle")?.setAttribute("aria-expanded", "true");
  document.querySelector("#nav-close")?.focus();
}

function closeNavigation() {
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
  const commands = Object.entries(viewMeta)
    .map(([view, meta]) => `<button type="button" class="command-item" data-command-view="${attr(view)}">
      <span><strong>${esc(meta.title)}</strong><small>${esc(meta.domain)} · ${esc(meta.summary)}</small></span>
      <kbd>↵</kbd>
    </button>`)
    .join("");
  backdrop.innerHTML = `
    <div class="modal command-palette" role="dialog" aria-modal="true" aria-labelledby="${titleId}">
      <h3 id="${titleId}">Go to Admin view</h3>
      <label class="command-search"><span class="sr-only">Filter views</span><input type="search" placeholder="Search views…" autocomplete="off" data-command-search></label>
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
    signedIn();
  } catch (error) {
    sessionStorage.removeItem(tokenKey);
    document.querySelector("#login-error").textContent = error.message;
  }
});

document.querySelector("#sign-out").addEventListener("click", () => {
  sessionStorage.removeItem(tokenKey);
  location.reload();
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

window.addEventListener("hashchange", () => {
  if (!token()) return;
  state.view = viewFromHash();
  syncNavigation();
  closeNavigation();
  window.scrollTo({ top: 0, behavior: "auto" });
  refresh({ focus: true });
});

async function refresh({ focus = false } = {}) {
  setNotice("");
  destroyOverviewChart();
  applyViewChrome(state.view);
  const meta = viewMeta[state.view] || viewMeta.overview;
  document.querySelector("#breadcrumb-domain").textContent = meta.domain;
  document.querySelector("#breadcrumb-title").textContent = meta.title;
  content.innerHTML = panel("", emptyState("Loading..."));
  content.setAttribute("aria-busy", "true");
  try {
    if (state.view === "overview") await overview();
    if (state.view === "projects") await projects();
    if (state.view === "keys") await keys();
    if (state.view === "guardrails") await guardrails();
    if (state.view === "audit") await audit();
    if (state.view === "providers") await providers();
    if (state.view === "routes") await routes();
    if (state.view === "services") await services();
    if (state.view === "usage") await usage();
    if (state.view === "health") await health();
    if (state.view === "settings") await settings();
    document.querySelector("#last-refreshed").textContent = `Updated ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
    if (focus) content.focus({ preventScroll: true });
  } catch (error) {
    setNotice(error.message);
    content.innerHTML = `<section class="panel"><div class="empty-state"><p>${esc(error.message)}</p></div></section>`;
  } finally {
    content.setAttribute("aria-busy", "false");
  }
}

async function overview() {
  const usageQuery = overviewUsageQuery();
  const [dashboard, healthRows, ready, keysRows, openaiRoutes, anthropicRoutes, servicesRows, projectsRows, auditEvents, policyLayers] = await Promise.all([
    api(`/admin-ui/admin/usage/dashboard?${usageQuery}`),
    api("/admin-ui/admin/provider-health"),
    json("/admin-ui/readyz"),
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/openai-routes"),
    api("/admin-ui/admin/anthropic-routes"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/projects"),
    api("/admin-ui/admin/audit-events?limit=8"),
    api("/admin-ui/admin/policy-layers"),
  ]);
  const summary = dashboard.summary;
  const activeKeys = keysRows.filter((key) => !key.disabled && !key.revoked_at).length;
  const enabledRoutes =
    openaiRoutes.filter((route) => route.enabled).length + anthropicRoutes.filter((route) => route.enabled).length;
  const totalRoutes = openaiRoutes.length + anthropicRoutes.length;
  const enabledServices = servicesRows.filter((service) => service.enabled).length;
  const risks = overviewRisks(healthRows);
  const readyState = ready.status === "ready";
  content.innerHTML = `
    <div class="overview-top">
      <section class="panel posture-panel">
        <div class="posture-state ${risks.length ? "warn" : "good"}">
          <span class="posture-icon" aria-hidden="true"><i class="ti ${readyState ? "ti-check" : "ti-alert-triangle"}"></i></span>
          <div><strong>${readyState ? "Ready" : esc(ready.status)}</strong><span>${risks.length ? `${risks.length} risk${risks.length === 1 ? "" : "s"} require attention` : "No active risks"}</span></div>
          <button type="button" class="link-button" data-overview-nav="health">View details <i class="ti ti-arrow-right" aria-hidden="true"></i></button>
        </div>
        <div class="posture-facts">
          ${overviewFact("Requests", summary.request_count, overviewWindowLabel())}
          ${overviewFact("Failures", summary.failure_count, overviewWindowLabel(), "bad")}
          ${overviewFact("Cost", money(summary.estimated_cost_usd), overviewWindowLabel(), "good")}
          ${overviewFact("Active keys", activeKeys, `${projectsRows.length} project${projectsRows.length === 1 ? "" : "s"}`)}
        </div>
      </section>
      <section class="panel change-center">
        <div class="panel-heading"><h3>Change center</h3></div>
        <button type="button" class="change-row" data-overview-nav="audit"><i class="ti ti-history" aria-hidden="true"></i><span><strong>Recent governed changes</strong><small>Operator audit history</small></span><b>${auditEvents.length}</b><i class="ti ti-chevron-right" aria-hidden="true"></i></button>
        <button type="button" class="change-row" data-overview-nav="keys"><i class="ti ti-layers-subtract" aria-hidden="true"></i><span><strong>Policy layers</strong><small>Inherited governance</small></span><b>${policyLayers.length}</b><i class="ti ti-chevron-right" aria-hidden="true"></i></button>
        <div class="change-divider"></div>
        <button type="button" class="change-row" data-overview-nav="keys"><i class="ti ti-plus" aria-hidden="true"></i><span><strong>Create key</strong><small>Issue a virtual key with policy</small></span><i class="ti ti-chevron-right" aria-hidden="true"></i></button>
        <button type="button" class="change-row" data-overview-nav="services"><i class="ti ti-plus" aria-hidden="true"></i><span><strong>Register service</strong><small>Add an upstream service</small></span><i class="ti ti-chevron-right" aria-hidden="true"></i></button>
        <button type="button" class="change-row" data-overview-nav="providers"><i class="ti ti-plus" aria-hidden="true"></i><span><strong>Add provider</strong><small>Connect a model provider</small></span><i class="ti ti-chevron-right" aria-hidden="true"></i></button>
      </section>
    </div>
    <div class="overview-insights">
      <section class="panel chart-panel">
        <div class="panel-heading chart-heading">
          <div><h3>Traffic, failures & latency</h3><span class="subtle">Real usage events grouped by ${state.overviewWindow === "30d" ? "day" : "hour"}</span></div>
          <label class="compact-field"><span class="sr-only">Overview time range</span><select id="overview-window">
            ${option("24h", state.overviewWindow).replace(">24h<", ">Last 24 hours<")}
            ${option("7d", state.overviewWindow).replace(">7d<", ">Last 7 days<")}
            ${option("30d", state.overviewWindow).replace(">30d<", ">Last 30 days<")}
          </select></label>
        </div>
        <div class="overview-chart-wrap"><canvas id="overview-chart" role="img" aria-label="Requests, failures, and average latency over ${overviewWindowLabel().toLowerCase()}"></canvas></div>
        <p class="sr-only">${esc(overviewChartSummary(dashboard.timeseries || []))}</p>
      </section>
      <section class="panel attention-panel">
        <div class="panel-heading"><h3><i class="ti ti-alert-triangle" aria-hidden="true"></i>Attention and recommendations</h3><span class="badge warn">${risks.length}</span></div>
        <div class="attention-list">${risks.length ? risks.slice(0, 3).map(overviewRiskRow).join("") : emptyState("No provider or service risks in this window.")}</div>
        <button type="button" class="link-button panel-link" data-overview-nav="health">View all health signals <i class="ti ti-arrow-right" aria-hidden="true"></i></button>
      </section>
    </div>
    <section class="panel operations-panel">
      <div class="panel-heading"><div><h3>Gateway operations</h3><span class="subtle">Live provider, service, route, and key posture</span></div><span class="subtle">${enabledRoutes}/${totalRoutes} routes · ${enabledServices} services</span></div>
      ${overviewOperationsTable(healthRows, keysRows)}
    </section>
  `;
  renderOverviewChart(dashboard.timeseries || []);
  document.querySelector("#overview-window")?.addEventListener("change", (event) => {
    state.overviewWindow = event.currentTarget.value;
    overview();
  });
  document.querySelectorAll("[data-overview-nav]").forEach((button) => {
    button.addEventListener("click", () => navigateToView(button.dataset.overviewNav));
  });
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
    timeseries_limit: state.overviewWindow === "30d" ? "31" : state.overviewWindow === "7d" ? "168" : "24",
    service_timeseries_limit: "1",
  });
  return query.toString();
}

function overviewWindowLabel() {
  if (state.overviewWindow === "30d") return "Last 30 days";
  if (state.overviewWindow === "24h") return "Last 24 hours";
  return "Last 7 days";
}

function overviewFact(label, value, detail, tone = "") {
  return `<div class="posture-fact ${tone}"><strong>${esc(value)}</strong><span>${esc(label)}</span><small>${esc(detail)}</small></div>`;
}

function overviewRisks(rows) {
  return rows
    .map((row) => {
      const requests = Math.max(Number(row.request_count || 0), 1);
      const errors = Number(row.error_count || 0);
      const timeouts = Number(row.timeout_count || 0);
      const fallbacks = Number(row.fallback_count || 0);
      const errorRate = errors / requests;
      const timeoutRate = timeouts / requests;
      const fallbackRate = fallbacks / requests;
      let severity = "low";
      let issue = "Provider signal requires review";
      let score = errorRate + timeoutRate + fallbackRate;
      if (String(row.status).toLowerCase().includes("timeout") || timeoutRate >= 0.05) {
        severity = timeoutRate >= 0.1 ? "high" : "medium";
        issue = "Timeout rate elevated";
        score += 2;
      } else if (errorRate >= 0.05) {
        severity = errorRate >= 0.1 ? "high" : "medium";
        issue = "Error rate elevated";
        score += 1.5;
      } else if (fallbackRate > 0) {
        severity = fallbackRate >= 0.1 ? "medium" : "low";
        issue = "Fallback activity detected";
        score += 1;
      } else if (["unhealthy", "degraded", "open"].includes(String(row.status).toLowerCase())) {
        severity = row.status === "unhealthy" ? "high" : "medium";
        issue = `${row.status} health state`;
        score += 1;
      }
      return { ...row, severity, issue, score, errorRate, timeoutRate, fallbackRate };
    })
    .filter((row) => {
      const status = String(row.status || "").trim().toLowerCase();
      return row.score > 0 || (status.length > 0 && !["healthy", "ready", "ok"].includes(status));
    })
    .sort((a, b) => b.score - a.score);
}

function overviewRiskRow(row) {
  const signal = row.issue.startsWith("Timeout") ? row.timeoutRate : row.issue.startsWith("Error") ? row.errorRate : row.fallbackRate;
  return `<div class="attention-row">
    ${badge(row.severity, row.severity === "high" ? "bad" : "warn")}
    <span><strong>${esc(row.issue)}</strong><small>${esc(row.name)} · ${percent(signal)}</small></span>
    <button type="button" class="primary" data-overview-nav="health" aria-label="Open health investigation for ${attr(row.name)}">Open investigation</button>
  </div>`;
}

function overviewOperationsTable(healthRows, keysRows) {
  const health = healthRows.slice(0, 5).map((row) => [
    `<strong>${esc(row.name)}</strong>`,
    esc(row.provider || row.provider_kind || "Service"),
    healthBadge(row),
    `<strong>${esc(row.error_count || 0)} errors · ${esc(row.fallback_count || 0)} fallbacks</strong><div class="subtle">${averageLatency(row)}</div>`,
    "Live",
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
  const labels = rows.map((row) => time(row.bucket_start || row.bucket || row.name));
  const values = (key) => rows.map((row) => Number(row.summary?.[key] ?? row[key] ?? 0));
  overviewChart = new Chart(canvas, {
    type: "line",
    data: {
      labels,
      datasets: [
        { label: "Requests", data: values("request_count"), borderColor: "#087b60", backgroundColor: "#087b60", yAxisID: "y", tension: 0.32, pointRadius: 0, borderWidth: 2 },
        { label: "Failures", data: values("failure_count"), borderColor: "#d9474f", backgroundColor: "#d9474f", yAxisID: "y", tension: 0.32, pointRadius: 0, borderWidth: 2 },
        { label: "Latency (ms)", data: values("average_latency_ms"), borderColor: "#0ea5a0", backgroundColor: "#0ea5a0", yAxisID: "latency", tension: 0.32, pointRadius: 0, borderWidth: 2 },
      ],
    },
    options: {
      animation: false,
      maintainAspectRatio: false,
      responsive: true,
      interaction: { mode: "index", intersect: false },
      plugins: {
        legend: { position: "top", align: "start", labels: { boxWidth: 18, boxHeight: 2, usePointStyle: false, color: "#536276", font: { size: 11, weight: 600 } } },
        tooltip: { backgroundColor: "#062f36", padding: 10, titleFont: { size: 12 }, bodyFont: { size: 12 } },
      },
      scales: {
        x: { grid: { display: false }, ticks: { color: "#58736e", maxTicksLimit: 7, font: { size: 10 } } },
        y: { beginAtZero: true, grid: { color: "#dcebe7" }, ticks: { color: "#58736e", precision: 0, font: { size: 10 } } },
        latency: { beginAtZero: true, position: "right", grid: { drawOnChartArea: false }, ticks: { color: "#58736e", font: { size: 10 } } },
      },
    },
  });
}

function destroyOverviewChart() {
  if (!overviewChart) return;
  overviewChart.destroy();
  overviewChart = null;
}

function stat(label, value) {
  return metricTile(label, value);
}

async function projects() {
  [state.projects, state.services] = await Promise.all([api("/admin-ui/admin/projects"), api("/admin-ui/admin/services")]);
  content.innerHTML = `
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
}

function projectTable(rows) {
  return table(
    ["Name", "UUID", "Linked services", "Updated", "Actions"],
    rows.map((row) => [
      esc(row.name),
      `<code>${esc(row.id)}</code>`,
      projectServiceForm(row),
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
  await api("/admin-ui/admin/projects", { method: "POST", body: JSON.stringify({ name: form.get("name") }) });
  setNotice("Project created.", "success");
  await projects();
}

async function projectAction(event) {
  const { projectAction: action, projectId } = event.currentTarget.dataset;
  if (action === "usage") {
    const summary = await api(`/admin-ui/admin/projects/${projectId}/usage`);
    setNotice(`Project usage: ${summary.request_count} requests, ${money(summary.estimated_cost_usd)} cost.`, "success");
    return;
  }
  if (!(await confirmAction("Delete project", "Projects with linked keys, services, or usage cannot be deleted."))) return;
  await api(`/admin-ui/admin/projects/${projectId}`, { method: "DELETE" });
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
  [state.keys, state.projects, state.services, state.guardrails, state.policyLayers] = await Promise.all([
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/projects"),
    api("/admin-ui/admin/services"),
    api("/admin-ui/admin/guardrails"),
    api("/admin-ui/admin/policy-layers"),
  ]);
  const editing = state.keys.find((key) => key.id === state.editingKeyId);
  content.innerHTML = `
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
        <label>Key<select name="key_id"><option value="">Default policy</option>${state.keys.map((key) => `<option value="${attr(key.id)}">${esc(key.key_prefix)}</option>`).join("")}</select></label>
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
}

function policyFields(key = null, neutral = false) {
  const policy = key?.policy || {};
  return `
    <label>Routes<input name="allowed_routes" value="${attr(listValue(policy.allowed_routes, neutral ? "" : "/v1/chat/completions,/v1/responses"))}"></label>
    <label>Models<input name="allowed_models" value="${attr(listValue(policy.allowed_models, ""))}" placeholder="gpt-4o-mini"></label>
    <div class="field"><span>Providers</span>${providerPolicySelect(policy.allowed_providers, neutral)}</div>
    <label>RPM limit<input name="rpm_limit" type="number" min="0" value="${attr(policy.rpm_limit ?? "")}"></label>
    <label>TPM limit<input name="tpm_limit" type="number" min="0" value="${attr(policy.tpm_limit ?? "")}"></label>
    <label>Daily budget<input name="daily_budget_usd" type="number" min="0" step="0.01" value="${attr(policy.daily_budget_usd ?? "")}"></label>
    <label>Monthly budget<input name="monthly_budget_usd" type="number" min="0" step="0.01" value="${attr(policy.monthly_budget_usd ?? "")}"></label>
    <label>Max daily requests<input name="max_requests_per_day" type="number" min="0" value="${attr(policy.max_requests_per_day ?? "")}"></label>
    <label>Max daily tokens<input name="max_tokens_per_day" type="number" min="0" value="${attr(policy.max_tokens_per_day ?? "")}"></label>
    <label>Max cost/request<input name="max_cost_per_request" type="number" min="0" step="0.01" value="${attr(policy.max_cost_per_request ?? "")}"></label>
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
    <div class="field"><span>Mandatory guardrails</span>${guardrailSelectionControl(policy.mandatory_guardrails || [], "mandatory_guardrails", "Mandatory guardrails")}</div>
    <div class="field"><span>Optional guardrails</span>${guardrailSelectionControl(policy.optional_guardrails || [], "optional_guardrails", "Optional guardrails")}</div>
    <div class="field"><span>Forbidden guardrails</span>${guardrailSelectionControl(policy.forbidden_guardrails || [], "forbidden_guardrails", "Forbidden guardrails")}</div>
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
          <textarea name="guardrail_override_${attr(guardrail.name)}" rows="4">${esc(value)}</textarea>
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
    ["Prefix", "Owner", "Services", "Status", "Expiry", "Policy", "Updated", "Actions"],
    rows.map((key) => [
      `<code>${esc(key.key_prefix)}</code>`,
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
      badge(layer.kind),
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
  const keyLabel = attr(key.key_prefix);
  const toggle = key.revoked_at
    ? ""
    : key.disabled
      ? `<button data-key-action="enable" data-key-id="${attr(key.id)}" aria-label="Enable virtual key ${keyLabel}">Enable</button>`
      : `<button data-key-action="disable" data-key-id="${attr(key.id)}" aria-label="Disable virtual key ${keyLabel}">Disable</button>`;
  return `<div class="actions">
        <button data-key-action="edit" data-key-id="${attr(key.id)}" aria-label="Edit virtual key ${keyLabel}">Edit</button>
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
  const response = await api("/admin-ui/admin/keys", { method: "POST", body: JSON.stringify(body) });
  showRawToken(response.raw_key, "Virtual key shown once");
  state.editingKeyId = response.key.id;
  setNotice("Virtual key created.", "success");
  await keys();
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
  const formMarkup = `
    <label>Action<input name="action" placeholder="operator_token.rotate"></label>
    <label>Target type<input name="target_type" placeholder="key, policy_layer, provider"></label>
    <label>Target ID<input name="target_id"></label>
    <label>Operator token ID<input name="actor_token_id"></label>
    <label>Limit<input name="limit" type="number" min="1" max="500" value="100"></label>
    <div class="form-actions"><button class="primary">Apply</button></div>
  `;
  content.innerHTML = auditLogTemplate(formMarkup, '<div id="audit-results"></div>');
  document.querySelector("[data-filter-form]").addEventListener("submit", handleAsync(loadAuditEvents));
  await loadAuditEvents();
}

async function loadAuditEvents(event) {
  event?.preventDefault();
  const form = event ? new FormData(event.target) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["action", "target_type", "target_id", "actor_token_id", "limit"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  state.auditEvents = await api(`/admin-ui/admin/audit-events?${query}`);
  const results = document.querySelector("#audit-results");
  if (results) results.innerHTML = auditEventTable(state.auditEvents);
}

function auditEventTable(rows) {
  return table(
    ["Time", "Actor", "Action", "Target", "Request", "IP", "User agent", "Snapshots"],
    rows.map((row) => [
      time(row.created_at),
      `<code>${esc(row.actor_token_id || "system")}</code>`,
      badge(row.action),
      `<strong>${esc(row.target_type)}</strong><div class="subtle"><code>${esc(row.target_id || "")}</code></div>`,
      `<code>${esc(row.request_id || "")}</code>`,
      esc(row.ip || ""),
      esc(row.user_agent || ""),
      `<details><summary>Before/after</summary>${jsonBlock({ before: row.before, after: row.after })}</details>`,
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
  if (action === "usage") {
    const summary = await api(`/admin-ui/admin/keys/${keyId}/usage`);
    setNotice(
      `Key usage: ${summary.request_count} requests, ${summary.failure_count} failures, ${money(summary.estimated_cost_usd)} cost.`,
      "success",
    );
    return;
  }
  if (!(await confirmAction(`${action} virtual key`, "This lifecycle change is written to the database."))) return;
  await api(`/admin-ui/admin/keys/${keyId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`Virtual key ${action}d.`, "success");
  await keys();
}

async function providers() {
  [state.providers, state.litellmCredentialMappings, state.litellmPassthroughSettings, state.keys, state.projects] = await Promise.all([
    api("/admin-ui/admin/providers"),
    api("/admin-ui/admin/providers/litellm-credentials"),
    api("/admin-ui/admin/providers/litellm-passthrough"),
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/projects"),
  ]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Create provider</h3></div>
      <form id="provider-form" class="form-grid">
        ${formSection("Identity and endpoint", "Choose the adapter and upstream address.", `
          <label>Provider<select name="provider">${option("litellm", "litellm")}${option("internal-service", "")}</select></label>
          <label>Name<input name="name" required value="LiteLLM"></label>
          <label>Endpoint<input name="base_url" required placeholder="http://litellm:4000"></label>
          <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
        `, true)}
        ${formSection("Authentication", "Credentials remain write-only after save.", `
          <label>Default credential<input name="credential" type="password" autocomplete="new-password"></label>
          <label>Credential mode<select name="credential_header_mode">
            ${option("authorization_bearer", "authorization_bearer")}
            ${option("custom_header", "")}
          </select></label>
          <label>Custom header<input name="credential_header_name" placeholder="x-litellm-api-key"></label>
          <label>Header value<select name="credential_header_value_format">
            ${option("raw", "raw")}
            ${option("bearer", "")}
          </select></label>
          <div class="help wide-field">Use raw for headers like x-litellm-api-key: &lt;key&gt;. Use bearer for LiteLLM deployments that expect x-litellm-key: Bearer &lt;key&gt;.</div>
        `)}
        <div class="form-actions sticky-form-actions wide-field"><button class="primary">Create provider</button></div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Provider configuration</h3><span class="subtle">${state.providers.length} total</span></div>
      ${providerTable(state.providers)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>LiteLLM credential mappings</h3><span class="subtle">${state.litellmCredentialMappings.length} total</span></div>
      <form id="litellm-credential-form" class="form-grid">
        <label>Scope<select name="scope" data-litellm-mapping-scope>${option("key", "key")}${option("project", "")}</select></label>
        <label>Key<select name="key_target_id" data-litellm-key-target>${keyOptions()}</select></label>
        <label>Project<select name="project_target_id" data-litellm-project-target>${projectOptions()}</select></label>
        <label>LiteLLM virtual key<input name="credential" type="password" autocomplete="new-password" required></label>
        <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
        <div class="form-actions"><button class="primary">Save mapping</button></div>
      </form>
      ${litellmCredentialMappingTable(state.litellmCredentialMappings)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>LiteLLM passthrough</h3><span class="subtle">single ingress mode</span></div>
      ${litellmPassthroughForm(state.litellmPassthroughSettings)}
    </section>
  `;
  document.querySelector("#provider-form").addEventListener("submit", handleAsync(createProvider));
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
}

function litellmPassthroughForm(settings) {
  const current = settings || {};
  return `<form id="litellm-passthrough-form" class="form-grid">
    ${formSection("Ingress allowlist", "Restrict wildcard forwarding by path, method, timeout, and payload size.", `
      <label class="check"><input name="enabled" type="checkbox" ${current.enabled ? "checked" : ""}> Enable wildcard passthrough</label>
      <label>Allowed paths<input name="allowed_paths" value="${attr(listValue(current.allowed_paths, "/v1/*"))}"></label>
      <label>Allowed methods<input name="allowed_methods" value="${attr(listValue(current.allowed_methods, "GET,POST"))}"></label>
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
    ["Provider", "Endpoint", "State", "Credential", "LiteLLM auth", "Updated", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.provider)}</div>`,
      `<code>${esc(row.base_url)}</code>`,
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
      providerAuthSettingsForm(row),
      time(row.updated_at),
      `<div class="actions">
        <button data-provider-action="${row.enabled ? "disable" : "enable"}" data-provider-id="${attr(row.id)}" aria-label="${row.enabled ? "Disable" : "Enable"} provider ${attr(row.name)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-provider-action="delete" data-provider-id="${attr(row.id)}" aria-label="Delete provider ${attr(row.name)}">Delete</button>
      </div>`,
    ]),
  );
}

function providerAuthSettingsForm(row) {
  if (row.provider !== "litellm") {
    return '<span class="subtle">not applicable</span>';
  }
  return `<form class="inline-form" data-provider-config-form data-provider-id="${attr(row.id)}" aria-label="Authentication settings for provider ${attr(row.name)}">
    <select name="credential_header_mode">
      ${option("authorization_bearer", row.credential_header_mode || "authorization_bearer")}
      ${option("custom_header", row.credential_header_mode || "")}
    </select>
    <input name="credential_header_name" placeholder="x-litellm-api-key" value="${attr(row.credential_header_name || "")}">
    <select name="credential_header_value_format">
      ${option("raw", row.credential_header_value_format || "raw")}
      ${option("bearer", row.credential_header_value_format || "")}
    </select>
    <input name="credential" type="password" autocomplete="new-password" placeholder="rotate default credential">
    <button type="submit" aria-label="Update authentication for provider ${attr(row.name)}">Update</button>
    <span class="subtle">x-litellm-key usually needs bearer.</span>
  </form>`;
}

function litellmCredentialMappingTable(rows) {
  return table(
    ["Scope", "Target", "State", "Credential", "Updated", "Actions"],
    rows.map((row) => [
      esc(row.scope),
      `<strong>${esc(row.target_label || mappingTargetName(row))}</strong><div class="subtle"><code>${esc(row.target_id)}</code></div>`,
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
      time(row.updated_at),
      `<div class="actions">
        <button data-litellm-mapping-action="${row.enabled ? "disable" : "enable"}" data-mapping-id="${attr(row.id)}" aria-label="${row.enabled ? "Disable" : "Enable"} LiteLLM mapping for ${attr(row.target_label || mappingTargetName(row))}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-litellm-mapping-action="delete" data-mapping-id="${attr(row.id)}" aria-label="Delete LiteLLM mapping for ${attr(row.target_label || mappingTargetName(row))}">Delete</button>
      </div>`,
    ]),
  );
}

async function createProvider(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const credentialHeaderMode = form.get("credential_header_mode");
  const credentialHeaderName = nullableString(form.get("credential_header_name"));
  const credentialHeaderValueFormat = form.get("credential_header_value_format");
  await api("/admin-ui/admin/providers", {
    method: "POST",
    body: JSON.stringify({
      provider: form.get("provider"),
      name: form.get("name"),
      base_url: form.get("base_url"),
      credential: blankToUndefined(form.get("credential")),
      credential_header_mode: credentialHeaderMode,
      credential_header_name: credentialHeaderMode === "custom_header" ? credentialHeaderName : null,
      credential_header_value_format: credentialHeaderValueFormat,
      enabled: form.has("enabled"),
    }),
  });
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
  [state.openaiRoutes, state.anthropicRoutes, state.services] = await Promise.all([
    api("/admin-ui/admin/openai-routes"),
    api("/admin-ui/admin/anthropic-routes"),
    api("/admin-ui/admin/services"),
  ]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading">
        <h3>${routeFamilyLogo("openai")} OpenAI-compatible routes</h3>
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
    ["Route", "State", "Configuration", "Updated", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.route_id)}</strong><div class="subtle"><code>${esc(row.route)}</code></div>`,
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      modeForm(row),
      time(row.updated_at),
      `<div class="actions">
        <button ${actionAttr}="${row.enabled ? "disable" : "enable"}" data-route-id="${attr(row.route_id)}">${row.enabled ? "Disable" : "Enable"}</button>
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
    ["Service", "Route", "State", "Methods", "Upstream", "Timeout", "Health check", "Credential"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.source)}</div>`,
      `<code>${esc(row.route_pattern)}</code>`,
      serviceBadges(row),
      esc(listValue(row.allowed_methods, "none")),
      esc(row.upstream_base_url || "missing"),
      serviceRouteTimeoutForm(row),
      esc(healthCheckLabel(row)),
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
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

async function services() {
  [state.services, state.projects] = await Promise.all([api("/admin-ui/admin/services"), api("/admin-ui/admin/projects")]);
  const editing = state.services.find((service) => service.name === state.editingServiceName);
  content.innerHTML = `
    <div class="service-stack">
      <section class="panel">
        <div class="panel-heading">
          <h3>Create service</h3>
          <button type="button" data-service-action="studio-import">Import from Studio</button>
        </div>
        <form id="service-form" class="form-grid">
          ${formSection("Identity and routing", "Name the service and define its public route and upstream.", `
            <label>Name<input name="name" required pattern="[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?" placeholder="temp-service-2" title="Use lowercase letters, numbers, and hyphens; start and end with a letter or number."></label>
            <label>Route pattern<input name="route_pattern" list="service-routes" placeholder="/services/name/*"></label>
            <label>Upstream URL<input name="upstream_base_url"></label>
            <div class="field"><span>Methods</span>${methodSelect(["POST"])}</div>
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
          ${formSection("Usage pricing", "Choose the cost source and optional request-matching rules.", `
            <label>Cost mode<select name="cost_mode"><option value="none">None</option><option value="fixed">Fixed</option><option value="passthrough">Passthrough</option></select></label>
            <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01"></label>
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
      ${serviceTable(state.services)}
    </section>
    <datalist id="service-routes">${serviceRouteOptions()}</datalist>
  `;
  document.querySelector("#service-form").addEventListener("submit", handleAsync(submitService));
  document.querySelector("#service-edit-form")?.addEventListener("submit", handleAsync(patchService));
  bindPricingRuleEditors();
  bindEndpointPricingEditors();
  document.querySelectorAll("[data-service-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(serviceAction));
  });
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
      <label>Estimated cost<input data-pricing-rule-field="estimated_cost_usd" type="number" min="0" step="0.001" value="${attr(rule.estimated_cost_usd ?? "")}" placeholder="0.08"></label>
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
        <td><select data-endpoint-field="cost_mode">${option("none", rule.cost_mode)}${option("fixed", rule.cost_mode)}${option("passthrough", rule.cost_mode)}</select></td>
        <td><input data-endpoint-field="estimated_cost_usd" type="number" min="0" step="0.001" value="${attr(rule.estimated_cost_usd ?? "")}" aria-label="Estimated cost for ${attr(rule.method)} ${attr(rule.path_template)}"></td>
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
      ${formSection("Identity and routing", "Update registry identity, route, upstream, and methods.", `
        <label>Studio service ID<input name="studio_service_id" value="${attr(service.studio_service_id ?? "")}"></label>
        <label>Route pattern<input name="route_pattern" list="service-routes" value="${attr(service.route_pattern)}"></label>
        <label>Upstream URL<input name="upstream_base_url" value="${attr(service.upstream_base_url ?? "")}"></label>
        <div class="field"><span>Methods</span>${methodSelect(service.allowed_methods)}</div>
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
      ${formSection("Usage pricing", "Update cost source and request-matching rules.", `
        <label>Cost mode<select name="cost_mode">${option("none", service.cost_mode)}${option("fixed", service.cost_mode)}${option("passthrough", service.cost_mode)}</select></label>
        <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01" value="${attr(service.estimated_cost_usd ?? "")}"></label>
        <div class="help wide-field">Fixed uses the estimate configured here. Passthrough uses provider response cost fields such as usage.total_cost.</div>
        ${pricingRulesEditor(service.pricing_rules || [])}
        ${openApiEndpointPricingEditor(service)}
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
  [state.studioConnection, state.authSettings] = await Promise.all([
    api("/admin-ui/admin/studio/connection"),
    api("/admin-ui/admin/auth/front-door"),
  ]);
  content.innerHTML = `
    <div class="grid stats">
      ${stat("Studio source", state.studioConnection.source)}
      ${stat("Token", state.studioConnection.token_configured ? "Configured" : "Not configured")}
      ${stat("Base URL", state.studioConnection.base_url || "Unset")}
      ${stat("Auth source", state.authSettings.source)}
      ${stat("Entra ID", state.authSettings.entra.enabled ? "Enabled" : "Disabled")}
      ${stat("Apigee", state.authSettings.apigee.trusted_header_enabled ? "Enabled" : "Disabled")}
    </div>
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
      <div class="panel-heading"><h3>Entra ID and Apigee front door</h3><span class="subtle">${esc(state.authSettings.updated_at ? time(state.authSettings.updated_at) : "environment or unset")}</span></div>
      <form id="auth-settings-form" class="form-grid">
        ${formSection("Gateway headers", "Keep native key and trusted-ingress behavior explicit.", `
          <label>Relayna key header<input name="relayna_key_header" value="${attr(state.authSettings.relayna_key_header || "X-Relayna-Key")}"></label>
          <label class="check"><input name="apigee_trusted_header_enabled" type="checkbox" ${state.authSettings.apigee.trusted_header_enabled ? "checked" : ""}> Enable Apigee trusted headers</label>
          <label>Apigee secret<input name="apigee_trusted_header_secret" type="password" autocomplete="new-password" placeholder="${apigeeSecretPlaceholder()}"></label>
        `, true)}
        ${formSection("Microsoft Entra ID", "Validate tenant, audience, issuer, claims, and JWKS behavior.", `
          <label class="check"><input name="entra_enabled" type="checkbox" ${state.authSettings.entra.enabled ? "checked" : ""}> Enable Entra ID</label>
          <label>Tenant ID<input name="tenant_id" value="${attr(state.authSettings.entra.tenant_id || "")}"></label>
          <label>Audience<input name="audience" value="${attr(state.authSettings.entra.audience || "")}" placeholder="api://relayna-gateway"></label>
          <label>Trusted issuer<input name="issuer" type="url" value="${attr(state.authSettings.entra.issuer || "")}"></label>
          <label class="wide-field">OIDC discovery URL<input name="oidc_discovery_url" type="url" value="${attr(state.authSettings.entra.oidc_discovery_url || "")}"></label>
          <label>Required scope<input name="required_scope" value="${attr(state.authSettings.entra.required_scope || "")}" placeholder="gateway.invoke"></label>
          <label>Required role<input name="required_role" value="${attr(state.authSettings.entra.required_role || "")}" placeholder="Gateway.Invoke"></label>
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
        <div><strong>Release target</strong><span>${badge("v0.1.23")}</span></div>
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
  const body = {
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
    <th></th><th>Service</th><th>Environment</th><th>Status</th><th>Base URL</th><th>Tags</th><th>Route</th>
  </tr></thead><tbody>${rows
    .map((row, index) => `<tr>
      <td><input name="studio_index" type="checkbox" value="${attr(index)}"></td>
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
    <th></th><th>Service</th><th>Status</th><th>Route</th><th>Upstream</th>
  </tr></thead><tbody>${rows
    .map((row) => `<tr>
      <td><input name="service_name" type="checkbox" value="${attr(row.name)}" ${selected.has(row.name) ? "checked" : ""}></td>
      <td><strong>${esc(row.name)}</strong><div class="subtle">${esc(row.studio_service_id || "local")}</div></td>
      <td>${esc(row.sync_status || (row.enabled ? "enabled" : "disabled"))}</td>
      <td><code>${esc(row.route_pattern)}</code></td>
      <td><code>${esc(row.upstream_base_url || "missing")}</code></td>
    </tr>`)
    .join("")}</tbody></table></div>`;
}

function guardrailPickerTable(rows, selected) {
  if (!rows.length) return '<div class="empty-state"><p>No guardrails configured.</p></div>';
  return `<div class="table-wrap guardrail-picker-table"><table><thead><tr>
    <th></th><th>Guardrail</th><th>Provider</th><th>Modes</th><th>Failure</th><th>Default</th>
  </tr></thead><tbody>${rows
    .map((row) => `<tr>
      <td><input name="guardrail_name" type="checkbox" value="${attr(row.name)}" ${selected.has(row.name) ? "checked" : ""}></td>
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
  const form = new FormData(event.target);
  const serviceName = event.target.dataset.serviceName;
  await api(`/admin-ui/admin/services/${serviceName}`, {
    method: "PATCH",
    body: JSON.stringify(serviceBody(form, true)),
  });
  state.editingServiceName = null;
  setNotice("Service updated.", "success");
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

function serviceTable(rows) {
  return table(
    ["Name", "State", "Route", "Upstream", "Health check", "Credential", "Cost", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.source)}</div>`,
      serviceBadges(row),
      `<code>${esc(row.route_pattern)}</code>`,
      esc(row.upstream_base_url || "missing"),
      esc(healthCheckLabel(row)),
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
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
  const stateBadge = row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>';
  const syncBadge = row.sync_status === "synced" || row.sync_status === "local"
    ? `<span class="badge good">${esc(row.sync_status)}</span>`
    : `<span class="badge bad">${esc(row.sync_status)}</span>`;
  return `${stateBadge} ${syncBadge}`;
}

function healthCheckLabel(row) {
  return row.health_check_path ? `${row.health_check_method || "GET"} ${row.health_check_path}` : "upstream root";
}

async function usage() {
  [state.projects, state.services, state.keys] = await Promise.all([api("/admin-ui/admin/projects"), api("/admin-ui/admin/services"), api("/admin-ui/admin/keys")]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Usage filters</h3></div>
      <form id="usage-form" class="form-grid">
        ${formSection("Primary filters", "Start with ownership, service, status, and time range.", `
        <label>Project<select name="project_id"><option value="">All</option>${projectOptions()}</select></label>
        <label>Virtual key<select name="key_id"><option value="">All</option>${keyOptions()}</select></label>
        <label>Service<select name="service"><option value="">All</option>${serviceOptions()}</select></label>
        <label>Provider<select name="provider"><option value="">All</option><option value="litellm">litellm</option><option value="openai-compatible">openai-compatible</option><option value="internal-service">internal-service</option></select></label>
        <label>Status<select name="status"><option value="">All</option><option value="success">Success</option><option value="failure">Failure</option></select></label>
        <label>Status code<input name="status_code" type="number" min="100" max="599" placeholder="500"></label>
        <label>Time range<select name="time_preset">
          <option value="">All time</option>
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
        <label>Min cost<input name="min_cost_usd" type="number" min="0" step="0.0001"></label>
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
      <form id="usage-export-form" class="inline-form">
        <select name="export_format"><option value="csv">CSV</option><option value="json">JSON</option></select>
        <select name="export_limit"><option value="1000">1,000</option><option value="100">100</option><option value="5000">5,000</option><option value="10000">10,000</option></select>
        <input name="export_offset" type="number" min="0" value="0" aria-label="Export offset">
        <button type="button" data-usage-export-action="preview">Preview</button>
        <button type="button" data-usage-export-action="download">Download</button>
        <button type="button" data-usage-export-action="copy-url">Copy URL</button>
        <button type="button" data-usage-export-action="copy-curl">Copy curl</button>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Task drilldown</h3></div>
      <form id="task-usage-form" class="inline-form">
        <input name="task_lookup" placeholder="task ID" required>
        <button>Load task usage</button>
      </form>
      <div id="task-usage-result"></div>
    </section>
    <section class="panel"><div class="panel-heading"><h3>Usage breakdown</h3></div><div id="usage-results"></div></section>
  `;
  document.querySelector("#usage-form").addEventListener("submit", handleAsync(applyUsageFilters));
  document.querySelector("#task-usage-form").addEventListener("submit", handleAsync(loadTaskUsage));
  document.querySelectorAll("[data-usage-export-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(usageExportAction));
  });
  await loadUsage();
}

async function loadUsage(event) {
  event?.preventDefault();
  const query = usageQueryFromForm(event?.target);
  const filterQuery = usageFilterValuesQueryFromForm(event?.target);
  const pageSize = usagePageSize();
  query.set("offset", String(state.usagePagination.eventsOffset));
  query.set("timeseries_limit", String(pageSize));
  query.set("timeseries_offset", String(state.usagePagination.timeseriesOffset));
  query.set("service_timeseries_limit", String(pageSize));
  query.set("service_timeseries_offset", String(state.usagePagination.serviceTimeseriesOffset));
  const [dashboard, events, routeOptions, endpointOptions, modelOptions] = await Promise.all([
    api(`/admin-ui/admin/usage/dashboard?${query}`),
    api(`/admin-ui/admin/usage/events?${query}`),
    api(`/admin-ui/admin/usage/filter-values?${filterQuery}&field=route`),
    api(`/admin-ui/admin/usage/filter-values?${filterQuery}&field=endpoint`),
    api(`/admin-ui/admin/usage/filter-values?${filterQuery}&field=model`),
  ]);
  const summary = dashboard.summary;
  updateUsageDatalists(routeOptions.values, endpointOptions.values, modelOptions.values);
  const results = document.querySelector("#usage-results");
  if (!results) return;
  results.innerHTML = `
    <div class="grid stats">
      ${stat("Requests", summary.request_count)}
      ${stat("Failures", summary.failure_count)}
      ${stat("Cost", money(summary.estimated_cost_usd))}
      ${stat("Avg latency", summary.average_latency_ms == null ? "n/a" : `${Math.round(summary.average_latency_ms)} ms`)}
      ${stat("Fallback rate", percent(summary.fallback_rate))}
      ${stat("Expensive", summary.expensive_request_count || 0)}
      ${stat("Guardrail blocks", summary.guardrail_block_count || 0)}
    </div>
    <h4>Projects</h4>${usageBreakdownTable(dashboard.breakdowns.projects, projectName)}
    <h4>Keys</h4>${usageBreakdownTable(dashboard.breakdowns.keys, keyName)}
    <h4>Services</h4>${usageBreakdownTable(dashboard.breakdowns.services)}
    <h4>Endpoints</h4>${usageBreakdownTable(dashboard.breakdowns.endpoints || [])}
    <h4>Providers</h4>${usageBreakdownTable(dashboard.breakdowns.providers)}
    <h4>Models</h4>${usageBreakdownTable(dashboard.breakdowns.models)}
    <h4>Tasks</h4>${usageBreakdownTable(dashboard.breakdowns.tasks)}
    ${usagePagedTable("Recent requests", "events", usageEventsTable(events.rows), events, events.rows.length)}
    ${usagePagedTable("Timeseries", "timeseries", usageTimeseriesTable(dashboard.timeseries), dashboard.timeseries_page, dashboard.timeseries.length)}
    ${usagePagedTable("Service timeseries", "service-timeseries", usageServiceTimeseriesTable(dashboard.service_timeseries || []), dashboard.service_timeseries_page, (dashboard.service_timeseries || []).length)}
    <h4>Unused keys</h4>${unusedKeysTable(dashboard.unused_keys)}
  `;
  results.querySelectorAll("[data-debug-request]").forEach((button) => {
    button.addEventListener("click", handleAsync(openDebugRequest));
  });
  results.querySelectorAll("[data-usage-page-section]").forEach((button) => {
    button.addEventListener("click", handleAsync(changeUsagePage));
  });
}

async function applyUsageFilters(event) {
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
  const pageSize = usagePageSize();
  const key = usagePaginationKey(section);
  if (!key) return;
  const current = state.usagePagination[key];
  state.usagePagination[key] = direction === "next" ? current + pageSize : Math.max(0, current - pageSize);
  await loadUsage();
}

function usagePaginationKey(section) {
  if (section === "events") return "eventsOffset";
  if (section === "timeseries") return "timeseriesOffset";
  if (section === "service-timeseries") return "serviceTimeseriesOffset";
  return "";
}

function usageQueryFromForm(formElement = document.querySelector("#usage-form")) {
  const form = formElement ? new FormData(formElement) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["project_id", "key_id", "service", "route", "method", "endpoint", "provider", "model", "task_id", "run_id", "trace_id", "status", "status_code", "min_cost_usd", "breakdown_limit", "sort_by", "limit"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  const range = usageDateRange(form);
  if (range.from) query.set("from", range.from);
  if (range.to) query.set("to", range.to);
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

function updateUsageDatalists(routes = [], endpoints = [], models = []) {
  const routeDefaults = ["/v1/chat/completions", "/v1/responses", "/v1/embeddings", "/v1/messages", "/summary", "/translation", "/ocr", "/embeddings", "/services/*", "/providers/openai/*"];
  const routeList = document.querySelector("#usage-route-options");
  const endpointList = document.querySelector("#usage-endpoint-options");
  const modelList = document.querySelector("#usage-model-options");
  if (routeList) routeList.innerHTML = [...new Set([...routeDefaults, ...routes])].map((value) => `<option value="${attr(value)}"></option>`).join("");
  if (endpointList) endpointList.innerHTML = [...new Set(endpoints)].map((value) => `<option value="${attr(value)}"></option>`).join("");
  if (modelList) modelList.innerHTML = [...new Set(models)].map((value) => `<option value="${attr(value)}"></option>`).join("");
}

function usageDateRange(form) {
  const preset = form.get("time_preset");
  if (!preset) return {};

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
  const path = usageExportPath(format);
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
    headers: { authorization: `Bearer ${token()}` },
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

function usageExportPath(format) {
  const query = usageQueryFromForm();
  const form = document.querySelector("#usage-export-form");
  if (form) {
    const data = new FormData(form);
    if (data.get("export_limit")) query.set("limit", data.get("export_limit"));
    if (data.get("export_offset")) query.set("offset", data.get("export_offset"));
  }
  return `/admin-ui/admin/usage/export.${format}?${query}`;
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

function usageEventsTable(rows) {
  return table(
    ["Created", "Request", "Route", "Service", "Method", "Endpoint", "Model", "Provider", "Status", "Latency", "Tokens", "Cost", "Cost source", "Pricing rule", "Trace", "Actions"],
    rows.map((row) => [
      time(row.created_at),
      `<code>${esc(row.request_id)}</code>`,
      esc(row.route),
      esc(row.service_name || ""),
      esc(row.http_method || ""),
      `<code>${esc(row.endpoint_template || row.endpoint_path || "")}</code>`,
      esc(row.model || ""),
      esc(row.provider),
      `${badge(row.status === "success" ? "good" : "bad", row.status)} <code>${esc(row.status_code)}</code>`,
      `${esc(row.latency_ms)} ms`,
      esc(row.total_tokens),
      money(row.estimated_cost_usd),
      esc(row.cost_source || ""),
      esc(row.pricing_rule_name || ""),
      row.trace_id ? `<code>${esc(row.trace_id)}</code>` : "",
      `<button type="button" data-nav="health" data-debug-request="${attr(row.request_id)}">Debug</button>`,
    ]),
  );
}

async function openDebugRequest(event) {
  const requestId = event.currentTarget.dataset.debugRequest;
  state.view = "health";
  await refresh();
  const input = document.querySelector("#debug-bundle-form input[name='request_id']");
  if (input) input.value = requestId;
  await loadDebugBundle({ preventDefault() {}, target: document.querySelector("#debug-bundle-form") });
}

function unusedKeysTable(rows) {
  return table(
    ["Key", "Project", "Created", "Last used"],
    rows.map((row) => [
      `<code>${esc(row.key_prefix)}</code>`,
      esc(projectName(row.project_id || "")),
      time(row.created_at),
      row.last_used_at ? time(row.last_used_at) : "never",
    ]),
  );
}

function usageTimeseriesTable(rows) {
  return table(
    ["Bucket", "Requests", "Success", "Failure", "Cost"],
    rows.map((row) => [
      esc(row.bucket_start || row.bucket || row.name),
      row.summary?.request_count ?? row.request_count ?? 0,
      row.summary?.success_count ?? row.success_count ?? 0,
      row.summary?.failure_count ?? row.failure_count ?? 0,
      money(row.summary?.estimated_cost_usd ?? row.estimated_cost_usd),
    ]),
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
  const [ready, rows, healthState, importVersions] = await Promise.all([
    json("/admin-ui/readyz"),
    api("/admin-ui/admin/provider-health"),
    api("/admin-ui/admin/provider-health/state"),
    api("/admin-ui/admin/services/import/versions"),
  ]);
  state.providerHealthState = healthState;
  state.serviceImportVersions = importVersions;
  const requestCount = rows.reduce((sum, row) => sum + row.request_count, 0);
  const errorCount = rows.reduce((sum, row) => sum + row.error_count, 0);
  const fallbackCount = rows.reduce((sum, row) => sum + row.fallback_count, 0);
  const errorRate = requestCount ? `${((errorCount / requestCount) * 100).toFixed(1)}%` : "0.0%";
  content.innerHTML = `
    <div class="grid stats">
      ${stat("Gateway", ready.status)}
      ${stat("Routes observed", rows.length)}
      ${stat("Error rate", errorRate)}
      ${stat("Fallbacks", fallbackCount)}
    </div>
    <section class="panel">
      <div class="panel-heading"><h3>Provider and service health</h3></div>
      ${healthTable(rows)}
    </section>
    <section class="panel">
      <div class="panel-heading">
        <h3>Health state</h3>
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
        <input name="request_id" placeholder="request ID" required>
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
  document.querySelectorAll("[data-import-rollback]").forEach((button) => {
    button.addEventListener("click", handleAsync(rollbackImportVersion));
  });
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
  if (row.timeout_count > 0) return '<span class="badge bad">timeout</span>';
  if (row.error_count > 0 || row.fallback_count > 0) return '<span class="badge bad">degraded</span>';
  return '<span class="badge good">healthy</span>';
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
  return `<div class="details">
    <p><strong>${esc(bundle.request_id)}</strong> ${esc(bundle.route ?? "")} ${esc(bundle.provider ?? "")}</p>
    <p class="subtle">Request hash ${esc(bundle.request_hash ?? "none")} · Response hash ${esc(bundle.response_hash ?? "none")}</p>
    <pre>${esc(JSON.stringify({
      policy_trace: bundle.policy_trace,
      guardrail_trace: bundle.guardrail_trace,
      selection_trace: bundle.selection_trace,
      fallback_history: bundle.fallback_history,
      upstream_latency_ms: bundle.upstream_latency_ms,
    }, null, 2))}</pre>
  </div>`;
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
  [state.guardrails, state.guardrailExecutions, state.guardrailSummary] = await Promise.all([
    api("/admin-ui/admin/guardrails"),
    api("/admin-ui/admin/guardrails/executions?limit=50"),
    api("/admin-ui/admin/guardrails/summary"),
  ]);
  const selected = state.guardrails.guardrails.find((guardrail) => guardrail.name === state.editingGuardrailName);
  content.innerHTML = `
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
      <div class="panel-heading"><h3>Summary</h3></div>
      ${guardrailSummaryTable(state.guardrailSummary.summary)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Recent executions</h3></div>
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
}

function guardrailCatalogTable(rows) {
  return table(
    ["Name", "Provider", "Modes", "Default", "Failure", "Enabled", "Endpoint", "Token", "Actions"],
    rows.map((row) => [
      `<code>${esc(row.name)}</code><div class="subtle">${esc(row.description)}</div>`,
      esc(row.provider_kind),
      esc(listValue(row.modes, "")),
      row.default_on ? '<span class="badge good">default</span>' : '<span class="badge">opt-in</span>',
      esc(row.failure_policy),
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
      <div class="field"><span>Modes</span>${guardrailModeSelect(guardrail?.modes || ["pre_call"])}</div>
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
      esc(row.mode),
      esc(row.action),
      esc(row.failure_policy),
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
      esc(row.mode),
      esc(row.action),
      `${esc(row.latency_ms)} ms`,
      esc(row.reason || ""),
    ]),
  );
}

function serviceBody(form, patch) {
  const body = {
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
  if (key.revoked_at) return '<span class="badge bad">revoked</span>';
  if (key.disabled) return '<span class="badge bad">disabled</span>';
  if (key.expires_at && new Date(key.expires_at) <= new Date()) return '<span class="badge bad">expired</span>';
  if (!key.expires_at) return '<span class="badge good">non-expiring</span>';
  return '<span class="badge good">active</span>';
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
    .map((key) => `<option value="${attr(key.id)}" ${key.id === selected ? "selected" : ""}>${esc(key.key_prefix)} (${esc(key.owner_type || "project")})</option>`)
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
  return state.keys.find((key) => key.id === keyId)?.key_prefix || keyId;
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

function option(value, selected) {
  return `<option value="${attr(value)}" ${value === selected ? "selected" : ""}>${esc(value)}</option>`;
}

function time(value) {
  return value ? new Date(value).toLocaleString() : "n/a";
}

function money(value) {
  return value == null ? "n/a" : `$${Number(value).toFixed(4)}`;
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

if (token()) {
  signedIn();
}
