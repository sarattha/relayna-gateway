# Operations

## Configuration

Required environment variables:

| Name | Purpose |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection string for durable gateway state. |
| `REDIS_URL` | Redis connection string for readiness, rate limits, and budgets. |
| `LITELLM_BASE_URL` | LiteLLM or OpenAI-compatible upstream base URL. |
| `LITELLM_SERVICE_KEY` | Internal upstream credential used by the gateway. |
| `GATEWAY_BIND_ADDR` | Proxy listener, for example `0.0.0.0:8080`. |
| `GATEWAY_CONTROL_BIND_ADDR` | Control listener, for example `0.0.0.0:8081`. |
| `LOG_LEVEL` | Rust tracing filter. |

Optional variables:

| Name | Purpose |
| --- | --- |
| `DIRECT_OPENAI_BASE_URL` | Optional direct provider base URL. |
| `DIRECT_OPENAI_SERVICE_KEY` | Optional direct provider credential. |
| `RELAYNA_WORKER_TOKEN` | Optional shared token for Relayna worker integration. |
| `RELAYNA_STUDIO_BASE_URL` | Optional Relayna Studio backend base URL for Admin portal service import. |
| `RELAYNA_STUDIO_TOKEN` | Optional bearer token used when Gateway fetches the Studio service catalog. |

`RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN` are fallback settings.
Operators can set, replace, test, or clear the Studio connection in Admin portal
Settings after startup. Admin-saved settings override environment settings until
the persisted base URL is cleared. The base URL must point to the Studio
backend. For local development this is commonly `http://127.0.0.1:8000`; for
Docker Desktop from a container to a host backend use
`http://host.docker.internal:8000`; for Kubernetes use the backend Service DNS
name such as `http://relayna-studio-backend:8000`.

Validate the connection in two steps:

```bash
curl -sS "$RELAYNA_STUDIO_BASE_URL/studio/gateway/services"
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -X POST \
  http://127.0.0.1:8081/admin-ui/admin/studio/connection/test
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/studio/services
```

The first command proves Studio exports services. The test route proves Gateway
can reach the effective configured connection. The services route proves Gateway
can fetch and map the export.

## Health and Metrics

- `/admin-ui/healthz` confirms the process can serve the control API.
- `/admin-ui/readyz` checks PostgreSQL and Redis.
- `/admin-ui/metrics` exposes Prometheus text format.

Use readiness probes for traffic routing and liveness probes for process restart decisions. Do not use `/admin-ui/healthz` as a dependency readiness signal.

Current operator workflows add provider health state, circuit breaker
status, debug bundles, service import versions, trace-aware usage analytics,
and audit-event review to the control plane. See
[Current Feature Highlights](current-features.md) for a feature-oriented
overview with Admin UI screenshots.

Prometheus metrics are intentionally low-cardinality. Metric labels are bounded
to route, provider, status class, decision kind, denial reason, circuit state,
guardrail name, guardrail mode, guardrail action, failure policy, and stream
mode. Do not add request IDs, trace IDs, raw virtual keys, prompt text, raw
paths, or unbounded model/user values as metric labels.

Core metric names:

| Metric | Type | Labels |
| --- | --- | --- |
| `gateway_requests_total` | counter | none |
| `gateway_requests_by_dimension_total` | counter | `route`, `provider`, `status_class` |
| `gateway_request_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_upstream_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_guardrail_duration_ms` | histogram | `route`, `provider`, `stream` |
| `gateway_first_token_latency_ms` | histogram-compatible counter and buckets | `route`, `provider`, `stream` on buckets |
| `gateway_auth_failures_total` | counter | none |
| `gateway_denials_total` | counter | `kind`, `route`, `reason` |
| `gateway_rate_limit_rejections_total` | counter | none |
| `gateway_budget_rejections_total` | counter | none |
| `gateway_provider_fallbacks_by_provider_total` | counter | `from_provider`, `to_provider`, `reason` |
| `gateway_active_requests` | gauge | none |
| `gateway_active_streams` | gauge | none |
| `gateway_circuit_breaker_state` | gauge | `provider`, `name`, `state` |

Example Prometheus scrape configuration:

```yaml
scrape_configs:
  - job_name: relayna-gateway
    metrics_path: /admin-ui/metrics
    static_configs:
      - targets: ["relayna-gateway-control:8081"]
```

Grafana panels should prefer request rate, p95 request/upstream duration,
first-token latency, denials by kind, guardrail block count, fallback rate,
active streams, and circuit state. Use `route` and `provider` filters only from
the bounded label sets emitted by the gateway.

## Tracing

