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
  const terminalIds = new Set(rows.filter(row => row.completed).map(row => row.id));
  const merged = new Map(batch.gap ? [] : rows.map((row) => [row.id, row]));
  for (const row of batch.rows) {
    if (terminalIds.has(row.id) && !row.completed) continue;
    if (row.completed) terminalIds.add(row.id);
    merged.set(row.id, row);
  }
  if (batch.active) {
    // The registry is authoritative even when a quiet connection's lifecycle
    // updates have left the journal. Completed history has its own page budget.
    const completed = [...merged.values()].filter(row => row.completed);
    const active = batch.active.rows.filter(row => !terminalIds.has(row.id)).map(row => {
      const full = merged.get(row.id);
      if (!full || full.summary_only || !row.summary_only) return row;
      // Projections carry fresh progress but intentionally empty investigation fields.
      return { ...full, ...row, summary_only: false, timeline: full.timeline,
        upstream_timings: full.upstream_timings, usage: full.usage, debug_bundle: full.debug_bundle,
        recording_failures: [...new Set([...(full.recording_failures || []), ...(row.recording_failures || [])])],
        diagnostics: { ...full.diagnostics, ...row.diagnostics,
          authentication_profile: full.diagnostics.authentication_profile, entra: full.diagnostics.entra } };
    });
    return [...active, ...completed.sort((a, b) => (b.completed_at || b.started_at).localeCompare(a.completed_at || a.started_at)).slice(0, limit)];
  }
  return [...merged.values()].sort((a, b) => b.started_at.localeCompare(a.started_at) || b.id.localeCompare(a.id)).slice(0, limit);
}

export function trafficProtocol(row) {
  return row.diagnostics?.protocol || (row.diagnostics?.websocket ? "websocket" : "unknown");
}

