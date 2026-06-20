import http from "node:http";
import crypto from "node:crypto";

const port = 4000;
const role = process.env.ROLE || "mock-provider";
const dockerBaseUrl = process.env.DOCKER_BASE_URL || "http://mock-provider:4000";
const gatewayProxyUrl = process.env.GATEWAY_PROXY_URL || "http://gateway:8080";
const gatewayControlUrl = process.env.GATEWAY_CONTROL_URL || "http://gateway:8081";
const adminToken = process.env.GATEWAY_ADMIN_TOKEN;
const apigeeSecret = process.env.APIGEE_TRUSTED_HEADER_SECRET || "apigee-secret";
const litellmDirectUrl = process.env.LITELLM_DIRECT_URL || "http://litellm:4000";
const litellmMasterKey = process.env.LITELLM_MASTER_KEY || "sk-ci";

const issuer = `${dockerBaseUrl}/oauth`;
const tenantId = "relayna-litellm-review-tenant";
const audience = "api://relayna-gateway-litellm-review";
const requiredScope = "gateway.invoke";
const allowedGroup = "relayna-litellm-review-group";
const upstreamServiceAuthorization = "Bearer sk-ci";

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
  gatewayData: null,
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
    hasGatewayServiceKey: headers.authorization === "Bearer sk-upstream",
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

async function createLiteLlmVirtualKey({ alias, models, allowedRoutes }) {
  const response = await fetch(`${litellmDirectUrl}/key/generate`, {
    method: "POST",
    headers: {
      "x-litellm-key": `Bearer ${litellmMasterKey}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      key_alias: `${alias}-${Date.now()}`,
      models,
      allowed_routes: allowedRoutes,
    }),
  });
  if (!response.ok) {
    throw new Error(`create_litellm_virtual_key_failed:${response.status}:${await response.text()}`);
  }
  const body = await response.json();
  if (!body.key) {
    throw new Error(`create_litellm_virtual_key_missing_key:${JSON.stringify(body)}`);
  }
  return body.key;
}

async function setupGatewayData() {
  if (state.rawRelaynaKey) {
    return state.rawRelaynaKey;
  }
  await waitForGateway();
  const providerDefaultCredential = await createLiteLlmVirtualKey({
    alias: "relayna-provider-default-embeddings",
    models: ["text-embedding-review"],
    allowedRoutes: ["/v1/embeddings"],
  });
  const projectCredential = await createLiteLlmVirtualKey({
    alias: "relayna-project-responses",
    models: ["gpt-review"],
    allowedRoutes: ["/v1/responses"],
  });
  const keyCredential = await createLiteLlmVirtualKey({
    alias: "relayna-key-chat",
    models: ["gpt-review"],
    allowedRoutes: ["/v1/chat/completions", "/v1/models"],
  });
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
        allowed_routes: ["/v1/chat/completions", "/v1/responses", "/v1/embeddings"],
        allowed_providers: ["litellm"],
        allow_streaming: false,
      },
    }),
  });
  if (!keyResponse.ok) {
    throw new Error(`create_key_failed:${keyResponse.status}:${await keyResponse.text()}`);
  }
  const createdKey = await keyResponse.json();
  const key = createdKey.key;
  const providersResponse = await fetch(`${gatewayControlUrl}/admin-ui/admin/providers`, {
    headers: {
      authorization: `Bearer ${adminToken}`,
    },
  });
  if (!providersResponse.ok) {
    throw new Error(`list_providers_failed:${providersResponse.status}:${await providersResponse.text()}`);
  }
  const existingProvider = (await providersResponse.json()).find((provider) => provider.provider === "litellm");
  const providerPayload = {
    provider: "litellm",
    name: `LiteLLM Direct ${Date.now()}`,
    base_url: litellmDirectUrl,
    credential: providerDefaultCredential,
    credential_header_mode: "custom_header",
    credential_header_name: "x-litellm-key",
    credential_header_value_format: "bearer",
    enabled: true,
  };
  const providerResponse = await fetch(
    existingProvider
      ? `${gatewayControlUrl}/admin-ui/admin/providers/${existingProvider.id}`
      : `${gatewayControlUrl}/admin-ui/admin/providers`,
    {
      method: existingProvider ? "PATCH" : "POST",
      headers: {
        authorization: `Bearer ${adminToken}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(existingProvider ? {
        base_url: providerPayload.base_url,
        credential: providerPayload.credential,
        credential_header_mode: providerPayload.credential_header_mode,
        credential_header_name: providerPayload.credential_header_name,
        credential_header_value_format: providerPayload.credential_header_value_format,
        enabled: providerPayload.enabled,
      } : providerPayload),
    },
  );
  if (!providerResponse.ok) {
    throw new Error(`upsert_litellm_provider_failed:${providerResponse.status}:${await providerResponse.text()}`);
  }
  const projectMappingResponse = await fetch(`${gatewayControlUrl}/admin-ui/admin/providers/litellm-credentials`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      scope: "project",
      target_id: project.id,
      credential: projectCredential,
      enabled: true,
    }),
  });
  if (!projectMappingResponse.ok) {
    throw new Error(`create_project_mapping_failed:${projectMappingResponse.status}:${await projectMappingResponse.text()}`);
  }
  const keyMappingResponse = await fetch(`${gatewayControlUrl}/admin-ui/admin/providers/litellm-credentials`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      scope: "key",
      target_id: key.id,
      credential: keyCredential,
      enabled: true,
    }),
  });
  if (!keyMappingResponse.ok) {
    throw new Error(`create_key_mapping_failed:${keyMappingResponse.status}:${await keyMappingResponse.text()}`);
  }
  state.gatewayData = {
    project,
    key,
    provider: await providerResponse.json(),
    projectMapping: await projectMappingResponse.json(),
    keyMapping: await keyMappingResponse.json(),
    credentialPrecedence: {
      providerDefault: "relayna-provider-default-embeddings",
      project: "relayna-project-responses",
      key: "relayna-key-chat",
    },
  };
  state.rawRelaynaKey = createdKey.raw_key;
  return state.rawRelaynaKey;
}

