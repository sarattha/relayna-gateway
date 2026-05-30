# Apigee Gateway Path

Relayna Gateway `0.1.7` supports two Apigee front-door patterns for provider
traffic:

1. Apigee verifies or forwards the original Entra JWT, and Relayna Gateway
   revalidates that JWT exactly like direct Entra mode.
2. Apigee terminates Entra validation and forwards a sanitized identity header
   plus an HMAC proof that Relayna Gateway verifies before accepting the
   request.

The first pattern is preferred when Apigee can forward the original access
token to Gateway over a private, trusted network. The second pattern exists for
deployments where Apigee must not forward the original JWT but Gateway still
needs a cryptographic proof that the identity header came from Apigee.

## Shared Contract

Apigee does not replace Relayna virtual keys. The Relayna key is still required
for policy, budget, rate-limit, guardrail, project, and usage attribution.

The default Relayna key header is:

```http
X-Relayna-Key: rk_live_...
```

Operators can change it with:

```bash
export ENTRA_RELAYNA_KEY_HEADER="X-Company-Relayna-Key"
```

The Apigee path applies to the same proxy route families as direct Entra mode:

- `/v1/chat/completions`
- `/v1/responses`
- `/providers/openai/*`
- Built-in internal service routes such as `/summary`, `/translation`, `/ocr`,
  and `/embeddings`
- Registered service wildcard routes under `/services/<service-name>/*`

## Pattern 1: Apigee JWT Revalidation

In JWT revalidation mode, Apigee validates the token for edge policy and still
forwards the original JWT to Relayna Gateway:

```http
Authorization: Bearer <original Entra access token>
X-Relayna-Key: rk_live_...
```

Relayna Gateway then performs the same checks documented in
[Entra ID Auth](entra-id-auth.md): `kid`, algorithm, OIDC metadata, JWKS,
signature, issuer, tenant, audience, timestamps, token version, scopes, roles,
groups, and group overage.

Use this mode when:

- Gateway can safely receive the original JWT from Apigee.
- You want Gateway logs, metrics, and failures to reflect Gateway's own JWT
  validation decision.
- You want unknown `kid` refresh behavior and JWKS cache settings to live in
  Gateway.
- You want one validation path for direct and Apigee-routed traffic.

Configuration is the same as direct Entra mode:

```bash
export ENTRA_AUTH_ENABLED="true"
export ENTRA_TENANT_ID="00000000-0000-0000-0000-000000000000"
export ENTRA_AUDIENCE="api://relayna-gateway"
export ENTRA_ISSUER="https://login.microsoftonline.com/00000000-0000-0000-0000-000000000000/v2.0"
export ENTRA_OIDC_DISCOVERY_URL="https://login.microsoftonline.com/00000000-0000-0000-0000-000000000000/v2.0/.well-known/openid-configuration"
export ENTRA_REQUIRED_SCOPE="gateway.invoke"
export ENTRA_RELAYNA_KEY_HEADER="X-Relayna-Key"
```

Apigee should strip any inbound user-supplied `X-Relayna-Key` variants before
it sets the allowed Relayna key header, enforce TLS to Gateway, and avoid
logging raw JWTs or Relayna keys.

## Pattern 2: Trusted Signed Header

Trusted signed-header mode is for deployments where Apigee validates Entra and
then forwards only sanitized identity data to Relayna Gateway.

Enable it explicitly from the Admin portal Settings page, or set the equivalent
deployment environment variables:

```bash
export APIGEE_TRUSTED_HEADER_ENABLED="true"
export APIGEE_TRUSTED_HEADER_SECRET="<shared-hmac-secret>"
export ENTRA_RELAYNA_KEY_HEADER="X-Relayna-Key"
```

When `APIGEE_TRUSTED_HEADER_ENABLED=true`, Gateway accepts a trusted identity
only when both headers are present and the signature is valid:

```http
X-Apigee-Entra-Identity: <base64url-json>
X-Apigee-Entra-Signature: <base64url-hmac-sha256>
X-Relayna-Key: rk_live_...
```

