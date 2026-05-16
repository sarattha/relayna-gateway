# Relayna Gateway

Relayna Gateway is the Rust proxy and control plane for Relayna AI traffic. It validates Relayna virtual keys, enforces route and model policy, forwards approved OpenAI-compatible requests to LiteLLM or direct providers, records usage, and exposes an embedded operator admin portal.

Relayna remains the task execution runtime. Relayna Gateway is the public governance, routing, metering, and operator surface in front of provider access.

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

The first startup runs database migrations and prints one bootstrap operator token. Store that token securely; only its hash is persisted.

Useful endpoints:

- Proxy traffic: `http://127.0.0.1:8080/v1/chat/completions`
- Health: `http://127.0.0.1:8081/healthz`
- Readiness: `http://127.0.0.1:8081/readyz`
- Metrics: `http://127.0.0.1:8081/metrics`
- Admin portal: `http://127.0.0.1:8081/admin-ui`
- Guardrail catalog: `http://127.0.0.1:8081/admin/guardrails`
- Studio connection status: `http://127.0.0.1:8081/admin/studio/connection`
- Studio import preview: `http://127.0.0.1:8081/admin/studio/services`

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
docker build -t relayna-gateway:0.0.9 .
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
  relayna-gateway:0.0.9
```

## Kubernetes

Start from `deploy/kubernetes/relayna-gateway.yaml`, which defaults to the GitHub Container Registry image `ghcr.io/sarattha/relayna-gateway:0.0.9`, and provide `relayna-gateway-secrets` through your cluster secret manager. Keep the control port private unless it is protected by an internal ingress, VPN, or identity-aware proxy.

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
- `docs/operations.md`
- `CHANGELOG.md`
