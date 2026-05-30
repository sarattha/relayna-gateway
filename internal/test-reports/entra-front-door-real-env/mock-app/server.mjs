import http from "node:http";
import crypto from "node:crypto";

const port = 4000;
const publicBaseUrl = process.env.MOCK_PUBLIC_BASE_URL || "http://localhost:18082";
const dockerBaseUrl = process.env.MOCK_DOCKER_BASE_URL || "http://mock-app:4000";
const gatewayProxyUrl = process.env.GATEWAY_PROXY_URL || "http://gateway:8080";
const gatewayControlUrl = process.env.GATEWAY_CONTROL_URL || "http://gateway:8081";
const adminToken = process.env.GATEWAY_ADMIN_TOKEN;
const apigeeSecret = process.env.APIGEE_TRUSTED_HEADER_SECRET || "apigee-secret";

function fakeProviderCredential(name) {
  return `sk-${name}`;
}

const expectedUpstreamAuthorizations = new Set([
  `Bearer ${fakeProviderCredential("litellm-review-service-key")}`,
  `Bearer ${fakeProviderCredential("direct-openai-review-service-key")}`,
  `Bearer ${fakeProviderCredential("internal-summary-review-service-key")}`,
  `Bearer ${fakeProviderCredential("internal-review-service-key")}`,
]);
const issuer = `${dockerBaseUrl}/oauth`;
const tenantId = "relayna-review-tenant";
const audience = "api://relayna-gateway-review";
const requiredScope = "gateway.invoke";
const allowedGroup = "relayna-review-group";

const keyPair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const publicKey = crypto.createPublicKey(keyPair.publicKey);
const publicJwk = publicKey.export({ format: "jwk" });
const kid = "relayna-review-kid";

const state = {
  rawRelaynaKey: null,
  projectId: null,
  upstreamRequests: [],
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

async function readJson(req) {
  const chunks = [];
  for await (const chunk of req) {
    chunks.push(chunk);
  }
  if (chunks.length === 0) {
    return {};
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
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
    sub: "review-user-subject",
    oid: "review-user-object",
    azp: "review-client",
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
  const now = Math.floor(Date.now() / 1000);
  if (claims.iss !== issuer || claims.aud !== audience || claims.tid !== tenantId) {
    throw new Error("invalid_claims");
  }
  if (claims.exp <= now || (claims.nbf && claims.nbf > now)) {
    throw new Error("invalid_time");
  }
  if (!String(claims.scp || "").split(/\s+/).includes(requiredScope)) {
    throw new Error("missing_scope");
  }
  return claims;
}

function hmacIdentity(identityHeader) {
  return crypto.createHmac("sha256", apigeeSecret).update(identityHeader).digest("base64url");
}

function trustedIdentityHeader(claims) {
  const identity = {
    tenant_id: claims.tid,
    subject: claims.sub,
    object_id: claims.oid,
    app_id: claims.appid || null,
    authorized_party: claims.azp || null,
    scopes: String(claims.scp || "").split(/\s+/).filter(Boolean),
    roles: claims.roles || [],
    groups: claims.groups || [],
    token_version: claims.ver,
    source: "jwt",
  };
  return base64url(JSON.stringify(identity));
}

async function waitForGateway() {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    try {
      const response = await fetch(`${gatewayControlUrl}/admin-ui/readyz`);
      if (response.ok) {
        return;
      }
    } catch {
      // Retry until the gateway has finished migrations and readiness checks.
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
    body: JSON.stringify({ name: `entra-real-env-${Date.now()}` }),
  });
  if (!projectResponse.ok) {
    throw new Error(`create_project_failed:${projectResponse.status}:${await projectResponse.text()}`);
  }
  const project = await projectResponse.json();
  state.projectId = project.id;
  await ensureNeutralGlobalPolicyLayer();
  await ensureService("summary", {
    route_pattern: "/summary",
    upstream_base_url: dockerBaseUrl,
    allowed_methods: ["POST"],
    credential: fakeProviderCredential("internal-summary-review-service-key"),
  });
  await ensureService("review-service", {
    route_pattern: "/services/review-service/*",
    upstream_base_url: dockerBaseUrl,
    allowed_methods: ["GET", "POST"],
    credential: fakeProviderCredential("internal-review-service-key"),
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
        allowed_routes: ["/v1/chat/completions", "/v1/responses", "/providers/openai/*", "/summary", "/services/*"],
        allowed_providers: ["litellm", "openai-compatible", "internal-service"],
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

async function ensureNeutralGlobalPolicyLayer() {
  const response = await fetch(`${gatewayControlUrl}/admin-ui/admin/policy-layers`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ kind: "global", policy: {} }),
  });
  if (!response.ok) {
    throw new Error(`upsert_neutral_global_policy_failed:${response.status}:${await response.text()}`);
  }
  return response.json();
}

async function ensureService(name, overrides) {
  const response = await fetch(`${gatewayControlUrl}/admin-ui/admin/services`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      name,
      health_check_path: "/healthz",
      health_check_method: "GET",
      timeout_ms: 60000,
      max_body_bytes: 2097152,
      cost_mode: "fixed",
      estimated_cost_usd: 0.01,
      enabled: true,
      ...overrides,
    }),
  });
  if (!response.ok) {
    throw new Error(`create_service_${name}_failed:${response.status}:${await response.text()}`);
  }
  return response.json();
}

