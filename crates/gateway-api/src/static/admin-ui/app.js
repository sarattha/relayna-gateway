const tokenKey = "relayna_gateway_operator_token";
const state = {
  view: "overview",
  keys: [],
  openaiRoutes: [],
  services: [],
  editingKeyId: null,
  editingServiceName: null,
};

const login = document.querySelector("#login");
const app = document.querySelector("#app");
const content = document.querySelector("#content");
const title = document.querySelector("#view-title");
const notice = document.querySelector("#notice");
const requestTimeoutMs = 8000;

function token() {
  return sessionStorage.getItem(tokenKey);
}

function setNotice(message, kind = "error") {
  notice.textContent = message || "";
  notice.classList.toggle("hidden", !message);
  notice.dataset.kind = kind;
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
      message = body.error?.code || message;
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
  node.querySelector("[data-close-modal]").addEventListener("click", () => {
    document.querySelector(".modal-backdrop")?.remove();
  });
  document.body.appendChild(node);
}

function confirmAction(titleText, bodyText) {
  return new Promise((resolve) => {
    const backdrop = document.createElement("section");
    backdrop.className = "modal-backdrop";
    backdrop.innerHTML = `
      <div class="modal">
        <h3>${esc(titleText)}</h3>
        <p>${esc(bodyText)}</p>
        <div class="form-actions">
          <button class="danger" data-confirm-yes>Confirm</button>
          <button data-confirm-no>Cancel</button>
        </div>
      </div>
    `;
    const close = (value) => {
      backdrop.remove();
      resolve(value);
    };
    backdrop.querySelector("[data-confirm-yes]").addEventListener("click", () => close(true));
    backdrop.querySelector("[data-confirm-no]").addEventListener("click", () => close(false));
    backdrop.addEventListener("click", (event) => {
      if (event.target === backdrop) close(false);
    });
    document.body.appendChild(backdrop);
  });
}

function signedIn() {
  login.classList.add("hidden");
  app.classList.remove("hidden");
  refresh();
}

document.querySelector("#login-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const value = document.querySelector("#operator-token").value.trim();
  sessionStorage.setItem(tokenKey, value);
  try {
    await api("/admin/usage/summary");
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

document.querySelector("#rotate-token").addEventListener("click", async () => {
  if (!(await confirmAction("Rotate operator token", "The current token stops working."))) return;
  try {
    const body = await api("/admin/operator-token/rotate", { method: "POST", body: "{}" });
    sessionStorage.setItem(tokenKey, body.raw_token);
    showRawToken(body.raw_token, "Operator token shown once");
    setNotice("Operator token rotated. Store the new token now.", "success");
  } catch (error) {
    setNotice(error.message);
  }
});

document.querySelectorAll(".nav").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll(".nav").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    state.view = button.dataset.view;
    state.editingKeyId = null;
    state.editingServiceName = null;
    refresh();
  });
});

async function refresh() {
  setNotice("");
  title.textContent = state.view[0].toUpperCase() + state.view.slice(1);
  content.innerHTML = '<section class="panel"><div class="empty-state"><p>Loading...</p></div></section>';
  try {
    if (state.view === "overview") await overview();
    if (state.view === "keys") await keys();
    if (state.view === "routes") await routes();
    if (state.view === "services") await services();
    if (state.view === "usage") await usage();
    if (state.view === "health") await health();
  } catch (error) {
    setNotice(error.message);
    content.innerHTML = `<section class="panel"><div class="empty-state"><p>${esc(error.message)}</p></div></section>`;
  }
}

async function overview() {
  const [summary, healthRows, ready, keysRows, openaiRoutes, servicesRows] = await Promise.all([
    api("/admin/usage/summary"),
    api("/admin/provider-health"),
    json("/readyz"),
    api("/admin/keys"),
    api("/admin/openai-routes"),
    api("/admin/services"),
  ]);
  const activeKeys = keysRows.filter((key) => !key.disabled && !key.revoked_at).length;
  const enabledRoutes = openaiRoutes.filter((route) => route.enabled).length;
  const enabledServices = servicesRows.filter((service) => service.enabled).length;
  content.innerHTML = `
    <div class="grid stats">
      ${stat("Readiness", ready.status)}
      ${stat("Requests", summary.request_count)}
      ${stat("Active keys", activeKeys)}
      ${stat("OpenAI routes", `${enabledRoutes}/${openaiRoutes.length}`)}
      ${stat("Enabled services", enabledServices)}
      ${stat("Failures", summary.failure_count)}
      ${stat("Cost", money(summary.estimated_cost_usd))}
    </div>
    <section class="panel">
      <div class="panel-heading"><h3>Provider and service health</h3></div>
      ${healthTable(healthRows)}
    </section>
  `;
}

