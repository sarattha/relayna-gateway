# Relayna Gateway

Relayna Gateway is the Rust proxy and control plane for Relayna AI traffic. It validates Relayna virtual keys, enforces route and model policy, forwards approved OpenAI-compatible requests to LiteLLM or direct providers, records usage, and exposes an embedded operator admin portal.

Relayna remains the task execution runtime. Relayna Gateway is the public governance, routing, metering, and operator surface in front of provider access.

Version `0.1.28` is the current release target. Release `0.1.28` adds
read-only project-owner dashboards and APIs beside the existing service-owner
experience. Administrators can grant Entra portal members exact project Owner
or Viewer memberships and register exact project bindings for monitoring
managed identities. The shared confidential Web/API registration exposes
separate `gateway.invoke` and `gateway.monitor.read` application roles for
least-privilege managed identities; both service and project monitoring reuse
`gateway.monitor.read`, while Relayna bindings enforce the exact resource.
Owners can chart error rate and P95 latency, filter request outcomes and exact
status codes, and open sanitized scoped request details with optional debug
bundles. The portal uses certificate-backed PS256
`private_key_jwt`, and first-admin ConfigMap bootstrap matches tenant,
immutable Entra object ID, and email. Existing operator tokens remain available as
break-glass access, and all browser pages remain under `/admin-ui`. It retains
endpoint-level failure monitoring, buffered-body admission, OpenAPI endpoint
billing, the Aurora Teal Admin UI 2.0 shell, policy governance, provider
intelligence, supply-chain hardening, LiteLLM passthrough and credential
mapping, Entra front-door authorization, and Apigee provider-traffic support.
See `docs/operations/entra-portal-and-owner-monitoring.md`,
`docs/operations/entra-integration-requirements.md`,
`docs/openapi-service-pricing.md`, `docs/current-features.md`,
`docs/litellm-passthrough.md`, `docs/entra-id-auth.md`, and
`docs/apigee-gateway-path.md` for the public feature highlights.

## What This Repository Contains

- `crates/gateway-api`: Axum control API, health, readiness, metrics, admin APIs, embedded admin UI, and process startup.
- `crates/gateway-core`: Authentication, policy, routing, services, rate limits, budgets, usage, operator tokens, and shared error types.
- `crates/gateway-proxy`: Pingora proxy service for OpenAI-compatible traffic, upstream credential handling, streaming paths, and usage recording.
- `crates/gateway-store`: PostgreSQL migrations, SQLx queries, Redis readiness, rate-limit, and budget state.
- `crates/gateway-telemetry`: tracing and Prometheus output.
- `deploy/kubernetes`: baseline Kubernetes manifests.
- `docs`: Material for MkDocs documentation.

## Runtime Services

Relayna Gateway requires:

- PostgreSQL for virtual keys, route policies, usage events, service registrations, and operator token hashes.
- Redis for readiness, rate-limit counters, and budget counters.
- LiteLLM or another OpenAI-compatible upstream.

Secrets such as provider keys, LiteLLM service keys, operator tokens, and raw Relayna virtual keys must stay out of source control and logs.

## Local Development

Set the required environment variables:

```bash
export DATABASE_URL="postgres://relayna_gateway@localhost:5432/relayna_gateway"
export REDIS_URL="redis://127.0.0.1:6379"
export LITELLM_BASE_URL="http://127.0.0.1:4000"
export LITELLM_SERVICE_KEY="sk-litellm-service-key"
# Optional first-start admin bootstrap token. Must start with op_live_.
# Ignored after an active operator token exists in PostgreSQL.
# export GATEWAY_ADMIN_TOKEN="op_live_replace_with_secret_value"
export RELAYNA_STUDIO_BASE_URL="http://127.0.0.1:8000"
# Optional when Studio protects the Gateway export endpoint:
# export RELAYNA_STUDIO_TOKEN="studio-gateway-token"
# Optional guardrail PII mapping controls:
# export GUARDRAIL_PII_MAPPING_TTL_SECONDS="3600"
# export GUARDRAIL_MAPPING_ENCRYPTION_KEY="<base64-32-byte-key>"
export GATEWAY_BIND_ADDR="127.0.0.1:8080"
export GATEWAY_CONTROL_BIND_ADDR="127.0.0.1:8081"
# Optional process-wide managed-body admission limits:
# export GATEWAY_MAX_BUFFERED_REQUESTS="8"
# export GATEWAY_MAX_INFLIGHT_BUFFER_BYTES="536870912"
export LOG_LEVEL="gateway_api=info,gateway_proxy=info"
# Optional Entra front-door auth. Disabled by default.
# export ENTRA_AUTH_ENABLED="true"
# export ENTRA_APPLICATION_ID="<single-application-id-guid>"
# export ENTRA_TENANT_ID="<tenant-guid>"
# export ENTRA_ISSUER="https://login.microsoftonline.com/<tenant-guid>/v2.0"
# export ENTRA_OIDC_DISCOVERY_URL="https://login.microsoftonline.com/<tenant-guid>/v2.0/.well-known/openid-configuration"
# export ENTRA_REQUIRED_ROLE="gateway.invoke"
# export ENTRA_RELAYNA_KEY_HEADER="X-Relayna-Key"
# Optional Apigee trusted signed-header mode. Disabled by default.
# export APIGEE_TRUSTED_HEADER_ENABLED="true"
# export APIGEE_TRUSTED_HEADER_SECRET="<shared-hmac-secret>"
```

Run the gateway:

```bash
cargo run -p gateway-api
```

The first startup runs database migrations and creates one bootstrap operator token. If `GATEWAY_ADMIN_TOKEN` is set, Gateway stores that token hash in a fresh database and does not print the raw token. If it is not set, Gateway generates and prints one raw operator token. After an active operator token exists, later `GATEWAY_ADMIN_TOKEN` changes are ignored; rotate the token from the Admin portal to change it.

Useful endpoints:

- Proxy traffic: `http://127.0.0.1:8080/v1/chat/completions`
- Health: `http://127.0.0.1:8081/admin-ui/healthz`
- Readiness: `http://127.0.0.1:8081/admin-ui/readyz`
- Metrics: `http://127.0.0.1:8081/admin-ui/metrics`
- Admin portal: `http://127.0.0.1:8081/admin-ui`
- Guardrail catalog: `http://127.0.0.1:8081/admin-ui/admin/guardrails`
- Studio connection status: `http://127.0.0.1:8081/admin-ui/admin/studio/connection`
- Studio import preview: `http://127.0.0.1:8081/admin-ui/admin/studio/services`
- Usage export JSON: `http://127.0.0.1:8081/admin-ui/admin/usage/export.json`
- Usage export CSV: `http://127.0.0.1:8081/admin-ui/admin/usage/export.csv`

Post-freeze admin endpoints also include scoped audit events, policy
simulation, policy layers, provider health state, debug bundles, service import
preview/activation/version/rollback, and paginated expanded usage analytics. These are
documented in `docs/current-features.md`.

Release `0.1.28` can run Relayna Gateway as the single ingress in front of
LiteLLM. Canonical OpenAI-compatible and Anthropic-compatible Claude routes
remain governed by Relayna policy by default, and operators can optionally
switch each canonical route to direct LiteLLM passthrough while preserving
route enablement, policy, rate-limit, and budget checks. Wildcard LiteLLM
passthrough is disabled by default; when enabled, configure allowed paths and
methods from Admin portal Providers.

Gateway client authentication is Relayna-owned for governed traffic. When Entra
is disabled, clients send `Authorization: Bearer rk_live_...`. When Entra is
enabled, clients send the Entra access token in `Authorization: Bearer <jwt>`
and the Relayna virtual key in the configured Relayna key header, which defaults
to `X-Relayna-Key`. Gateway strips those client credentials before forwarding
and injects the resolved internal LiteLLM credential by using the configured
LiteLLM header mode/name and custom-header value format.