The HMAC is computed over the exact `X-Apigee-Entra-Identity` header value
using `APIGEE_TRUSTED_HEADER_SECRET` and SHA-256, then encoded as unpadded
base64url.

The identity JSON maps to Gateway's sanitized Entra identity context:

```json
{
  "tenant_id": "00000000-0000-0000-0000-000000000000",
  "subject": "user-or-client-subject",
  "object_id": "user-or-service-principal-object-id",
  "app_id": "client-application-id",
  "authorized_party": "client-application-id",
  "scopes": ["gateway.invoke"],
  "roles": ["Gateway.Invoke"],
  "groups": ["11111111-1111-1111-1111-111111111111"],
  "token_version": "2.0",
  "source": "jwt"
}
```

Gateway rewrites `source` to `apigee_trusted_header` after signature
verification, so downstream telemetry can distinguish direct JWT validation
from trusted Apigee identity proof.

## HMAC Pseudocode

Apigee policy implementations vary, but the proof must be equivalent to:

```text
identity_header = base64url_without_padding(json_utf8(identity_context))
signature = base64url_without_padding(HMAC_SHA256(secret, identity_header))
```

Node.js equivalent:

```javascript
import crypto from "node:crypto";

function base64url(buffer) {
  return Buffer.from(buffer).toString("base64url");
}

const identityHeader = base64url(JSON.stringify(identity));
const signature = base64url(
  crypto.createHmac("sha256", process.env.APIGEE_TRUSTED_HEADER_SECRET)
    .update(identityHeader)
    .digest(),
);
```

Gateway compares the expected signature using constant-time byte comparison.
Missing headers, malformed base64url, malformed JSON, an empty secret, or a
wrong signature all fail closed with `untrusted_apigee_identity`.

## Gateway Selection Logic

When Apigee trusted-header mode is configured, Gateway checks whether either
Apigee identity header is present.

- If either `X-Apigee-Entra-Identity` or `X-Apigee-Entra-Signature` is present,
  Gateway uses trusted signed-header verification.
- If neither Apigee header is present and `ENTRA_AUTH_ENABLED=true`, Gateway
  falls back to direct Entra JWT validation from `Authorization`.
- If neither Apigee header is present and direct Entra validation is not
  configured, Gateway rejects the request with `missing_entra_authorization`.

This means a deployment can support both Apigee trusted headers and direct
Gateway JWT validation during migration, but the trusted-header path is never
accepted without an HMAC proof.

## Apigee Responsibilities

Apigee should do the following before forwarding traffic to Gateway:

- Terminate external TLS and use TLS or mTLS to Gateway.
- Verify Entra JWT issuer, audience, expiry, signature, and authorization
  policy at the edge.
- Remove any inbound user-supplied `X-Apigee-Entra-Identity` and
  `X-Apigee-Entra-Signature` headers before setting its own values.
- Remove any inbound user-supplied Relayna key header if policy injects or
  normalizes Relayna keys.
- Build the identity JSON only from verified token claims or Apigee policy
  context, not from user-supplied headers.
- Sign the exact base64url identity header value with the shared HMAC secret.
- Keep `APIGEE_TRUSTED_HEADER_SECRET` out of logs, traces, and policy exports.
- Forward the Relayna virtual key in the configured Relayna key header.
- Avoid logging raw Relayna keys and provider credentials.

## Gateway Responsibilities

Relayna Gateway always performs the second-stage Relayna checks:

- Validate the HMAC proof or revalidate the original Entra JWT.
- Authenticate the Relayna virtual key from `ENTRA_RELAYNA_KEY_HEADER`.
- Enforce route, service, model, provider, streaming, tools, body-size, rate
  limit, budget, and guardrail policy.
- Resolve the upstream provider or registered service.
- Strip all client credentials and Apigee proof headers before forwarding.
- Inject internal provider or service credentials.
- Record usage and failure attribution without raw tokens.

## Header Stripping

