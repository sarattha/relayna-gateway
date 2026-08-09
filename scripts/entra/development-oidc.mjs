// Development-only Entra-shaped OIDC issuer adapted from Arcweft's local
// confidential-flow fixture. It deliberately refuses production environments.
import { constants, createHash, createPublicKey, createSign, generateKeyPairSync, randomUUID, timingSafeEqual, verify, X509Certificate } from "node:crypto";
import { readFileSync } from "node:fs";
import http from "node:http";

const environment = String(process.env.RELAYNA_ENV ?? process.env.NODE_ENV ?? "development").toLowerCase();
if (["production", "prod"].includes(environment)) throw new Error("Relayna development OIDC cannot run in production.");

const host = process.env.RELAYNA_DEV_OIDC_HOST ?? "127.0.0.1";
const port = Number(process.env.RELAYNA_DEV_OIDC_PORT ?? 18090);
const issuer = process.env.RELAYNA_DEV_OIDC_ISSUER ?? `http://127.0.0.1:${port}`;
const tenantId = "00000000-0000-0000-0000-000000000001";
const applicationId = "relayna-gateway-local";
const applicationIdentifierUri = `api://${applicationId}`;
const browserCertificatePath = process.env.RELAYNA_DEV_OIDC_BROWSER_CERTIFICATE_PATH;
if (!browserCertificatePath) throw new Error("RELAYNA_DEV_OIDC_BROWSER_CERTIFICATE_PATH is required.");
const browserCertificate = new X509Certificate(readFileSync(browserCertificatePath));
const browserCertificateThumbprint = createHash("sha256").update(browserCertificate.raw).digest("base64url");
const browserRedirectUri = process.env.RELAYNA_DEV_OIDC_BROWSER_REDIRECT_URI ?? "http://127.0.0.1:18381/admin-ui/auth/callback";
const browserPostLogoutRedirectUri = process.env.RELAYNA_DEV_OIDC_BROWSER_POST_LOGOUT_REDIRECT_URI ?? "http://127.0.0.1:18381/admin-ui";
const workloads = new Map([
  ["00000000-0000-0000-0000-000000000101", {
    objectId: "00000000-0000-0000-0000-000000000102",
    secret: "relayna-development-invoke-secret",
    roles: ["gateway.invoke"],
  }],
  ["00000000-0000-0000-0000-000000000201", {
    objectId: "00000000-0000-0000-0000-000000000202",
    secret: "relayna-development-monitor-secret",
    roles: ["gateway.monitor.read"],
  }],
]);

const personas = {
  pending_user: {
    oid: "00000000-0000-0000-0000-000000000003",
    name: "Pending Service Owner",
    email: "pending.owner@relayna.dev",
    badge: "Pending",
    description: "First sign-in creates a pending Relayna member without data access.",
  },
  gateway_admin: {
    oid: "00000000-0000-0000-0000-000000000002",
    name: "Gateway Administrator",
    email: "gateway.admin@relayna.dev",
    badge: "Admin",
    description: "Use break-glass access once to approve this identity as an Admin member.",
  },
  service_owner: {
    oid: "00000000-0000-0000-0000-000000000004",
    name: "Orders Service Owner",
    email: "orders.owner@relayna.dev",
    badge: "Owner",
    description: "Assign Owner access to a registered service from the Members page.",
  },
};

const privateKey = generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey;
const publicJwk = createPublicKey(privateKey).export({ format: "jwk" });
const kid = `relayna-dev-${createHash("sha256").update(JSON.stringify(publicJwk)).digest("hex").slice(0, 16)}`;
const codes = new Map();
const assertionJtis = new Map();

function base64url(value) {
  return Buffer.from(typeof value === "string" ? value : JSON.stringify(value)).toString("base64url");
}

function signJwt(claims) {
  const header = base64url({ alg: "RS256", typ: "JWT", kid });
  const payload = base64url(claims);
  const signer = createSign("RSA-SHA256");
  signer.update(`${header}.${payload}`);
  return `${header}.${payload}.${signer.sign(privateKey).toString("base64url")}`;
}

function secureEqual(left, right) {
  const a = Buffer.from(left);
  const b = Buffer.from(right);
  return a.length === b.length && timingSafeEqual(a, b);
}

function decodeJsonSegment(value) {
  return JSON.parse(Buffer.from(value, "base64url").toString("utf8"));
}

