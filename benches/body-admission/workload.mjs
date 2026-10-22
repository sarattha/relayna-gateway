import http from "node:http";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const bodyMiB = Number(process.env.BENCH_BODY_MIB ?? 5);
if (!Number.isInteger(bodyMiB) || bodyMiB < 1 || bodyMiB > 100) throw new Error("invalid benchmark body size");
const size = bodyMiB * 1024 * 1024;
const prefix = '{"payload":"';
const suffix = '"}';
const body = Buffer.alloc(size, "x");
body.write(prefix, 0);
body.write(suffix, size - suffix.length);
const digest = createHash("sha256").update(body).digest("hex");
const agent = new http.Agent({ keepAlive: true, maxSockets: 256 });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

function request(url, { method = "GET", token, data, timeout = 30000 } = {}) {
  return new Promise((resolve, reject) => {
    const start = performance.now();
    let received = false;
    let deadline;
    let settled = false;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      if (error) reject(error); else resolve(value);
    };
    const req = http.request(url, {
      method, agent,
      headers: {
        ...(token ? { authorization: `Bearer ${token}` } : {}),
        ...(data ? { "content-type": "application/json", "content-length": data.length } : {}),
      },
    }, res => {
      received = true;
      const ttfb = performance.now() - start;
      const chunks = [];
      let bytes = 0;
      res.on("data", chunk => {
        bytes += chunk.length;
        if (bytes > 65536) res.destroy(new Error("unexpected large benchmark response"));
        else chunks.push(chunk);
      });
      res.on("error", error => finish(error));
      res.on("end", () => finish(null, {
        status: res.statusCode, ms: performance.now() - start,
        ttfb_ms: ttfb, text: Buffer.concat(chunks).toString(),
      }));
    });
    // An early rejection may close the upload socket after returning its 503.
    req.on("error", error => { if (!received) finish(error); });
    req.setTimeout(timeout, () => req.destroy(new Error("request timeout")));
    const deadlineMs = Number(process.env.BENCH_CLIENT_DEADLINE_MS ?? 0);
    if (deadlineMs > 0) deadline = setTimeout(() => {
      const error = new Error("benchmark wall-clock deadline");
      finish(error);
      req.destroy(error);
    }, deadlineMs);
    req.end(data);
  });
}

async function seed(control) {
  const token = process.env.ADMIN_TOKEN;
  const post = async (path, value) => {
    const result = await request(control + path, { method: "POST", token, data: Buffer.from(JSON.stringify(value)) });
    if (result.status < 200 || result.status >= 300) throw new Error(`seed ${path}: ${result.status} ${result.text}`);
    return JSON.parse(result.text);
  };
  // An empty installation otherwise falls back to a LiteLLM-only global
  // policy. Explicitly configure this isolated namespace for service traffic.
  await post("/admin-ui/admin/policy-layers", { kind: "global", policy: {}, guardrail_policy: {} });
  const existing = await request(control + "/admin-ui/admin/services/body-benchmark", { token });
  const maxBodyBytes = Math.max(8 * 1024 * 1024, size);
  const serviceTimeout = Number(process.env.BENCH_SERVICE_TIMEOUT_MS ?? 30000);
  if (existing.status === 404) await post("/admin-ui/admin/services", {
    name: "body-benchmark", route_pattern: "/services/body-benchmark/*",
    upstream_base_url: "http://mock:4000", credential: process.env.SERVICE_TOKEN,
    allowed_methods: ["POST"], max_body_bytes: maxBodyBytes,
    timeout_ms: serviceTimeout, cost_mode: "none",
  });
  else if (existing.status !== 200) throw new Error(`service lookup: ${existing.status} ${existing.text}`);
  else {
    const updated = await request(control + "/admin-ui/admin/services/body-benchmark", {
      method: "PATCH", token, data: Buffer.from(JSON.stringify({ max_body_bytes: maxBodyBytes, timeout_ms: serviceTimeout })),
    });
    if (updated.status !== 200) throw new Error(`service update: ${updated.status} ${updated.text}`);
  }
  const created = await post("/admin-ui/admin/keys", {
    name: "body-admission-benchmark", owner_type: "individual",
    service_names: ["body-benchmark"],
    policy: {
      allowed_routes: ["/services/*"], allowed_services: ["body-benchmark"],
      allowed_providers: ["internal-service"], allow_streaming: true, allow_tools: true,
      max_request_body_bytes: maxBodyBytes,
    },
  });
  writeFileSync("/tmp/benchmark-key", created.raw_key, { mode: 0o600 });
  console.log(JSON.stringify({ key_id: created.key.id, service: "body-benchmark" }));
}

function distribution(values) {
  const sorted = [...values].sort((a, b) => a - b);
  if (!sorted.length) return null;
  const percentile = p => sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)];
  return {
    count: sorted.length, min: sorted[0], mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    p50: percentile(0.5), p95: percentile(0.95), p99: percentile(0.99), max: sorted.at(-1),
  };
}

