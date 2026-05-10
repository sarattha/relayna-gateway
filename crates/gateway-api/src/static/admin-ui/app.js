const tokenKey = "relayna_gateway_operator_token";
const state = {
  view: "overview",
  services: [],
};

const login = document.querySelector("#login");
const app = document.querySelector("#app");
const content = document.querySelector("#content");
const title = document.querySelector("#view-title");
const notice = document.querySelector("#notice");

function token() {
  return sessionStorage.getItem(tokenKey);
}

function setNotice(message) {
  notice.textContent = message || "";
  notice.classList.toggle("hidden", !message);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
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
  if (response.status === 204) {
    return null;
  }
  return response.json();
}

function showRawToken(rawToken) {
  const template = document.querySelector("#raw-token-template");
  const node = template.content.cloneNode(true);
  node.querySelector("textarea").value = rawToken;
  node.querySelector("[data-close-modal]").addEventListener("click", () => {
    document.querySelector(".modal-backdrop")?.remove();
  });
  document.body.appendChild(node);
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
  if (!confirm("Rotate the operator token? The current token stops working.")) return;
  try {
    const body = await api("/admin/operator-token/rotate", { method: "POST", body: "{}" });
    sessionStorage.setItem(tokenKey, body.raw_token);
    showRawToken(body.raw_token);
    setNotice("Operator token rotated. Store the new token now.");
  } catch (error) {
    setNotice(error.message);
  }
});

document.querySelectorAll(".nav").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll(".nav").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    state.view = button.dataset.view;
    refresh();
  });
});

async function refresh() {
  setNotice("");
  title.textContent = state.view[0].toUpperCase() + state.view.slice(1);
  try {
    if (state.view === "overview") await overview();
    if (state.view === "keys") await keys();
    if (state.view === "services") await services();
    if (state.view === "usage") await usage();
    if (state.view === "health") await health();
  } catch (error) {
    setNotice(error.message);
  }
}

async function overview() {
  const [summary, healthRows, ready] = await Promise.all([
    api("/admin/usage/summary"),
    api("/admin/provider-health"),
    fetch("/readyz").then((response) => response.json()),
  ]);
  content.innerHTML = `
    <div class="grid stats">
      ${stat("Readiness", ready.status)}
      ${stat("Requests", summary.request_count)}
      ${stat("Failures", summary.failure_count)}
      ${stat("Cost", summary.estimated_cost_usd ?? "n/a")}
    </div>
    <div class="panel"><h3>Provider and service health</h3>${healthTable(healthRows)}</div>
  `;
}

function stat(label, value) {
  return `<div class="panel"><h3>${label}</h3><strong>${value}</strong></div>`;
}

async function keys() {
  content.innerHTML = `
    <div class="panel">
      <h3>Create virtual key</h3>
      <form id="key-form" class="form-grid">
        <label>Project ID<input name="project_id" required></label>
        <label>Routes<input name="allowed_routes" value="/summary,/services/*"></label>
        <label>Providers<input name="allowed_providers" value="internal-service"></label>
        <label>Services<input name="allowed_services" value="summary,translation"></label>
        <label>RPM limit<input name="rpm_limit" type="number"></label>
        <label>Daily budget<input name="daily_budget_usd" type="number" step="0.01"></label>
        <button type="submit">Create</button>
      </form>
    </div>
    <div class="panel"><h3>Inspect key</h3>
      <form id="key-get" class="form-grid">
        <label>Key ID<input name="key_id"></label>
        <button type="submit">Load</button>
      </form>
      <pre id="key-output"></pre>
    </div>
  `;
  document.querySelector("#key-form").addEventListener("submit", createKey);
  document.querySelector("#key-get").addEventListener("submit", getKey);
}

async function createKey(event) {
  event.preventDefault();
  const form = new FormData(event.target);
  const rpm = form.get("rpm_limit");
  const daily = form.get("daily_budget_usd");
  const body = {
    project_id: form.get("project_id"),
    policy: {
      allowed_routes: csv(form.get("allowed_routes")),
      allowed_providers: csv(form.get("allowed_providers")),
      allowed_services: csv(form.get("allowed_services")),
    },
  };
  if (rpm) body.policy.rpm_limit = Number(rpm);
  if (daily) body.policy.daily_budget_usd = Number(daily);
  const response = await api("/admin/keys", { method: "POST", body: JSON.stringify(body) });
  showRawToken(response.raw_key);
}

async function getKey(event) {
  event.preventDefault();
  const keyId = new FormData(event.target).get("key_id");
  document.querySelector("#key-output").textContent = JSON.stringify(
    await api(`/admin/keys/${keyId}`),
    null,
    2,
  );
}