function validatePrivateKeyJwt(body) {
  const assertion = body.get("client_assertion") ?? "";
  if (body.get("client_assertion_type") !== "urn:ietf:params:oauth:client-assertion-type:jwt-bearer") return false;
  try {
    const [headerValue, payloadValue, signatureValue, extra] = assertion.split(".");
    if (!headerValue || !payloadValue || !signatureValue || extra) return false;
    const header = decodeJsonSegment(headerValue);
    const payload = decodeJsonSegment(payloadValue);
    const now = Math.floor(Date.now() / 1000);
    if (
      header.alg !== "PS256" || header.typ !== "JWT"
      || header["x5t#S256"] !== browserCertificateThumbprint
      || payload.iss !== applicationId || payload.sub !== applicationId
      || payload.aud !== `${issuer}/token`
      || typeof payload.jti !== "string" || !payload.jti
      || typeof payload.iat !== "number" || typeof payload.nbf !== "number" || typeof payload.exp !== "number"
      || payload.iat > now + 30 || payload.nbf > now + 30 || payload.exp <= now
      || payload.exp - payload.iat > 600 || assertionJtis.has(payload.jti)
    ) return false;
    const valid = verify(
      "sha256",
      Buffer.from(`${headerValue}.${payloadValue}`),
      {
        key: browserCertificate.publicKey,
        padding: constants.RSA_PKCS1_PSS_PADDING,
        saltLength: constants.RSA_PSS_SALTLEN_DIGEST,
      },
      Buffer.from(signatureValue, "base64url"),
    );
    if (!valid) return false;
    assertionJtis.set(payload.jti, payload.exp);
    for (const [jti, expiresAt] of assertionJtis) if (expiresAt <= now) assertionJtis.delete(jti);
    return true;
  } catch {
    return false;
  }
}

function json(response, status, body) {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(body));
}

function html(response, body) {
  response.writeHead(200, {
    "content-type": "text/html; charset=utf-8",
    "cache-control": "no-store",
    "content-security-policy": "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",
    "x-content-type-options": "nosniff",
  });
  response.end(body);
}

function escapeHtml(value) {
  return String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#039;");
}

