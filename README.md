# Relayna Gateway

Relayna Gateway is the Rust proxy and control plane for Relayna AI traffic. It validates Relayna virtual keys, enforces route and model policy, forwards approved OpenAI-compatible requests to LiteLLM or direct providers, records usage, and exposes an embedded operator admin portal.

Relayna remains the task execution runtime. Relayna Gateway is the public governance, routing, metering, and operator surface in front of provider access.

Version `0.1.0` builds on the `v0.0.14` freeze baseline with Admin UI 2.0,
scoped operator governance, policy simulation and inherited layers, provider
intelligence, richer usage analytics, and supply-chain hardening. See
`docs/current-features.md` for the public feature highlights and screenshots.

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
export LOG_LEVEL="gateway_api=info,gateway_proxy=info"
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
preview/activation/version/rollback, and expanded usage analytics. These are
documented in `docs/current-features.md`.

`RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN` are startup fallback
settings. Operators can also open Admin portal Settings after Gateway starts to
set, replace, test, or clear the Studio backend connection. Admin-saved settings
override the env fallback until the persisted base URL is cleared.

When Relayna Studio is running, verify the export path before importing:

```bash
curl http://127.0.0.1:8000/studio/gateway/services
```

## Docker

Build the single image that runs both the gateway proxy and embedded admin portal:

```bash
docker build -t relayna-gateway:0.1.0 .
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
  relayna-gateway:0.1.0
```

`GATEWAY_ADMIN_TOKEN` is optional and only seeds a fresh database. Omit it to
let Gateway generate and print a first-start operator token. Once PostgreSQL has
an active operator token, changing this env var has no effect; rotate from the
Admin portal instead.

## Kubernetes

Start from `deploy/kubernetes/relayna-gateway.yaml`, which defaults to the GitHub Container Registry image `ghcr.io/sarattha/relayna-gateway:0.1.0`, and provide `relayna-gateway-secrets` through your cluster secret manager. Set `GATEWAY_ADMIN_TOKEN` only before first startup when you want to seed a fresh database with a known operator token. Keep the control port private unless it is protected by an internal ingress, VPN, or identity-aware proxy.

## Budgets, TPM, and Usage Exports

Virtual key policies can enforce request-per-minute, token-per-minute, daily
budget, and monthly budget limits. Redis stores the fast control counters, and
PostgreSQL usage events remain the durable ledger.

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
`key_id`, `route`, `provider`, `service`, `task_id`, `model`, and `status`.
CSV exports escape cells and neutralize spreadsheet formula prefixes.

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
