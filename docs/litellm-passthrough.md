# LiteLLM Passthrough

LiteLLM passthrough lets Relayna Gateway sit in front of LiteLLM as the single
public ingress while keeping Relayna identity, policy, usage, and credential
ownership. Clients authenticate to Gateway with Relayna credentials. Gateway
then strips client credentials and injects the internal LiteLLM credential
selected by operator configuration.

This page covers the `0.1.9` behavior.

## Request Model

Gateway never treats a LiteLLM master key or LiteLLM virtual key as a client
credential. Those keys are upstream credentials managed by Gateway.

Client request contracts:

| Gateway auth mode | Client headers |
| --- | --- |
| Entra disabled | `Authorization: Bearer <Relayna rk_live_... key>` |
| Entra enabled | `Authorization: Bearer <Entra JWT>` and `X-Relayna-Key: <Relayna rk_live_... key>` unless the Relayna key header has been renamed. |
| Trusted Apigee mode | Signed Apigee identity headers plus the configured Relayna key header. |

Gateway strips the following before forwarding to LiteLLM:

- `Authorization`
- the configured Relayna key header
- `X-Relayna-Key`
- legacy `X-AIH-API-Key`
- Entra/Apigee identity proof headers
- `Proxy-Authorization`
- `X-API-Key`
- client-supplied LiteLLM credential headers
- worker-token headers

Gateway then injects the resolved LiteLLM credential using the active LiteLLM
provider's header mode:

```http
Authorization: Bearer <resolved LiteLLM credential>
```

or:

```http
x-litellm-api-key: <resolved LiteLLM credential>
```

The header name is configurable on the LiteLLM provider row and must pass
Gateway's sensitive-header validation.

## Credential Resolution

Gateway resolves LiteLLM credentials in this order:

1. Enabled LiteLLM mapping for the authenticated Relayna key.
2. Enabled LiteLLM mapping for the authenticated key's project.
3. Active LiteLLM provider default credential from `provider_configs`.
4. `LITELLM_SERVICE_KEY` startup fallback.

All LiteLLM credential values are write-only. Admin API responses, audit
snapshots, frontend state, logs, and test reports must show only configured or
missing state, never the raw credential.

## Route Precedence

When a request reaches the proxy listener, routing is evaluated in this order:

1. Relayna service/control/operational routes, where applicable.
2. Registered service routes such as `/services/<service-name>/*`.
3. Canonical OpenAI-compatible routes:
   - `POST /v1/chat/completions`
   - `POST /v1/responses`
   - `POST /v1/embeddings`
4. LiteLLM wildcard passthrough for remaining allowed paths.

This means `/services/*` and the Admin/control API cannot accidentally fall
through to LiteLLM wildcard passthrough.

## Canonical Route Modes

Open the Admin portal Routes page to choose a mode for each canonical
OpenAI-compatible endpoint.

| Mode | Behavior |
| --- | --- |
| `managed_by_gateway` | Full Gateway governance path. Gateway authenticates the Relayna key, checks global route enablement, evaluates policy, enforces model/provider allowlists, checks RPM/TPM and budgets, runs configured guardrails, forwards upstream, and records full usage when accounting data is available. |
| `direct_litellm_passthrough` | Direct LiteLLM forwarding with Gateway governance retained. Gateway authenticates the Relayna key, checks route enablement, evaluates policy, enforces model/provider allowlists, checks RPM/TPM and budgets, strips/injects credentials, and preserves the original request. Guardrail body rewriting and token accounting are bypassed; usage is status-only. |

Use direct mode when a canonical route must behave closest to LiteLLM while
still preserving Relayna access control and credential isolation.

## Wildcard Passthrough

Open Admin portal Providers and configure `LiteLLM passthrough`.

Recommended starting point:

```text
Enable wildcard passthrough: enabled
Allowed paths: /v1/*
Allowed methods: GET,POST
LiteLLM UI exposure: disabled
LiteLLM admin API exposure: disabled
```