export function trafficDuration(milliseconds) {
  if (!Number.isFinite(milliseconds)) return "—";
  const seconds = Math.max(0, milliseconds) / 1000;
  if (seconds < 1) return `${Math.round(Math.max(0, milliseconds))} ms`;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor(seconds / 60 % 60)}m`;
}

export function matchesTraffic(row, filters) {
  return (!filters.request_id || row.request_id.includes(filters.request_id))
    && (!filters.service || row.service === filters.service)
    && (!filters.project_id || row.project_id === filters.project_id)
    && (!filters.key_id || row.key_id === filters.key_id)
    && (!filters.failure_code || row.diagnostics.failure_code === filters.failure_code)
    && (!filters.status || String(row.client_status) === filters.status)
    && (!filters.protocol || trafficProtocol(row) === filters.protocol)
    && (filters.outcome !== "failures" || Boolean(row.diagnostics.failure_code))
    && (filters.outcome !== "active" || !row.completed)
    && (filters.outcome !== "completed" || row.completed);
}

export function mountTraffic({ content, api, headers, esc, attr, table, badge, time, routingModeLabel, mountDialog, investigationView, bindInvestigationActions, refreshWebSocketMetrics, initialFilters = {}, initialMode = "active", onFilters = () => {} }) {
  let rows = [], cursor = null, instance = "Connecting", selected = null, filters = { ...initialFilters };
  let mode = initialMode, paused = false, disposed = false, controller = null, reconnect = null;
  let warning = "", historyCursor = null, historyGeneration = 0, connectionGeneration = 0;
  let activeGeneration = 0, activeTimer = null, cluster = null, activeCursor = null, historyWindow = null;
  let historyTimer = null, historyAtNewest = true, activeAtNewest = true;
  let activeStale = false;
  let selectedInstance = "all";
  content.innerHTML = `
    <section class="panel traffic-explorer">
      <div class="panel-heading"><h3>Traffic explorer</h3><div class="actions traffic-source-actions">
        <label>Traffic view<select id="traffic-mode" aria-label="Traffic view" aria-describedby="traffic-source-help"><option value="active">Active · all instances</option><option value="history">Recent completed · all instances</option><option value="legacy">Saved history by arrival · all instances</option><option value="live">Live journal · one instance</option></select></label>
        <label id="traffic-instance-label">Gateway instance<select id="traffic-instance"><option value="all">All instances</option></select></label>
        <button id="traffic-pause" type="button">Pause</button></div></div>
      <p id="traffic-connection" class="monitor-connection" role="status" aria-live="polite">Connecting…</p>
      <p id="traffic-source-help" class="sr-only">Browse active sessions, recent completions, saved history or one process’s live journal.</p>
      <div id="traffic-warning" class="notice hidden" role="status"></div>
      <div id="traffic-protocols" class="traffic-protocol-tabs" role="group" aria-label="Protocol partition">
        <button type="button" data-traffic-protocol="" aria-pressed="true"><i class="ti ti-arrows-exchange" aria-hidden="true"></i>All protocols</button>
        <button type="button" data-traffic-protocol="http" aria-pressed="false"><i class="ti ti-world" aria-hidden="true"></i>HTTP / SSE</button>
        <button type="button" data-traffic-protocol="websocket" aria-pressed="false"><i class="ti ti-plug-connected" aria-hidden="true"></i>WebSocket</button>
      </div>
      <form id="traffic-filters">
        <div class="traffic-quick-filters"><label>Request ID<input name="request_id" maxlength="128" placeholder="Search correlation ID"></label>
          <label>Service<input name="service" maxlength="256" placeholder="Exact service name"></label>
          <label>Outcome<select name="outcome"><option value="all">All requests</option><option value="failures">Failures only</option><option value="active">Active only</option><option value="completed">Completed only</option></select></label>
          <button type="submit"><i class="ti ti-filter" aria-hidden="true"></i> Apply filters</button></div>
        <details class="monitor-details traffic-advanced-filters"><summary><i class="ti ti-adjustments-horizontal" aria-hidden="true"></i>Advanced filters</summary><div class="form-grid">
          <label>Project ID<input name="project_id" placeholder="Exact project UUID"></label>
          <label>Key ID<input name="key_id" placeholder="Exact key UUID"></label>
          <label>Client HTTP status<input name="status" type="number" min="100" max="599" placeholder="e.g. 503"></label>
          <label>Failure reason<input name="failure_code" maxlength="80" placeholder="Exact failure code"></label>
          <label>Protocol partition<select name="protocol"><option value="">All protocols</option><option value="http">HTTP / SSE</option><option value="websocket">WebSocket</option><option value="unknown">Not recorded</option></select></label>
          <label>Window from<input name="from" type="datetime-local"></label><label>Window to<input name="to" type="datetime-local"></label>
        </div></details>
      </form>
    </section>
    <section class="panel"><div class="panel-heading"><h3>Requests</h3><div id="traffic-pages" class="actions hidden"><button id="traffic-newest">Newest</button><button id="traffic-older">Older</button></div></div>
      <p id="traffic-summary" class="sr-only"></p><div id="traffic-counts" class="traffic-result-summary"></div><div id="traffic-failure-groups" class="actions"></div><div id="traffic-rows"></div>
      <details class="monitor-details"><summary><i class="ti ti-info-circle" aria-hidden="true"></i>Sources & retention</summary><ul class="monitor-definitions"><li>Active HTTP/SSE and WebSocket sessions have independent retention budgets. Counts can exceed displayed rows.</li><li>Recent completed requests use completion time, default to 30 minutes, and refresh every five seconds on Newest.</li><li>Older pages and fixed end-time windows stay frozen. Saved history by arrival includes legacy records and defaults to 24 hours.</li><li>The live journal covers one process. Inspect a request for routing, timing, failure and recording details. Recording failures may only be available in live records or gateway logs.</li></ul></details>
    </section>
    <section id="traffic-detail" class="panel hidden" tabindex="-1"></section>`;
  let detailBackdrop = null, closeDetail = null, renderedDetailRow = null;
  const element = (id) => content.querySelector(`#${id}`) || detailBackdrop?.querySelector(`#${id}`);
  element("traffic-mode").value = mode;
  element("traffic-pages").classList.toggle("hidden", mode === "live");
  element("traffic-instance-label").classList.toggle("hidden", mode !== "active");
  for (const [name, value] of Object.entries(filters)) {
    const field = element("traffic-filters").elements.namedItem(name);
    if (field) field.value = value;
  }
  const label = (value) => (value || "unknown").replaceAll("_", " ");
  const isHistory = () => mode === "history" || mode === "legacy";
  const staleRow = row => mode === "active" && (activeStale || cluster?.unavailable || cluster?.instances?.some(item => item.instance_id === row.instance_id && item.stale));
  function connection(message) {
    if (disposed) return;
    const value = `${message} · ${mode === "live" ? `Instance ${instance}` : selectedInstance === "all" || isHistory() ? "All instances" : `Instance ${selectedInstance}`}`;
    if (element("traffic-connection").textContent !== value) element("traffic-connection").textContent = value;
  }
  function render() {
    if (disposed) return;
    element("traffic-warning").classList.toggle("hidden", !warning);
    element("traffic-warning").textContent = warning;
    const visible = rows.filter((row) => matchesTraffic(row, filters));
    const groups = new Map();
    for (const row of visible) if (row.diagnostics.failure_code) groups.set(row.diagnostics.failure_code, (groups.get(row.diagnostics.failure_code) || 0) + 1);
    const activeCounts = mode === "active" && !cluster?.unavailable && cluster?.counts;
    const countText = activeCounts ? `${cluster.partial ? "Known" : "Exact"} active before filters: HTTP/SSE ${activeCounts.http ?? "unknown"} · WebSocket ${activeCounts.websocket ?? "unknown"}` : mode === "active" ? "Current counts unknown" : isHistory() ? "Saved page · counts cover displayed rows" : `${visible.filter((row) => !row.completed).length} active in retained records`;
    element("traffic-summary").textContent = `${visible.length} displayed · ${countText} · ${[...groups].map(([reason, count]) => `${label(reason)}: ${count}`).join(" · ") || "No failures in displayed records"}`;
    element("traffic-counts").innerHTML = `<span><strong>${visible.length}</strong> displayed</span><span>${esc(countText)}</span><span class="${groups.size ? "bad" : ""}"><i class="ti ti-${groups.size ? "alert-circle" : "circle-check"}" aria-hidden="true"></i> ${groups.size ? `${[...groups.values()].reduce((a, b) => a + b, 0)} failures on this page` : "No failures on this page"}</span>`;
    element("traffic-protocols").querySelectorAll("[data-traffic-protocol]").forEach(button => button.setAttribute("aria-pressed", String(button.dataset.trafficProtocol === (filters.protocol || ""))));
    // Keep the active filter removable while history loads or its last match leaves the live window.
    if (filters.failure_code && !groups.has(filters.failure_code)) groups.set(filters.failure_code, 0);
    const focusedReason = element("traffic-failure-groups").contains(document.activeElement) ? document.activeElement?.dataset?.trafficReason : null;
    element("traffic-failure-groups").innerHTML = [...groups].map(([reason, count]) => `<button type="button" data-traffic-reason="${attr(reason)}" aria-pressed="${filters.failure_code === reason}">${esc(label(reason))}: ${count}</button>`).join("")
      + (filters.failure_code ? `<span class="help">Click the selected reason again to clear its filter.</span>` : "");
    element("traffic-failure-groups").querySelectorAll("[data-traffic-reason]").forEach((button) => button.addEventListener("click", () => {
      filters = { ...filters, failure_code: filters.failure_code === button.dataset.trafficReason ? "" : button.dataset.trafficReason };
      onFilters(filters);
      element("traffic-filters").elements.namedItem("failure_code").value = filters.failure_code;
      refresh();
    }));
    if (focusedReason) {
      const replacement = [...element("traffic-failure-groups").querySelectorAll("[data-traffic-reason]")].find(button => button.dataset.trafficReason === focusedReason);
      (replacement || element("traffic-filters").elements.namedItem("failure_code")).focus({ preventScroll: true });
    }
    const tableMarkup = table(
      [mode === "history" ? "Completed / arrived" : "Arrived", "Request / service", "Protocol", "Status / outcome", "Client / upstream", "Duration", "Details"],
      visible.map(row => [
        `<span class="traffic-clock">${mode === "history" ? `${row.completed_at ? time(row.completed_at) : "Completion not recorded"}<div class="subtle">Arrived ${time(row.started_at)}</div>` : time(row.started_at)}</span>`,
        `<div class="traffic-request-cell"><code title="${attr(row.request_id)}">${esc(row.request_id)}</code><small>${esc(row.method)} ${esc(row.service || row.provider || "Unresolved route")}</small></div>`,
        trafficProtocol(row) === "websocket" ? badge("WS", "neutral") + (row.diagnostics.websocket ? `<div class="subtle">↑ ${esc(row.diagnostics.websocket.client_bytes)} B · ↓ ${esc(row.diagnostics.websocket.upstream_bytes)} B</div>` : "") : badge(trafficProtocol(row) === "http" ? row.streaming ? "HTTP / SSE" : "HTTP" : "Not recorded", "neutral"),
        badge(staleRow(row) ? `Last known ${label(row.stage)} · stale` : label(row.completed ? row.diagnostics.outcome : row.stage), staleRow(row) ? "neutral" : row.diagnostics.failure_code ? "bad" : row.completed ? "good" : "neutral") + (row.diagnostics.failure_code ? `<div class="subtle">${esc(label(row.diagnostics.failure_code))}</div>` : "") + (row.recording_failures?.length ? `<div>${badge("Recording failed", "bad")}</div>` : ""),
        `<span class="traffic-request-http">${esc(row.client_status ?? "—")}<small>Upstream ${esc(row.diagnostics.upstream_status ?? "—")}</small></span>`,
        `<span title="${attr(`${row.elapsed_ms} ms`)}" data-traffic-elapsed="${attr(row.id)}">${esc(trafficDuration(row.completed || staleRow(row) ? row.elapsed_ms : Math.max(row.elapsed_ms, Date.now() - Date.parse(row.started_at))))}${staleRow(row) ? " · last observed" : ""}</span>`,
        `<button type="button" class="icon-button" data-traffic-id="${attr(row.id)}" aria-label="Inspect request ${attr(row.request_id)}" title="Inspect request"><i class="ti ti-chevron-right" aria-hidden="true"></i></button>`,
      ]));
    element("traffic-rows").innerHTML = visible.length ? tableMarkup : `<div class="monitor-empty"><i class="ti ti-arrows-exchange" aria-hidden="true"></i><span>${mode === "active" ? "No matching active requests. New sessions appear automatically." : "No matching requests in this window."}</span></div>`;
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
      detailBackdrop.innerHTML = `<div class="modal resource-drawer" role="dialog" aria-modal="true" aria-labelledby="traffic-investigation-title"><div class="drawer-heading"><span class="drawer-mark" aria-hidden="true"><i class="ti ti-activity"></i></span><div class="drawer-title"><span class="eyebrow">Inspect & analyze</span><h3 id="traffic-investigation-title">Request investigation</h3></div><button type="button" class="icon-button" id="traffic-close" aria-label="Close Request investigation"><i class="ti ti-x" aria-hidden="true"></i></button></div><div class="drawer-body"></div></div>`;
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
    detail.innerHTML = (row.summary_only ? `<p class="notice">Active summary. Full investigation details are available from retained lifecycle updates or saved history after completion.</p>` : "") + investigationView({ traffic: row });
    if (rawOpen) detail.querySelector("[data-investigation-section=raw]").open = true;
    bindInvestigationActions(detail, row.request_id);
    if (copyFocus) [...detail.querySelectorAll("[data-investigation-copy]")].find(button => button.dataset.investigationCopy === copyFocus)?.focus({ preventScroll: true });
    if (rawFocus) detail.querySelector("[data-investigation-section=raw] > summary").focus({ preventScroll: true });
  }
  function stopConnection() { connectionGeneration++; activeGeneration++; historyGeneration++; controller?.abort(); controller = null; clearTimeout(reconnect); clearTimeout(activeTimer); clearTimeout(historyTimer); }
  function refresh() {
    if (isHistory()) history();
    else if (mode === "active") active();
    else { stopConnection(); cursor = null; render(); connect(); }
  }
  async function active(older = false, polling = false) {
    if (disposed || paused || mode !== "active") return;
    const before = older ? activeCursor : null;
    if (older && !before) return;
    clearTimeout(activeTimer);
    activeAtNewest = !older;
    const generation = ++activeGeneration;
    element("traffic-older").disabled = true;
    element("traffic-newest").disabled = true;
    if (!polling) { if (!older) { rows = []; cluster = null; activeCursor = null; } selected = null; render(); connection("Loading active traffic"); }
    try {
      const query = new URLSearchParams({ limit: "100", instance_id: selectedInstance });
      for (const key of ["request_id", "service", "project_id", "key_id", "status", "failure_code", "protocol"]) if (filters[key]) query.set(key, filters[key]);
      if (filters.outcome === "failures") query.set("failures_only", "true");
      if (before) { query.set("before", before.started_at); query.set("before_id", before.id); }
      const result = await api(`/admin-ui/admin/traffic/cluster?${query}`);
      if (disposed || paused || mode !== "active" || generation !== activeGeneration) return;
      cluster = result; activeStale = false;
      rows = filters.outcome === "completed" ? [] : result.rows || [];
      activeCursor = result.has_more && result.before && result.before_id ? { started_at: result.before, id: result.before_id } : null;
      element("traffic-older").disabled = !activeCursor;
      const knownInstances = result.instances || [];
      const options = `<option value="all">All instances</option>` + knownInstances.map(item => `<option value="${attr(item.instance_id)}">${esc(item.instance_id)}${item.stale ? " · stale" : ""}</option>`).join("")
        + (selectedInstance !== "all" && !knownInstances.some(item => item.instance_id === selectedInstance) ? `<option value="${attr(selectedInstance)}">${esc(selectedInstance)} · unavailable</option>` : "");
      const selector = element("traffic-instance");
      if (selector.innerHTML !== options) selector.innerHTML = options;
      selector.value = selectedInstance;
      warning = result.unavailable ? "Active traffic unavailable. Counts and coverage are unknown; retrying."
        : [result.partial ? "Cluster coverage is partial; displayed rows may be stale. Stale or unavailable instances are excluded from current counts." : "",
          result.truncated ? "Active summary retention is saturated. Counts are independent of retained rows; filtered counts may be unknown." : "",
          result.has_more ? "More matching active summaries are available. Select a protocol or narrower filter, or use Older when available." : ""].filter(Boolean).join(" ");
      connection(result.unavailable ? "Active traffic unavailable" : `Active traffic · sampled ${time(result.sampled_at)}`);
      render();
    } catch (error) {
      if (!disposed && mode === "active" && generation === activeGeneration) {
        cluster = null; activeStale = true;
        warning = `Active traffic unavailable: ${error.message}. Displayed rows are stale; current counts are unknown.`;
        connection("Active traffic unavailable · retrying"); render();
      }
    } finally {
      if (!disposed && !paused && mode === "active" && generation === activeGeneration) {
        element("traffic-newest").disabled = false;
        // Browsing an older active page freezes that page until Newest or filters.
        if (!older) activeTimer = setTimeout(() => active(false, true), 2000);
      }
    }
  }
  async function connect() {
    if (disposed || paused || mode !== "live") return;
    const generation = connectionGeneration;
    const attempt = new AbortController(); controller = attempt;
    let watchdog = setTimeout(() => attempt.abort(), 12000);
    let reader;
    try {
      const query = new URLSearchParams({ snapshot: "true" });
      for (const key of ["request_id", "service", "project_id", "key_id", "status", "failure_code", "protocol"]) if (filters[key]) query.set(key, filters[key]);
      if (filters.outcome === "failures") query.set("failures_only", "true");
      const response = await fetch(`/admin-ui/admin/traffic/live?${query}`, { headers: { ...headers(), ...(cursor ? { "last-event-id": cursor } : {}) }, signal: attempt.signal, cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}; check admin access and gateway health`);
      reader = response.body.getReader();
      const decoder = new TextDecoder(); let pending = "";
      while (!disposed && mode === "live" && !paused && generation === connectionGeneration) {
        const { value, done } = await reader.read();
        // An aborted read may still resolve with buffered data after a mode change.
        if (done || disposed || paused || mode !== "live" || generation !== connectionGeneration) break;
        clearTimeout(watchdog); watchdog = setTimeout(() => attempt.abort(), 12000);
        pending += decoder.decode(value, { stream: true });
        const parsed = parseTrafficFrames(pending); pending = parsed.remainder;
        for (const batch of parsed.batches) {
          if (instance !== "Connecting" && instance !== batch.instance_id) warning = "Gateway instance changed. Live records now cover the new instance; use saved history for other instances.";
          if (batch.gap) warning = "Updates were missed during disconnection or journal eviction. Retained records were reloaded; use saved history for completed requests.";
          else if (!warning && batch.evicted_updates) warning = "The live journal has discarded older updates. This is a retained window, not a complete traffic count.";
          instance = batch.instance_id; cursor = batch.cursor; rows = mergeTrafficRows(rows, batch);
          connection("Live"); if (batch.rows.length || batch.gap || batch.active) render();
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
  async function history(older = false, polling = false) {
    if (disposed || !isHistory() || (polling && paused)) return;
    const before = older ? historyCursor : null;
    if (older && !before) return;
    clearTimeout(historyTimer);
    historyAtNewest = !older;
    if (!polling && !older) historyCursor = null;
    element("traffic-older").disabled = true;
    element("traffic-newest").disabled = true;
    const generation = ++historyGeneration;
    if (!polling) { if (!older) rows = []; selected = null; render(); connection("Loading saved history"); }
    try {
      const query = new URLSearchParams({ limit: "100", time_basis: mode === "legacy" ? "started" : "completed" });
      for (const key of ["request_id", "service", "project_id", "key_id", "status", "failure_code", "protocol"]) if (filters[key]) query.set(key, filters[key]);
      if (filters.outcome === "active") throw new Error("Active requests are available in Live instance mode.");
      if (filters.outcome === "failures") query.set("failures_only", "true");
      if (!older || !historyWindow) {
        const end = filters.to ? new Date(filters.to) : new Date();
        historyWindow = { from: filters.from ? new Date(filters.from).toISOString() : new Date(end.getTime() - (mode === "legacy" ? 24 * 60 : 30) * 60 * 1000).toISOString(), to: end.toISOString() };
      }
      for (const key of ["from", "to"]) query.set(key, historyWindow[key]);
      if (before) { query.set("before", mode === "legacy" ? before.started_at : before.completed_at || before.started_at); query.set("before_id", before.id); }
      const result = await api(`/admin-ui/admin/traffic/history?${query}`);
      if (disposed || !isHistory() || generation !== historyGeneration) return;
      rows = result; historyCursor = rows.at(-1); if (!polling) selected = null;
      element("traffic-older").disabled = result.length < 100;
      warning = "";
      connection(`Saved history${paused ? " · paused" : mode === "legacy" ? " by arrival" : historyAtNewest && !filters.to ? " · newest refreshes every five seconds" : " · frozen window"}`); render();
    } catch (error) {
      if (!disposed && isHistory() && generation === historyGeneration) {
        warning = `History unavailable: ${error.message}${polling ? ". Displayed rows are stale; retrying." : ""}`;
        if (polling || older) element("traffic-older").disabled = !historyCursor || rows.length < 100;
        connection("History unavailable"); render();
      }
    } finally {
      if (!disposed && isHistory() && generation === historyGeneration) {
        element("traffic-newest").disabled = false;
        if (mode === "history" && historyAtNewest && !filters.to && !paused) historyTimer = setTimeout(() => history(false, true), 5000);
      }
    }
  }
  element("traffic-filters").addEventListener("submit", (event) => {
    event.preventDefault();
    filters = Object.fromEntries([...new FormData(event.currentTarget)].map(([name, value]) => [name, String(value).trim()]));
    for (const name of ["project_id", "key_id"]) if (filters[name]) filters[name] = filters[name].toLowerCase();
    if (filters.status) filters.status = String(Number(filters.status));
    onFilters(filters);
    for (const [name, value] of Object.entries(filters)) {
      const field = event.currentTarget.elements.namedItem(name);
      if (field) field.value = value;
    }
    refresh();
  });
  element("traffic-mode").addEventListener("change", (event) => {
    mode = event.target.value; stopConnection(); historyGeneration++; rows = []; selected = null; warning = ""; cluster = null;
    element("traffic-pages").classList.toggle("hidden", mode === "live");
    element("traffic-instance-label").classList.toggle("hidden", mode !== "active");
    element("traffic-pause").disabled = false;
    cursor = null; paused = false; element("traffic-pause").textContent = "Pause";
    refresh();
  });
  element("traffic-instance").addEventListener("change", event => { selectedInstance = event.target.value; active(); });
  element("traffic-pause").addEventListener("click", () => {
    paused = !paused; element("traffic-pause").textContent = paused ? "Resume" : "Pause";
    if (paused) { stopConnection(); connection("Paused · updates are not being received"); }
    else if (mode === "active") {
      if (activeAtNewest) active();
      else {
        element("traffic-older").disabled = !activeCursor;
        element("traffic-newest").disabled = false;
        connection("Active traffic · older page remains frozen");
      }
    }
    else if (isHistory()) {
      if (historyAtNewest) history(false, true);
      else {
        element("traffic-older").disabled = !historyCursor || rows.length < 100;
        element("traffic-newest").disabled = false;
        connection("Saved history · older page remains frozen");
      }
    }
    else connect();
  });
  element("traffic-protocols").querySelectorAll("[data-traffic-protocol]").forEach(button => button.addEventListener("click", () => {
    filters = { ...filters, protocol: button.dataset.trafficProtocol };
    element("traffic-filters").elements.namedItem("protocol").value = filters.protocol;
    onFilters(filters); refresh();
  }));
  element("traffic-newest").addEventListener("click", () => mode === "active" ? active() : history());
  element("traffic-older").addEventListener("click", () => mode === "active" ? active(true) : history(true));
  const elapsedTimer = setInterval(() => {
    if (disposed || paused || isHistory()) return;
    if (!staleRow(rows.find(row => row.id === selected) || {})) refreshWebSocketMetrics?.(element("traffic-detail"));
    for (const row of rows) {
      if (row.completed || staleRow(row)) continue;
      const cell = content.querySelector(`[data-traffic-elapsed="${row.id}"]`);
      if (cell) cell.textContent = trafficDuration(Math.max(row.elapsed_ms, Date.now() - Date.parse(row.started_at)));
    }
  }, 1000);
  render(); if (mode === "active") active(); else if (isHistory()) history(); else connect();
  return () => { closeDetail?.(); disposed = true; historyGeneration++; clearInterval(elapsedTimer); stopConnection(); };
}
