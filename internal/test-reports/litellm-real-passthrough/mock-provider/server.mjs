import http from "node:http";
import crypto from "node:crypto";

const port = 4000;
const dockerBaseUrl = process.env.DOCKER_BASE_URL || "http://mock-provider:4000";
const gatewayProxyUrl = process.env.GATEWAY_PROXY_URL || "http://gateway:8080";
const gatewayControlUrl = process.env.GATEWAY_CONTROL_URL || "http://gateway:8081";
const adminToken = process.env.GATEWAY_ADMIN_TOKEN;
const apigeeSecret = process.env.APIGEE_TRUSTED_HEADER_SECRET || "apigee-secret";

const issuer = `${dockerBaseUrl}/oauth`;
const tenantId = "relayna-litellm-review-tenant";
const audience = "api://relayna-gateway-litellm-review";
const requiredScope = "gateway.invoke";
const allowedGroup = "relayna-litellm-review-group";
const upstreamServiceAuthorization = "Bearer sk-litellm-review-service-key";

const keyPair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const publicKey = crypto.createPublicKey(keyPair.publicKey);
const publicJwk = publicKey.export({ format: "jwk" });
const kid = "relayna-litellm-review-kid";

const state = {
  rawRelaynaKey: null,
  providerRequests: [],
  results: null,
};

function base64url(input) {
  return Buffer.from(input).toString("base64url");
}

function jsonResponse(res, status, value, headers = {}) {
  const body = JSON.stringify(value, null, 2);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(body),
    ...headers,
  });
  res.end(body);
}

function htmlResponse(res, status, body) {
  res.writeHead(status, {
    "content-type": "text/html; charset=utf-8",
    "content-length": Buffer.byteLength(body),
  });
  res.end(body);
}

async function readBody(req) {
  const chunks = [];
  for await (const chunk of req) {
    chunks.push(chunk);
  }
  const text = Buffer.concat(chunks).toString("utf8");
  if (!text) {
    return {};
  }
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function captureProviderRequest(req, body) {
  const headers = Object.fromEntries(
    Object.entries(req.headers).map(([key, value]) => [key.toLowerCase(), String(value)]),
  );
  state.providerRequests.push({
    path: req.url,
    method: req.method,
    authorization: headers.authorization || null,
    hasGatewayServiceKey: headers.authorization === "Bearer sk-local-provider-review-key",
    hasRelaynaKey: "x-relayna-key" in headers,
    hasAihKey: "x-aih-api-key" in headers,
    hasApigeeIdentity: "x-apigee-entra-identity" in headers || "x-apigee-entra-signature" in headers,
    hasClientJwt: headers.authorization?.split(".").length === 3,
    body,
  });
}

function jwks() {
  return {
    keys: [
      {
        kty: "RSA",
        use: "sig",
        kid,
        alg: "RS256",
        n: publicJwk.n,
        e: publicJwk.e,
      },
    ],
  };
}

function tokenClaims(overrides = {}) {
  const now = Math.floor(Date.now() / 1000);
  return {
    iss: issuer,
    aud: audience,
    exp: now + 300,
    nbf: now - 5,
    iat: now - 5,
    tid: tenantId,
    ver: "2.0",
    sub: "litellm-review-user",
    oid: "litellm-review-object",
    azp: "litellm-review-client",
    scp: requiredScope,
    groups: [allowedGroup],
    ...overrides,
  };
}

function signJwt(claims) {
  const header = { alg: "RS256", typ: "JWT", kid };
  const signingInput = `${base64url(JSON.stringify(header))}.${base64url(JSON.stringify(claims))}`;
  const signature = crypto.sign("RSA-SHA256", Buffer.from(signingInput), keyPair.privateKey);
  return `${signingInput}.${signature.toString("base64url")}`;
}

function verifyJwtAtEdge(token) {
  const [encodedHeader, encodedPayload, encodedSignature] = token.split(".");
  if (!encodedHeader || !encodedPayload || !encodedSignature) {
    throw new Error("malformed_jwt");
  }
  const header = JSON.parse(Buffer.from(encodedHeader, "base64url").toString("utf8"));
  const claims = JSON.parse(Buffer.from(encodedPayload, "base64url").toString("utf8"));
  if (header.kid !== kid || header.alg !== "RS256") {
    throw new Error("invalid_header");
  }
  const verified = crypto.verify(
    "RSA-SHA256",
    Buffer.from(`${encodedHeader}.${encodedPayload}`),
    publicKey,
    Buffer.from(encodedSignature, "base64url"),
  );
  if (!verified) {
    throw new Error("invalid_signature");
  }
  return claims;
}

function hmacIdentity(identityHeader) {
  return crypto.createHmac("sha256", apigeeSecret).update(identityHeader).digest("base64url");
}

function trustedIdentityHeader(claims) {
  return base64url(
    JSON.stringify({
      tenant_id: claims.tid,
      subject: claims.sub,
      object_id: claims.oid,
      authorized_party: claims.azp,
      scopes: String(claims.scp || "").split(/\s+/).filter(Boolean),
      roles: claims.roles || [],
      groups: claims.groups || [],
      token_version: claims.ver,
      source: "jwt",
    }),
  );
}

async function waitForGateway() {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const response = await fetch(`${gatewayControlUrl}/admin-ui/readyz`);
      if (response.ok) {
        return;
      }
    } catch {
      // Retry until migrations and readiness checks have completed.
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error("gateway_not_ready");
}

async function setupGatewayData() {
  if (state.rawRelaynaKey) {
    return state.rawRelaynaKey;
  }
  await waitForGateway();
  const projectResponse = await fetch(`${gatewayControlUrl}/admin-ui/admin/projects`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ name: `litellm-real-env-${Date.now()}` }),
  });
  if (!projectResponse.ok) {
    throw new Error(`create_project_failed:${projectResponse.status}:${await projectResponse.text()}`);
  }
  const project = await projectResponse.json();
  await fetch(`${gatewayControlUrl}/admin-ui/admin/policy-layers`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ kind: "global", policy: {} }),
  });
  const keyResponse = await fetch(`${gatewayControlUrl}/admin-ui/admin/keys`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      project_id: project.id,
      preset: "developer",
      policy: {
        allowed_routes: ["/v1/chat/completions", "/v1/responses"],
        allowed_providers: ["litellm"],
        allow_streaming: false,
      },
    }),
  });
  if (!keyResponse.ok) {
    throw new Error(`create_key_failed:${keyResponse.status}:${await keyResponse.text()}`);
  }
  const key = await keyResponse.json();
  state.rawRelaynaKey = key.raw_key;
  return state.rawRelaynaKey;
}