function stat(label, value) {
  return `<section class="panel stat"><span>${esc(label)}</span><strong>${esc(value)}</strong></section>`;
}

async function keys() {
  state.keys = await api("/admin/keys");
  const editing = state.keys.find((key) => key.id === state.editingKeyId);
  content.innerHTML = `
    <div class="split">
      <section class="panel">
        <div class="panel-heading">
          <h3>Create virtual key</h3>
        </div>
        <form id="key-form" class="form-grid">
          <label>Project ID<input name="project_id" required placeholder="uuid"></label>
          <label>Expires at<input name="expires_at" type="datetime-local"></label>
          ${policyFields()}
          <div class="form-actions">
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
        <h3>Virtual keys</h3>
        <span class="subtle">${state.keys.length} total</span>
      </div>
      ${keyTable(state.keys)}
    </section>
  `;
  document.querySelector("#key-form").addEventListener("submit", createKey);
  document.querySelector("#key-edit-form")?.addEventListener("submit", patchKey);
  document.querySelectorAll("[data-key-action]").forEach((button) => {
    button.addEventListener("click", keyAction);
  });
}

function policyFields(key = null) {
  const policy = key?.policy || {};
  return `
    <label>Routes<input name="allowed_routes" value="${attr(listValue(policy.allowed_routes, "/v1/chat/completions,/v1/responses"))}"></label>
    <label>Models<input name="allowed_models" value="${attr(listValue(policy.allowed_models, ""))}" placeholder="gpt-4o-mini"></label>
    <label>Providers<input name="allowed_providers" value="${attr(listValue(policy.allowed_providers, "litellm"))}"></label>
    <label>Services<input name="allowed_services" value="${attr(listValue(policy.allowed_services, ""))}" placeholder="summary,translation"></label>
    <label>RPM limit<input name="rpm_limit" type="number" min="0" value="${attr(policy.rpm_limit ?? "")}"></label>
    <label>TPM limit<input name="tpm_limit" type="number" min="0" value="${attr(policy.tpm_limit ?? "")}"></label>
    <label>Daily budget<input name="daily_budget_usd" type="number" min="0" step="0.01" value="${attr(policy.daily_budget_usd ?? "")}"></label>
    <label>Monthly budget<input name="monthly_budget_usd" type="number" min="0" step="0.01" value="${attr(policy.monthly_budget_usd ?? "")}"></label>
    <label class="check"><input name="allow_streaming" type="checkbox" ${policy.allow_streaming ? "checked" : ""}> Allow streaming</label>
    <label class="check"><input name="allow_tools" type="checkbox" ${policy.allow_tools ? "checked" : ""}> Allow tools</label>
  `;
}

function keyEditForm(key) {
  return `
    <div class="panel-heading">
      <h3>Edit virtual key</h3>
      <span class="subtle">${esc(key.key_prefix)}</span>
    </div>
    <form id="key-edit-form" class="form-grid" data-key-id="${attr(key.id)}">
      <label>Expires at<input name="expires_at" type="datetime-local" value="${attr(toLocalInput(key.expires_at))}"></label>
      <label class="check"><input name="clear_expires_at" type="checkbox"> Clear expiry</label>
      <label class="check"><input name="disabled" type="checkbox" ${key.disabled ? "checked" : ""}> Disabled</label>
      ${policyFields(key)}
      <div class="form-actions">
        <button type="submit" class="primary">Save changes</button>
        <button type="button" data-key-action="cancel-edit">Cancel</button>
      </div>
    </form>
  `;
}

function keyTable(rows) {
  return table(
    ["Prefix", "Project", "Status", "Policy", "Updated", "Actions"],
    rows.map((key) => [
      `<code>${esc(key.key_prefix)}</code>`,
      `<code>${esc(key.project_id)}</code>`,
      keyStatus(key),
      keyPolicySummary(key),
      time(key.updated_at),
      keyLifecycleActions(key),
    ]),
  );
}

