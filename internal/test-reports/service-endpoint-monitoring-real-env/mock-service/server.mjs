import http from "node:http";

const credential = process.env.SERVICE_CREDENTIAL || "";

function json(response, status, value) {
  const body = JSON.stringify(value);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
  });
  response.end(body);
}

const openapi = {
  openapi: "3.0.3",
  info: { title: "Endpoint monitoring fixture", version: "1.0.0" },
  paths: {
    "/jobs/{job_id}": {
      post: {
        operationId: "submit_job",
        responses: { "200": { description: "success" }, "503": { description: "failure" } },
      },
    },
  },
};

http.createServer((request, response) => {
  const url = new URL(request.url || "/", `http://${request.headers.host || "localhost"}`);
  if (request.method === "GET" && url.pathname === "/health") {
    json(response, 200, { status: "ok" });
    return;
  }
  if (request.method === "GET" && url.pathname === "/openapi.json") {
    json(response, 200, openapi);
    return;
  }
  if (request.headers.authorization !== `Bearer ${credential}`) {
    json(response, 401, { error: "invalid_service_credential" });
    return;
  }
  if (request.method === "POST" && url.pathname === "/jobs/ok-123") {
    json(response, 200, { status: "completed" });
    return;
  }
  if (request.method === "POST" && url.pathname === "/jobs/fail-503") {
    json(response, 503, { error: "planned_service_failure" });
    return;
  }
  if (request.method === "POST" && url.pathname === "/unlisted/fail-500") {
    json(response, 500, { error: "planned_unlisted_failure" });
    return;
  }
  json(response, 404, { error: "not_found" });
}).listen(4000, "0.0.0.0", () => {
  console.log("endpoint monitoring mock service listening on 4000");
});
