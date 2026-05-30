import http from "node:http";
import crypto from "node:crypto";

const port = 4000;
const dockerBaseUrl = process.env.DOCKER_BASE_URL || "http://mock-provider:4000";
const publicBaseUrl = process.env.PUBLIC_BASE_URL || "http://localhost:18282";
const adminToken = process.env.GATEWAY_ADMIN_TOKEN || "op_live_ci_token1";
const apigeeSecret = process.env.APIGEE_TRUSTED_HEADER_SECRET || "apigee-pentest-secret";

const frontDoors = {
  no_entra: {
    label: "No EntraID",
    proxyUrl: process.env.NO_ENTRA_PROXY_URL || "http://gateway-no-entra:8080",
    controlUrl: process.env.NO_ENTRA_CONTROL_URL || "http://gateway-no-entra:8081",
  },
  entra: {
    label: "EntraID",
    proxyUrl: process.env.ENTRA_PROXY_URL || "http://gateway-entra:8080",
    controlUrl: process.env.ENTRA_CONTROL_URL || "http://gateway-entra:8081",
  },
  apigee: {
    label: "Apigee trusted header",
    proxyUrl: process.env.APIGEE_PROXY_URL || "http://gateway-apigee:8080",
    controlUrl: process.env.APIGEE_CONTROL_URL || "http://gateway-apigee:8081",
  },
};

const issuer = `${dockerBaseUrl}/oauth`;
const tenantId = "relayna-pentest-tenant";
const audience = "api://relayna-gateway-pentest";
const requiredScope = "gateway.invoke";
const allowedGroup = "relayna-pentest-group";
const providerCredential = "Bearer sk-local-provider-pentest-key";

const keyPair = crypto.generateKeyPairSync("rsa", {
  modulusLength: 2048,
  publicKeyEncoding: { type: "spki", format: "pem" },
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
});
const publicKey = crypto.createPublicKey(keyPair.publicKey);
const publicJwk = publicKey.export({ format: "jwk" });
const kid = "relayna-pentest-kid";

const state = {
  keys: {},
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
    sub: "pentest-user-subject",
    oid: "pentest-user-object",
    azp: "pentest-client",
    scp: requiredScope,
    groups: [allowedGroup],
    ...overrides,
  };
}

function signJwt(claims, headerOverrides = {}) {
  const header = { alg: "RS256", typ: "JWT", kid, ...headerOverrides };
  const signingInput = `${base64url(JSON.stringify(header))}.${base64url(JSON.stringify(claims))}`;
  const signature = crypto.sign("RSA-SHA256", Buffer.from(signingInput), keyPair.privateKey);
  return `${signingInput}.${signature.toString("base64url")}`;
}

function tamperJwt(token) {
  const [encodedHeader, encodedPayload] = token.split(".");
  return `${encodedHeader}.${encodedPayload}.tampered-signature`;
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

function hmacIdentity(identityHeader) {
  return crypto.createHmac("sha256", apigeeSecret).update(identityHeader).digest("base64url");
}

async function waitForGateway(name) {
  const frontDoor = frontDoors[name];
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const response = await fetch(`${frontDoor.controlUrl}/admin-ui/readyz`);
      if (response.ok) {
        return;
      }
    } catch {
      // Retry until migrations and readiness checks finish.
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error(`${name}_gateway_not_ready`);
}

async function setupGatewayData(name) {
  if (state.keys[name]) {
    return state.keys[name];
  }
  const frontDoor = frontDoors[name];
  await waitForGateway(name);

  const policyLayerResponse = await fetch(`${frontDoor.controlUrl}/admin-ui/admin/policy-layers`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ kind: "global", policy: {} }),
  });
  if (!policyLayerResponse.ok) {
    throw new Error(`${name}_create_policy_layer_failed:${policyLayerResponse.status}:${await policyLayerResponse.text()}`);
  }

  const projectResponse = await fetch(`${frontDoor.controlUrl}/admin-ui/admin/projects`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ name: `front-door-pentest-${name}-${Date.now()}` }),
  });
  if (!projectResponse.ok) {
    throw new Error(`${name}_create_project_failed:${projectResponse.status}:${await projectResponse.text()}`);
  }
  const project = await projectResponse.json();

  const keyResponse = await fetch(`${frontDoor.controlUrl}/admin-ui/admin/keys`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      project_id: project.id,
      preset: "developer",
      policy: {
        allowed_routes: ["/v1/chat/completions", "/v1/responses", "/v1/embeddings"],
        allowed_providers: ["litellm"],
        allow_streaming: false,
      },
    }),
  });
  if (!keyResponse.ok) {
    throw new Error(`${name}_create_key_failed:${keyResponse.status}:${await keyResponse.text()}`);
  }
  const key = await keyResponse.json();
  state.keys[name] = key.raw_key;
  return state.keys[name];
}