function keyLifecycleActions(key) {
  const toggle = key.revoked_at
    ? ""
    : key.disabled
      ? `<button data-key-action="enable" data-key-id="${attr(key.id)}">Enable</button>`
      : `<button data-key-action="disable" data-key-id="${attr(key.id)}">Disable</button>`;
  return `<div class="actions">
        <button data-key-action="edit" data-key-id="${attr(key.id)}">Edit</button>
        <button data-key-action="usage" data-key-id="${attr(key.id)}">Usage</button>
        ${toggle}
        <button class="danger" data-key-action="revoke" data-key-id="${attr(key.id)}" ${key.revoked_at ? "disabled" : ""}>Revoke</button>
      </div>`;
}

async function createKey(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const body = {
    project_id: form.get("project_id"),
    expires_at: isoDate(form.get("expires_at")),
    policy: policyBody(form),
  };
  if (!body.expires_at) delete body.expires_at;
  const response = await api("/admin/keys", { method: "POST", body: JSON.stringify(body) });
  showRawToken(response.raw_key, "Virtual key shown once");
  state.editingKeyId = response.key.id;
  setNotice("Virtual key created.", "success");
  await keys();
}

async function patchKey(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const keyId = event.target.dataset.keyId;
  const body = {
    disabled: form.has("disabled"),
    policy: policyBody(form),
  };
  if (form.has("clear_expires_at")) {
    body.expires_at = null;
  } else if (form.get("expires_at")) {
    body.expires_at = isoDate(form.get("expires_at"));
  }
  await api(`/admin/keys/${keyId}`, { method: "PATCH", body: JSON.stringify(body) });
  setNotice("Virtual key updated.", "success");
  await keys();
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
    const summary = await api(`/admin/keys/${keyId}/usage`);
    setNotice(
      `Key usage: ${summary.request_count} requests, ${summary.failure_count} failures, ${money(summary.estimated_cost_usd)} cost.`,
      "success",
    );
    return;
  }
  if (!(await confirmAction(`${action} virtual key`, "This lifecycle change is written to the database."))) return;
  await api(`/admin/keys/${keyId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`Virtual key ${action}d.`, "success");
  await keys();
}

async function routes() {
  [state.openaiRoutes, state.services] = await Promise.all([
    api("/admin/openai-routes"),
    api("/admin/services"),
  ]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>OpenAI-compatible routes</h3><span class="subtle">${state.openaiRoutes.length} total</span></div>
      ${openaiRouteTable(state.openaiRoutes)}
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Registered service routes</h3><span class="subtle">${state.services.length} total</span></div>
      ${serviceRouteTable(state.services)}
    </section>
  `;
  document.querySelectorAll("[data-openai-route-action]").forEach((button) => {
    button.addEventListener("click", openaiRouteAction);
  });
}

function openaiRouteTable(rows) {
  return table(
    ["Route", "State", "Updated", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.route_id)}</strong><div class="subtle"><code>${esc(row.route)}</code></div>`,
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      time(row.updated_at),
      `<div class="actions">
        <button data-openai-route-action="${row.enabled ? "disable" : "enable"}" data-route-id="${attr(row.route_id)}">${row.enabled ? "Disable" : "Enable"}</button>
      </div>`,
    ]),
  );
}

function serviceRouteTable(rows) {
  return table(
    ["Service", "Route", "State", "Methods", "Upstream", "Credential"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.source)}</div>`,
      `<code>${esc(row.route_pattern)}</code>`,
      serviceBadges(row),
      esc(listValue(row.allowed_methods, "none")),
      esc(row.upstream_base_url || "missing"),
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
    ]),
  );
}

async function openaiRouteAction(event) {
  const { routeId, openaiRouteAction: action } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} ${routeId}`, "This gateway route change is written to the database."))) return;
  await api(`/admin/openai-routes/${routeId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`OpenAI route ${action}d.`, "success");
  await routes();
}