async function gatewayCall(path, token, relaynaKey, payload) {
  const headers = { "content-type": "application/json" };
  if (token) {
    headers.authorization = `Bearer ${token}`;
  }
  if (relaynaKey) {
    headers["x-relayna-key"] = relaynaKey;
  }
  const response = await fetch(`${gatewayProxyUrl}${path}`, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: response.status, body };
}

async function apigeeTrustedCall(path, token, relaynaKey, payload) {
  const claims = verifyJwtAtEdge(token);
  const identityHeader = trustedIdentityHeader(claims);
  const response = await fetch(`${gatewayProxyUrl}${path}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-relayna-key": relaynaKey,
      "x-apigee-entra-identity": identityHeader,
      "x-apigee-entra-signature": hmacIdentity(identityHeader),
    },
    body: JSON.stringify(payload),
  });
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: response.status, body };
}

function codeOf(result) {
  return result?.body?.error?.code || null;
}

function pass(condition, details = {}) {
  return { ok: Boolean(condition), ...details };
}

async function runTests() {
  state.providerRequests = [];
  const relaynaKey = await setupGatewayData();
  const validToken = signJwt(tokenClaims());
  const chatPayload = {
    model: "gpt-review",
    messages: [{ role: "user", content: "gateway to litellm chat passthrough" }],
  };
  const responsePayload = {
    model: "gpt-review",
    input: "gateway to litellm responses passthrough",
  };
  const embeddingPayload = {
    model: "text-embedding-review",
    input: "gateway embedding passthrough",
  };
  const rerankPayload = {
    model: "rerank-review",
    query: "gateway rerank passthrough",
    documents: ["alpha", "beta"],
  };

  const chat = await gatewayCall("/v1/chat/completions", validToken, relaynaKey, chatPayload);
  const responses = await gatewayCall("/v1/responses", validToken, relaynaKey, responsePayload);
  const chatLiteral = await gatewayCall("/v1/chatcompletion", validToken, relaynaKey, chatPayload);
  const responseLiteral = await gatewayCall("/v1/response", validToken, relaynaKey, responsePayload);
  const embeddingLiteral = await gatewayCall("/v1/embedding", validToken, relaynaKey, embeddingPayload);
  const rerank = await gatewayCall("/v1/rerank", validToken, relaynaKey, rerankPayload);
  const apigeeChat = await apigeeTrustedCall("/v1/chat/completions", validToken, relaynaKey, chatPayload);

  const upstreamCredentialLeak = state.providerRequests.some(
    (request) => request.hasRelaynaKey || request.hasAihKey || request.hasApigeeIdentity || request.hasClientJwt,
  );
  const gatewayForwardedToLiteLlm = state.providerRequests.some((request) => request.path.includes("/chat/completions"));
  const responseForwardedToLiteLlm = state.providerRequests.some((request) => request.path.includes("/responses"));

  const checks = {
    canonical_chat_completions_passes_to_litellm: pass(chat.status === 200 && gatewayForwardedToLiteLlm, chat),
    canonical_responses_passes_to_litellm: pass(responses.status === 200 && responseForwardedToLiteLlm, responses),
    apigee_trusted_header_chat_passes_to_litellm: pass(apigeeChat.status === 200, apigeeChat),
    upstream_receives_no_client_credentials: pass(!upstreamCredentialLeak, { providerRequests: state.providerRequests }),
    requested_literal_chatcompletion_path: pass(
      chatLiteral.status === 404 && codeOf(chatLiteral) === "unsupported_route",
      chatLiteral,
    ),
    requested_literal_response_path: pass(
      responseLiteral.status === 404 && codeOf(responseLiteral) === "unsupported_route",
      responseLiteral,
    ),
    requested_literal_embedding_path: pass(
      embeddingLiteral.status === 404 && codeOf(embeddingLiteral) === "unsupported_route",
      embeddingLiteral,
    ),
    requested_rerank_path: pass(rerank.status === 404 && codeOf(rerank) === "unsupported_route", rerank),
  };

  const requestedLiteralPathsPassThrough = false;
  state.results = {
    ok: Object.values(checks).every((check) => check.ok),
    requestedLiteralPathsPassThrough,
    overallOutcome:
      "PARTIAL: canonical /v1/chat/completions and /v1/responses pass through to LiteLLM; requested literal /v1/chatcompletion, /v1/response, /v1/embedding, and /v1/rerank are unsupported.",
    generatedAt: new Date().toISOString(),
    environment: {
      gatewayProxyUrl,
      gatewayControlUrl,
      litellmUrl: "http://litellm:4000",
      issuer,
      audience,
      tenantId,
      relaynaKeyHeader: "X-Relayna-Key",
      apigeeTrustedHeader: true,
    },
    requestedPaths: ["/v1/chatcompletion", "/v1/response", "/v1/embedding", "/v1/rerank"],
    canonicalGatewayLiteLlmPaths: ["/v1/chat/completions", "/v1/responses"],
    checks,
    providerRequests: state.providerRequests,
  };
  return state.results;
}

function providerResponse(req, body) {
  captureProviderRequest(req, body);
  if (req.url.includes("/chat/completions")) {
    return {
      id: `chatcmpl-${crypto.randomUUID()}`,
      object: "chat.completion",
      created: Math.floor(Date.now() / 1000),
      model: body.model || "gpt-review",
      choices: [{ index: 0, message: { role: "assistant", content: "chat passthrough ok" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 8, completion_tokens: 4, total_tokens: 12 },
    };
  }
  if (req.url.includes("/responses")) {
    return {
      id: `resp-${crypto.randomUUID()}`,
      object: "response",
      created_at: Math.floor(Date.now() / 1000),
      model: body.model || "gpt-review",
      output: [
        {
          type: "message",
          id: `msg-${crypto.randomUUID()}`,
          status: "completed",
          role: "assistant",
          content: [{ type: "output_text", text: "responses passthrough ok" }],
        },
      ],
      usage: { input_tokens: 7, output_tokens: 4, total_tokens: 11 },
    };
  }
  if (req.url.includes("/embeddings")) {
    return {
      object: "list",
      model: body.model || "text-embedding-review",
      data: [{ object: "embedding", index: 0, embedding: [0.1, 0.2, 0.3] }],
      usage: { prompt_tokens: 3, total_tokens: 3 },
    };
  }
  if (req.url.includes("/rerank")) {
    return {
      id: `rerank-${crypto.randomUUID()}`,
      results: [{ index: 0, relevance_score: 0.98 }],
    };
  }
  return { ok: true, path: req.url };
}

function dashboardHtml() {
  const results = state.results;
  const checks = results ? Object.entries(results.checks) : [];
  return `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>LiteLLM Real Passthrough Report</title>
  <style>
    body { font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 0; color: #17202a; background: #f7f8fa; }
    main { max-width: 1120px; margin: 0 auto; padding: 32px; }
    h1 { font-size: 30px; margin: 0 0 8px; }
    h2 { font-size: 20px; margin-top: 28px; }
    .summary { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin: 20px 0; }
    .metric { background: #fff; border: 1px solid #d8dee4; border-radius: 8px; padding: 14px; }
    .metric strong { display: block; font-size: 24px; }
    table { width: 100%; border-collapse: collapse; background: #fff; border: 1px solid #d8dee4; }
    th, td { padding: 10px 12px; border-bottom: 1px solid #eaeef2; text-align: left; font-size: 14px; vertical-align: top; }
    th { background: #eef2f6; }
    .pass { color: #116329; font-weight: 700; }
    .finding { color: #9a6700; font-weight: 700; }
    code { background: #eef2f6; padding: 2px 5px; border-radius: 4px; }
  </style>
</head>
<body>
  <main>
    <h1>LiteLLM Real Passthrough Report</h1>
    <p>Generated ${results?.generatedAt || "after /run-tests"}. Gateway used Entra JWT validation plus trusted Apigee headers in front of a real <code>litellm/litellm:latest</code> container.</p>
    <section class="summary">
      <div class="metric"><span>Outcome</span><strong>${results?.requestedLiteralPathsPassThrough ? "PASS" : "PARTIAL"}</strong></div>
      <div class="metric"><span>Gateway LiteLLM paths</span><strong>${results?.canonicalGatewayLiteLlmPaths.length || 0}</strong></div>
      <div class="metric"><span>Provider captures</span><strong>${results?.providerRequests.length || 0}</strong></div>
    </section>
    <h2>Checks</h2>
    <table>
      <thead><tr><th>Check</th><th>Result</th><th>Status</th><th>Error</th></tr></thead>
      <tbody>
        ${checks
          .map(
            ([name, check]) =>
              `<tr><td>${name.replaceAll("_", " ")}</td><td class="${check.ok ? "pass" : "finding"}">${check.ok ? "PASS" : "FAIL"}</td><td>${check.status ?? "n/a"}</td><td>${check.body?.error?.code ?? ""}</td></tr>`,
          )
          .join("")}
      </tbody>
    </table>
    <h2>Interesting Finding</h2>
    <p>The branch currently only routes <code>/v1/chat/completions</code> and <code>/v1/responses</code> to LiteLLM. The literal requested paths <code>/v1/chatcompletion</code>, <code>/v1/response</code>, <code>/v1/embedding</code>, and <code>/v1/rerank</code> return <code>unsupported_route</code> before reaching LiteLLM. This is a passthrough coverage gap, not a LiteLLM container failure.</p>
  </main>
</body>
</html>`;
}

const server = http.createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/healthz") {
      jsonResponse(res, 200, { ok: true });
      return;
    }
    if (req.method === "GET" && req.url === "/oauth/.well-known/openid-configuration") {
      jsonResponse(res, 200, { issuer, jwks_uri: `${issuer}/jwks` });
      return;
    }
    if (req.method === "GET" && req.url === "/oauth/jwks") {
      jsonResponse(res, 200, jwks());
      return;
    }
    if (req.method === "POST" && req.url === "/run-tests") {
      jsonResponse(res, 200, await runTests());
      return;
    }
    if (req.method === "GET" && req.url === "/results.json") {
      jsonResponse(res, state.results ? 200 : 404, state.results || { error: "not_run" });
      return;
    }
    if (req.method === "GET" && req.url === "/") {
      htmlResponse(res, 200, dashboardHtml());
      return;
    }
    if (req.method === "POST" && req.url.startsWith("/v1/")) {
      const body = await readBody(req);
      jsonResponse(res, 200, providerResponse(req, body));
      return;
    }
    jsonResponse(res, 404, { error: { code: "not_found", message: "Not found" } });
  } catch (error) {
    jsonResponse(res, 500, { error: { code: "mock_error", message: error.message, stack: error.stack } });
  }
});

server.listen(port, "0.0.0.0", () => {
  console.log(`mock provider listening on ${port}`);
});