Gateway preserves valid W3C `traceparent` headers on upstream provider and
service calls. When `traceparent` is present, the 32-character trace ID is stored
on usage events and request debug bundles so operators can move from Studio
analytics to provider traces or gateway logs without exposing raw keys or
prompts.

JSON logs include tracing span fields from gateway request, auth verification,
policy evaluation, guardrail, rate-limit, budget, upstream, and usage recording
points. Configure `LOG_LEVEL` with standard Rust tracing filters, for example:

```bash
LOG_LEVEL=info,gateway_proxy=debug,gateway_api=info
```

If logs are shipped to an OpenTelemetry collector through the deployment
platform, map the `otel.trace_id` field and `traceparent` header to the same
trace context. The gateway does not use request IDs or trace IDs as Prometheus
labels.

## Budgets and Rate Limits

Virtual key policies can set request-per-minute (`rpm_limit`),
token-per-minute (`tpm_limit`), daily budget, and monthly budget limits. Request
and token rate limits are Redis minute counters. Budget checks use Redis daily
and monthly counters for fast enforcement, while PostgreSQL usage events remain
the durable accounting ledger.

On startup, Gateway waits for Redis readiness before rehydrating current daily
and monthly budget counters for keys with configured budgets. It also runs
periodic reconciliation so a Redis restart or flush can recover budget spend
from PostgreSQL usage events without manual counter repair. In-flight
reservation keys are short-lived control state and are not reconstructed.

Requests that exceed `tpm_limit` return the stable
`token_rate_limit_exceeded` error. When Redis exposes the active bucket TTL,
Gateway includes retry timing in the response.

## Secret Handling

- Store `DATABASE_URL`, `REDIS_URL`, provider credentials, LiteLLM credentials, Studio tokens, and operator tokens in a secret manager.
- Never log raw virtual keys, operator tokens, provider keys, prompts, or request bodies.
- Use `GATEWAY_ADMIN_TOKEN` only to seed a fresh database. After an active
  operator token exists, env changes are ignored; rotate the token from the
  Admin portal to change it.
- Assign the narrowest operator scopes practical for automation. Use
  `audit:read` for audit-only readers, `usage:read` and `usage:export` for
  analytics workflows, and mutation scopes such as `keys:create`,
  `keys:disable`, `providers:update`, or `services:update` only where needed.
- Review `/admin-ui/admin/audit-events` after key, policy, guardrail, provider,
  service, Studio settings, or operator token changes. Audit rows include
  request ID, actor token ID, action, target, IP, user agent, and redacted
  before/after snapshots.
- Prefer private control-plane access for `/admin-ui/admin/*`, `/admin-ui`, and `/admin-ui/metrics`.
- Configure `RELAYNA_WORKER_TOKEN` only through secret management. Worker token
  verification uses constant-time comparison, and the gateway strips
  `x-relayna-worker-token` before forwarding upstream.
- Treat non-expiring virtual keys as high-risk service credentials. Store them
  only in a secret manager, scope their policies narrowly, rotate them through
  an external process, and revoke or disable them immediately when ownership or
  deployment context changes.

## Backup and Retention

Back up PostgreSQL because it contains virtual key metadata, policies, usage events, service registry state, and operator token hashes. Redis can be treated as volatile for rate-limit and budget counters unless your operating model requires counter persistence across restarts. Budget counters for configured budgets are rehydrated from PostgreSQL, but request-per-minute, token-per-minute, and in-flight reservation keys remain transient.

## Upgrade Notes

Before deploying a new release:

1. Read `CHANGELOG.md`.
2. Build and scan the Docker image.
3. Run CI, including Rust checks, security scans, admin UI tests, freeze
   perimeter tests, and docs build.
4. Confirm PostgreSQL migrations apply in a staging database.
5. Confirm release metadata validation passes for the intended tag, for example `python3 scripts/validate-release-metadata.py v0.1.0`.
6. Roll out one gateway replica and check `/admin-ui/readyz`, `/admin-ui/metrics`, proxy traffic, route toggles, service routes, and the admin portal before scaling out.

## Supply Chain and Runtime Hardening

CI runs dependency, secret, static-analysis, filesystem, and image security
checks. Treat failures as blocking unless a temporary exception is documented in
`docs/security-exceptions.md`.

Release images are published to GHCR with SBOM, signature, and provenance
artifacts. Verify signatures and attestations before promotion into production
clusters.

Run production pods with the restricted settings from
`deploy/kubernetes/relayna-gateway.yaml`: non-root UID/GID `10001`, read-only
root filesystem, default seccomp profile, no privilege escalation, and no Linux
capabilities. Keep proxy and control-plane Services separate, and expose the
control plane only through private ingress, VPN, identity-aware proxy, or
equivalent access control.