function accountChooser(authorizeUrl) {
  const accounts = Object.entries(personas).map(([key, persona]) => {
    const target = new URL(authorizeUrl);
    target.searchParams.set("mock_user", key);
    return `<a class="account" href="${escapeHtml(target)}"><span class="avatar">${escapeHtml(persona.name.split(/\s+/).map((part) => part[0]).join("").slice(0, 2))}</span><span><strong>${escapeHtml(persona.name)}</strong><small>${escapeHtml(persona.email)}</small><em>${escapeHtml(persona.description)}</em></span><b>${escapeHtml(persona.badge)}</b></a>`;
  }).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Relayna development sign-in</title><style>
  :root{font-family:Inter,ui-sans-serif,system-ui;background:#e8f3f0;color:#153431}*{box-sizing:border-box}body{min-height:100vh;margin:0;display:grid;place-items:center;padding:24px;background:radial-gradient(circle at top left,#fff 0,transparent 44%),#e8f3f0}main{width:min(720px,100%)}h1{font-size:clamp(30px,7vw,48px);letter-spacing:-.04em;margin:0}.intro{color:#58736e;line-height:1.6}.panel{background:#fff;border:1px solid #cfe4df;border-radius:16px;padding:10px;box-shadow:0 24px 70px #062f3620}.account{display:grid;grid-template-columns:48px minmax(0,1fr) auto;gap:14px;align-items:center;padding:16px;color:inherit;text-decoration:none;border-radius:10px}.account+.account{border-top:1px solid #dceae6}.account:hover,.account:focus-visible{background:#f3faf8;outline:3px solid #087b6040}.avatar{display:grid;place-items:center;width:48px;height:48px;border-radius:12px;background:#dcefe9;color:#05634e;font-weight:800}.account span:nth-child(2){display:grid;gap:4px;min-width:0}.account small,.account em{color:#58736e;font-size:12px;font-style:normal}.account b{border:1px solid #cfe4df;border-radius:999px;padding:4px 8px;font-size:11px}@media(max-width:520px){body{padding:14px;align-items:start}.account{grid-template-columns:42px minmax(0,1fr)}.avatar{width:42px;height:42px}.account b{grid-column:2}}
  </style></head><body><main><p>Relayna development identity provider</p><h1>Choose a test account</h1><p class="intro">These Entra-shaped identities exercise the real database authorization path. No password or production credential is used.</p><section class="panel">${accounts}</section></main></body></html>`;
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    let value = "";
    request.on("data", (chunk) => {
      value += chunk;
      if (value.length > 32_768) request.destroy();
    });
    request.on("end", () => resolve(new URLSearchParams(value)));
    request.on("error", reject);
  });
}

function issueBrowserToken(persona, nonce) {
  const now = Math.floor(Date.now() / 1000);
  return signJwt({
    iss: issuer, aud: applicationId, tid: tenantId, ver: "2.0", sub: persona.oid,
    oid: persona.oid, azp: applicationId, nonce, name: persona.name, email: persona.email,
    preferred_username: persona.email, iat: now, nbf: now - 1, exp: now + 600,
  });
}

function issueWorkloadToken(clientId, workload) {
  const now = Math.floor(Date.now() / 1000);
  return signJwt({
    iss: issuer, aud: applicationId, tid: tenantId, ver: "2.0", sub: workload.objectId,
    oid: workload.objectId, appid: clientId, azp: clientId,
    roles: workload.roles, iat: now, nbf: now - 1, exp: now + 600,
  });
}

const server = http.createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", issuer);
  if (url.pathname === "/.well-known/openid-configuration") return json(response, 200, {
    issuer, authorization_endpoint: `${issuer}/authorize`, token_endpoint: `${issuer}/token`,
    end_session_endpoint: `${issuer}/logout`,
    jwks_uri: `${issuer}/.well-known/jwks.json`, response_types_supported: ["code"],
    grant_types_supported: ["authorization_code", "client_credentials"],
    subject_types_supported: ["public"], id_token_signing_alg_values_supported: ["RS256"],
    token_endpoint_auth_methods_supported: ["private_key_jwt", "client_secret_post"], code_challenge_methods_supported: ["S256"],
  });
  if (url.pathname === "/.well-known/jwks.json") return json(response, 200, { keys: [{ ...publicJwk, kid, use: "sig", alg: "RS256" }] });
  if (url.pathname === "/health") return json(response, 200, { status: "ok", service: "relayna-development-oidc" });
  if (url.pathname === "/logout" && request.method === "GET") {
    const target = url.searchParams.get("post_logout_redirect_uri");
    if (target !== browserPostLogoutRedirectUri) return json(response, 400, { error: "invalid_request" });
    response.writeHead(302, { location: target, "cache-control": "no-store" });
    return response.end();
  }
  if (url.pathname === "/authorize" && request.method === "GET") {
    const valid = url.searchParams.get("client_id") === applicationId
      && url.searchParams.get("redirect_uri") === browserRedirectUri
      && url.searchParams.get("response_type") === "code"
      && url.searchParams.get("code_challenge_method") === "S256"
      && (url.searchParams.get("scope") ?? "").split(" ").includes("openid");
    if (!valid) return json(response, 400, { error: "invalid_request" });
    const key = url.searchParams.get("mock_user");
    if (!key) return html(response, accountChooser(url));
    const persona = personas[key];
    if (!persona) return json(response, 400, { error: "invalid_request" });
    const code = randomUUID();
    codes.set(code, {
      persona, nonce: url.searchParams.get("nonce"), redirectUri: url.searchParams.get("redirect_uri"),
      challenge: url.searchParams.get("code_challenge"), expiresAt: Date.now() + 120_000,
    });
    const redirect = new URL(url.searchParams.get("redirect_uri"));
    redirect.searchParams.set("code", code);
    redirect.searchParams.set("state", url.searchParams.get("state") ?? "");
    response.writeHead(302, { location: redirect.toString(), "cache-control": "no-store" });
    return response.end();
  }
  if (url.pathname === "/token" && request.method === "POST") {
    const body = await readBody(request);
    if (body.get("grant_type") === "client_credentials") {
      const clientId = body.get("client_id") ?? "";
      const workload = workloads.get(clientId);
      if (!workload || !secureEqual(body.get("client_secret") ?? "", workload.secret) || body.get("scope") !== `${applicationIdentifierUri}/.default`) return json(response, 401, { error: "invalid_client" });
      return json(response, 200, { access_token: issueWorkloadToken(clientId, workload), token_type: "Bearer", expires_in: 600 });
    }
    const code = codes.get(body.get("code"));
    codes.delete(body.get("code"));
    const verifierChallenge = createHash("sha256").update(body.get("code_verifier") ?? "").digest("base64url");
    const valid = body.get("grant_type") === "authorization_code"
      && body.get("client_id") === applicationId
      && validatePrivateKeyJwt(body)
      && body.get("redirect_uri") === browserRedirectUri
      && code && code.expiresAt > Date.now() && code.redirectUri === browserRedirectUri
      && secureEqual(verifierChallenge, code.challenge ?? "");
    if (!valid) return json(response, 400, { error: "invalid_grant" });
    return json(response, 200, { id_token: issueBrowserToken(code.persona, code.nonce), token_type: "Bearer", expires_in: 600 });
  }
  json(response, 404, { error: "not_found" });
});

server.listen(port, host, () => {
  console.log(`Relayna development OIDC listening at ${issuer}`);
  console.log(`Application: ${applicationId}; resource: ${applicationIdentifierUri}; workload clients: ${[...workloads.keys()].join(", ")}`);
});
