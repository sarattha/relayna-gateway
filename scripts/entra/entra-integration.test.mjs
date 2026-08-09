import assert from "node:assert/strict";
import { constants, createHash, randomUUID, sign, X509Certificate } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptDirectory = fileURLToPath(new URL(".", import.meta.url));
const generator = join(scriptDirectory, "generate-development-portal-certificate.sh");
const issuerScript = join(scriptDirectory, "development-oidc.mjs");
const deploymentManifest = fileURLToPath(new URL("../../deploy/kubernetes/relayna-gateway.yaml", import.meta.url));
const checkedInHarnesses = [
  [
    "../../internal/test-reports/entra-front-door-real-env/docker-compose.yml",
    "../../internal/test-reports/entra-front-door-real-env/mock-app/server.mjs",
  ],
  [
    "../../internal/test-reports/front-door-penetration/docker-compose.yml",
    "../../internal/test-reports/front-door-penetration/mock-provider/server.mjs",
  ],
  [
    "../../internal/test-reports/litellm-real-passthrough/docker-compose.yml",
    "../../internal/test-reports/litellm-real-passthrough/mock-provider/server.mjs",
  ],
].map(([compose, issuer]) => [
  fileURLToPath(new URL(compose, import.meta.url)),
  fileURLToPath(new URL(issuer, import.meta.url)),
]);
const redirectUri = "http://127.0.0.1:18381/admin-ui/auth/callback";
let directory;
let privateKey;
let certificate;
let certificateThumbprint;
let issuer;
let child;

function base64url(value) {
  return Buffer.from(typeof value === "string" ? value : JSON.stringify(value)).toString("base64url");
}

function clientAssertion(overrides = {}) {
  const now = Math.floor(Date.now() / 1000);
  const header = {
    alg: "PS256",
    typ: "JWT",
    "x5t#S256": certificateThumbprint,
    ...overrides.header,
  };
  const claims = {
    iss: "relayna-gateway-local",
    sub: "relayna-gateway-local",
    aud: `${issuer}/token`,
    iat: now,
    nbf: now - 5,
    exp: now + 300,
    jti: randomUUID(),
    ...overrides.claims,
  };
  const signingInput = `${base64url(header)}.${base64url(claims)}`;
  const signature = sign("sha256", Buffer.from(signingInput), {
    key: privateKey,
    padding: constants.RSA_PKCS1_PSS_PADDING,
    saltLength: constants.RSA_PSS_SALTLEN_DIGEST,
  });
  return `${signingInput}.${signature.toString("base64url")}`;
}

async function reservePort() {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => resolve(address.port));
    });
  });
}

async function authorizationCode(verifier) {
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  const url = new URL(`${issuer}/authorize`);
  for (const [name, value] of Object.entries({
    client_id: "relayna-gateway-local",
    redirect_uri: redirectUri,
    response_type: "code",
    response_mode: "query",
    scope: "openid profile email",
    state: randomUUID(),
    nonce: randomUUID(),
    code_challenge: challenge,
    code_challenge_method: "S256",
    mock_user: "gateway_admin",
  })) url.searchParams.set(name, value);
  const response = await fetch(url, { redirect: "manual" });
  assert.equal(response.status, 302);
  return new URL(response.headers.get("location")).searchParams.get("code");
}

async function exchange(code, verifier, assertion) {
  return await fetch(`${issuer}/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "authorization_code",
      client_id: "relayna-gateway-local",
      client_assertion_type: "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
      client_assertion: assertion,
      redirect_uri: redirectUri,
      code,
      code_verifier: verifier,
    }),
  });
}

async function workloadToken(clientId, clientSecret, scope = "api://relayna-gateway-local/.default") {
  return await fetch(`${issuer}/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "client_credentials",
      client_id: clientId,
      client_secret: clientSecret,
      scope,
    }),
  });
}

function tokenClaims(token) {
  return JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString("utf8"));
}

