# Admin Portal

The admin portal is a static operator console embedded in `gateway-api`. It is served from the control listener at `/admin-ui` and calls the same `/admin-ui/admin/*` APIs used by automation.

## Authentication

Use the operator token seeded by `GATEWAY_ADMIN_TOKEN` on first startup, or the
generated operator token printed when no env token was set. The token is stored
in browser session storage and sent as:

```http
Authorization: Bearer <operator-token>
```

Rotate the token from the portal after bootstrap or whenever access changes. Rotation returns the new raw token once.

## Views

- Overview shows readiness, request count, active keys, enabled OpenAI routes, enabled services, failures, cost, and provider health.
- Projects creates and lists project UUIDs used to link services and
  project-owned virtual keys. Use `Select services` to open the service picker
  modal and manage a project's linked services.
- Keys creates, edits, disables, enables, revokes, and inspects virtual keys.
  Project-owned keys inherit service access from their selected project.
  Individual keys use `Select services` to open the service picker modal and
  choose services directly. Use `No expiration` for service keys whose rotation
  is managed outside Gateway.
- Providers configures LiteLLM and internal-service endpoints with write-only credentials.
- Routes disables and enables the global OpenAI-compatible LiteLLM routes `/v1/chat/completions` and `/v1/responses`, and lists registered service routes with their allowed methods and credential status.
- Services creates, imports from Relayna Studio, edits, sync-checks, disables, enables, and deletes service registrations. Method selection uses explicit checkboxes for `GET`, `POST`, `PUT`, `PATCH`, and `DELETE`.
- Usage filters usage by project, virtual key, service, route, provider, model,
  task, and status, then shows project, key, service, and exportable row-level
  usage data.
- Guardrails shows the gateway guardrail catalog, recent sanitized execution
  events, and execution summaries. Key create/edit forms can set mandatory,
  optional, and forbidden guardrails.
- Health shows provider and service request, error, timeout, fallback, and latency status.

## Security Notes

- The portal never receives provider credentials or LiteLLM service keys.
- Raw virtual keys and operator tokens are shown once.
- Provider and service credentials can be configured, replaced, or cleared, but existing secret values are not displayed.
- Studio import reads catalog metadata only. Gateway preserves local credentials, enabled state, route overrides, limits, fallback services, project links, and cost settings on re-import.
- Disabling an OpenAI route is global and affects every virtual key until the route is enabled again.
- Service wildcard routes can accept `GET` only when the service registration includes `GET` in its allowed methods.
- Guardrail execution records never include raw request bodies, response bodies,
  provider credentials, bearer tokens, or PII mappings.
- The control listener should be protected by network policy, ingress rules, or private access controls in production.

## Usage Export

Operators can export usage rows through admin-token-protected endpoints:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  "http://127.0.0.1:8081/admin-ui/admin/usage/export.json?status=success&limit=1000"

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  "http://127.0.0.1:8081/admin-ui/admin/usage/export.csv?status=failure&limit=1000"
```

Supported filters match the usage dashboard query model: `from`, `to`,
`project_id`, `key_id`, `route`, `provider`, `service`, `task_id`, `model`,
and `status`. Export rows are ordered by creation time and request ID. `limit`
defaults to `1000`, is clamped to `10000`, and `offset` can be used for
pagination.

JSON exports include a `summary` object plus `rows`. CSV exports include the row
fields directly and neutralize spreadsheet formula prefixes before escaping
cells.

## Guardrails

Gateway guardrails are configured by operators and enforced by virtual-key
policy. `pii-redact` is seeded as an opt-in built-in guardrail. Add it to a
key's `mandatory_guardrails` to apply it even when clients omit the
`guardrails` request field, or add it to `optional_guardrails` to let callers
request it explicitly.

The Guardrails view manages the global catalog. Use `New guardrail` to add a
custom HTTP guardrail, or select an existing row to open the detail drawer.
Built-ins such as `pii-redact` allow safe edits to enabled state, modes, failure
policy, schema, and runtime config. Built-ins do not expose endpoint, token, or
delete controls. Custom HTTP guardrails expose endpoint URL, timeout, and
write-only bearer token controls.

Catalog config has two fields with different jobs:

- `config_schema` documents the expected JSON shape for operators.
- `runtime_config` is the actual global default config passed to the guardrail
  when it executes.

For `pii-redact`, `runtime_config` can include `restore_output`. When true,
post-call guardrails restore request-local placeholders before redacting any new
PII generated by the provider. When false, placeholders remain redacted in the
final response.

Key create and edit forms configure how the catalog applies to each virtual
key:

- Mandatory guardrails always run for that key.
- Optional guardrails are allowed for client-requested use.
- Forbidden guardrails are hidden from client discovery and rejected if
  requested.
- Guardrail config overrides tune selected guardrails only for that key.

The Admin portal shows per-key override editors only after a guardrail is
selected as mandatory or optional. This keeps unselected catalog entries out of
the key form and makes the execution rule explicit: an override is dormant until
that guardrail is actually applied.

Example key policy with per-key config:

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

Effective config is a shallow JSON object merge:

```text
effective_config = catalog runtime_config + key guardrail_config_overrides[name]
```

Unknown override guardrails, forbidden override guardrails, and non-object
override values are rejected with stable guardrail error envelopes. HTTP
guardrail endpoint URL, timeout, and bearer token remain catalog-level provider
settings; per-key overrides only tune runtime config.

Operator APIs:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/guardrails

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  "http://127.0.0.1:8081/admin-ui/admin/guardrails/executions?limit=50"

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/guardrails/summary
```