async function services() {
  state.services = await api("/admin/services");
  const editing = state.services.find((service) => service.name === state.editingServiceName);
  content.innerHTML = `
    <div class="split">
      <section class="panel">
        <div class="panel-heading"><h3>Create or import service</h3></div>
        <form id="service-form" class="form-grid">
          <label>Name<input name="name" required></label>
          <label>Studio service ID<input name="studio_service_id"></label>
          <label>Route pattern<input name="route_pattern" placeholder="/services/name/*"></label>
          <label>Upstream URL<input name="upstream_base_url"></label>
          <label>Credential<input name="credential" type="password" autocomplete="new-password"></label>
          <label>Methods<input name="allowed_methods" value="POST"></label>
          <label>Timeout ms<input name="timeout_ms" type="number" min="1" value="60000"></label>
          <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="2097152"></label>
          <label>Cost mode<select name="cost_mode"><option value="none">None</option><option value="fixed">Fixed</option><option value="passthrough">Passthrough</option></select></label>
          <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01"></label>
          <label>Fallback services<input name="fallback_services" placeholder="backup-a,backup-b"></label>
          <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
          <div class="form-actions">
            <button name="action" value="create" class="primary">Create</button>
            <button name="action" value="import">Import Studio</button>
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
  `;
  document.querySelector("#service-form").addEventListener("submit", submitService);
  document.querySelector("#service-edit-form")?.addEventListener("submit", patchService);
  document.querySelectorAll("[data-service-action]").forEach((button) => {
    button.addEventListener("click", serviceAction);
  });
}

function serviceEditForm(service) {
  return `
    <div class="panel-heading"><h3>Edit service</h3><span class="subtle">${esc(service.name)}</span></div>
    <form id="service-edit-form" class="form-grid" data-service-name="${attr(service.name)}">
      <label>Studio service ID<input name="studio_service_id" value="${attr(service.studio_service_id ?? "")}"></label>
      <label>Route pattern<input name="route_pattern" value="${attr(service.route_pattern)}"></label>
      <label>Upstream URL<input name="upstream_base_url" value="${attr(service.upstream_base_url ?? "")}"></label>
      <label>Credential<input name="credential" type="password" autocomplete="new-password" placeholder="${service.credential_configured ? "configured" : "missing"}"></label>
      <label>Methods<input name="allowed_methods" value="${attr(listValue(service.allowed_methods, "POST"))}"></label>
      <label>Timeout ms<input name="timeout_ms" type="number" min="1" value="${attr(service.timeout_ms)}"></label>
      <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="${attr(service.max_body_bytes)}"></label>
      <label>Cost mode<select name="cost_mode">${option("none", service.cost_mode)}${option("fixed", service.cost_mode)}${option("passthrough", service.cost_mode)}</select></label>
      <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01" value="${attr(service.estimated_cost_usd ?? "")}"></label>
      <label>Fallback services<input name="fallback_services" value="${attr(listValue(service.fallback_services, ""))}"></label>
      <label>Sync status<select name="sync_status">${["local", "synced", "incomplete", "stale", "failed"].map((value) => option(value, service.sync_status)).join("")}</select></label>
      <label class="check"><input name="enabled" type="checkbox" ${service.enabled ? "checked" : ""}> Enabled</label>
      <label class="check"><input name="clear_credential" type="checkbox"> Clear credential</label>
      <div class="form-actions">
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
    await api("/admin/services/import", {
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
    await api("/admin/services", {
      method: "POST",
      body: JSON.stringify(serviceBody(form, false)),
    });
  }
  setNotice("Service saved.", "success");
  await services();
}

async function patchService(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const serviceName = event.target.dataset.serviceName;
  await api(`/admin/services/${serviceName}`, {
    method: "PATCH",
    body: JSON.stringify(serviceBody(form, true)),
  });
  state.editingServiceName = null;
  setNotice("Service updated.", "success");
  await services();
}

async function serviceAction(event) {
  const { serviceName, serviceAction: action } = event.currentTarget.dataset;
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
    const body = await api(`/admin/services/${serviceName}/sync-status`);
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
    await api(`/admin/services/${serviceName}`, { method: "DELETE" });
  } else {
    await api(`/admin/services/${serviceName}/${action}`, { method: "POST", body: "{}" });
  }
  setNotice(`Service ${action}d.`, "success");
  await services();
}

function serviceTable(rows) {
  return table(
    ["Name", "State", "Route", "Upstream", "Credential", "Cost", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.source)}</div>`,
      serviceBadges(row),
      `<code>${esc(row.route_pattern)}</code>`,
      esc(row.upstream_base_url || "missing"),
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
      `${esc(row.cost_mode)} ${row.estimated_cost_usd == null ? "" : money(row.estimated_cost_usd)}`,
      `<div class="actions">
        <button data-service-action="edit" data-service-name="${attr(row.name)}">Edit</button>
        <button data-service-action="sync-status" data-service-name="${attr(row.name)}">Status</button>
        <button data-service-action="${row.enabled ? "disable" : "enable"}" data-service-name="${attr(row.name)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-service-action="delete" data-service-name="${attr(row.name)}">Delete</button>
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

async function usage() {
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Usage filters</h3></div>
      <form id="usage-form" class="form-grid">
        <label>Service<input name="service"></label>
        <label>Provider<input name="provider"></label>
        <label>Model<input name="model"></label>
        <label>Task<input name="task_id"></label>
        <div class="form-actions"><button class="primary">Apply</button></div>
      </form>
    </section>
    <section class="panel"><div class="panel-heading"><h3>Usage by service</h3></div><div id="usage-results"></div></section>
  `;
  document.querySelector("#usage-form").addEventListener("submit", loadUsage);
  await loadUsage();
}

