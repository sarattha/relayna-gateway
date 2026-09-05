// Metadata-only live monitor. Transport parsing and bounded merging are exported
// so reconnect, retention and split-frame behavior can be tested independently.
export function parseTrafficFrames(buffer) {
  const normalized = buffer.replace(/\r\n/g, "\n");
  const frames = normalized.split("\n\n");
  const remainder = frames.pop() || "";
  const batches = frames.flatMap((frame) => {
    const data = frame.split("\n").filter((line) => line.startsWith("data:")).map((line) => line.slice(5).trimStart()).join("\n");
    return data ? [JSON.parse(data)] : [];
  });
  if (remainder.length > 4 * 1024 * 1024) throw new Error("Traffic update exceeded the display limit.");
  return { batches, remainder };
}

export function mergeTrafficRows(rows, batch, limit = 200) {
  const merged = new Map(batch.gap ? [] : rows.map((row) => [row.id, row]));
  for (const row of batch.rows) merged.set(row.id, row);
  return [...merged.values()].sort((a, b) => b.started_at.localeCompare(a.started_at) || b.id.localeCompare(a.id)).slice(0, limit);
}

export function matchesTraffic(row, filters) {
  return (!filters.request_id || row.request_id.includes(filters.request_id))
    && (!filters.service || row.service === filters.service)
    && (!filters.project_id || row.project_id === filters.project_id)
    && (!filters.key_id || row.key_id === filters.key_id)
    && (!filters.failure_code || row.diagnostics.failure_code === filters.failure_code)
    && (!filters.status || String(row.client_status) === filters.status)
    && (filters.outcome !== "failures" || Boolean(row.diagnostics.failure_code))
    && (filters.outcome !== "active" || !row.completed);
}

