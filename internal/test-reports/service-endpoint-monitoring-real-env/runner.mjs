import fs from "node:fs";

const controlUrl = "http://127.0.0.1:19281";
const proxyUrl = "http://127.0.0.1:19280";
const adminToken = process.env.GATEWAY_ADMIN_TOKEN;
if (!adminToken) {
  throw new Error("GATEWAY_ADMIN_TOKEN is required");
}
const serviceName = "endpoint-monitor";

async function request(baseUrl, path, options = {}) {
  const response = await fetch(`${baseUrl}${path}`, options);
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: response.status, body, text };
}

async function admin(path, options = {}) {
  return request(controlUrl, path, {
    ...options,
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
      ...(options.headers || {}),
    },
  });
}

async function waitForGateway() {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const ready = await request(controlUrl, "/admin-ui/readyz");
      if (ready.status === 200) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error("gateway_not_ready");
}

function requireStatus(result, expected, label) {
  if (result.status !== expected) {
    throw new Error(`${label}: expected ${expected}, received ${result.status}: ${result.text}`);
  }
}

await waitForGateway();

const service = await admin("/admin-ui/admin/services", {
  method: "POST",
  body: JSON.stringify({
    name: serviceName,
    route_pattern: `/services/${serviceName}/*`,
    upstream_base_url: "http://mock-service:4000",
    health_check_path: "/health",
    health_check_method: "GET",
    enabled: true,
    allowed_methods: ["POST"],
    credential: "svc_endpoint_monitor_secret",
    timeout_ms: 60000,
    max_body_bytes: 1048576,
    cost_mode: "none",
    openapi_source_path: "/openapi.json",
  }),
});
requireStatus(service, 201, "create service");

const preview = await admin(`/admin-ui/admin/services/${serviceName}/openapi/preview`, {
  method: "POST",
  body: JSON.stringify({ source_path: "/openapi.json" }),
});
requireStatus(preview, 200, "preview OpenAPI");
const sync = await admin(`/admin-ui/admin/services/${serviceName}/openapi/sync`, {
  method: "POST",
  body: JSON.stringify({ source_path: "/openapi.json", schema_hash: preview.body.schema_hash }),
});
requireStatus(sync, 200, "sync OpenAPI");

const key = await admin("/admin-ui/admin/keys", {
  method: "POST",
  body: JSON.stringify({
    owner_type: "individual",
    project_id: null,
    service_names: [serviceName],
    expires_at: null,
    policy: {
      allowed_routes: ["/services/*"],
      allowed_providers: ["internal-service"],
      allowed_services: [serviceName],
      allow_streaming: false,
      allow_tools: false,
    },
  }),
});
requireStatus(key, 201, "create virtual key");
const relaynaKey = key.body.raw_key;

async function serviceCall(path) {
  return request(proxyUrl, `/services/${serviceName}${path}`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${relaynaKey}`,
      "content-type": "application/json",
    },
    body: "{}",
  });
}

const success = await serviceCall("/jobs/ok-123?ignored=secret-query-value");
const templateFailure = await serviceCall("/jobs/fail-503?ignored=secret-query-value");
const fallbackFailure = await serviceCall("/unlisted/fail-500?ignored=secret-query-value");
requireStatus(success, 200, "templated success");
requireStatus(templateFailure, 503, "templated failure");
requireStatus(fallbackFailure, 500, "fallback failure");

await new Promise((resolve) => setTimeout(resolve, 1000));
const events = await admin(`/admin-ui/admin/usage/events?service=${serviceName}&limit=20`);
requireStatus(events, 200, "usage events");
const rows = events.body.rows;
const byStatus = new Map(rows.map((row) => [row.status_code, row]));
const row200 = byStatus.get(200);
const row503 = byStatus.get(503);
const row500 = byStatus.get(500);

const dashboard = await admin(`/admin-ui/admin/usage/dashboard?service=${serviceName}&sort_by=failures`);
requireStatus(dashboard, 200, "usage dashboard");
const endpointBreakdowns = dashboard.body.breakdowns.endpoints;
const templatedBreakdown = endpointBreakdowns.find((row) => row.name === "POST /jobs/{job_id}");
const fallbackBreakdown = endpointBreakdowns.find((row) => row.name === "POST /unlisted/fail-500");

const filtered = await admin(
  `/admin-ui/admin/usage/events?service=${serviceName}&method=post&endpoint=${encodeURIComponent("/jobs/{job_id}")}&status_code=503`,
);
requireStatus(filtered, 200, "filtered usage events");

const exported = await admin(`/admin-ui/admin/usage/export.csv?service=${serviceName}`);
requireStatus(exported, 200, "usage CSV export");

const checks = {
  templated_success_recorded: Boolean(
    row200?.http_method === "POST" &&
      row200?.endpoint_path === "/jobs/ok-123" &&
      row200?.endpoint_template === "/jobs/{job_id}" &&
      !JSON.stringify(row200).includes("secret-query-value"),
  ),
  templated_failure_recorded: Boolean(
    row503?.status === "failure" &&
      row503?.http_method === "POST" &&
      row503?.endpoint_path === "/jobs/fail-503" &&
      row503?.endpoint_template === "/jobs/{job_id}",
  ),
  fallback_failure_recorded: Boolean(
    row500?.status === "failure" &&
      row500?.endpoint_path === "/unlisted/fail-500" &&
      row500?.endpoint_template == null,
  ),
  endpoint_breakdown_aggregates_template: Boolean(
    templatedBreakdown?.summary?.request_count === 2 &&
      templatedBreakdown?.summary?.success_count === 1 &&
      templatedBreakdown?.summary?.failure_count === 1,
  ),
  endpoint_breakdown_uses_path_fallback: Boolean(fallbackBreakdown?.summary?.failure_count === 1),
  exact_filters_find_failure: Boolean(filtered.body.rows?.length === 1 && filtered.body.rows[0].status_code === 503),
  csv_appends_endpoint_columns: Boolean(
    exported.text.split("\n", 1)[0].endsWith("pricing_rule_name,http_method,endpoint_path,endpoint_template"),
  ),
};

const ok = Object.values(checks).every(Boolean);
const result = {
  generated_at: new Date().toISOString(),
  ok,
  image: process.env.RELAYNA_GATEWAY_IMAGE || "unknown",
  environment: { proxy: proxyUrl, control: controlUrl, service: "http://127.0.0.1:19282" },
  requests: { success: success.status, templated_failure: templateFailure.status, fallback_failure: fallbackFailure.status },
  checks,
};

fs.writeFileSync(new URL("./results.json", import.meta.url), `${JSON.stringify(result, null, 2)}\n`);
const rowsMarkdown = Object.entries(checks)
  .map(([name, passed]) => `| ${name.replaceAll("_", " ")} | ${passed ? "PASS" : "FAIL"} |`)
  .join("\n");
fs.writeFileSync(
  new URL("./report.md", import.meta.url),
  `# Service Endpoint Monitoring Real-Environment Report\n\nGenerated: ${result.generated_at}\n\nOverall result: **${ok ? "PASS" : "FAIL"}**\n\nImage: \`${result.image}\`\n\n| Check | Result |\n| --- | --- |\n${rowsMarkdown}\n\n## Computer Use Evidence\n\nPending Admin UI verification.\n`,
);

if (!ok) {
  throw new Error(`real_environment_checks_failed:${JSON.stringify(checks)}`);
}
console.log(JSON.stringify(result, null, 2));