function metrics(text) {
  const value = name => Number(text.match(new RegExp(`^${name} ([0-9.e+\\-]+)$`, "m"))?.[1] ?? 0);
  const denial = reason => text.split("\n").filter(line => line.startsWith("gateway_denials_total{") && line.includes('kind="body_admission"') && line.includes(`reason="${reason}"`)).reduce((total, line) => total + Number(line.split(" ").at(-1)), 0);
  return {
    buffered_requests: value("gateway_buffered_requests"),
    buffered_bytes: value("gateway_buffered_body_bytes"),
    active_requests: value("gateway_active_requests"),
    concurrency_denials: denial("concurrency"), byte_denials: denial("bytes"),
  };
}

async function run(config) {
  agent.maxSockets = Math.max(256, config.concurrent_per_pod ?? 0);
  const token = readFileSync("/tmp/benchmark-key", "utf8");
  const results = [];
  const samples = [];
  let sampling = true;
  const fetchMetrics = async () => Promise.all(config.pods.map(async pod => {
    try {
      const res = await request(`http://${pod.ip}:8081/admin-ui/metrics`, { token: process.env.ADMIN_TOKEN });
      if (res.status !== 200) throw new Error(`metrics status ${res.status}`);
      samples.push({ timestamp_ms: Date.now(), pod: pod.name, ...metrics(res.text) });
    } catch (error) {
      samples.push({ timestamp_ms: Date.now(), pod: pod.name, error: error.message });
    }
  }));
  const send = async (pod, scheduled, warmup = false) => {
    const dispatchLag = performance.now() - scheduled;
    try {
      const res = await request(`http://${pod.ip}:8080/services/body-benchmark/${warmup ? "warmup" : "work"}`, { method: "POST", token, data: body });
      let verification = true;
      if (res.status === 200) {
        const upstream = JSON.parse(res.text);
        verification = upstream.bytes === size && upstream.sha256 === digest && upstream.credential_ok === true;
      }
      results.push({ pod: pod.name, status: res.status, ms: res.ms, ttfb_ms: res.ttfb_ms, dispatch_lag_ms: dispatchLag, verified: verification,
        ...(res.status !== 200 ? { error_code: JSON.parse(res.text).error?.code } : {}) });
    } catch (error) {
      results.push({ pod: pod.name, status: 0, error: error.message, dispatch_lag_ms: dispatchLag });
    }
  };
  // Warm authentication, database pools, code paths, and allocators equally.
  await Promise.all(config.pods.map(pod => send(pod, performance.now(), true)));
  if (results.some(result => result.status !== 200 || !result.verified)) {
    process.stdout.write(JSON.stringify({ ...config, body_bytes: size, body_sha256: digest, warmup_failed: true, warmup_results: results }) + "\n");
    return;
  }
  results.length = 0;
  await sleep(500);
  await fetchMetrics();
  console.error("MEASUREMENT_START");
  const sampleTask = (async () => { while (sampling) { await fetchMetrics(); await sleep(250); } })();
  const start = performance.now();
  const startUnix = Date.now();
  const count = config.concurrent_per_pod ? 2 * config.concurrent_per_pod : Math.round(config.duration_seconds * config.rps);
  await Promise.all(Array.from({ length: count }, (_, i) => new Promise(resolve => {
    const scheduled = config.concurrent_per_pod ? start : start + i * 1000 / config.rps;
    setTimeout(() => send(config.pods[i % 2], scheduled).then(resolve), Math.max(0, scheduled - performance.now()));
  })));
  const elapsed = performance.now() - start;
  await sleep(500);
  sampling = false;
  await sampleTask;
  await fetchMetrics();
  const successes = results.filter(result => result.status === 200);
  const rejected = results.filter(result => result.status === 503);
  const counts = {};
  for (const result of results) counts[result.status] = (counts[result.status] ?? 0) + 1;
  const output = {
    ...config, body_bytes: size, body_sha256: digest, started_at: new Date(startUnix).toISOString(),
    elapsed_ms: elapsed, scheduled_requests: count, completed_requests: results.length,
    successful_requests: successes.length, status_counts: counts,
    verified_requests: successes.filter(result => result.verified).length,
    http_max_sockets_per_host: agent.maxSockets,
    success_rps_offered_window: config.concurrent_per_pod ? null : successes.length / config.duration_seconds,
    success_rps_including_drain: successes.length / (elapsed / 1000),
    success_latency_ms: distribution(successes.map(result => result.ms)),
    success_ttfb_ms: distribution(successes.map(result => result.ttfb_ms)),
    rejection_latency_ms: distribution(rejected.map(result => result.ms)),
    dispatch_lag_ms: distribution(results.map(result => result.dispatch_lag_ms)),
    results, samples,
  };
  process.stdout.write(JSON.stringify(output) + "\n");
}

try {
  if (process.argv[2] === "seed") await seed(process.argv[3]);
  else await run(JSON.parse(process.argv[2]));
} finally {
  agent.destroy();
}
