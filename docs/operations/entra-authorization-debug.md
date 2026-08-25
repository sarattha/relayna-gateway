# Entra Authorization Debug Mode

`ENTRA_AUTH_DEBUG=true` enables a structured decision trail for every Relayna
Gateway authentication surface that uses Microsoft Entra identity. It is an
incident diagnostic, not a normal logging level. The default is `false`.

Use it to answer four questions from one request ID:

1. Did Gateway receive the expected credential or browser cookie?
2. Which discovery, key, signature, claim, role, membership, or session check
   ran?
3. What was the last accepted phase and the first rejected phase?
4. If portal login succeeded at Entra, did Gateway persist the server session,
   emit the browser cookies, and receive them on the next request?

The mode covers direct request-plane access tokens, trusted Apigee identity,
portal OIDC login and logout, portal session/CSRF checks, and managed-identity
owner APIs for both services and projects.

## Enable and collect

Set the environment variable and restart or roll the Gateway process:

```bash
ENTRA_AUTH_DEBUG=true
```

The dedicated diagnostic target is enabled at warning level independently of
`LOG_LEVEL`. A restrictive ordinary tracing filter therefore does not hide the
authorization trail. A startup event named `relayna.authorization_debug`
confirms the mode is active.

For the checked-in local stack:

```bash
ENTRA_AUTH_DEBUG=true docker compose \
  -f deploy/local/docker-compose.yml up -d --build

docker compose -f deploy/local/docker-compose.yml logs gateway \
  | rg 'relayna.authorization_debug'
```

For Kubernetes, temporarily change `ENTRA_AUTH_DEBUG` in
`relayna-gateway-config` to `"true"`, roll the Deployment, reproduce one
request, export the relevant request IDs, change it back to `"false"`, and roll
again. Do not leave it enabled after the investigation.

## Event format

Each event is emitted by the `relayna_authorization_debug` tracing target. In
JSON logs, the `authorization_debug` field is itself a JSON object:

```json
{
  "event": "relayna.authorization_debug",
  "surface": "portal_browser",
  "phase": "session_cookie",
  "outcome": "rejected",
  "reason": "login_cookie_not_returned",
  "request_id": "01J...",
  "details": {
    "configured_secure": true,
    "external_scheme": "http",
    "known_configuration_warning": "secure_cookie_over_http",
    "server_can_observe_browser_acceptance": false
  }
}
```

The stable envelope fields are:

| Field | Meaning |
| --- | --- |
| `surface` | Authorization entry point, such as `request_plane`, `portal_browser`, `owner_service`, or `owner_project`. |
| `phase` | The check that just completed, such as `jwt_header`, `jwt_signature`, `member_authorization`, or `cookie_emission`. |
| `outcome` | `started`, `accepted`, or `rejected`. |
| `reason` | Machine-searchable explanation for the result. This is more precise than the public HTTP error code. |
| `request_id` | Gateway request correlation ID when an HTTP request exists. Startup/cache events can omit it. |
| `details` | Safe diagnostic context for that phase. The complete serialized details object is bounded to 64 KiB. |

Public HTTP statuses and error shapes do not change. Debug reasons are an
operator aid and are not an external API contract.

## Token validation trail

For direct Entra and owner managed-identity tokens, Gateway reports these
stages in order:

| Phase | What the log can show |
| --- | --- |
| `authorization_header` | Whether the header was missing, used the wrong scheme, was empty, or was not valid header text. It never logs the header value. |
| `jwt_header` | Decoded JOSE header, segment count, `kid`, and `alg`; failures include missing `kid`, invalid base64/JSON, and disallowed algorithms. |
| `oidc_discovery` | Cache use, network category, HTTP status, JSON parsing, discovered issuer match, and JWKS URI availability. |
| `jwks_refresh` / `jwks_cache` | Refresh reason, network/status/JSON result, key count, and cache-write result. |
| `jwks_key_selection` | Whether `kid` was found after refresh and whether the key is RSA with a matching algorithm and usable modulus/exponent. |
| `jwt_claims` | Decoded claims from the JWT payload before signature trust is established. OAuth transaction claims (`nonce`, `at_hash`, `c_hash`, and `s_hash`) retain their field names but have redacted values. The event marks this identity data as `unverified`. |
| `jwt_signature` | Signature/schema validation result. After acceptance, subsequent identity data is marked `signature_verified`. |
| `issuer`, `tenant`, `audience`, `timestamps`, `token_version` | Configured expectation and the claim that accepted or failed. Timestamp checks distinguish expired, not-yet-valid, and issued-in-the-future cases. |
| `scope`, `role`, `groups` | Required values and whether `scp`, `roles`, group overage, or the group allowlist caused rejection. |
| `normalized_identity` | The verified identity Relayna will use: tenant, subject, object/application IDs, scopes, roles, groups, token version, and identity source. |

Typical rejection reasons include `kid_missing`, `algorithm_not_allowed`,
`discovery_issuer_mismatch`, `kid_not_found_after_refresh`,
`jwt_verification_failed`, `issuer_mismatch`, `tenant_mismatch`,
`audience_mismatch`, `token_expired`, `issued_at_in_future`,
`required_scope_missing`, `required_role_missing`,
`group_overage_not_supported`, and `allowed_group_missing`.

The mode intentionally shows the decoded claims object because that
is often required to diagnose Entra application-role, optional-claim, token
version, and managed-identity differences. OAuth transaction claim values are
redacted even in this mode. Other claims can contain names, email
addresses, object IDs, tenant IDs, and group IDs. The payload is not trusted
until the later signature event accepts it.

## Portal OIDC and cookie-session trail