Wildcard passthrough preserves the original path and query string. For example:

```text
GET /v1/models?source=gateway
```

is forwarded to LiteLLM as:

```text
GET /v1/models?source=gateway
```

Wildcard non-canonical paths record reduced status-only usage. They do not run
Relayna policy, budgets, guardrails, or token accounting because they are not
known canonical generation routes.

## Sensitive Paths

LiteLLM `/ui` and admin-like paths are sensitive because they may expose key
management, spend, config, user, team, organization, budget, and global
administration surfaces.

Sensitive path groups:

- `/ui`, `/ui/*`
- `/key`, `/key/*`, `/keys`, `/keys/*`
- `/user`, `/user/*`
- `/team`, `/team/*`
- `/config`, `/config/*`
- `/spend`, `/spend/*`
- `/global`, `/global/*`
- `/budget`, `/budget/*`
- `/customer`, `/customer/*`
- `/organization`, `/organization/*`

Exposure modes:

| Mode | Effect |
| --- | --- |
| `disabled` | Sensitive paths are blocked even if listed in `Allowed paths`. |
| `operator_only` | Sensitive paths require Gateway Entra or trusted Apigee identity plus Relayna virtual-key auth. Use this behind identity-aware operator ingress. |
| `explicitly_exposed` | Sensitive paths are reachable to authenticated Relayna virtual-key clients when path and method allowlists match. Use only with explicit network and identity controls. |

To expose LiteLLM UI to operators through Gateway:

1. Add `/ui` and `/ui/*` to `Allowed paths`.
2. Set `LiteLLM UI exposure` to `operator_only` when your ingress supplies
   Entra/Apigee identity, or `explicitly_exposed` only when authenticated
   Relayna virtual-key access is intentionally acceptable.
3. Keep `LiteLLM admin API exposure` disabled unless you have a separate
   operator workflow for those APIs.
4. Confirm the request path reaches Gateway with the required auth headers.

A plain browser address bar cannot attach `Authorization` and Relayna key
headers on its own. Browser access through Gateway needs an identity-aware
ingress, reverse proxy, or operator portal flow that supplies Gateway auth.

## Admin API

Read settings:

```bash
curl -sS \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  http://127.0.0.1:8081/admin-ui/admin/providers/litellm-passthrough
```

Enable `/v1/*` wildcard passthrough:

```bash
curl -sS -X PATCH \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{
    "enabled": true,
    "allowed_paths": ["/v1/*"],
    "allowed_methods": ["GET", "POST"],
    "ui_exposure": "disabled",
    "admin_api_exposure": "disabled"
  }' \
  http://127.0.0.1:8081/admin-ui/admin/providers/litellm-passthrough
```

Set a canonical route to direct LiteLLM passthrough:

```bash
curl -sS -X PATCH \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"mode":"direct_litellm_passthrough"}' \
  http://127.0.0.1:8081/admin-ui/admin/openai-routes/chat-completions/mode
```

Return it to managed mode:

```bash
curl -sS -X PATCH \
  -H "Authorization: Bearer $GATEWAY_OPERATOR_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{"mode":"managed_by_gateway"}' \
  http://127.0.0.1:8081/admin-ui/admin/openai-routes/chat-completions/mode
```

All mutating calls require operator auth and write audit events.

## Verification

Focused local checks:

```bash
node tests/freeze-v0.1.8-perimeter.test.mjs
cargo test -p gateway-core route_settings --all-features
cargo test -p gateway-proxy passthrough --all-features
```

Real LiteLLM harness:

```bash
bash internal/test-reports/litellm-real-passthrough/run.sh
```

That harness starts PostgreSQL, Redis, Gateway, a real `litellm/litellm`
container, and a front-door capture service. It verifies canonical managed and
direct modes, wildcard `/v1/models` query preservation, `/ui` default blocking,
credential stripping, custom LiteLLM header injection, and credential
resolution precedence.
