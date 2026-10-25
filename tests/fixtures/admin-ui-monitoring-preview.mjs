// Read-only, loopback-only browser QA against disposable Gateway data.
// Credentials stay in the proxy process; the browser receives a synthetic BFF session.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";

const control = new URL(process.env.GATEWAY_MONITORING_CONTROL_URL || "http://127.0.0.1:58081");
if (!["127.0.0.1", "localhost", "[::1]"].includes(control.hostname)) throw new Error("Disposable loopback Gateway required");
const tokenFile = process.env.GATEWAY_MONITORING_TOKEN_FILE;
if (!tokenFile || (await stat(tokenFile)).mode & 0o077) throw new Error("A private mode-0600 token file is required");
const credential = (await readFile(tokenFile, "utf8")).trim();
const assets = new URL("../../crates/gateway-api/src/static/admin-ui/", import.meta.url);
const faultFile = process.env.GATEWAY_MONITORING_FAULT_FILE;

const server = createServer(async (request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  const send = (value, status = 200) => {
    response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
    response.end(JSON.stringify(value));
  };
  if (request.method !== "GET") return send({ error: { message: "Read-only QA fixture" } }, 405);
  if (url.pathname === "/admin-ui/auth/config") return send({ enabled: true });
  if (url.pathname === "/admin-ui/auth/session") return send({
    authenticated: true, csrf_token: "local-qa-csrf",
    member: { status: "active", display_name: "Monitoring QA", email: "qa@example.test", roles: ["admin"] },
    service_memberships: [], project_memberships: [],
  });
  if (url.pathname.startsWith("/admin-ui/admin/") || url.pathname === "/admin-ui/readyz") {
    try {
      if (faultFile) {
        const fault = JSON.parse(await readFile(faultFile, "utf8"))[url.pathname];
        if (fault?.delay_ms) await new Promise(resolve => setTimeout(resolve, Math.min(30_000, fault.delay_ms)));
        if (fault?.status) return send({ error: { message: "Synthetic QA failure" } }, fault.status);
      }
      const upstream = await fetch(new URL(url.pathname + url.search, control), {
        headers: { authorization: `Bearer ${credential}` }, signal: AbortSignal.timeout(30_000),
      });
      response.writeHead(upstream.status, {
        "content-type": upstream.headers.get("content-type") || "application/json",
        "cache-control": "no-store",
      });
      if (url.pathname.endsWith("/traffic/live")) {
        for await (const chunk of upstream.body) {
          if (response.destroyed) break;
          response.write(chunk);
        }
        response.end();
      } else response.end(Buffer.from(await upstream.arrayBuffer()));
    } catch { if (!response.headersSent) send({ error: { message: "Disposable Gateway unavailable" } }, 503); else response.destroy(); }
    return;
  }
  const file = ["/admin-ui", "/admin-ui/"].includes(url.pathname) ? "index.html" : /^\/admin-ui\/([a-zA-Z0-9._-]+)$/.exec(url.pathname)?.[1];
  if (!file || file.includes("..")) return send({ error: { message: "Not in fixture" } }, 404);
  try {
    const body = await readFile(new URL(file, assets));
    response.writeHead(200, { "content-type": file.endsWith(".html") ? "text/html" : file.endsWith(".js") ? "text/javascript" : file.endsWith(".css") ? "text/css" : file.endsWith(".svg") ? "image/svg+xml" : "application/octet-stream", "cache-control": "no-store" });
    response.end(body);
  } catch { send({ error: { message: "Missing generated asset" } }, 404); }
});
server.listen(58463, "127.0.0.1", () => console.log("Read-only monitoring QA: http://127.0.0.1:58463/admin-ui"));