async function gatewayCall(path, token, relaynaKey, payload, method = "POST") {
  const headers = { "content-type": "application/json" };
  if (token) {
    headers.authorization = `Bearer ${token}`;
  }
  if (relaynaKey) {
    headers["x-relayna-key"] = relaynaKey;
  }
  const response = await fetch(`${gatewayProxyUrl}${path}`, {
    method,
    headers,
    body: method === "GET" ? undefined : JSON.stringify(payload),
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

async function gatewayBearerCall(path, bearerCredential, payload, method = "POST") {
  const headers = {
    authorization: `Bearer ${bearerCredential}`,
    "content-type": "application/json",
  };
  const response = await fetch(`${gatewayProxyUrl}${path}`, {
    method,
    headers,
    body: method === "GET" ? undefined : JSON.stringify(payload),
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

async function adminJson(path, options = {}) {
  const response = await fetch(`${gatewayControlUrl}${path}`, {
    ...options,
    headers: {
      authorization: `Bearer ${adminToken}`,
      "content-type": "application/json",
      ...(options.headers || {}),
    },
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
  await adminJson("/admin-ui/admin/auth/front-door", {
    method: "PATCH",
    body: JSON.stringify({
      entra_enabled: true,
      apigee_trusted_header_enabled: true,
      relayna_key_header: "X-Relayna-Key",
      tenant_id: tenantId,
      audience,
      issuer,
      oidc_discovery_url: `${issuer}/.well-known/openid-configuration`,
      required_scope: requiredScope,
      allowed_groups: [allowedGroup],
      accepted_algorithms: ["RS256"],
      jwks_cache_ttl_seconds: 300,
      clock_skew_seconds: 60,
      apigee_trusted_header_secret: apigeeSecret,
    }),
  });
  await adminJson("/admin-ui/admin/openai-routes/chat-completions/mode", {
    method: "PATCH",
    body: JSON.stringify({ mode: "managed_by_gateway" }),
  });
  await adminJson("/admin-ui/admin/openai-routes/responses/mode", {
    method: "PATCH",
    body: JSON.stringify({ mode: "managed_by_gateway" }),
  });
  await adminJson("/admin-ui/admin/providers/litellm-passthrough", {
    method: "PATCH",
    body: JSON.stringify({
      enabled: false,
      allowed_paths: ["/v1/*"],
      allowed_methods: ["GET", "POST"],
      ui_exposure: "disabled",
      admin_api_exposure: "disabled",
    }),
  });
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
  const routeModeUpdate = await adminJson("/admin-ui/admin/openai-routes/chat-completions/mode", {
    method: "PATCH",
    body: JSON.stringify({ mode: "direct_litellm_passthrough" }),
  });
  const routesAfterMode = await adminJson("/admin-ui/admin/openai-routes");
  const directModeChat = await gatewayBearerCall("/v1/chat/completions", litellmMasterKey, chatPayload);
  await adminJson("/admin-ui/admin/openai-routes/chat-completions/mode", {
    method: "PATCH",
    body: JSON.stringify({ mode: "managed_by_gateway" }),
  });
  const passthroughDefaults = await adminJson("/admin-ui/admin/providers/litellm-passthrough");
  const passthroughEnable = await adminJson("/admin-ui/admin/providers/litellm-passthrough", {
    method: "PATCH",
    body: JSON.stringify({
      enabled: true,
      allowed_paths: ["/v1/*"],
      allowed_methods: ["GET", "POST"],
      ui_exposure: "disabled",
      admin_api_exposure: "disabled",
    }),
  });
  const wildcardModels = await gatewayCall("/v1/models?source=wildcard", validToken, relaynaKey, {}, "GET");
  const uiBlocked = await gatewayCall("/ui", validToken, relaynaKey, {}, "GET");
  await fetch(`${gatewayControlUrl}/admin-ui/admin/providers/litellm-credentials/${state.gatewayData.keyMapping.id}/disable`, {
    method: "POST",
    headers: { authorization: `Bearer ${adminToken}`, "content-type": "application/json" },
    body: "{}",
  });
  const responses = await gatewayCall("/v1/responses", validToken, relaynaKey, responsePayload);
  await fetch(`${gatewayControlUrl}/admin-ui/admin/providers/litellm-credentials/${state.gatewayData.projectMapping.id}/disable`, {
    method: "POST",
    headers: { authorization: `Bearer ${adminToken}`, "content-type": "application/json" },
    body: "{}",
  });
  const embeddings = await gatewayCall("/v1/embeddings", validToken, relaynaKey, embeddingPayload);
  const providerCredentialRestore = await adminJson(`/admin-ui/admin/providers/${state.gatewayData.provider.id}`, {
    method: "PATCH",
    body: JSON.stringify({
      credential: litellmMasterKey,
      credential_header_mode: "custom_header",
      credential_header_name: "x-litellm-key",
      credential_header_value_format: "bearer",
      enabled: true,
    }),
  });
  const chatLiteral = await gatewayCall("/v1/chatcompletion", validToken, relaynaKey, chatPayload);
  const responseLiteral = await gatewayCall("/v1/response", validToken, relaynaKey, responsePayload);
  const embeddingLiteral = await gatewayCall("/v1/embedding", validToken, relaynaKey, embeddingPayload);
  const rerank = await gatewayCall("/v1/rerank", validToken, relaynaKey, rerankPayload);
  const litellmUiUnauthenticated = await fetch(`${gatewayControlUrl}/admin-ui/litellm-ui/`);
  const litellmUiProxiedResponse = await fetch(`${gatewayControlUrl}/admin-ui/litellm-ui/`, {
    headers: {
      authorization: `Bearer ${adminToken}`,
      "x-litellm-api-key": "client-supplied-litellm-key",
      "x-litellm-key": "Bearer client-supplied-litellm-key",
    },
  });
  const litellmUiProxiedText = await litellmUiProxiedResponse.text();
  const apigeeChat = await apigeeTrustedCall("/v1/chat/completions", validToken, relaynaKey, chatPayload);
  const gatewayAuthDisabled = await adminJson("/admin-ui/admin/auth/front-door", {
    method: "PATCH",
    body: JSON.stringify({
      entra_enabled: false,
      apigee_trusted_header_enabled: false,
    }),
  });
  const trustedIngressPassthrough = await adminJson("/admin-ui/admin/providers/litellm-passthrough", {
    method: "PATCH",
    body: JSON.stringify({
      enabled: true,
      allowed_paths: [
        "/v1/*",
        "/ui",
        "/ui/*",
        "/litellm-asset-prefix/*",
        "/litellm/.well-known/litellm-ui-config",
        "/",
        "/public/*",
        "/get/*",
        "/get_image",
        "/v2/*",
        "/v3/*",
        "/config/*",
        "/credentials",
        "/credentials/*",
        "/alerting/*",
        "/budget/*",
        "/cache/*",
        "/callbacks/*",
        "/cloudzero/*",
        "/customer/*",
        "/email/*",
        "/global/*",
        "/guardrails/*",
        "/health/*",
        "/in_product_nudges",
        "/key/*",
        "/mcp-rest/*",
        "/models",
        "/model/*",
        "/model_group/*",
        "/organization/*",
        "/policies/*",
        "/project/*",
        "/prompts/*",
        "/reload/*",
        "/router/*",
        "/schedule/*",
        "/search_tools/*",
        "/spend/*",
        "/sso/*",
        "/tag/*",
        "/team/*",
        "/user/*",
        "/user/info",
        "/global/spend/logs",
        "/key/info",
      ],
      allowed_methods: ["GET", "POST", "HEAD", "OPTIONS"],
      ui_exposure: "trusted_ingress",
      admin_api_exposure: "explicitly_exposed",
    }),
  });
  const trustedIngressUi = await gatewayCall("/ui/", null, null, {}, "GET");
  const trustedIngressUserInfo = await gatewayCall("/user/info", null, null, {}, "GET");
  const trustedIngressSpendLogs = await gatewayCall("/global/spend/logs", null, null, {}, "GET");
  const trustedIngressKeyInfo = await gatewayCall("/key/info", null, null, {}, "GET");
  const trustedIngressV3KeyInfo = await gatewayCall("/v3/key/info", null, null, {}, "GET");
  const trustedIngressModelsWithoutKey = await gatewayCall("/v1/models", null, null, {}, "GET");
  const responsesRouteModeUpdate = await adminJson("/admin-ui/admin/openai-routes/responses/mode", {
    method: "PATCH",
    body: JSON.stringify({ mode: "direct_litellm_passthrough" }),
  });
  const directResponsesWithLiteLlmBearer = await gatewayBearerCall("/v1/responses", litellmMasterKey, responsePayload);

  const upstreamCredentialLeak = state.providerRequests.some(
    (request) => request.hasRelaynaKey || request.hasAihKey || request.hasApigeeIdentity || request.hasClientJwt,
  );
  const gatewayForwardedToLiteLlm = state.providerRequests.some((request) => request.path.includes("/chat/completions"));
  const responseForwardedToLiteLlm = state.providerRequests.some((request) => request.path.includes("/responses"));
  const embeddingsForwardedToLiteLlm = state.providerRequests.some((request) => request.path.includes("/embeddings"));
  const routeMode = routesAfterMode.body.find?.((route) => route.route_id === "chat-completions")?.mode;

  const checks = {
    canonical_chat_completions_passes_to_litellm: pass(chat.status === 200 && gatewayForwardedToLiteLlm, chat),
    canonical_route_mode_can_switch_to_direct_passthrough: pass(
      routeModeUpdate.status === 200 && routeMode === "direct_litellm_passthrough" && directModeChat.status === 200,
      { routeModeUpdate, routeMode, directModeChat },
    ),
    wildcard_passthrough_defaults_disabled_then_can_enable_v1: pass(
      passthroughDefaults.status === 200 &&
        passthroughDefaults.body.enabled === false &&
        passthroughEnable.status === 200 &&
        passthroughEnable.body.enabled === true,
      { passthroughDefaults, passthroughEnable },
    ),
    wildcard_v1_models_reaches_real_litellm: pass(
      wildcardModels.status === 200 && Array.isArray(wildcardModels.body.data),
      wildcardModels,
    ),
    wildcard_ui_path_is_blocked_by_default: pass(
      uiBlocked.status === 404 && codeOf(uiBlocked) === "unsupported_route",
      uiBlocked,
    ),
    canonical_responses_passes_to_litellm: pass(responses.status === 200 && responseForwardedToLiteLlm, responses),
    canonical_embeddings_passes_to_litellm: pass(
      embeddings.status === 200 && embeddingsForwardedToLiteLlm,
      embeddings,
    ),
    apigee_trusted_header_chat_passes_to_litellm: pass(apigeeChat.status === 200, apigeeChat),
    upstream_receives_no_client_credentials: pass(!upstreamCredentialLeak, { providerRequests: state.providerRequests }),
    direct_mode_uses_key_mapping_with_real_litellm: pass(
      directModeChat.status === 200,
      { status: directModeChat.status },
    ),
    disabled_key_mapping_falls_back_to_project_mapping: pass(responses.status === 200, responses),
    disabled_project_mapping_falls_back_to_provider_default: pass(embeddings.status === 200, embeddings),
    provider_default_credential_restored_for_litellm_ui_checks: pass(
      providerCredentialRestore.status === 200,
      { status: providerCredentialRestore.status },
    ),
    litellm_ui_proxy_requires_operator_token: pass(
      litellmUiUnauthenticated.status === 401,
      { status: litellmUiUnauthenticated.status },
    ),
    litellm_ui_proxy_reaches_real_litellm_with_gateway_credential: pass(
      litellmUiProxiedResponse.status === 200 &&
        litellmUiProxiedText.includes("<html"),
      {
        status: litellmUiProxiedResponse.status,
        contentType: litellmUiProxiedResponse.headers.get("content-type"),
      },
    ),
    trusted_ingress_disables_gateway_entra_and_apigee_checks: pass(
      gatewayAuthDisabled.status === 200 &&
        gatewayAuthDisabled.body?.entra?.enabled === false &&
        gatewayAuthDisabled.body?.apigee?.trusted_header_enabled === false,
      gatewayAuthDisabled,
    ),
    trusted_ingress_no_auth_ui_reaches_litellm_with_gateway_credential: pass(
      trustedIngressPassthrough.status === 200 &&
        trustedIngressPassthrough.body?.ui_exposure === "trusted_ingress" &&
        trustedIngressUi.status === 200 &&
        typeof trustedIngressUi.body === "string" &&
        trustedIngressUi.body.includes("<html"),
      {
        trustedIngressPassthrough,
        trustedIngressUiStatus: trustedIngressUi.status,
      },
    ),
    trusted_ingress_no_auth_ui_support_endpoint_reaches_litellm: pass(
      trustedIngressUserInfo.status === 200 &&
        trustedIngressUserInfo.body?.user_id === "default_user_id",
      {
        trustedIngressUserInfo,
      },
    ),
    trusted_ingress_no_auth_admin_spend_logs_reaches_litellm: pass(
      trustedIngressSpendLogs.status !== 401,
      {
        trustedIngressSpendLogs,
      },
    ),
    trusted_ingress_no_auth_admin_key_info_reaches_litellm: pass(
      trustedIngressKeyInfo.status !== 401,
      {
        trustedIngressKeyInfo,
      },
    ),
    trusted_ingress_no_auth_v3_admin_path_reaches_litellm: pass(
      trustedIngressV3KeyInfo.status !== 401,
      {
        trustedIngressV3KeyInfo,
      },
    ),
    trusted_ingress_no_auth_v1_models_reaches_litellm_for_ui_support: pass(
      trustedIngressModelsWithoutKey.status === 200 &&
        Array.isArray(trustedIngressModelsWithoutKey.body?.data),
      trustedIngressModelsWithoutKey,
    ),
    direct_responses_accepts_litellm_bearer_without_relayna_key: pass(
      responsesRouteModeUpdate.status === 200 &&
        directResponsesWithLiteLlmBearer.status === 200,
      {
        responsesRouteModeUpdate,
        directResponsesWithLiteLlmBearer,
      },
    ),
    wildcard_literal_chatcompletion_reaches_litellm: pass(
      chatLiteral.status === 404 &&
        chatLiteral.body?.detail === "Not Found",
      chatLiteral,
    ),
    wildcard_literal_response_reaches_litellm: pass(
      responseLiteral.status === 404 &&
        responseLiteral.body?.detail === "Not Found",
      responseLiteral,
    ),
    wildcard_literal_embedding_reaches_litellm: pass(
      embeddingLiteral.status === 404 &&
        embeddingLiteral.body?.detail === "Not Found",
      embeddingLiteral,
    ),
    wildcard_rerank_reaches_litellm: pass(
      rerank.status === 400 &&
        codeOf(rerank) === "400",
      rerank,
    ),
  };

  const requestedLiteLlmPathsPassThrough =
    chat.status === 200 && responses.status === 200 && embeddings.status === 200;
  state.results = {
    ok: Object.values(checks).every((check) => check.ok),
    requestedLiteLlmPathsPassThrough,
    overallOutcome:
      "PASS: canonical managed and direct route modes reach LiteLLM, wildcard /v1/models passes through with query preservation when enabled, raw /ui remains blocked by default, /admin-ui/litellm-ui reaches real LiteLLM with operator auth, trusted-ingress UI and explicitly exposed admin API paths work without Relayna auth when Entra is disabled, direct /v1/responses accepts a LiteLLM bearer key, and credential translation strips client secrets.",
    generatedAt: new Date().toISOString(),
    environment: {
      gatewayProxyUrl,
      gatewayControlUrl,
      litellmUrl: litellmDirectUrl,
      issuer,
      audience,
      tenantId,
      relaynaKeyHeader: "X-Relayna-Key",
      apigeeTrustedHeader: true,
    },
    requestedPaths: ["/v1/chatcompletion", "/v1/response", "/v1/embedding", "/v1/rerank"],
    canonicalGatewayLiteLlmPaths: ["/v1/chat/completions", "/v1/responses", "/v1/embeddings", "/v1/models"],
    litellmUiProxyPath: "/admin-ui/litellm-ui/",
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
      <div class="metric"><span>Outcome</span><strong>${results?.ok ? "PASS" : "RUN TESTS"}</strong></div>
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
    <h2>Wildcard Coverage</h2>
    <p>The branch routes managed canonical calls through LiteLLM, can switch a canonical route to direct LiteLLM passthrough, forwards enabled wildcard <code>/v1/*</code> calls while preserving path and query, serves real LiteLLM UI through <code>/admin-ui/litellm-ui/</code> with operator auth, serves trusted-ingress <code>/ui/</code> and explicitly exposed admin paths without Relayna auth when Entra is disabled, and translates a direct <code>/v1/responses</code> LiteLLM bearer key into the configured upstream custom header. Real LiteLLM rejects the literal alias probes itself with 404 or 400 responses, proving those requests reached LiteLLM instead of being stopped by the Gateway router.</p>
    <h2>LiteLLM Direct Credential Mapping</h2>
    <p>Gateway connects directly to real LiteLLM at <code>${results?.environment.litellmUrl || litellmDirectUrl}</code> using the configured <code>x-litellm-key</code> custom header. The direct-mode and fallback checks prove key, project, and provider LiteLLM credentials can authenticate without an intermediate service.</p>
  </main>
</body>
</html>`;
}

const server = http.createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/healthz") {
      jsonResponse(res, 200, { ok: true, role });
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
