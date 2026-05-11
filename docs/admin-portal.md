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
- Keys creates, edits, disables, enables, revokes, and inspects virtual keys.
- Routes disables and enables the global OpenAI-compatible LiteLLM routes `/v1/chat/completions` and `/v1/responses`, and lists registered service routes with their allowed methods and credential status.
- Services creates, imports, edits, sync-checks, disables, enables, and deletes service registrations. Method selection uses explicit checkboxes for `GET`, `POST`, `PUT`, `PATCH`, and `DELETE`.
- Usage filters usage by service, provider, model, and task.
- Health shows provider and service request, error, timeout, fallback, and latency status.

## Security Notes

- The portal never receives provider credentials or LiteLLM service keys.
- Raw virtual keys and operator tokens are shown once.
- Service credentials can be configured, replaced, or cleared, but existing secret values are not displayed.
- Disabling an OpenAI route is global and affects every virtual key until the route is enabled again.
- Service wildcard routes can accept `GET` only when the service registration includes `GET` in its allowed methods.
- The control listener should be protected by network policy, ingress rules, or private access controls in production.