export function mountTraffic({ content, api, headers, esc, attr, table, badge, time, mountDialog, investigationView, bindInvestigationActions, initialFilters = {}, onFilters = () => {} }) {
  let rows = [], cursor = null, instance = "Connecting", selected = null, filters = { ...initialFilters };
  let mode = "live", paused = false, disposed = false, controller = null, reconnect = null;
  let warning = "", historyCursor = null, historyGeneration = 0, connectionGeneration = 0;
  content.innerHTML = `
    <section class="panel">
      <div class="panel-heading"><h3>Traffic monitor</h3><div class="actions">
        <label>Traffic source<select id="traffic-mode" aria-label="Traffic source" aria-describedby="traffic-source-help"><option value="live">Live instance</option><option value="history">Saved history · all instances</option></select></label>
        <button id="traffic-pause" type="button">Pause</button></div></div>
      <p id="traffic-connection" role="status" aria-live="polite">Connecting…</p>
      <p id="traffic-source-help" class="help">Live view retains up to 200 recent requests from one gateway process. Saved history includes completed records across instances; its default window is 24 hours. Unknown routes are hidden to avoid capturing sensitive paths.</p>
      <div id="traffic-warning" class="notice hidden" role="status"></div>
      <form id="traffic-filters" class="form-grid">
        <label>Request ID<input name="request_id" maxlength="128" placeholder="Client correlation ID"></label>
        <label>Service<input name="service" maxlength="256" placeholder="Exact service name"></label>
        <label>Project ID<input name="project_id" placeholder="Exact project UUID"></label>
        <label>Key ID<input name="key_id" placeholder="Exact key UUID"></label>
        <label>Client HTTP status<input name="status" type="number" min="100" max="599" placeholder="503"></label>
        <label>Failure reason<input name="failure_code" maxlength="80" placeholder="control_state_unavailable"></label>
        <label>Outcome<select name="outcome"><option value="all">All requests</option><option value="failures">Failures only</option><option value="active">Active only · live</option></select></label>
        <label>History from<input name="from" type="datetime-local"></label>
        <label>History to<input name="to" type="datetime-local"></label>
        <div class="form-actions"><button type="submit">Apply filters</button></div>
      </form>
    </section>
    <section class="panel"><div class="panel-heading"><h3>Requests</h3><div id="traffic-pages" class="actions hidden"><button id="traffic-newest">Newest</button><button id="traffic-older">Older</button></div></div>
      <p id="traffic-summary" class="help"></p><div id="traffic-failure-groups" class="actions"></div><div id="traffic-rows"></div></section>
    <section id="traffic-detail" class="panel hidden" tabindex="-1"></section>`;
  let detailBackdrop = null, closeDetail = null, renderedDetailRow = null;
  const element = (id) => content.querySelector(`#${id}`) || detailBackdrop?.querySelector(`#${id}`);
  for (const [name, value] of Object.entries(filters)) {
    const field = element("traffic-filters").elements.namedItem(name);
    if (field) field.value = value;
  }
  const label = (value) => (value || "unknown").replaceAll("_", " ");
  function connection(message) {
    if (disposed) return;
    const value = `${message} · ${mode === "live" ? `Instance ${instance}` : "All instances"}`;
    if (element("traffic-connection").textContent !== value) element("traffic-connection").textContent = value;
  }
  function render() {
    if (disposed) return;
    element("traffic-warning").classList.toggle("hidden", !warning);
    element("traffic-warning").textContent = warning;
    const visible = rows.filter((row) => matchesTraffic(row, filters));
    const groups = new Map();
    for (const row of visible) if (row.diagnostics.failure_code) groups.set(row.diagnostics.failure_code, (groups.get(row.diagnostics.failure_code) || 0) + 1);
    element("traffic-summary").textContent = `${visible.length} displayed · ${visible.filter((row) => !row.completed).length} active in retained records · ${[...groups].map(([reason, count]) => `${label(reason)}: ${count}`).join(" · ") || "No failures in displayed records"}`;
    element("traffic-failure-groups").innerHTML = [...groups].map(([reason, count]) => `<button type="button" data-traffic-reason="${attr(reason)}">${esc(label(reason))}: ${count}</button>`).join("");
    element("traffic-failure-groups").querySelectorAll("[data-traffic-reason]").forEach((button) => button.addEventListener("click", () => {
      filters.failure_code = button.dataset.trafficReason;
      element("traffic-filters").elements.namedItem("failure_code").value = filters.failure_code;
      if (mode === "history") history(); else render();
    }));
    element("traffic-rows").innerHTML = table(
      ["Arrived", "Request", "Endpoint / service", "Stage / outcome", "Client HTTP", "Upstream HTTP", "Attempts", "Elapsed", "Failure reason", "Recording", "Details"],
      visible.map((row) => [
        time(row.started_at), `<code>${esc(row.request_id)}</code>`,
        `${esc(row.method)} ${esc(row.endpoint || "Unresolved route")}<br><span class="subtle">${esc(row.service || row.provider || "Not selected")}</span>`,
        badge(label(row.completed ? row.diagnostics.outcome : row.stage), row.diagnostics.failure_code ? "bad" : row.completed ? "good" : "warn"),
        esc(row.client_status ?? "—"), esc(row.diagnostics.upstream_status ?? "—"), esc(row.attempts),
        `<span data-traffic-elapsed="${attr(row.id)}">${esc(row.completed ? row.elapsed_ms : Math.max(row.elapsed_ms, Date.now() - Date.parse(row.started_at)))} ms</span>`,
        esc(label(row.diagnostics.failure_code || "none")),
        row.recording_failures.length ? badge("Recording failed", "bad") : esc(row.completed ? "No reported failure" : "In progress"),
        `<button type="button" data-traffic-id="${attr(row.id)}">Inspect</button>`,
      ]));
    element("traffic-rows").querySelectorAll("[data-traffic-id]").forEach((button) => button.addEventListener("click", () => {
      selected = button.dataset.trafficId; openDetail();
    }));
    renderDetail();
  }
  function openDetail() {
    if (!detailBackdrop) {
      const detail = element("traffic-detail");
      detailBackdrop = document.createElement("section");
      detailBackdrop.className = "modal-backdrop drawer-backdrop";
      detailBackdrop.innerHTML = `<div class="modal resource-drawer" role="dialog" aria-modal="true" aria-labelledby="traffic-investigation-title"><div class="drawer-heading"><h3 id="traffic-investigation-title">Request investigation</h3><button type="button" id="traffic-close" aria-label="Close Request investigation">Close</button></div><div class="drawer-body"></div></div>`;
      detailBackdrop.querySelector(".drawer-body").appendChild(detail);
      document.body.appendChild(detailBackdrop);
      closeDetail = mountDialog(detailBackdrop, { restoreFocus: () =>
        [...content.querySelectorAll("[data-traffic-id]")].find((button) => button.dataset.trafficId === selected) || element("traffic-mode"), onClose: () => {
        selected = null; renderedDetailRow = null; detail.classList.add("hidden"); content.appendChild(detail); detailBackdrop = null; closeDetail = null;
      } });
      element("traffic-close").addEventListener("click", () => closeDetail?.());
    }
    renderDetail();
  }
  function renderDetail() {
    const row = rows.find((value) => value.id === selected);
    const detail = element("traffic-detail");
    detail.classList.toggle("hidden", !row);
    if (!row) { closeDetail?.(); return; }
    if (row === renderedDetailRow) return;
    renderedDetailRow = row;
    const copyFocus = detail.contains(document.activeElement) ? document.activeElement?.dataset?.investigationCopy : null;
    const rawFocus = document.activeElement === detail.querySelector("[data-investigation-section=raw] > summary");
    const rawOpen = detail.querySelector("[data-investigation-section=raw]")?.open;
    detail.innerHTML = investigationView({ traffic: row });
    if (rawOpen) detail.querySelector("[data-investigation-section=raw]").open = true;
    bindInvestigationActions(detail, row.request_id);
    if (copyFocus) [...detail.querySelectorAll("[data-investigation-copy]")].find(button => button.dataset.investigationCopy === copyFocus)?.focus({ preventScroll: true });
    if (rawFocus) detail.querySelector("[data-investigation-section=raw] > summary").focus({ preventScroll: true });
  }
  function stopConnection() { connectionGeneration++; controller?.abort(); controller = null; clearTimeout(reconnect); }
  async function connect() {
    if (disposed || paused || mode !== "live") return;
    const generation = connectionGeneration;
    const attempt = new AbortController(); controller = attempt;
    let watchdog = setTimeout(() => attempt.abort(), 12000);
    let reader;
    try {
      const response = await fetch("/admin-ui/admin/traffic/live", { headers: { ...headers(), ...(cursor ? { "last-event-id": cursor } : {}) }, signal: attempt.signal, cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}; check admin access and gateway health`);
      reader = response.body.getReader();
      const decoder = new TextDecoder(); let pending = "";
      while (!disposed && mode === "live" && !paused && generation === connectionGeneration) {
        const { value, done } = await reader.read(); if (done) break;
        clearTimeout(watchdog); watchdog = setTimeout(() => attempt.abort(), 12000);
        pending += decoder.decode(value, { stream: true });
        const parsed = parseTrafficFrames(pending); pending = parsed.remainder;
        for (const batch of parsed.batches) {
          if (instance !== "Connecting" && instance !== batch.instance_id) warning = "Gateway instance changed. Live records now cover the new instance; use saved history for other instances.";
          if (batch.gap) warning = "Updates were missed during disconnection or journal eviction. Retained records were reloaded; use saved history for completed requests.";
          else if (!warning && batch.evicted_updates) warning = "The live journal has discarded older updates. This is a retained window, not a complete traffic count.";
          instance = batch.instance_id; cursor = batch.cursor; rows = mergeTrafficRows(rows, batch);
          connection("Live"); if (batch.rows.length || batch.gap) render();
        }
      }
      if (!disposed && !paused && mode === "live" && generation === connectionGeneration) connection("Reconnecting for authorization refresh");
    } catch (error) {
      if (!disposed && !paused && mode === "live" && generation === connectionGeneration) connection(`Disconnected · ${error.name === "AbortError" ? "no updates received" : error.message} · retrying`);
    } finally {
      clearTimeout(watchdog); await reader?.cancel().catch(() => {});
      if (!disposed && !paused && mode === "live" && generation === connectionGeneration) reconnect = setTimeout(connect, 1500);
    }
  }
  async function history(older = false) {
    const generation = ++historyGeneration;
    rows = []; selected = null; render();
    connection("Loading saved history");
    try {
      const query = new URLSearchParams({ limit: "100" });
      for (const key of ["request_id", "service", "project_id", "key_id", "status", "failure_code"]) if (filters[key]) query.set(key, filters[key]);
      if (filters.outcome === "active") throw new Error("Active requests are available in Live instance mode.");
      if (filters.outcome === "failures") query.set("failures_only", "true");
      for (const key of ["from", "to"]) if (filters[key]) query.set(key, new Date(filters[key]).toISOString());
      if (older && historyCursor) { query.set("before", historyCursor.started_at); query.set("before_id", historyCursor.id); }
      const result = await api(`/admin-ui/admin/traffic/history?${query}`);
      if (disposed || mode !== "history" || generation !== historyGeneration) return;
      rows = result; historyCursor = rows.at(-1); selected = null;
      element("traffic-older").disabled = result.length < 100;
      warning = "Saved history contains records successfully written to the database. Recording failures and in-flight requests may only be available in live records or gateway logs.";
      connection("Saved history"); render();
    } catch (error) { if (!disposed && generation === historyGeneration) { warning = `History unavailable: ${error.message}`; connection("History unavailable"); render(); } }
  }
  element("traffic-filters").addEventListener("submit", (event) => {
    event.preventDefault(); filters = Object.fromEntries(new FormData(event.currentTarget)); onFilters(filters);
    event.currentTarget.elements.namedItem("key_id").value = filters.key_id || "";
    if (mode === "history") history(); else render();
  });
  element("traffic-mode").addEventListener("change", (event) => {
    mode = event.target.value; stopConnection(); historyGeneration++; rows = []; selected = null; warning = "";
    element("traffic-pages").classList.toggle("hidden", mode !== "history");
    element("traffic-pause").disabled = mode === "history";
    if (mode === "history") history(); else { cursor = null; paused = false; element("traffic-pause").textContent = "Pause"; render(); connect(); }
  });
  element("traffic-pause").addEventListener("click", () => {
    paused = !paused; element("traffic-pause").textContent = paused ? "Resume" : "Pause";
    if (paused) { stopConnection(); connection("Paused · updates are not being received"); } else connect();
  });
  element("traffic-newest").addEventListener("click", () => history());
  element("traffic-older").addEventListener("click", () => history(true));
  const elapsedTimer = setInterval(() => {
    if (disposed || paused || mode !== "live") return;
    for (const row of rows) {
      if (row.completed) continue;
      const cell = content.querySelector(`[data-traffic-elapsed="${row.id}"]`);
      if (cell) cell.textContent = `${Math.max(row.elapsed_ms, Date.now() - Date.parse(row.started_at))} ms`;
    }
  }, 1000);
  render(); connect();
  return () => { closeDetail?.(); disposed = true; historyGeneration++; clearInterval(elapsedTimer); stopConnection(); };
}