function chatPayload(label) {
  return {
    model: "gpt-review",
    messages: [{ role: "user", content: `review ${label}` }],
  };
}

async function gatewayCall(path, token, relaynaKey, payload = chatPayload(path)) {
  const headers = {
    "content-type": "application/json",
  };
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

async function apigeeCall(path, token, relaynaKey, trusted = false, tamper = false) {
  let claims;
  try {
    claims = verifyJwtAtEdge(token);
  } catch (error) {
    return {
      status: 401,
      body: {
        error: {
          code: "apigee_verify_jwt_failed",
          message: error.message,
        },
      },
      edgeValidated: false,
    };
  }

  const headers = {
    "content-type": "application/json",
    "x-relayna-key": relaynaKey,
  };
  if (trusted) {
    const identityHeader = trustedIdentityHeader(claims);
    headers["x-apigee-entra-identity"] = identityHeader;
    headers["x-apigee-entra-signature"] = tamper ? "tampered-signature" : hmacIdentity(identityHeader);
  } else {
    headers.authorization = `Bearer ${token}`;
  }
  const response = await fetch(`${gatewayProxyUrl}${path}`, {
    method: "POST",
    headers,
    body: JSON.stringify(chatPayload(trusted ? "apigee-trusted" : "apigee-revalidate")),
  });
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: response.status, body, edgeValidated: true };
}

function codeOf(result) {
  return result?.body?.error?.code || null;
}

function pass(condition, details = {}) {
  return { ok: Boolean(condition), ...details };
}