For canonical routes set to `direct_litellm_passthrough`, non-Relayna
`Authorization: Bearer ...` credentials can be delegated directly to LiteLLM
and translated to the configured upstream header. Relayna `rk_live_...` bearer
keys keep the Relayna-authenticated path with policy, mapping lookup, rate
limits, budgets, credential stripping, and status-only usage.

Sensitive LiteLLM `/ui` and admin-like paths remain blocked unless explicitly
configured. For browser access, choose either operator-token proxy flow
(`/admin-ui/litellm-ui/...`) or trusted-ingress `trusted_ingress` mode for
browser-safe `/ui` support. The former requires a request with operator
`Authorization`; the latter allows trusted front-door contexts while keeping
non-UI passthrough on normal Relayna auth unless the admin API exposure is
intentionally set to `explicitly_exposed` and path/method allowlists match.
See `docs/litellm-passthrough.md`,
`docs/current-features.md`, `docs/operations.md`, `docs/entra-id-auth.md`, and
`docs/apigee-gateway-path.md`.

Operators can configure Entra ID and Apigee front-door auth from Admin portal
Settings, including enablement, tenant, audience, issuer, OIDC discovery,
scope, role, groups, accepted algorithms, JWKS cache TTL, clock skew, Relayna
key header, and the write-only Apigee secret.

`RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN` are startup fallback
settings. Operators can also open Admin portal Settings after Gateway starts to
set, replace, test, or clear the Studio backend connection. Admin-saved settings
override the env fallback until the persisted base URL is cleared.

When Relayna Studio is running, verify the export path before importing:

```bash
curl http://127.0.0.1:8000/studio/gateway/services
```

## OpenAPI Service Pricing

Registered services can preview and explicitly sync OpenAPI 3.x JSON from a
same-origin relative source such as `/openapi.json`. The durable endpoint
catalog supports `none`, `fixed`, and `passthrough` billing per method/path;
Relayna operational endpoints default to `none`, and operators may override
them. Billable endpoints still compose with JSON and multipart body selectors,
so an OCR `/engine = docint` rule can charge `$0.50` without charging feed,
status, event, or DLQ traffic.

Discovery is authenticated, audited, redirect-free, credential-free, bounded,
and never runs on the client request path. See
[`docs/openapi-service-pricing.md`](docs/openapi-service-pricing.md) for the UI
and API workflow, budget behavior, security requirements, and troubleshooting.

Authenticated registered-service requests also use the synced catalog for
usage observability. The effective monitoring endpoint is the matching template
or the query-free concrete path when no operation matches. Every routed status
of 400 or greater remains a failure, whether returned by Gateway or upstream.

## Docker

Build the single image that runs both the gateway proxy and embedded admin portal:

```bash
docker build -t relayna-gateway:0.1.28 .
```

Run it:

```bash
docker run --rm \
  -p 8080:8080 \
  -p 8081:8081 \
  -e DATABASE_URL="postgres://user:password@host.docker.internal:5432/relayna_gateway" \
  -e REDIS_URL="redis://host.docker.internal:6379" \
  -e LITELLM_BASE_URL="http://host.docker.internal:4000" \
  -e LITELLM_SERVICE_KEY="sk-litellm-service-key" \
  -e GATEWAY_ADMIN_TOKEN="op_live_replace_with_secret_value" \
  relayna-gateway:0.1.28
```

`GATEWAY_ADMIN_TOKEN` is optional and only seeds a fresh database. Omit it to
let Gateway generate and print a first-start operator token. Once PostgreSQL has
an active operator token, changing this env var has no effect; rotate from the
Admin portal instead.

## Kubernetes

