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

Set `RELAYNA_STUDIO_BASE_URL` to enable the Services view `Import from Studio` action. If Studio requires authentication for the gateway export contract, also set `RELAYNA_STUDIO_TOKEN`; Gateway sends it as a bearer token.

Gateway fetches `GET /studio/gateway/services` and expects a response with a `services` array. Each row should include `studio_service_id` or `service_id`, a gateway-safe `name` or `gateway_service_name`, optional `display_name`, `base_url`, `environment`, `status`, `tags`, optional `allowed_methods`, optional `default_route_pattern`, and optional pricing hints.

Imported services are created with `source = studio` and remain disabled/incomplete until Gateway-owned runtime fields, especially credentials and routability, are configured. If Studio is unavailable or the response cannot be mapped safely, the Admin portal reports `studio_unavailable` and no local registration is changed.

## Cost Modes

`fixed` records the configured estimate on each routed service request. For example, a service with `estimated_cost_usd` set to `0.01` contributes `$0.0100` per recorded request.

`passthrough` records the cost reported by the upstream response when present, such as `usage.total_cost` or LiteLLM response-cost fields. If the provider omits cost data, the usage event has no per-request cost and aggregate summaries treat missing cost as zero.
