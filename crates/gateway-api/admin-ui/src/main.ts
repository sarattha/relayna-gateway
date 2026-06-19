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
} from "./design-system";

const tokenKey = "relayna_gateway_operator_token";
const state = {
  view: "overview",
  keys: [],
  projects: [],
  providers: [],
  litellmCredentialMappings: [],
  litellmPassthroughSettings: null,
  openaiRoutes: [],
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
  editingGuardrailName: null,
};

const login = document.querySelector("#login");
const app = document.querySelector("#app");
const content = document.querySelector("#content");
const requestTimeoutMs = 8000;
let noticeTimer: ReturnType<typeof setTimeout> | null = null;

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
    try {
      await handler(event);
    } catch (error) {
      setNotice(error.message);
    }
  };
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

function showTextModal(titleText, value) {
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  backdrop.innerHTML = `
    <div class="modal wide">
      <h3>${esc(titleText)}</h3>
      <textarea readonly rows="18">${esc(value)}</textarea>
      ${actionGroup('<button type="button" data-close-modal>Close</button>')}
    </div>
  `;
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => backdrop.remove());
  backdrop.addEventListener("click", (event) => {
    if (event.target === backdrop) backdrop.remove();
  });
  document.body.appendChild(backdrop);
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
  button.addEventListener("click", () => {
    document.querySelectorAll(".nav").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    state.view = button.dataset.view;
    state.editingKeyId = null;
    state.editingServiceName = null;
    state.editingGuardrailName = null;
    refresh();
  });
});

async function refresh() {
  setNotice("");
  applyViewChrome(state.view);
  content.innerHTML = panel("", emptyState("Loading..."));
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
  } catch (error) {
    setNotice(error.message);
    content.innerHTML = `<section class="panel"><div class="empty-state"><p>${esc(error.message)}</p></div></section>`;
  }
}

