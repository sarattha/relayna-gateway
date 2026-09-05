// Shared lifecycle and reliability semantics. Counters describe recorded requests,
// never current availability; availability comes from provider-health/state.
export function keyLifecycle(key, now = Date.now()) {
  if (key.revoked_at) return "revoked";
  if (key.disabled) return "disabled";
  if (key.expires_at && Date.parse(key.expires_at) <= now) return "expired";
  return key.expires_at ? "active" : "non-expiring";
}

export function reliability(row) {
  const count = Number(row.request_count || 0);
  if (count < 20) return { label: "Insufficient sample", tone: "neutral", signal: null, score: 0 };
  const signals = [["timeout_count", "Timeout rate"], ["error_count", "Error rate"], ["fallback_count", "Fallback rate"]]
    .map(([field, label]) => ({ label, rate: Number(row[field] || 0) / count }))
    .sort((left, right) => right.rate - left.rate);
  const highest = signals[0];
  if (highest.rate >= 0.05) return { label: `${highest.label} elevated`, tone: highest.rate >= 0.1 ? "bad" : "warn", signal: highest.rate, score: highest.rate };
  return { label: "Within thresholds", tone: "good", signal: null, score: 0 };
}

// These callers consume finite JSON/export responses. Live SSE uses its own
// reader and watchdog in traffic.ts and must never use this buffered helper.
export async function fetchComplete(path, options = {}, { timeoutMs = 8000, signal } = {}) {
  const controller = new AbortController();
  const sources = [options.signal, signal].filter(Boolean);
  const abort = () => controller.abort();
  sources.forEach((source) => {
    source.addEventListener("abort", abort, { once: true });
    if (source.aborted) abort();
  });
  let timedOut = false;
  const timer = setTimeout(() => { timedOut = true; controller.abort(); }, timeoutMs);
  try {
    const response = await fetch(path, { ...options, signal: controller.signal });
    const body = response.status === 204 || response.status === 205 || response.status === 304 ? null : await response.arrayBuffer();
    if (controller.signal.aborted) throw new DOMException("Request superseded", "AbortError");
    return new Response(body, { status: response.status, statusText: response.statusText, headers: response.headers });
  } catch (error) {
    if (timedOut) throw new Error("request_timeout");
    throw error;
  } finally {
    clearTimeout(timer);
    sources.forEach((source) => source.removeEventListener("abort", abort));
  }
}
