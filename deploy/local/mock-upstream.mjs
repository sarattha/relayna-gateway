import http from "node:http";
import { randomUUID } from "node:crypto";

const host = process.env.MOCK_UPSTREAM_HOST ?? "0.0.0.0";
const port = Number(process.env.MOCK_UPSTREAM_PORT ?? 4000);

function json(response, status, body) {
  response.writeHead(status, {
    "content-type": "application/json",
    "cache-control": "no-store",
    "x-request-id": `mock-${randomUUID()}`,
  });
  response.end(JSON.stringify(body));
}

function readJson(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
      if (body.length > 1_048_576) request.destroy();
    });
    request.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    request.on("error", reject);
  });
}

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "localhost"}`);
  if (url.pathname === "/health") return json(response, 200, { status: "ok", service: "relayna-mock-upstream" });
  if (url.pathname === "/v1/models") return json(response, 200, {
    object: "list",
    data: [
      { id: "gpt-4.1-mini", object: "model", owned_by: "relayna-local" },
      { id: "gpt-4.1", object: "model", owned_by: "relayna-local" },
    ],
  });
  if (["/chat/completions", "/v1/chat/completions"].includes(url.pathname) && request.method === "POST") {
    let body;
    try {
      body = await readJson(request);
    } catch {
      return json(response, 400, { error: { message: "Request body must be valid JSON.", type: "invalid_request_error" } });
    }
    const id = `chatcmpl-${randomUUID()}`;
    const model = body.model ?? "gpt-4.1-mini";
    return json(response, 200, {
      id,
      object: "chat.completion",
      created: Math.floor(Date.now() / 1000),
      model,
      choices: [{ index: 0, message: { role: "assistant", content: "Hello from the Relayna local mock upstream." }, finish_reason: "stop" }],
      usage: { prompt_tokens: 12, completion_tokens: 10, total_tokens: 22 },
      relayna_mock_path: url.pathname,
    });
  }
  if (["/responses", "/v1/responses"].includes(url.pathname) && request.method === "POST") {
    let body;
    try {
      body = await readJson(request);
    } catch {
      return json(response, 400, { error: { message: "Request body must be valid JSON.", type: "invalid_request_error" } });
    }
    return json(response, 200, {
      id: `resp-${randomUUID()}`,
      object: "response",
      status: "completed",
      model: body.model ?? "gpt-4.1-mini",
      output: [],
      usage: { input_tokens: 12, output_tokens: 10, total_tokens: 22 },
      relayna_mock_path: url.pathname,
    });
  }
  json(response, 404, { error: { message: "Mock endpoint not found.", type: "not_found" } });
});

server.listen(port, host, () => {
  console.log(`Relayna mock upstream listening on http://${host}:${port}`);
});
