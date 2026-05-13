# Admin Portal

The admin portal is a static operator console embedded in `gateway-api`. It is served from the control listener at `/admin-ui` and calls the same `/admin/*` APIs used by automation.

## Authentication

Use the operator token printed on first startup. The token is stored in browser session storage and sent as:

```http
Authorization: Bearer <operator-token>
```

Rotate the token from the portal after bootstrap or whenever access changes. Rotation returns the new raw token once.

## Views

- Overview shows readiness, request count, active keys, enabled OpenAI routes, enabled services, failures, cost, and provider health.
- Projects creates and lists project UUIDs used to link virtual keys and services.
- Keys creates, edits, disables, enables, revokes, and inspects virtual keys. Use `No expiration` for service keys whose rotation is managed outside Gateway.
- Providers configures LiteLLM and internal-service endpoints with write-only credentials.
- Routes disables and enables the global OpenAI-compatible LiteLLM routes `/v1/chat/completions` and `/v1/responses`, and lists registered service routes with their allowed methods and credential status.
- Services creates, imports from Relayna Studio, edits, sync-checks, disables, enables, and deletes service registrations. Method selection uses explicit checkboxes for `GET`, `POST`, `PUT`, `PATCH`, and `DELETE`.
- Usage filters usage by service, provider, model, and task.
- Health shows provider and service request, error, timeout, fallback, and latency status.

## Security Notes

- The portal never receives provider credentials or LiteLLM service keys.
- Raw virtual keys and operator tokens are shown once.
- Provider and service credentials can be configured, replaced, or cleared, but existing secret values are not displayed.
- Studio import reads catalog metadata only. Gateway preserves local credentials, enabled state, route overrides, limits, fallback services, project links, and cost settings on re-import.
- Disabling an OpenAI route is global and affects every virtual key until the route is enabled again.
- Service wildcard routes can accept `GET` only when the service registration includes `GET` in its allowed methods.
- The control listener should be protected by network policy, ingress rules, or private access controls in production.

## Import From Studio

Relayna Studio owns the operator-facing service catalog. Relayna Gateway owns
public traffic authentication, policy, route matching, upstream credential
injection, usage, costs, budgets, and fail-closed routing. The import flow copies
Studio catalog metadata into Gateway service registrations; it does not copy
provider credentials or allow Studio metadata to bypass Gateway policy.

Set `RELAYNA_STUDIO_BASE_URL` to the Studio backend base URL, not the Studio
frontend URL. Gateway appends `/studio/gateway/services` when it fetches the
catalog.

Local example:

```bash
export RELAYNA_STUDIO_BASE_URL="http://127.0.0.1:8000"
```

Docker on macOS or Windows when Studio runs on the host:

```bash
export RELAYNA_STUDIO_BASE_URL="http://host.docker.internal:8000"
```

Kubernetes example when Studio is another Service in the same namespace:

```bash
export RELAYNA_STUDIO_BASE_URL="http://relayna-studio-backend:8000"
```

If Studio protects the Gateway export endpoint, also set `RELAYNA_STUDIO_TOKEN`.
Gateway sends it as:

```http
Authorization: Bearer <RELAYNA_STUDIO_TOKEN>
```

Gateway expects `GET /studio/gateway/services` to return JSON with a top-level
`services` array. Each row should include `studio_service_id` or `service_id`, a
gateway-safe `name` or `gateway_service_name`, optional `display_name`,
`base_url`, `environment`, `status`, `tags`, optional `allowed_methods`, optional
`default_route_pattern`, and optional pricing hints. A minimal response looks
like this:

```json
{
  "services": [
    {
      "studio_service_id": "payments-api",
      "name": "payments-api",
      "display_name": "Payments API",
      "base_url": "https://payments.example.test",
      "environment": "prod",
      "tags": ["core", "billing"],
      "status": "healthy",
      "default_route_pattern": "/services/payments-api/*"
    }
  ]
}
```

Before opening the Gateway Admin portal, test Studio directly:

```bash
curl -sS "$RELAYNA_STUDIO_BASE_URL/studio/gateway/services"
```

Then test the Gateway-to-Studio connection through the protected Gateway admin
route:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin/studio/services
```

The Gateway response is the mapped import preview used by the portal. It should
show `studio_service_id`, `name`, `route_pattern`, and an `import_request` for
each service. If Studio is unreachable, stalls, returns non-JSON, or returns an
invalid service shape, Gateway returns `studio_unavailable`.

Operator flow:

1. Start Studio backend and verify `/studio/gateway/services`.
2. Start Gateway with `RELAYNA_STUDIO_BASE_URL` and optional
   `RELAYNA_STUDIO_TOKEN`.
3. Open `/admin-ui`, sign in with the Gateway operator token, and go to
   Services.
4. Click `Import from Studio`.
5. Select one or more services and click `Import selected`.
6. Configure Gateway-owned runtime fields such as credentials, enabled state,
   route overrides, limits, fallback services, project links, and cost mode.

Imported services are created with `source = studio`. They remain disabled or
incomplete until Gateway-owned runtime fields are configured. Re-importing by
`studio_service_id` is idempotent and preserves Gateway-owned fields by default.

For routed traffic, wildcard service aliases subtract the matched prefix before
forwarding upstream. For example:

```text
Gateway route pattern: /services/payments-api/*
Client request:        POST /services/payments-api/charges?trace=1
Upstream receives:     POST /charges?trace=1
```

Exact route patterns do not subtract a prefix. A route pattern of `/charges`
forwards `/charges` as `/charges`.

## Non-Expiring Virtual Keys

Virtual keys can be created or edited with no expiration date. In the Admin
portal, open Keys and select `No expiration` in the create or edit form. Through
the Admin API, send `expires_at: null`:

```bash
curl -sS -X POST http://127.0.0.1:8081/admin/keys \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "project_id": "<project-id>",
    "expires_at": null,
    "policy": {
      "allowed_routes": ["/services/*"],
      "allowed_providers": ["internal-service"]
    }
  }'
```

To clear expiration on an existing key:

```bash
curl -sS -X PATCH http://127.0.0.1:8081/admin/keys/<key-id> \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"expires_at": null}'
```

To set an expiration again:

```bash
curl -sS -X PATCH http://127.0.0.1:8081/admin/keys/<key-id> \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"expires_at": "2030-01-01T00:00:00Z"}'
```

Warning: non-expiring keys are long-lived bearer credentials. Anyone with the
raw key can use it until it is revoked, disabled, or restricted by policy. Use
non-expiring keys only for service-to-service integrations with external
rotation controls, narrow route/provider/service policy, secret-manager storage,
audit coverage, and a documented revocation procedure. Prefer expiring keys for
human users, temporary automation, demos, and CI jobs.

## Cost Modes

`fixed` records the configured estimate on each routed service request. For example, a service with `estimated_cost_usd` set to `0.01` contributes `$0.0100` per recorded request.

`passthrough` records the cost reported by the upstream response when present, such as `usage.total_cost` or LiteLLM response-cost fields. If the provider omits cost data, the usage event has no per-request cost and aggregate summaries treat missing cost as zero.