function payloadFor(path, label) {
  if (path === "/v1/responses") {
    return { model: "gpt-pentest", input: `front-door pentest ${label}` };
  }
  if (path === "/v1/embeddings") {
    return { model: "text-embedding-pentest", input: `front-door pentest ${label}` };
  }
  return {
    model: "gpt-pentest",
    messages: [{ role: "user", content: `front-door pentest ${label}` }],
  };
}

async function callGateway(name, path, headers, body = payloadFor(path, name)) {
  const response = await fetch(`${frontDoors[name].proxyUrl}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    parsed = text;
  }
  return { status: response.status, body: parsed };
}

function noEntraHeaders(rawKey) {
  return { authorization: `Bearer ${rawKey}` };
}

function entraHeaders(rawKey, token = signJwt(tokenClaims())) {
  return { authorization: `Bearer ${token}`, "x-relayna-key": rawKey };
}

function apigeeHeaders(rawKey, claims = tokenClaims(), options = {}) {
  const identityHeader = trustedIdentityHeader(claims);
  const signature = options.badSignature ? "invalid-signature" : hmacIdentity(identityHeader);
  const headers = { "x-relayna-key": rawKey, "x-apigee-entra-identity": identityHeader };
  if (!options.omitSignature) {
    headers["x-apigee-entra-signature"] = signature;
  }
  if (options.includeClientJwt) {
    headers.authorization = `Bearer ${signJwt(claims)}`;
  }
  return headers;
}

function codeOf(result) {
  return result?.body?.error?.code || null;
}

function check(condition, result, notes = "") {
  return {
    ok: Boolean(condition),
    status: result?.status ?? null,
    error_code: codeOf(result),
    notes,
  };
}

async function positivePassthrough(name, headersFactory) {
  const rawKey = await setupGatewayData(name);
  const checks = {};
  for (const path of ["/v1/chat/completions", "/v1/responses", "/v1/embeddings"]) {
    const before = state.providerRequests.length;
    const result = await callGateway(name, path, headersFactory(rawKey), payloadFor(path, name));
    const captured = state.providerRequests.slice(before).some((request) => request.path.includes(path.replace("/v1", "")));
    checks[`${name}${path.replaceAll("/", "_")}_passes_to_litellm`] = check(
      result.status === 200 && captured,
      result,
      captured ? "LiteLLM forwarded to mock provider" : "No provider capture after gateway call",
    );
  }
  return checks;
}

async function runTests() {
  state.keys = {};
  state.providerRequests = [];

  const validClaims = tokenClaims();
  const validJwt = signJwt(validClaims);
  const expiredJwt = signJwt(tokenClaims({ exp: Math.floor(Date.now() / 1000) - 60 }));
  const wrongAudienceJwt = signJwt(tokenClaims({ aud: "api://attacker" }));
  const noScopeJwt = signJwt(tokenClaims({ scp: "profile.read" }));

  const noEntraKey = await setupGatewayData("no_entra");
  const entraKey = await setupGatewayData("entra");
  const apigeeKey = await setupGatewayData("apigee");

  const checks = {
    ...(await positivePassthrough("no_entra", noEntraHeaders)),
    ...(await positivePassthrough("entra", (key) => entraHeaders(key, validJwt))),
    ...(await positivePassthrough("apigee", (key) => apigeeHeaders(key, validClaims))),
  };

  const noEntraMissingAuth = await callGateway("no_entra", "/v1/chat/completions", {});
  checks.no_entra_missing_authorization_rejected = check(
    noEntraMissingAuth.status === 401 && codeOf(noEntraMissingAuth) === "missing_authorization",
    noEntraMissingAuth,
  );

  const noEntraHeaderSmuggling = await callGateway("no_entra", "/v1/chat/completions", {
    "x-relayna-key": noEntraKey,
  });
  checks.no_entra_x_relayna_key_without_authorization_rejected = check(
    noEntraHeaderSmuggling.status === 401,
    noEntraHeaderSmuggling,
    "No-Entra mode must not accept the Entra header as a bypass",
  );

  const entraMissingJwt = await callGateway("entra", "/v1/chat/completions", { "x-relayna-key": entraKey });
  checks.entra_missing_jwt_rejected = check(entraMissingJwt.status === 401, entraMissingJwt);

  const entraLegacyBypass = await callGateway("entra", "/v1/chat/completions", noEntraHeaders(entraKey));
  checks.entra_legacy_relayna_authorization_bypass_rejected = check(entraLegacyBypass.status === 401, entraLegacyBypass);

  const entraExpired = await callGateway("entra", "/v1/chat/completions", entraHeaders(entraKey, expiredJwt));
  checks.entra_expired_token_rejected = check(entraExpired.status === 401, entraExpired);

  const entraWrongAudience = await callGateway("entra", "/v1/chat/completions", entraHeaders(entraKey, wrongAudienceJwt));
  checks.entra_wrong_audience_rejected = check(entraWrongAudience.status === 401, entraWrongAudience);

  const entraMissingScope = await callGateway("entra", "/v1/chat/completions", entraHeaders(entraKey, noScopeJwt));
  checks.entra_missing_scope_rejected = check(entraMissingScope.status === 403 || entraMissingScope.status === 401, entraMissingScope);

  const entraTampered = await callGateway("entra", "/v1/chat/completions", entraHeaders(entraKey, tamperJwt(validJwt)));
  checks.entra_tampered_signature_rejected = check(entraTampered.status === 401, entraTampered);

  const apigeeMissingProof = await callGateway("apigee", "/v1/chat/completions", { "x-relayna-key": apigeeKey });
  checks.apigee_missing_identity_proof_rejected = check(apigeeMissingProof.status === 401, apigeeMissingProof);

  const apigeeMissingSignature = await callGateway(
    "apigee",
    "/v1/chat/completions",
    apigeeHeaders(apigeeKey, validClaims, { omitSignature: true }),
  );
  checks.apigee_missing_signature_rejected = check(apigeeMissingSignature.status === 401, apigeeMissingSignature);

  const apigeeBadSignature = await callGateway(
    "apigee",
    "/v1/chat/completions",
    apigeeHeaders(apigeeKey, validClaims, { badSignature: true }),
  );
  checks.apigee_bad_signature_rejected = check(apigeeBadSignature.status === 401, apigeeBadSignature);

  const apigeeMissingScope = await callGateway("apigee", "/v1/chat/completions", apigeeHeaders(apigeeKey, tokenClaims({ scp: "" })));
  checks.apigee_missing_scope_rejected = check(apigeeMissingScope.status === 403 || apigeeMissingScope.status === 401, apigeeMissingScope);

  const apigeeClientJwtLeakAttempt = await callGateway(
    "apigee",
    "/v1/chat/completions",
    apigeeHeaders(apigeeKey, validClaims, { includeClientJwt: true }),
  );
  checks.apigee_client_jwt_header_does_not_break_trusted_path = check(apigeeClientJwtLeakAttempt.status === 200, apigeeClientJwtLeakAttempt);

  const aliasPath = await callGateway("entra", "/v1/embedding", entraHeaders(entraKey, validJwt), {
    model: "text-embedding-pentest",
    input: "singular alias probe",
  });
  checks.alias_embedding_path_rejected_before_litellm = check(
    aliasPath.status === 404 && codeOf(aliasPath) === "unsupported_route",
    aliasPath,
  );

  const rerankPayload = {
    model: "rerank-pentest",
    query: "front door rerank probe",
    documents: ["relayna gateway", "litellm passthrough"],
  };
  const noEntraRerank = await callGateway("no_entra", "/v1/rerank", noEntraHeaders(noEntraKey), rerankPayload);
  checks.no_entra_rerank_path_rejected_before_litellm = check(
    noEntraRerank.status === 404 && codeOf(noEntraRerank) === "unsupported_route",
    noEntraRerank,
  );
  const entraRerank = await callGateway("entra", "/v1/rerank", entraHeaders(entraKey, validJwt), rerankPayload);
  checks.entra_rerank_path_rejected_before_litellm = check(
    entraRerank.status === 404 && codeOf(entraRerank) === "unsupported_route",
    entraRerank,
  );
  const apigeeRerank = await callGateway("apigee", "/v1/rerank", apigeeHeaders(apigeeKey, validClaims), rerankPayload);
  checks.apigee_rerank_path_rejected_before_litellm = check(
    apigeeRerank.status === 404 && codeOf(apigeeRerank) === "unsupported_route",
    apigeeRerank,
  );

  const upstreamLeak = state.providerRequests.some(
    (request) =>
      request.hasRelaynaKey ||
      request.hasAihKey ||
      request.hasApigeeIdentity ||
      request.hasClientJwt ||
      request.authorization !== providerCredential,
  );
  checks.no_client_or_front_door_credentials_reached_provider = {
    ok: !upstreamLeak,
    status: null,
    error_code: null,
    notes: "Mock provider only saw the LiteLLM provider credential",
  };

  state.results = {
    ok: Object.values(checks).every((entry) => entry.ok),
    generatedAt: new Date().toISOString(),
    environment: {
      publicBaseUrl,
      litellmUrl: "http://litellm:4000",
      issuer,
      audience,
      tenantId,
      requiredScope,
      allowedGroup,
      frontDoors: Object.fromEntries(
        Object.entries(frontDoors).map(([name, frontDoor]) => [
          name,
          { label: frontDoor.label, proxyUrl: frontDoor.proxyUrl, controlUrl: frontDoor.controlUrl },
        ]),
      ),
    },
    summary: {
      checks: Object.keys(checks).length,
      passed: Object.values(checks).filter((entry) => entry.ok).length,
      failed: Object.values(checks).filter((entry) => !entry.ok).length,
      providerCaptures: state.providerRequests.length,
    },
    checks,
    providerRequests: state.providerRequests,
  };
  return state.results;
}

function captureProviderRequest(req, body) {
  const headers = Object.fromEntries(
    Object.entries(req.headers).map(([key, value]) => [key.toLowerCase(), String(value)]),
  );
  state.providerRequests.push({
    method: req.method,
    path: req.url,
    authorization: headers.authorization || null,
    hasRelaynaKey: "x-relayna-key" in headers,
    hasAihKey: "x-aih-api-key" in headers,
    hasApigeeIdentity: "x-apigee-entra-identity" in headers || "x-apigee-entra-signature" in headers,
    hasClientJwt: headers.authorization?.split(".").length === 3,
    body,
  });
}

function providerResponse(req, body) {
  captureProviderRequest(req, body);
  if (req.url.includes("/chat/completions")) {
    return {
      id: `chatcmpl-${crypto.randomUUID()}`,
      object: "chat.completion",
      created: Math.floor(Date.now() / 1000),
      model: body.model || "gpt-pentest",
      choices: [{ index: 0, message: { role: "assistant", content: "front-door pentest chat ok" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 8, completion_tokens: 5, total_tokens: 13 },
    };
  }
  if (req.url.includes("/responses")) {
    return {
      id: `resp-${crypto.randomUUID()}`,
      object: "response",
      created_at: Math.floor(Date.now() / 1000),
      model: body.model || "gpt-pentest",
      output: [
        {
          type: "message",
          id: `msg-${crypto.randomUUID()}`,
          status: "completed",
          role: "assistant",
          content: [{ type: "output_text", text: "front-door pentest response ok" }],
        },
      ],
      usage: { input_tokens: 7, output_tokens: 5, total_tokens: 12 },
    };
  }
  if (req.url.includes("/embeddings")) {
    return {
      object: "list",
      model: body.model || "text-embedding-pentest",
      data: [{ object: "embedding", index: 0, embedding: [0.11, 0.22, 0.33] }],
      usage: { prompt_tokens: 3, total_tokens: 3 },
    };
  }
  return { ok: true, path: req.url };
}

function css() {
  return `
    body { margin: 0; color: #18212f; background: #f5f7fb; font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    main { max-width: 1180px; margin: 0 auto; padding: 30px; }
    h1 { margin: 0 0 8px; font-size: 30px; }
    h2 { margin: 26px 0 12px; font-size: 20px; }
    p { line-height: 1.5; }
    code { background: #eef2f7; border-radius: 4px; padding: 2px 5px; }
    .summary { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin: 20px 0; }
    .metric { background: #fff; border: 1px solid #d7dde8; border-radius: 8px; padding: 14px; }
    .metric span { color: #607086; font-size: 13px; }
    .metric strong { display: block; font-size: 25px; margin-top: 4px; }
    table { width: 100%; border-collapse: collapse; background: #fff; border: 1px solid #d7dde8; }
    th, td { text-align: left; vertical-align: top; border-bottom: 1px solid #e8edf4; padding: 9px 11px; font-size: 13px; }
    th { background: #edf2f8; color: #2e3a4a; }
    .pass { color: #12672f; font-weight: 700; }
    .fail { color: #b42318; font-weight: 700; }
    .finding { background: #fff9e6; border-left: 4px solid #d99a00; padding: 12px 14px; }
  `;
}

function dashboardHtml() {
  const result = state.results;
  const checks = result ? Object.entries(result.checks) : [];
  const captures = result?.providerRequests || [];
  return `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Front Door Penetration Report</title>
  <style>${css()}</style>
</head>
<body>
  <main>
    <h1>Front Door Penetration Report</h1>
    <p>Generated ${result?.generatedAt || "after /run-tests"}. This run attacks the No EntraID, direct EntraID, and trusted Apigee front-door paths before traffic reaches a real <code>litellm/litellm:latest</code> container.</p>
    <section class="summary">
      <div class="metric"><span>Outcome</span><strong>${result?.ok ? "PASS" : "RUN"}</strong></div>
      <div class="metric"><span>Checks</span><strong>${result?.summary.checks || 0}</strong></div>
      <div class="metric"><span>Passed</span><strong>${result?.summary.passed || 0}</strong></div>
      <div class="metric"><span>Provider captures</span><strong>${result?.summary.providerCaptures || 0}</strong></div>
    </section>
    <h2 id="attack-checks">Attack Checks</h2>
    <table>
      <thead><tr><th>Check</th><th>Result</th><th>Status</th><th>Error</th><th>Notes</th></tr></thead>
      <tbody>${checks
        .map(
          ([name, entry]) =>
            `<tr><td>${name.replaceAll("_", " ")}</td><td class="${entry.ok ? "pass" : "fail"}">${entry.ok ? "PASS" : "FAIL"}</td><td>${entry.status ?? "n/a"}</td><td>${entry.error_code ?? ""}</td><td>${entry.notes ?? ""}</td></tr>`,
        )
        .join("")}</tbody>
    </table>
    <h2 id="provider-capture">Provider Credential Capture</h2>
    <table>
      <thead><tr><th>Request</th><th>Authorization</th><th>Client credential leaked?</th><th>Apigee proof leaked?</th></tr></thead>
      <tbody>${captures
        .map(
          (entry) =>
            `<tr><td>${entry.method} ${entry.path}</td><td><code>${entry.authorization || ""}</code></td><td>${entry.hasRelaynaKey || entry.hasAihKey || entry.hasClientJwt ? "yes" : "no"}</td><td>${entry.hasApigeeIdentity ? "yes" : "no"}</td></tr>`,
        )
        .join("")}</tbody>
    </table>
    <h2 id="interesting-findings">Interesting Finding</h2>
    <div class="finding">Canonical <code>/v1/chat/completions</code>, <code>/v1/responses</code>, and <code>/v1/embeddings</code> pass through all three front doors to LiteLLM. Alias paths such as <code>/v1/embedding</code> and unsupported paths such as <code>/v1/rerank</code> still stop at Relayna Gateway with <code>unsupported_route</code>.</div>
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
      jsonResponse(res, 200, providerResponse(req, await readBody(req)));
      return;
    }
    jsonResponse(res, 404, { error: { code: "not_found", message: "Not found" } });
  } catch (error) {
    jsonResponse(res, 500, { error: { code: "mock_error", message: error.message, stack: error.stack } });
  }
});

server.listen(port, "0.0.0.0", () => {
  console.log(`front-door penetration mock provider listening on ${port}`);
});