async function services() {
  state.services = await api("/admin/services");
  content.innerHTML = `
    <div class="panel">
      <h3>Create or import service</h3>
      <form id="service-form" class="form-grid">
        <label>Name<input name="name" required></label>
        <label>Studio service ID<input name="studio_service_id"></label>
        <label>Route pattern<input name="route_pattern" placeholder="/services/name/*"></label>
        <label>Upstream URL<input name="upstream_base_url"></label>
        <label>Credential<input name="credential" type="password"></label>
        <label>Estimated cost<input name="estimated_cost_usd" type="number" step="0.01"></label>
        <button name="action" value="create">Create</button>
        <button name="action" value="import">Import Studio</button>
      </form>
    </div>
    <div class="panel"><h3>Registered services</h3>${serviceTable(state.services)}</div>
  `;
  document.querySelector("#service-form").addEventListener("submit", submitService);
  document.querySelectorAll("[data-service-action]").forEach((button) => {
    button.addEventListener("click", serviceAction);
  });
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
        route_pattern: form.get("route_pattern") || undefined,
        default_pricing: form.get("estimated_cost_usd")
          ? { cost_mode: "fixed", estimated_cost_usd: Number(form.get("estimated_cost_usd")) }
          : undefined,
      }),
    });
  } else {
    await api("/admin/services", {
      method: "POST",
      body: JSON.stringify({
        name: form.get("name"),
        route_pattern: form.get("route_pattern") || undefined,
        upstream_base_url: form.get("upstream_base_url") || undefined,
        credential: form.get("credential") || undefined,
        cost_mode: form.get("estimated_cost_usd") ? "fixed" : "none",
        estimated_cost_usd: form.get("estimated_cost_usd")
          ? Number(form.get("estimated_cost_usd"))
          : undefined,
      }),
    });
  }
  await services();
}

async function serviceAction(event) {
  const { serviceName, serviceAction } = event.target.dataset;
  if (["delete", "disable", "enable"].includes(serviceAction) && !confirm(`${serviceAction} ${serviceName}?`)) return;
  if (serviceAction === "delete") {
    await api(`/admin/services/${serviceName}`, { method: "DELETE" });
  } else {
    await api(`/admin/services/${serviceName}/${serviceAction}`, { method: "POST", body: "{}" });
  }
  await services();
}

function serviceTable(rows) {
  return table(
    ["Name", "State", "Route", "Upstream", "Credential", "Actions"],
    rows.map((row) => [
      row.name,
      badges(row),
      row.route_pattern,
      row.upstream_base_url || "missing",
      row.credential_configured ? "configured" : "missing",
      `<div class="actions">
        <button data-service-action="${row.enabled ? "disable" : "enable"}" data-service-name="${row.name}">${row.enabled ? "Disable" : "Enable"}</button>
        <button class="danger" data-service-action="delete" data-service-name="${row.name}">Delete</button>
      </div>`,
    ]),
  );
}

function badges(row) {
  const state = row.enabled ? '<span class="badge good">enabled</span>' : '<span class="badge bad">disabled</span>';
  const sync = row.sync_status === "synced" || row.sync_status === "local"
    ? `<span class="badge good">${row.sync_status}</span>`
    : `<span class="badge bad">${row.sync_status}</span>`;
  return `${state} ${sync}`;
}

async function usage() {
  content.innerHTML = `
    <div class="panel">
      <h3>Usage filters</h3>
      <form id="usage-form" class="form-grid">
        <label>Service<input name="service"></label>
        <label>Provider<input name="provider"></label>
        <label>Model<input name="model"></label>
        <label>Task<input name="task_id"></label>
        <button>Apply</button>
      </form>
    </div>
    <div class="panel"><h3>Usage by service</h3><div id="usage-results"></div></div>
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
    ["Name", "Requests", "Success", "Failure", "Latency"],
    rows.map((row) => [
      row.name,
      row.summary.request_count,
      row.summary.success_count,
      row.summary.failure_count,
      row.summary.total_latency_ms,
    ]),
  );
}

async function health() {
  const rows = await api("/admin/provider-health");
  content.innerHTML = `<div class="panel"><h3>Provider and service health</h3>${healthTable(rows)}</div>`;
}

function healthTable(rows) {
  return table(
    ["Name", "Requests", "Errors", "Timeouts", "Fallbacks", "Latency"],
    rows.map((row) => [
      row.name,
      row.request_count,
      row.error_count,
      row.timeout_count,
      row.fallback_count,
      row.total_latency_ms,
    ]),
  );
}

function table(headers, rows) {
  if (!rows.length) return "<p>No rows.</p>";
  return `<table><thead><tr>${headers.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows
    .map((row) => `<tr>${row.map((cell) => `<td>${cell ?? ""}</td>`).join("")}</tr>`)
    .join("")}</tbody></table>`;
}

function csv(value) {
  return String(value || "")
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

if (token()) {
  signedIn();
}