async function loadUsage(event) {
  event?.preventDefault();
  const form = event ? new FormData(event.target) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["service", "provider", "model", "task_id"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  const rows = await api(`/admin/usage/by-service?${query}`);
  document.querySelector("#usage-results").innerHTML = table(
    ["Name", "Requests", "Success", "Failure", "Latency", "Cost"],
    rows.map((row) => [
      esc(row.name),
      row.summary.request_count,
      row.summary.success_count,
      row.summary.failure_count,
      `${row.summary.total_latency_ms} ms`,
      money(row.summary.estimated_cost_usd),
    ]),
  );
}

async function health() {
  const [ready, rows] = await Promise.all([
    json("/readyz"),
    api("/admin/provider-health"),
  ]);
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
  `;
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
      row.fallback_count,
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

function table(headers, rows) {
  if (!rows.length) return '<div class="empty-state"><p>No rows.</p></div>';
  return `<div class="table-wrap"><table><thead><tr>${headers.map((h) => `<th>${esc(h)}</th>`).join("")}</tr></thead><tbody>${rows
    .map((row) => `<tr>${row.map((cell) => `<td>${cell ?? ""}</td>`).join("")}</tr>`)
    .join("")}</tbody></table></div>`;
}

function policyBody(form) {
  const body = {
    allowed_routes: csv(form.get("allowed_routes")),
    allowed_models: csv(form.get("allowed_models")),
    allowed_providers: csv(form.get("allowed_providers")),
    allowed_services: csv(form.get("allowed_services")),
    rpm_limit: nullableNumber(form.get("rpm_limit")),
    tpm_limit: nullableNumber(form.get("tpm_limit")),
    daily_budget_usd: nullableNumber(form.get("daily_budget_usd")),
    monthly_budget_usd: nullableNumber(form.get("monthly_budget_usd")),
    allow_streaming: form.has("allow_streaming"),
    allow_tools: form.has("allow_tools"),
  };
  return body;
}

function serviceBody(form, patch) {
  const body = {
    studio_service_id: patch ? nullableString(form.get("studio_service_id")) : blankToUndefined(form.get("studio_service_id")),
    route_pattern: form.get("route_pattern") || undefined,
    upstream_base_url: patch ? nullableString(form.get("upstream_base_url")) : blankToUndefined(form.get("upstream_base_url")),
    enabled: form.has("enabled"),
    allowed_methods: csv(form.get("allowed_methods")),
    timeout_ms: Number(form.get("timeout_ms")),
    max_body_bytes: Number(form.get("max_body_bytes")),
    cost_mode: form.get("cost_mode"),
    estimated_cost_usd: nullableNumber(form.get("estimated_cost_usd")),
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

function keyStatus(key) {
  if (key.revoked_at) return '<span class="badge bad">revoked</span>';
  if (key.disabled) return '<span class="badge bad">disabled</span>';
  if (key.expires_at && new Date(key.expires_at) <= new Date()) return '<span class="badge bad">expired</span>';
  return '<span class="badge good">active</span>';
}

function keyPolicySummary(key) {
  const policy = key.policy;
  return `<div>${esc((policy.allowed_routes || []).join(", ") || "no routes")}</div>
    <div class="subtle">${esc((policy.allowed_providers || []).join(", ") || "no providers")}</div>
    <div class="subtle">RPM ${esc(policy.rpm_limit ?? "none")} / daily ${esc(money(policy.daily_budget_usd))}</div>`;
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

function blankToUndefined(value) {
  return value === null || String(value).trim() === "" ? undefined : String(value).trim();
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

function option(value, selected) {
  return `<option value="${attr(value)}" ${value === selected ? "selected" : ""}>${esc(value)}</option>`;
}

function time(value) {
  return value ? new Date(value).toLocaleString() : "n/a";
}

function money(value) {
  return value == null ? "n/a" : `$${Number(value).toFixed(4)}`;
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