async function overview() {
  const [summary, healthRows, ready, keysRows, openaiRoutes, servicesRows] = await Promise.all([
    api("/admin-ui/admin/usage/summary"),
    api("/admin-ui/admin/provider-health"),
    json("/admin-ui/readyz"),
    api("/admin-ui/admin/keys"),
    api("/admin-ui/admin/openai-routes"),
    api("/admin-ui/admin/services"),
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
        <button data-project-action="usage" data-project-id="${attr(row.id)}">Usage</button>
        <button class="danger" data-project-action="delete" data-project-id="${attr(row.id)}">Delete</button>
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
          ${policyFields()}
          ${guardrailPolicyFields()}
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
        <h3>Inherited policy layers</h3>
        <span class="subtle">${state.policyLayers.length} configured</span>
      </div>
      <form id="policy-layer-form" class="form-grid">
        <label>Layer<select name="kind">
          <option value="global">Global</option>
          <option value="project">Project</option>
          <option value="team">Team</option>
          <option value="route">Route</option>
          <option value="model">Model</option>
        </select></label>
        <label>Scope<input name="scope_id" placeholder="project UUID, team, route, or model"></label>
        ${policyFields(null, true)}
        ${guardrailPolicyFields()}
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
      ${keyOwnershipFields(key)}
      <label>Expires at<input name="expires_at" type="datetime-local" value="${attr(toLocalInput(key.expires_at))}"></label>
      <label>Rotation due<input name="rotation_due_at" type="datetime-local" value="${attr(toLocalInput(key.rotation_due_at))}"></label>
      <label class="check"><input name="no_expires_at" type="checkbox" ${key.expires_at ? "" : "checked"}> No expiration</label>
      <label class="check"><input name="disabled" type="checkbox" ${key.disabled ? "checked" : ""}> Disabled</label>
      ${policyFields(key)}
      ${guardrailPolicyFields(key)}
      <div class="form-actions">
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
  if (path.trim() === "/services/*") {
    setNotice("Use a concrete service path such as /services/service-name/test.");
    return;
  }
  const serviceName = serviceMode ? form.get("service_name") || null : null;
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
  state.policySimulation = await api("/admin-ui/admin/policy/simulate", { method: "POST", body: JSON.stringify(body) });
  document.querySelector("#policy-sim-result").innerHTML = policySimulationResult();
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
        <label>Provider<select name="provider">${option("litellm", "litellm")}${option("internal-service", "")}</select></label>
        <label>Name<input name="name" required value="LiteLLM"></label>
        <label>Endpoint<input name="base_url" required placeholder="http://litellm:4000"></label>
        <label>Default credential<input name="credential" type="password" autocomplete="new-password"></label>
        <label>Credential mode<select name="credential_header_mode">
          ${option("authorization_bearer", "authorization_bearer")}
          ${option("custom_header", "")}
        </select></label>
        <label>Custom header<input name="credential_header_name" placeholder="x-litellm-api-key"></label>
        <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
        <div class="form-actions"><button class="primary">Create provider</button></div>
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
    <label class="check"><input name="enabled" type="checkbox" ${current.enabled ? "checked" : ""}> Enable wildcard passthrough</label>
    <label>Allowed paths<input name="allowed_paths" value="${attr(listValue(current.allowed_paths, "/v1/*"))}"></label>
    <label>Allowed methods<input name="allowed_methods" value="${attr(listValue(current.allowed_methods, "GET,POST"))}"></label>
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
    <div class="notice warn"><strong>Exposure risk</strong><span>/ui, key, config, user/team, spend, and other LiteLLM admin endpoints stay blocked unless explicitly exposed.</span></div>
    <div class="form-actions"><button class="primary">Save passthrough settings</button></div>
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
        <button data-provider-action="${row.enabled ? "disable" : "enable"}" data-provider-id="${attr(row.id)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-provider-action="delete" data-provider-id="${attr(row.id)}">Delete</button>
      </div>`,
    ]),
  );
}

function providerAuthSettingsForm(row) {
  if (row.provider !== "litellm") {
    return '<span class="subtle">not applicable</span>';
  }
  return `<form class="inline-form" data-provider-config-form data-provider-id="${attr(row.id)}">
    <select name="credential_header_mode">
      ${option("authorization_bearer", row.credential_header_mode || "authorization_bearer")}
      ${option("custom_header", row.credential_header_mode || "")}
    </select>
    <input name="credential_header_name" placeholder="x-litellm-api-key" value="${attr(row.credential_header_name || "")}">
    <input name="credential" type="password" autocomplete="new-password" placeholder="rotate default credential">
    <button type="submit">Update</button>
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
        <button data-litellm-mapping-action="${row.enabled ? "disable" : "enable"}" data-mapping-id="${attr(row.id)}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-litellm-mapping-action="delete" data-mapping-id="${attr(row.id)}">Delete</button>
      </div>`,
    ]),
  );
}

async function createProvider(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const credentialHeaderMode = form.get("credential_header_mode");
  const credentialHeaderName = nullableString(form.get("credential_header_name"));
  await api("/admin-ui/admin/providers", {
    method: "POST",
    body: JSON.stringify({
      provider: form.get("provider"),
      name: form.get("name"),
      base_url: form.get("base_url"),
      credential: blankToUndefined(form.get("credential")),
      credential_header_mode: credentialHeaderMode,
      credential_header_name: credentialHeaderMode === "custom_header" ? credentialHeaderName : null,
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
  [state.openaiRoutes, state.services] = await Promise.all([
    api("/admin-ui/admin/openai-routes"),
    api("/admin-ui/admin/services"),
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
    button.addEventListener("click", handleAsync(openaiRouteAction));
  });
  document.querySelectorAll("[data-openai-route-mode-form]").forEach((form) => {
    form.addEventListener("submit", handleAsync(saveOpenAiRouteMode));
  });
}

function openaiRouteTable(rows) {
  return table(
    ["Route", "State", "Mode", "Updated", "Actions"],
    rows.map((row) => [
      `<strong>${esc(row.route_id)}</strong><div class="subtle"><code>${esc(row.route)}</code></div>`,
      row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>',
      openaiRouteModeForm(row),
      time(row.updated_at),
      `<div class="actions">
        <button data-openai-route-action="${row.enabled ? "disable" : "enable"}" data-route-id="${attr(row.route_id)}">${row.enabled ? "Disable" : "Enable"}</button>
      </div>`,
    ]),
  );
}

function openaiRouteModeForm(row) {
  return `<form class="inline-form" data-openai-route-mode-form data-route-id="${attr(row.route_id)}">
    <select name="mode">
      ${option("managed_by_gateway", row.mode || "managed_by_gateway")}
      ${option("direct_litellm_passthrough", row.mode || "")}
    </select>
    <button type="submit">Save</button>
  </form>`;
}

function serviceRouteTable(rows) {
  return table(
    ["Service", "Route", "State", "Methods", "Upstream", "Health check", "Credential"],
    rows.map((row) => [
      `<strong>${esc(row.name)}</strong><div class="subtle">${esc(row.source)}</div>`,
      `<code>${esc(row.route_pattern)}</code>`,
      serviceBadges(row),
      esc(listValue(row.allowed_methods, "none")),
      esc(row.upstream_base_url || "missing"),
      esc(healthCheckLabel(row)),
      row.credential_configured ? '<span class="badge good">configured</span>' : '<span class="badge bad">missing</span>',
    ]),
  );
}

async function openaiRouteAction(event) {
  const { routeId, openaiRouteAction: action } = event.currentTarget.dataset;
  if (!(await confirmAction(`${action} ${routeId}`, "This gateway route change is written to the database."))) return;
  await api(`/admin-ui/admin/openai-routes/${routeId}/${action}`, { method: "POST", body: "{}" });
  setNotice(`OpenAI route ${action}d.`, "success");
  await routes();
}

async function saveOpenAiRouteMode(event) {
  event.preventDefault();
  const formElement = event.currentTarget;
  const routeId = formElement.dataset.routeId;
  const form = new FormData(formElement);
  await api(`/admin-ui/admin/openai-routes/${routeId}/mode`, {
    method: "PATCH",
    body: JSON.stringify({ mode: form.get("mode") }),
  });
  setNotice("OpenAI route mode updated.", "success");
  await routes();
}

async function services() {
  [state.services, state.projects] = await Promise.all([api("/admin-ui/admin/services"), api("/admin-ui/admin/projects")]);
  const editing = state.services.find((service) => service.name === state.editingServiceName);
  content.innerHTML = `
    <div class="split">
      <section class="panel">
        <div class="panel-heading">
          <h3>Create service</h3>
          <button type="button" data-service-action="studio-import">Import from Studio</button>
        </div>
        <form id="service-form" class="form-grid">
          <label>Name<input name="name" required pattern="[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?" placeholder="temp-service-2" title="Use lowercase letters, numbers, and hyphens; start and end with a letter or number."></label>
          <label>Route pattern<input name="route_pattern" list="service-routes" placeholder="/services/name/*"></label>
          <label>Upstream URL<input name="upstream_base_url"></label>
          <label>Health path<input name="health_check_path" placeholder="/health"></label>
          <label>Health method<select name="health_check_method"><option value="GET">GET</option><option value="HEAD">HEAD</option></select></label>
          <label>Credential<input name="credential" type="password" autocomplete="new-password"></label>
          <div class="field"><span>Methods</span>${methodSelect(["POST"])}</div>
          <label>Timeout ms<input name="timeout_ms" type="number" min="1" value="60000"></label>
          <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="2097152"></label>
          <label>Cost mode<select name="cost_mode"><option value="none">None</option><option value="fixed">Fixed</option><option value="passthrough">Passthrough</option></select></label>
          <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01"></label>
          <div class="help">Fixed records the configured estimate per request. Passthrough records provider-reported response cost when the upstream returns one.</div>
          <label>Fallback services<input name="fallback_services" placeholder="backup-a,backup-b"></label>
          <label class="check"><input name="enabled" type="checkbox" checked> Enabled</label>
          <div class="form-actions">
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
  document.querySelectorAll("[data-service-action]").forEach((button) => {
    button.addEventListener("click", handleAsync(serviceAction));
  });
}

function serviceEditForm(service) {
  return `
    <div class="panel-heading"><h3>Edit service</h3><span class="subtle">${esc(service.name)}</span></div>
    <form id="service-edit-form" class="form-grid" data-service-name="${attr(service.name)}">
      <label>Studio service ID<input name="studio_service_id" value="${attr(service.studio_service_id ?? "")}"></label>
      <label>Route pattern<input name="route_pattern" list="service-routes" value="${attr(service.route_pattern)}"></label>
      <label>Upstream URL<input name="upstream_base_url" value="${attr(service.upstream_base_url ?? "")}"></label>
      <label>Health path<input name="health_check_path" value="${attr(service.health_check_path ?? "")}" placeholder="/health"></label>
      <label>Health method<select name="health_check_method">${["GET", "HEAD"].map((value) => option(value, service.health_check_method || "GET")).join("")}</select></label>
      <label>Credential<input name="credential" type="password" autocomplete="new-password" placeholder="${service.credential_configured ? "configured" : "missing"}"></label>
      <div class="field"><span>Methods</span>${methodSelect(service.allowed_methods)}</div>
      <label>Timeout ms<input name="timeout_ms" type="number" min="1" value="${attr(service.timeout_ms)}"></label>
      <label>Max body bytes<input name="max_body_bytes" type="number" min="1" value="${attr(service.max_body_bytes)}"></label>
      <label>Cost mode<select name="cost_mode">${option("none", service.cost_mode)}${option("fixed", service.cost_mode)}${option("passthrough", service.cost_mode)}</select></label>
      <label>Estimated cost<input name="estimated_cost_usd" type="number" min="0" step="0.01" value="${attr(service.estimated_cost_usd ?? "")}"></label>
      <div class="help">Fixed uses the estimate configured here. Passthrough uses provider response cost fields such as usage.total_cost.</div>
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
    backdrop.innerHTML = `
      <div class="modal wide">
        <h3>Import from Studio</h3>
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
    backdrop.querySelector("[data-close-modal]").addEventListener("click", () => backdrop.remove());
    backdrop.addEventListener("click", (event) => {
      if (event.target === backdrop) backdrop.remove();
    });
    backdrop.querySelector("#studio-import-form").addEventListener("submit", handleAsync(importSelectedStudioServices));
    backdrop.querySelector("[data-import-preview]").addEventListener("click", handleAsync(previewSelectedStudioServices));
    backdrop.querySelector("[data-import-sync]").addEventListener("click", handleAsync(syncSelectedStudioServices));
    document.body.appendChild(backdrop);
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
        <label>Base URL<input name="base_url" type="url" placeholder="http://127.0.0.1:8000" value="${attr(state.studioConnection.base_url || "")}"></label>
        <label>Bearer token<input name="token" type="password" autocomplete="new-password" placeholder="${state.studioConnection.token_configured ? "Leave blank to keep current token" : "Optional"}"></label>
        <div class="form-actions">
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
        <label class="check"><input name="entra_enabled" type="checkbox" ${state.authSettings.entra.enabled ? "checked" : ""}> Enable Entra ID</label>
        <label class="check"><input name="apigee_trusted_header_enabled" type="checkbox" ${state.authSettings.apigee.trusted_header_enabled ? "checked" : ""}> Enable Apigee trusted headers</label>
        <label>Relayna key header<input name="relayna_key_header" value="${attr(state.authSettings.relayna_key_header || "X-Relayna-Key")}"></label>
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
        <label>Apigee secret<input name="apigee_trusted_header_secret" type="password" autocomplete="new-password" placeholder="${apigeeSecretPlaceholder()}"></label>
        <div class="form-actions wide-field">
          <button class="primary">Save auth settings</button>
          <button type="button" data-auth-action="clear-apigee-secret">Clear Apigee secret</button>
        </div>
      </form>
    </section>
    <section class="panel">
      <div class="panel-heading"><h3>Security and release posture</h3><span class="subtle">Static operator references</span></div>
      <div class="kv">
        <div><strong>Freeze baseline</strong><span>${badge("v0.1.0")}</span></div>
        <div><strong>Admin contracts</strong><span>Preserve <code>/admin-ui</code> and <code>/admin-ui/admin/*</code> unless an implementation strategy changes the boundary.</span></div>
        <div><strong>Supply-chain exceptions</strong><span><a href="https://github.com/sarattha/relayna-gateway/blob/main/docs/security-exceptions.md" target="_blank" rel="noreferrer">docs/security-exceptions.md</a></span></div>
        <div><strong>Release guard</strong><span><a href="https://github.com/sarattha/relayna-gateway/blob/main/tests/freeze-v0.1.0-perimeter.test.mjs" target="_blank" rel="noreferrer">freeze perimeter test</a></span></div>
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
  backdrop.innerHTML = `
    <div class="modal wide">
      <h3>${esc(trigger.dataset.servicePickerTitle || "Select services")}</h3>
      <form id="service-picker-form" class="modal-form">
        <div class="modal-scroll">${servicePickerTable(state.services, selected)}</div>
        <div class="form-actions">
          <button class="primary" ${state.services.length ? "" : "disabled"}>Apply selection</button>
          <button type="button" data-close-modal>Cancel</button>
        </div>
      </form>
    </div>
  `;
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => backdrop.remove());
  backdrop.addEventListener("click", (event) => {
    if (event.target === backdrop) backdrop.remove();
  });
  backdrop.querySelector("#service-picker-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const values = new FormData(event.target).getAll("service_name");
    setSelectedServiceNames(form, fieldName, values);
    backdrop.remove();
  });
  document.body.appendChild(backdrop);
}

function openGuardrailSelectionPicker(trigger) {
  const form = trigger.closest("form");
  const fieldName = trigger.dataset.guardrailPicker;
  const selected = new Set(selectedServiceNames(form, fieldName));
  const rows = state.guardrails?.guardrails || [];
  const backdrop = document.createElement("section");
  backdrop.className = "modal-backdrop";
  backdrop.innerHTML = `
    <div class="modal wide">
      <h3>${esc(trigger.dataset.guardrailPickerTitle || "Select guardrails")}</h3>
      <form id="guardrail-picker-form" class="modal-form">
        <div class="modal-scroll">${guardrailPickerTable(rows, selected)}</div>
        <div class="form-actions">
          <button class="primary" ${rows.length ? "" : "disabled"}>Apply selection</button>
          <button type="button" data-close-modal>Cancel</button>
        </div>
      </form>
    </div>
  `;
  backdrop.querySelector("[data-close-modal]").addEventListener("click", () => backdrop.remove());
  backdrop.addEventListener("click", (event) => {
    if (event.target === backdrop) backdrop.remove();
  });
  backdrop.querySelector("#guardrail-picker-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const values = new FormData(event.target).getAll("guardrail_name");
    setSelectedServiceNames(form, fieldName, values);
    updateGuardrailOverrideControls(form);
    backdrop.remove();
  });
  document.body.appendChild(backdrop);
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
  document.querySelector(".modal-backdrop")?.remove();
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
  document.querySelector(".modal-backdrop")?.remove();
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

function healthCheckLabel(row) {
  return row.health_check_path ? `${row.health_check_method || "GET"} ${row.health_check_path}` : "upstream root";
}

async function usage() {
  [state.projects, state.services, state.keys] = await Promise.all([api("/admin-ui/admin/projects"), api("/admin-ui/admin/services"), api("/admin-ui/admin/keys")]);
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Usage filters</h3></div>
      <form id="usage-form" class="form-grid">
        <label>Project<select name="project_id"><option value="">All</option>${projectOptions()}</select></label>
        <label>Virtual key<select name="key_id"><option value="">All</option>${keyOptions()}</select></label>
        <label>Service<select name="service"><option value="">All</option>${serviceOptions()}</select></label>
        <label>Route<input name="route"></label>
        <label>Provider<input name="provider"></label>
        <label>Model<input name="model"></label>
        <label>Task<input name="task_id"></label>
        <label>Run<input name="run_id"></label>
        <label>Trace<input name="trace_id"></label>
        <label>Status<select name="status"><option value="">All</option><option value="success">Success</option><option value="failure">Failure</option></select></label>
        <label>Min cost<input name="min_cost_usd" type="number" min="0" step="0.0001"></label>
        <div class="form-actions">
          <button class="primary">Apply</button>
          <button type="button" data-usage-export="json">Export JSON</button>
          <button type="button" data-usage-export="csv">Export CSV</button>
        </div>
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
  document.querySelector("#usage-form").addEventListener("submit", handleAsync(loadUsage));
  document.querySelector("#task-usage-form").addEventListener("submit", handleAsync(loadTaskUsage));
  document.querySelectorAll("[data-usage-export]").forEach((button) => {
    button.addEventListener("click", handleAsync(loadUsageExport));
  });
  await loadUsage();
}

async function loadUsage(event) {
  event?.preventDefault();
  const query = usageQueryFromForm(event?.target);
  const [summary, projectRows, keyRows, serviceRows, providerRows, modelRows, taskRows, timeseriesRows, unusedKeys] = await Promise.all([
    api(`/admin-ui/admin/usage/summary?${query}`),
    api(`/admin-ui/admin/usage/by-project?${query}`),
    api(`/admin-ui/admin/usage/by-key?${query}`),
    api(`/admin-ui/admin/usage/by-service?${query}`),
    api(`/admin-ui/admin/usage/by-provider?${query}`),
    api(`/admin-ui/admin/usage/by-model?${query}`),
    api(`/admin-ui/admin/usage/by-task?${query}`),
    api(`/admin-ui/admin/usage/timeseries?${query}`),
    api(`/admin-ui/admin/usage/unused-keys?${query}`),
  ]);
  const results = document.querySelector("#usage-results");
  if (!results) return;
  results.innerHTML = `
    <div class="grid stats">
      ${stat("Requests", summary.request_count)}
      ${stat("Failures", summary.failure_count)}
      ${stat("Cost", money(summary.estimated_cost_usd))}
      ${stat("Fallback rate", percent(summary.fallback_rate))}
      ${stat("Expensive", summary.expensive_request_count || 0)}
      ${stat("Guardrail blocks", summary.guardrail_block_count || 0)}
    </div>
    <h4>Projects</h4>${usageBreakdownTable(projectRows, projectName)}
    <h4>Keys</h4>${usageBreakdownTable(keyRows, keyName)}
    <h4>Services</h4>${usageBreakdownTable(serviceRows)}
    <h4>Providers</h4>${usageBreakdownTable(providerRows)}
    <h4>Models</h4>${usageBreakdownTable(modelRows)}
    <h4>Tasks</h4>${usageBreakdownTable(taskRows)}
    <h4>Timeseries</h4>${usageTimeseriesTable(timeseriesRows)}
    <h4>Unused keys</h4>${unusedKeysTable(unusedKeys)}
  `;
}

function usageQueryFromForm(formElement = document.querySelector("#usage-form")) {
  const form = formElement ? new FormData(formElement) : new FormData();
  const query = new URLSearchParams();
  for (const key of ["project_id", "key_id", "service", "route", "provider", "model", "task_id", "run_id", "trace_id", "status", "min_cost_usd"]) {
    const value = form.get(key);
    if (value) query.set(key, value);
  }
  return query;
}

async function loadUsageExport(event) {
  const format = event.currentTarget.dataset.usageExport;
  const query = usageQueryFromForm();
  if (format === "json") {
    const body = await api(`/admin-ui/admin/usage/export.json?${query}`);
    showTextModal("Usage export JSON", JSON.stringify(body, null, 2));
    return;
  }
  const response = await fetchWithTimeout(`/admin-ui/admin/usage/export.csv?${query}`, {
    headers: { authorization: `Bearer ${token()}` },
  });
  if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
  showTextModal("Usage export CSV", await response.text());
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
    ["Name", "Requests", "Success", "Failure", "Latency", "Cost"],
    rows.map((row) => [
      esc(label(row.name)),
      row.summary.request_count,
      row.summary.success_count,
      row.summary.failure_count,
      `${row.summary.total_latency_ms} ms`,
      money(row.summary.estimated_cost_usd),
    ]),
  );
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
  state.debugBundle = await api(`/admin-ui/admin/debug-bundles/${encodeURIComponent(requestId)}`);
  await health();
}

async function rollbackImportVersion(event) {
  const version = event.currentTarget.dataset.importRollback;
  if (!(await confirmAction(`Rollback import ${version}`, "This activates the stored service registry snapshot."))) return;
  await api(`/admin-ui/admin/services/import/rollback/${version}`, { method: "POST", body: "{}" });
  setNotice(`Service registry rolled back to ${version}.`, "success");
  await health();
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
  return `<div class="kv">
    <div><strong>Decision</strong><span>${badge(decision.allowed ? "allowed" : decision.error_code || "denied", decision.allowed ? "good" : "bad")}</span></div>
    <div><strong>Matched route</strong><span>${esc(result.route_match?.route || "")}</span></div>
    <div><strong>Provider</strong><span>${esc(result.route_match?.provider || "")}</span></div>
    <div><strong>Service</strong><span>${esc(result.route_match?.service_name || "none")}</span></div>
    <div><strong>Policy version</strong><span>${esc(result.policy_merge?.policy_version ?? "n/a")}</span></div>
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
