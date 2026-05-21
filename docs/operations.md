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

## Secret Handling

- Store `DATABASE_URL`, `REDIS_URL`, provider credentials, LiteLLM credentials, Studio tokens, and operator tokens in a secret manager.
- Never log raw virtual keys, operator tokens, provider keys, prompts, or request bodies.
- Use `GATEWAY_ADMIN_TOKEN` only to seed a fresh database. After an active
  operator token exists, env changes are ignored; rotate the token from the
  Admin portal to change it.
- Prefer private control-plane access for `/admin-ui/admin/*`, `/admin-ui`, and `/admin-ui/metrics`.
- Treat non-expiring virtual keys as high-risk service credentials. Store them
  only in a secret manager, scope their policies narrowly, rotate them through
  an external process, and revoke or disable them immediately when ownership or
  deployment context changes.

## Backup and Retention

Back up PostgreSQL because it contains virtual key metadata, policies, usage events, service registry state, and operator token hashes. Redis can be treated as volatile for rate-limit and budget counters unless your operating model requires counter persistence across restarts.

## Upgrade Notes

Before deploying a new release:

1. Read `CHANGELOG.md`.
2. Build and scan the Docker image.
3. Run CI, including Rust checks, admin UI tests, and docs build.
4. Confirm PostgreSQL migrations apply in a staging database.
5. Confirm release metadata validation passes for the intended tag, for example `python3 scripts/validate-release-metadata.py v0.0.11`.
6. Roll out one gateway replica and check `/admin-ui/readyz`, `/admin-ui/metrics`, proxy traffic, route toggles, service routes, and the admin portal before scaling out.