Gateway strips these headers before upstream forwarding:

- `Authorization`
- The configured Relayna key header
- `X-Relayna-Key`
- `X-AIH-API-Key`
- `Proxy-Authorization`
- `X-Apigee-Entra-Identity`
- `X-Apigee-Entra-Signature`

This protects LiteLLM, direct providers, and registered services from receiving
client bearer tokens, Relayna virtual keys, or Apigee proof material.

## Failure Modes

| Error code | Cause |
| --- | --- |
| `untrusted_apigee_identity` | Missing identity header, missing signature header, bad HMAC, malformed identity JSON, malformed base64url, or empty Apigee secret. |
| `missing_entra_authorization` | Trusted-header mode is enabled but no Apigee proof headers are present and direct Entra JWT validation is not configured. |
| `malformed_entra_authorization` | Direct JWT fallback is enabled but `Authorization` is not `Bearer <token>`. |
| `invalid_entra_token` | Direct JWT fallback found an invalid token, unsupported algorithm, unknown `kid`, bad signature, or invalid timing claim. |
| `insufficient_entra_authorization` | Direct JWT fallback lacks required scope, role, or group, or token has group overage. |
| `missing_authorization` | Enterprise identity passed, but the configured Relayna key header is missing. |
| Existing virtual-key errors | Enterprise identity passed, but the Relayna key is invalid, disabled, revoked, or expired. |

## Example Request Through Apigee

Trusted-header mode request received by Gateway:

```bash
curl -sS http://relayna-gateway-proxy/v1/chat/completions \
  -H "X-Apigee-Entra-Identity: $APIGEE_IDENTITY_HEADER" \
  -H "X-Apigee-Entra-Signature: $APIGEE_SIGNATURE_HEADER" \
  -H "X-Relayna-Key: $RELAYNA_VIRTUAL_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Classify this ticket"}]
  }'
```

JWT revalidation mode request received by Gateway:

```bash
curl -sS http://relayna-gateway-proxy/services/review-service/v1/analyze \
  -H "Authorization: Bearer $ENTRA_ACCESS_TOKEN" \
  -H "X-Relayna-Key: $RELAYNA_VIRTUAL_KEY" \
  -H "Content-Type: application/json" \
  -d '{"text":"review this"}'
```

## Rollout Checklist

1. Start with direct Entra JWT revalidation if Apigee can forward the JWT.
2. Confirm Relayna key policy and usage attribution match virtual-key-only
   traffic.
3. If JWT forwarding is not allowed, enable trusted signed-header mode in
   staging with a new HMAC secret.
4. Configure Apigee to strip inbound proof headers before adding its own.
5. Send a valid signed identity and confirm Gateway accepts the request.
6. Tamper with one byte of the identity header and confirm
   `untrusted_apigee_identity`.
7. Send an unsigned identity header and confirm rejection.
8. Confirm upstream services never receive `Authorization`, `X-Relayna-Key`,
   `X-AIH-API-Key`, `X-Apigee-Entra-Identity`, or
   `X-Apigee-Entra-Signature`.
9. Rotate the HMAC secret through your secret manager. During rotation, deploy
   the new secret to Gateway and Apigee in a coordinated maintenance window
   because Gateway currently accepts one trusted-header secret.

## Verification

Focused local checks:

```bash
cargo test -p gateway-core verifies_apigee_trusted_identity_signature --all-features
cargo test -p gateway-proxy relayna_key_header_is_available_for_apigee_only_mode --all-features
```

Full Gateway verification:

```bash
node tests/freeze-v0.1.7-perimeter.test.mjs
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Real-environment harness:

```bash
internal/test-reports/entra-front-door-real-env/run.sh
```

The harness starts local Postgres, Redis, Gateway, mock OIDC/JWKS authority,
mock Apigee request paths, and mock upstream services. It verifies direct JWT
validation, Apigee JWT revalidation, trusted signed-header mode, signature
tampering rejection, configured Relayna key headers, multiple proxy route
families, credential stripping, and usage attribution.
