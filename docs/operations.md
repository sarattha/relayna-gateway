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

## Health and Metrics

- `/healthz` confirms the process can serve the control API.
- `/readyz` checks PostgreSQL and Redis.
- `/metrics` exposes Prometheus text format.

Use readiness probes for traffic routing and liveness probes for process restart decisions. Do not use `/healthz` as a dependency readiness signal.

## Secret Handling

- Store `DATABASE_URL`, `REDIS_URL`, provider credentials, LiteLLM credentials, and operator tokens in a secret manager.
- Never log raw virtual keys, operator tokens, provider keys, prompts, or request bodies.
- Rotate the bootstrap operator token after first startup.
- Prefer private control-plane access for `/admin/*`, `/admin-ui`, and `/metrics`.

## Backup and Retention

Back up PostgreSQL because it contains virtual key metadata, policies, usage events, service registry state, and operator token hashes. Redis can be treated as volatile for rate-limit and budget counters unless your operating model requires counter persistence across restarts.

## Upgrade Notes

Before deploying a new release:

1. Read `CHANGELOG.md`.
2. Build and scan the Docker image.
3. Run CI, including Rust checks, admin UI tests, and docs build.
4. Confirm PostgreSQL migrations apply in a staging database.
5. Roll out one gateway replica and check `/readyz`, `/metrics`, proxy traffic, and the admin portal before scaling out.
