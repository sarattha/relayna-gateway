// Local-only UI fixture for Computer Use; no real accounts, secrets or writes.
// Run: node tests/fixtures/admin-ui-owner-preview.mjs (127.0.0.1:18463).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
const assets = new URL(
  "../../crates/gateway-api/src/static/admin-ui/",
  import.meta.url,
);
const project = {
  id: "00000000-0000-0000-0000-000000000001",
  name: "Design validation",
  service_names: ["fixture-service"],
};
const service = {
  name: "fixture-service",
  route_pattern: "/services/fixture/*",
  enabled: true,
  allowed_methods: ["POST"],
};
const dashboard = {
  role: "viewer",
  summary: {
    request_count: 0,
    failure_count: 0,
    p95_latency_ms: null,
    estimated_cost_usd: 0,
  },
  timeseries: [],
  version_markers: [],
  status_codes: [200, 500],
  endpoints: [],
  providers: [],
  models: [],
  services: [],
};
const server = createServer(async (req, res) => {
  const path = new URL(req.url, "http://127.0.0.1").pathname;
  const send = (value) => {
    res.setHeader("Content-Type", "application/json");
    res.end(JSON.stringify(value));
  };
  if (path.startsWith("/fixture/")) {
    const status = path.split("/").at(-1);
    if (!["active", "pending", "blocked"].includes(status)) {
      res.statusCode = 404;
      res.end();
      return;
    }
    res.writeHead(302, {
      "Set-Cookie": `fixture_member=${status}; Path=/; SameSite=Strict`,
      Location: "/admin-ui#/my-services",
    });
    res.end();
    return;
  }
  if (req.method !== "GET") {
    res.statusCode = 405;
    send({ error: { message: "Read-only fixture" } });
    return;
  }
  if (path === "/admin-ui/auth/config") {
    send({ enabled: true });
    return;
  }
  if (path === "/admin-ui/auth/session") {
    send({
      authenticated: true,
      member: {
        status:
          /fixture_member=(pending|blocked)/.exec(
            req.headers.cookie || "",
          )?.[1] || "active",
        display_name: "UI validation viewer",
        email: "viewer@example.test",
        roles: [],
      },
      service_memberships: [{ service_name: service.name, role: "viewer" }],
      project_memberships: [{ project_id: project.id, role: "viewer" }],
    });
    return;
  }
  if (path === "/owner/v1/services") {
    send([{ service, role: "viewer" }]);
    return;
  }
  if (path === "/owner/v1/projects") {
    send([{ project, role: "viewer" }]);
    return;
  }
  if (path.startsWith("/owner/v1/") && path.endsWith("/dashboard")) {
    send(dashboard);
    return;
  }
  if (path.startsWith("/owner/v1/") && path.endsWith("/events")) {
    send({ rows: [], offset: 0, has_more: false });
    return;
  }
  const file =
    path === "/admin-ui" || path === "/admin-ui/"
      ? "index.html"
      : path.startsWith("/admin-ui/")
        ? path.slice("/admin-ui/".length)
        : "";
  if (!file || file.includes("/") || file.includes("..")) {
    res.statusCode = 404;
    send({ error: { message: "Not in fixture" } });
    return;
  }
  try {
    const data = await readFile(new URL(file, assets));
    res.setHeader(
      "Content-Type",
      file.endsWith(".html")
        ? "text/html"
        : file.endsWith(".js")
          ? "application/javascript"
          : file.endsWith(".css")
            ? "text/css"
            : file.endsWith(".svg")
              ? "image/svg+xml"
              : "application/octet-stream",
    );
    res.end(data);
  } catch {
    res.statusCode = 404;
    res.end();
  }
});
server.listen(18463, "127.0.0.1", () =>
  console.log("Read-only Owner UI fixture: http://127.0.0.1:18463/admin-ui"),
);