A browser session has three independent server-visible stages:

```text
OIDC transaction stored -> server session stored -> Set-Cookie emitted
                                      |
                                      v
                          browser returns both cookies
```

Debug mode records each boundary:

| Phase | Accepted result | Important rejection reasons |
| --- | --- | --- |
| `login_initialization` | Portal OIDC runtime is available. | `portal_oidc_disabled` |
| `authorization_request` | Discovery succeeded and an authorization URL was created. | Discovery network/status/JSON/issuer failures or invalid authorization endpoint. |
| `login_transaction` | State, nonce hash, PKCE verifier, browser binding hash, expiry, and safe return path were stored. Values and hashes are not logged. | `login_transaction_store_failed` |
| `login_cookie` | Gateway created and emitted the short-lived `relayna_portal_login` header. | `login_cookie_header_invalid` |
| `callback` | Provider returned a code/state and no provider error. | `identity_provider_returned_error`, `authorization_code_or_state_missing` |
| `callback_cookie_observation` | The browser returned the login-binding cookie. | `login_cookie_not_returned` |
| `login_transaction` | State and browser binding matched an unexpired stored transaction. | `transaction_missing_expired_used_or_binding_mismatch` or a transaction-store failure. |
| `client_assertion` | A PS256 private-key JWT assertion was created for the token endpoint. | Local certificate/key/configuration failures. |
| `token_endpoint` | Entra accepted the code exchange and returned an ID token. | Network category, HTTP status, safe Entra error fields, invalid JSON, or missing ID token. |
| `nonce_validation` | Verified ID-token nonce matches the stored transaction. | `nonce_mismatch` |
| `member_resolution` / `member_authorization` | Member was upserted and is active or matches the bootstrap admin policy. | Store failure, blocked member, or pending member. |
| `session_persistence` | The opaque server session hash and CSRF hash were persisted. | `session_database_create_failed` |
| `cookie_emission` | Session and CSRF `Set-Cookie` headers were created. | `session_or_csrf_cookie_header_invalid`; the event says whether the database session already exists. |
| `session_cookie` | A later request returned both opaque cookies. | `session_cookie_not_returned`, `csrf_cookie_not_returned`, or stale mixed cookie state. |
| `session_resolution` / `cookie_observation` | The server session exists and is unexpired. | `session_not_found_or_expired` or `session_store_unavailable`. |
| `csrf_validation` | Required CSRF header exists and matches the session-bound cookie/hash. | Missing header, missing cookie, cookie/header mismatch, or stored hash mismatch. |
| `membership_resolution` | Current persisted member and memberships were loaded. | Blocked/pending member or store failure. |

Cookie events show only lengths, attribute choices (`HttpOnly`, `SameSite`,
`Path`, `Max-Age`, `Secure`), header count, and presence on a later request.
They never show a cookie value or its database hash.

When an emitted cookie is not returned, Gateway can infer common
misconfiguration but cannot inspect a browser's cookie jar. Diagnostic details
therefore include:

- configured `Secure` behavior;
- externally observed scheme from `X-Forwarded-Proto` when present;
- whether the request host matches the registered redirect host; and
- `secure_cookie_over_http` or
  `callback_host_differs_from_registered_redirect_host` when one is evident.

The field `server_can_observe_browser_acceptance` is always `false`: an emitted
header proves only that Gateway sent it. Browser privacy policy, proxy header
rewrites, domain/path rules, and local cookie settings remain client-side or
intermediary evidence.

Logout logs server-session deletion, provider end-session URL discovery, and
the three cookie-clear headers emitted to the browser.

## Trusted Apigee and Relayna key trail

For trusted Apigee mode, the log records whether both trusted headers exist,
whether HMAC verification passed, and then the decoded identity and role/group
decision. It does not log either raw trusted header or the signature.

After Entra or Apigee identity succeeds on the request plane, a separate event
reports Relayna virtual-key presence and validation. Only the already-safe key
prefix is included; raw virtual keys are never logged. Owner API events report
whether an exact enabled service/project binding was resolved for the verified
identity.

## Data that is never logged

Debug mode does **not** log:

- compact access tokens, ID tokens, or client assertions;
- JWT signature bytes;
- authorization codes, PKCE verifiers, state, nonce, or their stored hashes;
- session, login-binding, or CSRF cookie values or hashes;
- raw Relayna virtual keys, operator tokens, provider credentials, or private
  certificate material;
- trusted Apigee header/signature values;
- prompts, request bodies, or response bodies; or
- Entra `error_description`, which can echo supplied values. Only `error`,
  `suberror`, `error_codes`, `timestamp`, `trace_id`, and `correlation_id` are
  allowlisted from an Entra token endpoint error.

## Investigation workflow

1. Restrict log access and shorten retention before enabling the mode.
2. Enable it on the smallest practical replica set and confirm the startup
   event.
3. Reproduce one request or login and save its `request_id`. For browser login,
   follow the redirect requests because each HTTP hop has its own request ID.
4. Filter by `surface`, then read accepted phases until the first rejection.
5. Correlate Entra `trace_id`/`correlation_id` with Entra sign-in logs when the
   token endpoint rejected the exchange.
6. For a missing cookie, compare the emission event with the first callback or
   session observation event, then inspect the browser and reverse proxy.
7. Disable the mode, roll the process, and confirm no new
   `relayna.authorization_debug` events appear.
8. Delete or age out the incident logs according to the approved diagnostic
   retention policy.

Because decoded claims can be personal and security-sensitive data, do not
send these logs to ordinary support channels, long-lived analytics indexes, or
public issue trackers. Redact claim values before attaching a minimal excerpt
to an incident or pull request.