Client discovery and test APIs use Relayna virtual keys:

```bash
curl -sS \
  -H "Authorization: Bearer rk_live_xxx" \
  http://127.0.0.1:8081/admin-ui/v1/guardrails

curl -sS \
  -H "Authorization: Bearer rk_live_xxx" \
  -H "Content-Type: application/json" \
  -X POST http://127.0.0.1:8081/admin-ui/v1/guardrails/test \
  -d '{"guardrails":["pii-redact"],"mode":"pre_call","input":{"messages":[{"role":"user","content":"email alice@example.com"}]}}'
```

Custom HTTP guardrails can be added through the admin API. Gateway sends a
sanitized JSON payload with `request_id`, `guardrail`, `mode`, `context`,
`config`, and one of `request` or `response`. The provider returns `action`,
optional modified `request` or `response`, optional `reason`, and sanitized
`metadata`.

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -X POST http://127.0.0.1:8081/admin-ui/admin/guardrails \
  -d '{
    "name": "custom-check",
    "description": "Company policy check",
    "endpoint_url": "https://guardrails.example/check",
    "modes": ["pre_call", "post_call", "during_call"],
    "failure_policy": "fail_open",
    "timeout_ms": 1500,
    "bearer_token": "secret-token"
  }'
```

Streaming requests with guarded responses require selected response guardrails
to support `during_call`. `pii-redact` redacts common PII in streaming chunks
with a small holdback window for values split across chunks. If a required
guardrail cannot run during streaming, Gateway fails closed with
`guardrail_unavailable`.

## Import From Studio

Relayna Studio owns the operator-facing service catalog. Relayna Gateway owns
public traffic authentication, policy, route matching, upstream credential
injection, usage, costs, budgets, and fail-closed routing. The import flow copies
Studio catalog metadata into Gateway service registrations; it does not copy
provider credentials or allow Studio metadata to bypass Gateway policy.

Configure the Studio backend in Admin portal Settings, or set
`RELAYNA_STUDIO_BASE_URL` as a deployment fallback. Use the Studio backend base
URL, not the frontend URL. Gateway appends `/studio/gateway/services` when it
fetches the catalog. Admin-saved settings override environment settings until
the persisted base URL is cleared, at which point the environment fallback is
effective again.

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

If Studio protects the Gateway export endpoint, set the optional bearer token in
Admin portal Settings or with `RELAYNA_STUDIO_TOKEN`. Gateway sends it as:

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

Then test the Gateway-to-Studio connection through Admin portal Settings or the
protected Gateway admin route:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -X POST \
  http://127.0.0.1:8081/admin-ui/admin/studio/connection/test

curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/studio/services
```

The test route returns `ok` and `service_count` when the catalog is reachable.
The services route returns the mapped import preview used by the portal. It
should show `studio_service_id`, `name`, `route_pattern`, and an
`import_request` for each service. If Studio is unreachable, stalls, returns
non-JSON, or returns an invalid service shape, Gateway returns
`studio_unavailable`.

Operator flow:

1. Start Studio backend and verify `/studio/gateway/services`.
2. Start Gateway with optional `RELAYNA_STUDIO_BASE_URL` and
   `RELAYNA_STUDIO_TOKEN`, or configure the connection in Admin Settings.
3. Open `/admin-ui`, sign in with the Gateway operator token, and go to
   Settings.
4. Save or test the Studio connection. Token values are write-only and are never
   returned by the API.
5. Go to Services and click `Import from Studio`.
6. Select one or more services and click `Import selected`.
7. Configure Gateway-owned runtime fields such as credentials, enabled state,
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
the Admin API, send `expires_at: null`. Project-owned keys specify
`owner_type: "project"` and a `project_id`:

```bash
curl -sS -X POST http://127.0.0.1:8081/admin-ui/admin/keys \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "owner_type": "project",
    "project_id": "<project-id>",
    "expires_at": null,
    "policy": {
      "allowed_routes": ["/services/*"],
      "allowed_providers": ["internal-service"]
    }
  }'
```

Individual keys specify `owner_type: "individual"` and direct `service_names`:

```bash
curl -sS -X POST http://127.0.0.1:8081/admin-ui/admin/keys \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "owner_type": "individual",
    "service_names": ["payments-api"],
    "expires_at": null,
    "policy": {
      "allowed_routes": ["/services/*"],
      "allowed_providers": ["internal-service"]
    }
  }'
```

To clear expiration on an existing key:

```bash
curl -sS -X PATCH http://127.0.0.1:8081/admin-ui/admin/keys/<key-id> \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"expires_at": null}'
```

To set an expiration again:

```bash
curl -sS -X PATCH http://127.0.0.1:8081/admin-ui/admin/keys/<key-id> \
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