Start from `deploy/kubernetes/relayna-gateway.yaml`, which defaults to the GitHub Container Registry image `ghcr.io/sarattha/relayna-gateway:0.1.28`, and provide `relayna-gateway-secrets` through your cluster secret manager. Set `GATEWAY_ADMIN_TOKEN` only before first startup when you want to seed a fresh database with a known operator token. Keep the control port private unless it is protected by an internal ingress, VPN, or identity-aware proxy.

## Budgets, TPM, and Usage Exports

Virtual key policies can enforce request-per-minute, token-per-minute, daily
budget, and monthly budget limits. Redis stores the fast control counters, and
PostgreSQL usage events remain the durable ledger.

The Admin UI Usage view groups the selected top results as Project → Virtual
key → Service. Apply the existing time, project, key, service, provider, route,
status, or cost filters first, then expand a project and safe key prefix to
compare request, failure, token, latency, and estimated-cost totals by service.

Gateway rehydrates current daily and monthly Redis budget counters from
PostgreSQL usage events after Redis readiness succeeds, then keeps reconciling
periodically while the process runs. This lets budget enforcement recover after
Redis loss without treating Redis as the source of truth. Token-per-minute
limits use Redis minute buckets and return `token_rate_limit_exceeded` when the
estimated request token impact exceeds the key policy.

Operators can export usage data from the protected admin endpoints:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  "http://127.0.0.1:8081/admin-ui/admin/usage/export.json?limit=1000"

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  "http://127.0.0.1:8081/admin-ui/admin/usage/export.csv?limit=1000"
```

Exports support the dashboard filters, including `from`, `to`, `project_id`,
`key_id`, `route`, `provider`, `service`, `method`, `endpoint`, `status_code`,
`task_id`, `model`, and `status`. JSON rows and the appended CSV columns expose
`http_method`, `endpoint_path`, and `endpoint_template`. CSV exports escape
cells and neutralize spreadsheet formula prefixes.

## Guardrails

Guardrails are opt-in policy controls for Relayna virtual keys. The catalog
defines global guardrail behavior, and each key decides which guardrails are
mandatory, optional, or forbidden. `pii-redact` is seeded as an enabled built-in
guardrail, but it is not default-on for existing keys.

Operator setup flow:

1. Open Admin portal Guardrails and review the catalog entry for `pii-redact`.
2. Edit the catalog runtime config for global defaults, such as
   `{ "restore_output": true }`.
3. Add custom HTTP guardrails when an external policy service should run before
   or after provider calls.
4. Open Keys and use the guardrail pickers to select mandatory, optional, and
   forbidden guardrails.
5. Configure per-key overrides only after a guardrail is selected as mandatory
   or optional. Override editors are intentionally hidden for unselected
   guardrails.

Effective config is a shallow merge of catalog `runtime_config` plus the
per-key override for the selected guardrail. Overrides must be JSON objects, are
dormant until the guardrail is applied, and are rejected for unknown or
forbidden guardrails.

Example key policy:

```json
{
  "guardrail_policy": {
    "mandatory_guardrails": ["pii-redact"],
    "optional_guardrails": ["custom-check"],
    "forbidden_guardrails": [],
    "guardrail_config_overrides": {
      "pii-redact": {
        "restore_output": false
      },
      "custom-check": {
        "threshold": 0.85
      }
    }
  }
}
```

Custom HTTP guardrail endpoint URL, timeout, and bearer token are catalog
settings. Per-key overrides tune only that guardrail's runtime config, so
secrets are not copied into key policy.

## Checks

Run the release checks before pushing:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node tests/admin-ui.test.mjs
mkdocs build --strict
```

The repository also provides:

```bash
make verify
make docs-check
```

## Documentation

Full documentation is built with Material for MkDocs:

```bash
pip install mkdocs-material
mkdocs serve
```

See:

- `docs/architecture.md`
- `docs/getting-started.md`
- `docs/deployment.md`
- `docs/guardrails.md`
- `docs/current-features.md`
- `docs/provider-intelligence.md`
- `docs/operations.md`
- `CHANGELOG.md`
