import http from "node:http";

const port = Number(process.env.PORT || "4000");
const serviceCredential = process.env.SERVICE_CREDENTIAL || "";
const litellmBaseUrl = (process.env.LITELLM_BASE_URL || "http://litellm:4000").replace(/\/+$/, "");
const litellmKey = process.env.LITELLM_KEY || "";
const litellmModel = process.env.LITELLM_MODEL || "gpt-5.4-mini";

function sendJson(response, status, body) {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
  });
  response.end(payload);
}

async function readJson(request) {
  let raw = "";
  for await (const chunk of request) {
    raw += chunk;
    if (raw.length > 16_384) {
      throw Object.assign(new Error("request too large"), { status: 413 });
    }
  }
  try {
    return JSON.parse(raw || "{}");
  } catch {
    throw Object.assign(new Error("invalid JSON body"), { status: 400 });
  }
}

function requireGatewayCredential(request) {
  if (!serviceCredential) return true;
  return request.headers.authorization === `Bearer ${serviceCredential}`;
}

async function callLiteLlm(userInput) {
  const response = await fetch(`${litellmBaseUrl}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-litellm-key": `Bearer ${litellmKey}`,
    },
    body: JSON.stringify({
      model: litellmModel,
      messages: [{ role: "user", content: userInput }],
    }),
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = body?.error?.message || body?.detail || `LiteLLM returned ${response.status}`;
    throw Object.assign(new Error(error), { status: 502, upstreamStatus: response.status, upstreamBody: body });
  }
  return body?.choices?.[0]?.message?.content ?? "";
}

const server = http.createServer(async (request, response) => {
  try {
    const url = new URL(request.url || "/", `http://${request.headers.host || "localhost"}`);
    if (request.method === "GET" && url.pathname === "/health") {
      sendJson(response, 200, { status: "ok" });
      return;
    }
    if (request.method !== "POST" || url.pathname !== "/hi") {
      sendJson(response, 404, { error: "not_found" });
      return;
    }
    if (!requireGatewayCredential(request)) {
      sendJson(response, 401, { error: "missing_or_invalid_service_credential" });
      return;
    }
    const body = await readJson(request);
    const keys = Object.keys(body);
    if (keys.length !== 1 || keys[0] !== "user_input" || typeof body.user_input !== "string") {
      sendJson(response, 400, { error: "expected_body_with_user_input_only" });
      return;
    }
    const output = await callLiteLlm(body.user_input);
    sendJson(response, 200, { output });
  } catch (error) {
    sendJson(response, error.status || 500, {
      error: error.message || "mock_service_error",
      upstream_status: error.upstreamStatus,
      upstream_body: error.upstreamBody,
    });
  }
});

server.listen(port, "0.0.0.0", () => {
  console.log(`mock-hi-service listening on ${port}`);
});