async function runTests() {
  state.upstreamRequests = [];
  const relaynaKey = await setupGatewayData();
  const validToken = signJwt(tokenClaims());
  const wrongAudienceToken = signJwt(tokenClaims({ aud: "https://graph.microsoft.com" }));
  const expiredToken = signJwt(tokenClaims({ exp: Math.floor(Date.now() / 1000) - 120 }));
  const missingScopeToken = signJwt(tokenClaims({ scp: "gateway.read" }));
  const invalidSignatureToken = `${validToken}x`;

  const directValid = await gatewayCall("/v1/chat/completions", validToken, relaynaKey);
  const responsesValid = await gatewayCall("/v1/responses", validToken, relaynaKey, {
    model: "gpt-review",
    input: "review responses",
  });
  const directProviderValid = await gatewayCall(
    "/providers/openai/v1/chat/completions",
    validToken,
    relaynaKey,
    chatPayload("direct-provider"),
  );
  const summaryValid = await gatewayCall("/summary", validToken, relaynaKey, {
    input: "summarize this review",
  });
  const serviceWildcardValid = await gatewayCall("/services/review-service/execute", validToken, relaynaKey, {
    input: "service wildcard review",
  });
  const serviceWildcardMissingJwt = await gatewayCall("/services/review-service/execute", null, relaynaKey, {
    input: "missing service jwt",
  });
  const missingJwt = await gatewayCall("/v1/chat/completions", null, relaynaKey);
  const wrongAudience = await gatewayCall("/v1/chat/completions", wrongAudienceToken, relaynaKey);
  const expired = await gatewayCall("/v1/chat/completions", expiredToken, relaynaKey);
  const missingScope = await gatewayCall("/v1/chat/completions", missingScopeToken, relaynaKey);
  const invalidSignature = await gatewayCall("/v1/chat/completions", invalidSignatureToken, relaynaKey);
  const invalidRelaynaKey = await gatewayCall("/v1/chat/completions", validToken, "rk_live_invalid_review_key");
  const oldHeaderOnly = await fetch(`${gatewayProxyUrl}/v1/chat/completions`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${validToken}`,
      "x-aih-api-key": relaynaKey,
      "content-type": "application/json",
    },
    body: JSON.stringify(chatPayload("old-header")),
  }).then(async (response) => ({
    status: response.status,
    body: await response.json(),
  }));
  const apigeeRevalidate = await apigeeCall("/v1/chat/completions", validToken, relaynaKey, false);
  const apigeeRejectsWrongAudience = await apigeeCall(
    "/v1/chat/completions",
    wrongAudienceToken,
    relaynaKey,
    false,
  );
  const apigeeTrusted = await apigeeCall("/v1/chat/completions", validToken, relaynaKey, true);
  const apigeeTrustedTamper = await apigeeCall("/v1/chat/completions", validToken, relaynaKey, true, true);
  const lastUpstream = state.upstreamRequests.at(-1);
  const upstreamHeaders = state.upstreamRequests.map((request) => request.headers);
  const leakedClientCredentials = upstreamHeaders.some(
    (headers) =>
      "x-relayna-key" in headers ||
      "x-aih-api-key" in headers ||
      "x-apigee-entra-identity" in headers ||
      "x-apigee-entra-signature" in headers ||
      !expectedUpstreamAuthorizations.has(headers.authorization),
  );

  const checks = {
    direct_valid_jwt_and_relayna_key: pass(directValid.status === 200, directValid),
    responses_valid_jwt_and_relayna_key: pass(responsesValid.status === 200, responsesValid),
    direct_provider_route_valid_jwt_and_relayna_key: pass(
      directProviderValid.status === 200,
      directProviderValid,
    ),
    builtin_internal_summary_valid_jwt_and_relayna_key: pass(summaryValid.status === 200, summaryValid),
    service_wildcard_valid_jwt_and_relayna_key: pass(serviceWildcardValid.status === 200, serviceWildcardValid),
    service_wildcard_missing_jwt_fails_before_upstream: pass(
      serviceWildcardMissingJwt.status === 401 &&
        codeOf(serviceWildcardMissingJwt) === "missing_entra_authorization",
      serviceWildcardMissingJwt,
    ),
    missing_jwt_fails_before_upstream: pass(
      missingJwt.status === 401 && codeOf(missingJwt) === "missing_entra_authorization",
      missingJwt,
    ),
    wrong_audience_rejected_by_gateway: pass(
      wrongAudience.status === 401 && codeOf(wrongAudience) === "invalid_entra_audience",
      wrongAudience,
    ),
    expired_token_rejected_by_gateway: pass(
      expired.status === 401 && codeOf(expired) === "expired_entra_token",
      expired,
    ),
    missing_scope_rejected_by_gateway: pass(
      missingScope.status === 403 && codeOf(missingScope) === "insufficient_entra_authorization",
      missingScope,
    ),
    invalid_signature_rejected_by_gateway: pass(
      invalidSignature.status === 401 && codeOf(invalidSignature) === "invalid_entra_token",
      invalidSignature,
    ),
    invalid_relayna_key_rejected_after_jwt: pass(
      invalidRelaynaKey.status === 401 && codeOf(invalidRelaynaKey) === "invalid_virtual_key",
      invalidRelaynaKey,
    ),
    default_header_is_x_relayna_key_not_x_aih_api_key: pass(
      oldHeaderOnly.status === 401 && codeOf(oldHeaderOnly) === "missing_authorization",
      oldHeaderOnly,
    ),
    apigee_revalidation_path_forwards_after_edge_validation: pass(
      apigeeRevalidate.status === 200 && apigeeRevalidate.edgeValidated,
      apigeeRevalidate,
    ),
    apigee_edge_rejects_wrong_audience: pass(
      apigeeRejectsWrongAudience.status === 401 &&
        apigeeRejectsWrongAudience.body.error.code === "apigee_verify_jwt_failed",
      apigeeRejectsWrongAudience,
    ),
    apigee_trusted_header_path_forwards_with_signed_identity: pass(
      apigeeTrusted.status === 200 && apigeeTrusted.edgeValidated,
      apigeeTrusted,
    ),
    apigee_trusted_header_tamper_rejected_by_gateway: pass(
      apigeeTrustedTamper.status === 401 && codeOf(apigeeTrustedTamper) === "untrusted_apigee_identity",
      apigeeTrustedTamper,
    ),
    upstream_receives_only_internal_provider_credentials: pass(!leakedClientCredentials, {
      upstreamRequestCount: state.upstreamRequests.length,
      lastUpstreamAuthorization: lastUpstream?.headers?.authorization,
      upstreamAuthorizations: [...new Set(upstreamHeaders.map((headers) => headers.authorization))],
    }),
  };
  const ok = Object.values(checks).every((check) => check.ok);
  state.results = {
    ok,
    generatedAt: new Date().toISOString(),
    environment: {
      gatewayProxyUrl,
      gatewayControlUrl,
      mockPublicBaseUrl: publicBaseUrl,
      issuer,
      audience,
      tenantId,
      relaynaKeyHeader: "X-Relayna-Key",
      apigeeTrustedHeader: true,
      coveredRoutes: [
        "/v1/chat/completions",
        "/v1/responses",
        "/providers/openai/*",
        "/summary",
        "/services/*",
      ],
    },
    checks,
    upstreamRequests: state.upstreamRequests.map((request) => ({
      path: request.path,
      authorization: request.headers.authorization,
      hasRelaynaKey: "x-relayna-key" in request.headers,
      hasAihKey: "x-aih-api-key" in request.headers,
      hasClientJwt: !expectedUpstreamAuthorizations.has(request.headers.authorization),
      hasApigeeIdentity: "x-apigee-entra-identity" in request.headers,
    })),
  };
  return state.results;
}

function dashboardHtml() {
  const resultScript = JSON.stringify({
    publicBaseUrl,
    results: state.results,
    captures: state.upstreamRequests,
  });
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Relayna Entra Front Door Review</title>
  <style>
    :root { color-scheme: light; --ink:#17202a; --muted:#637083; --line:#d9e1ea; --ok:#236b4f; --bad:#a43f3f; --surface:#fff; --band:#f5f7fa; }
    body { margin:0; font:14px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; color:var(--ink); background:var(--band); }
    main { max-width:1180px; margin:0 auto; padding:24px; background:var(--surface); min-height:100vh; }
    h1 { margin:0; font-size:28px; }
    .top { display:flex; align-items:flex-start; justify-content:space-between; gap:16px; border-bottom:1px solid var(--line); padding-bottom:16px; }
    .status { font-size:18px; font-weight:700; color:var(--bad); }
    .status.ok { color:var(--ok); }
    .grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(280px,1fr)); gap:12px; margin-top:18px; }
    .card { border:1px solid var(--line); border-radius:8px; padding:12px; background:#fbfcfd; }
    .card strong { display:block; margin-bottom:6px; }
    .pass { color:var(--ok); font-weight:700; }
    .fail { color:var(--bad); font-weight:700; }
    code { background:#eef3f7; border:1px solid #dce6ef; border-radius:4px; padding:1px 4px; }
    table { width:100%; border-collapse:collapse; margin-top:18px; }
    th,td { border:1px solid var(--line); padding:8px; text-align:left; vertical-align:top; }
    th { background:#edf2f6; }
    pre { overflow:auto; background:#17202a; color:#f5f7fa; padding:12px; border-radius:8px; max-height:360px; }
  </style>
</head>
<body>
<main>
  <div class="top">
    <div>
      <h1>Relayna Gateway Entra Front Door Review</h1>
      <p>Docker-backed mock OIDC/JWKS, Apigee revalidation, trusted Apigee headers, Gateway, Postgres, Redis, and upstream capture.</p>
    </div>
    <div id="overall" class="status">Not run</div>
  </div>
  <section class="grid" id="cards"></section>
  <section>
    <h2>Upstream Credential Capture</h2>
    <table>
      <thead><tr><th>Path</th><th>Authorization at upstream</th><th>Client key leaked?</th><th>Apigee identity leaked?</th></tr></thead>
      <tbody id="captures"></tbody>
    </table>
  </section>
  <section>
    <h2>Raw Result JSON</h2>
    <pre id="raw"></pre>
  </section>
</main>
<script>
const data = ${resultScript};
const results = data.results;
const overall = document.querySelector("#overall");
if (results) {
  overall.textContent = results.ok ? "PASS" : "FAIL";
  overall.classList.toggle("ok", Boolean(results.ok));
  const cards = document.querySelector("#cards");
  for (const [name, check] of Object.entries(results.checks)) {
    const div = document.createElement("div");
    div.className = "card";
    div.innerHTML = "<strong>" + name.replaceAll("_", " ") + "</strong><span class='" + (check.ok ? "pass" : "fail") + "'>" + (check.ok ? "PASS" : "FAIL") + "</span><br><code>status " + (check.status || "n/a") + "</code>";
    cards.appendChild(div);
  }
  const captures = document.querySelector("#captures");
  for (const capture of results.upstreamRequests) {
    const row = document.createElement("tr");
    row.innerHTML = "<td>" + capture.path + "</td><td><code>" + capture.authorization + "</code></td><td>" + (capture.hasRelaynaKey || capture.hasAihKey || capture.hasClientJwt ? "yes" : "no") + "</td><td>" + (capture.hasApigeeIdentity ? "yes" : "no") + "</td>";
    captures.appendChild(row);
  }
  document.querySelector("#raw").textContent = JSON.stringify(results, null, 2);
}
</script>
</body>
</html>`;
}

async function handleApigee(req, res, trusted = false, tamper = false) {
  const auth = req.headers.authorization || "";
  const token = auth.startsWith("Bearer ") ? auth.slice("Bearer ".length) : "";
  const relaynaKey = req.headers["x-relayna-key"];
  const result = await apigeeCall("/v1/chat/completions", token, relaynaKey, trusted, tamper);
  jsonResponse(res, result.status, result.body, { "x-apigee-edge-validated": String(result.edgeValidated) });
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, publicBaseUrl);
    if (req.method === "GET" && url.pathname === "/healthz") {
      return jsonResponse(res, 200, { ok: true });
    }
    if (req.method === "GET" && url.pathname === "/oauth/.well-known/openid-configuration") {
      return jsonResponse(res, 200, { issuer, jwks_uri: `${dockerBaseUrl}/oauth/jwks` });
    }
    if (req.method === "GET" && url.pathname === "/oauth/jwks") {
      return jsonResponse(res, 200, jwks());
    }
    if (req.method === "GET" && url.pathname === "/token") {
      const scenario = url.searchParams.get("scenario") || "valid";
      const overrides = {
        wrong_audience: { aud: "https://graph.microsoft.com" },
        expired: { exp: Math.floor(Date.now() / 1000) - 120 },
        missing_scope: { scp: "gateway.read" },
      }[scenario] || {};
      let token = signJwt(tokenClaims(overrides));
      if (scenario === "invalid_signature") {
        token = `${token}x`;
      }
      return jsonResponse(res, 200, { token, scenario });
    }
    if (req.method === "POST" && url.pathname === "/v1/chat/completions") {
      const body = await readJson(req);
      state.upstreamRequests.push({
        path: url.pathname,
        headers: req.headers,
        body,
        at: new Date().toISOString(),
      });
      return jsonResponse(res, 200, {
        id: `chatcmpl-${crypto.randomUUID()}`,
        object: "chat.completion",
        model: body.model || "gpt-review",
        choices: [{ index: 0, message: { role: "assistant", content: "mock upstream ok" }, finish_reason: "stop" }],
        usage: { prompt_tokens: 4, completion_tokens: 3, total_tokens: 7 },
      });
    }
    if (req.method === "POST" && url.pathname === "/apigee/v1/chat/completions") {
      return handleApigee(req, res, false, false);
    }
    if (req.method === "POST" && url.pathname === "/apigee-trusted/v1/chat/completions") {
      return handleApigee(req, res, true, false);
    }
    if (req.method === "POST" && url.pathname === "/apigee-trusted-tamper/v1/chat/completions") {
      return handleApigee(req, res, true, true);
    }
    if (req.method === "POST" && url.pathname === "/run-tests") {
      const results = await runTests();
      return jsonResponse(res, results.ok ? 200 : 500, results);
    }
    if (req.method === "GET" && url.pathname === "/results") {
      return jsonResponse(res, state.results ? 200 : 404, state.results || { error: "not_run" });
    }
    if (req.method === "GET" && url.pathname === "/captures") {
      return jsonResponse(res, 200, state.upstreamRequests);
    }
    if (req.method === "POST") {
      const body = await readJson(req);
      state.upstreamRequests.push({
        path: url.pathname,
        headers: req.headers,
        body,
        at: new Date().toISOString(),
      });
      return jsonResponse(res, 200, {
        id: `mock-${crypto.randomUUID()}`,
        object: "mock.response",
        model: body.model || "gpt-review",
        choices: [{ index: 0, message: { role: "assistant", content: "mock upstream ok" }, finish_reason: "stop" }],
        output: [{ type: "message", content: [{ type: "output_text", text: "mock upstream ok" }] }],
        usage: { prompt_tokens: 4, completion_tokens: 3, total_tokens: 7 },
      });
    }
    if (req.method === "GET" && (url.pathname === "/" || url.pathname === "/app")) {
      return htmlResponse(res, 200, dashboardHtml());
    }
    jsonResponse(res, 404, { error: "not_found" });
  } catch (error) {
    jsonResponse(res, 500, { error: String(error?.stack || error) });
  }
});

server.listen(port, "0.0.0.0", () => {
  console.log(`mock app listening on ${port}`);
});