before(async () => {
  directory = await mkdtemp(join(tmpdir(), "relayna-entra-integration-"));
  const generated = spawnSync("bash", [generator, "--output-dir", directory, "--days", "1"], {
    encoding: "utf8",
  });
  assert.equal(generated.status, 0, generated.stderr);
  const refused = spawnSync("bash", [generator, "--output-dir", directory, "--days", "1"], {
    encoding: "utf8",
  });
  assert.notEqual(refused.status, 0, "generator must refuse to overwrite keys");
  privateKey = await readFile(join(directory, "portal-private-key.pem"), "utf8");
  certificate = new X509Certificate(await readFile(join(directory, "portal-certificate.pem")));
  certificateThumbprint = createHash("sha256").update(certificate.raw).digest("base64url");
  const port = await reservePort();
  issuer = `http://127.0.0.1:${port}`;
  child = spawn(process.execPath, [issuerScript], {
    env: {
      ...process.env,
      RELAYNA_DEV_OIDC_PORT: String(port),
      RELAYNA_DEV_OIDC_ISSUER: issuer,
      RELAYNA_DEV_OIDC_BROWSER_CERTIFICATE_PATH: join(directory, "portal-certificate.pem"),
      RELAYNA_DEV_OIDC_BROWSER_REDIRECT_URI: redirectUri,
      RELAYNA_DEV_OIDC_BROWSER_POST_LOGOUT_REDIRECT_URI: "http://127.0.0.1:18381/admin-ui",
    },
    stdio: "ignore",
  });
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(`${issuer}/health`);
      if (response.ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("development OIDC did not become ready");
});

after(async () => {
  child?.kill();
  await rm(directory, { recursive: true, force: true });
});

test("development issuer accepts certificate private_key_jwt and rejects replay", async () => {
  const discovery = await fetch(`${issuer}/.well-known/openid-configuration`).then((response) => response.json());
  assert.deepEqual(discovery.token_endpoint_auth_methods_supported, ["private_key_jwt", "client_secret_post"]);
  const verifier = `pkce-${randomUUID()}`;
  const assertion = clientAssertion();
  const response = await exchange(await authorizationCode(verifier), verifier, assertion);
  assert.equal(response.status, 200);
  assert.ok((await response.json()).id_token);
  const replay = await exchange(await authorizationCode(verifier), verifier, assertion);
  assert.equal(replay.status, 400);
});

test("development issuer rejects assertions with the wrong audience or certificate thumbprint", async () => {
  const verifier = `pkce-${randomUUID()}`;
  const wrongAudience = await exchange(
    await authorizationCode(verifier),
    verifier,
    clientAssertion({ claims: { aud: `${issuer}/wrong-token-endpoint` } }),
  );
  assert.equal(wrongAudience.status, 400);
  const wrongThumbprint = await exchange(
    await authorizationCode(verifier),
    verifier,
    clientAssertion({ header: { "x5t#S256": "wrong-thumbprint" } }),
  );
  assert.equal(wrongThumbprint.status, 400);
});

test("one application issues least-privilege tokens to separate managed identities", async () => {
  const invokeResponse = await workloadToken(
    "00000000-0000-0000-0000-000000000101",
    "relayna-development-invoke-secret",
  );
  assert.equal(invokeResponse.status, 200);
  const invokeClaims = tokenClaims((await invokeResponse.json()).access_token);
  assert.equal(invokeClaims.aud, "relayna-gateway-local");
  assert.equal(invokeClaims.azp, "00000000-0000-0000-0000-000000000101");
  assert.deepEqual(invokeClaims.roles, ["gateway.invoke"]);

  const monitorResponse = await workloadToken(
    "00000000-0000-0000-0000-000000000201",
    "relayna-development-monitor-secret",
  );
  assert.equal(monitorResponse.status, 200);
  const monitorClaims = tokenClaims((await monitorResponse.json()).access_token);
  assert.equal(monitorClaims.aud, "relayna-gateway-local");
  assert.equal(monitorClaims.azp, "00000000-0000-0000-0000-000000000201");
  assert.deepEqual(monitorClaims.roles, ["gateway.monitor.read"]);

  const oldOwnerResource = await workloadToken(
    "00000000-0000-0000-0000-000000000201",
    "relayna-development-monitor-secret",
    "api://relayna-gateway-owner/.default",
  );
  assert.equal(oldOwnerResource.status, 401);
  const crossedCredential = await workloadToken(
    "00000000-0000-0000-0000-000000000101",
    "relayna-development-monitor-secret",
  );
  assert.equal(crossedCredential.status, 401);
});

test("raw Kubernetes manifest preserves the Entra certificate and owner routing contract", async () => {
  const manifest = await readFile(deploymentManifest, "utf8");
  for (const expected of [
    'PORTAL_OIDC_PRIVATE_KEY_PATH: "/var/run/secrets/relayna-portal-oidc/portal-private-key.pem"',
    'PORTAL_OIDC_CERTIFICATE_PATH: "/var/run/secrets/relayna-portal-oidc/portal-certificate.pem"',
    'ENTRA_APPLICATION_ID: ""',
    'PORTAL_ADMIN_OBJECT_IDS: ""',
    'PORTAL_ADMIN_EMAILS: ""',
    "name: relayna-gateway-portal-oidc",
    "mountPath: /var/run/secrets/relayna-portal-oidc",
    "readOnly: true",
    "- path: /owner/v1",
    'relayna.io/control-plane-access: "true"',
  ]) assert.match(manifest, new RegExp(expected.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  assert.doesNotMatch(manifest, /PORTAL_OIDC_CLIENT_SECRET|PORTAL_OIDC_CLIENT_ID|OWNER_ENTRA_AUDIENCE|ENTRA_AUDIENCE/);
});

test("checked-in Entra harnesses use the shared application and invoke-role contract", async () => {
  for (const [composePath, issuerPath] of checkedInHarnesses) {
    const [compose, mockIssuer] = await Promise.all([
      readFile(composePath, "utf8"),
      readFile(issuerPath, "utf8"),
    ]);
    assert.match(compose, /ENTRA_APPLICATION_ID:/);
    assert.doesNotMatch(compose, /ENTRA_AUDIENCE:/);
    assert.match(mockIssuer, /roles: \["gateway\.invoke"\]/);
  }
});
