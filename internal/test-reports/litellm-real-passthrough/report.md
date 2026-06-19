# LiteLLM Real Passthrough Test Report

Generated: 2026-06-19T10:43:21.770Z

Overall result: **PASS**

PASS: canonical managed and direct route modes reach LiteLLM, wildcard /v1/models passes through with query preservation when enabled, raw /ui remains blocked by default, /admin-ui/litellm-ui reaches real LiteLLM with operator auth, trusted-ingress UI and explicitly exposed admin API paths work without Relayna auth when Entra is disabled, direct /v1/responses accepts a LiteLLM bearer key, and credential translation strips client secrets.

## Environment

- Gateway proxy: `http://gateway:8080`
- Gateway control: `http://gateway:8081`
- LiteLLM upstream: `http://litellm:4000`
- LiteLLM front door: `http://litellm-front-door:4000`
- LiteLLM image: `docker.io/litellm/litellm:latest`
- LiteLLM image digest pulled locally: `sha256:cae1ac3492d6d0bea69c26f4485381624e073eb753f3534ae7703a4204a4ce6b`
- Mock OIDC issuer: `http://mock-provider:4000/oauth`
- Audience: `api://relayna-gateway-litellm-review`
- Tenant: `relayna-litellm-review-tenant`
- Relayna key header: `X-Relayna-Key`
- Trusted Apigee header mode: `true`

## Checks

| Check | Result | Status | Error code |
| --- | --- | ---: | --- |
| canonical chat completions passes to litellm | PASS | 200 |  |
| canonical route mode can switch to direct passthrough | PASS | n/a |  |
| wildcard passthrough defaults disabled then can enable v1 | PASS | n/a |  |
| wildcard v1 models preserves query and reaches real litellm | PASS | n/a |  |
| wildcard ui path is blocked by default | PASS | 404 | unsupported_route |
| canonical responses passes to litellm | PASS | 200 |  |
| canonical embeddings passes to litellm | PASS | 200 |  |
| apigee trusted header chat passes to litellm | PASS | 200 |  |
| upstream receives no client credentials | PASS | n/a |  |
| litellm front door receives custom header only | PASS | n/a |  |
| litellm front door receives bearer prefixed custom header | PASS | n/a |  |
| litellm key mapping precedes project mapping | PASS | n/a |  |
| disabled key mapping falls back to project mapping | PASS | n/a |  |
| disabled project mapping falls back to provider default | PASS | n/a |  |
| litellm ui proxy requires operator token | PASS | 401 |  |
| litellm ui proxy reaches real litellm with gateway credential | PASS | 200 |  |
| trusted ingress disables entra and apigee front door checks | PASS | 200 |  |
| trusted ingress no auth ui reaches litellm with gateway credential | PASS | n/a |  |
| trusted ingress no auth ui support endpoint reaches litellm | PASS | n/a |  |
| trusted ingress no auth admin spend logs reaches litellm | PASS | n/a |  |
| trusted ingress no auth admin key info reaches litellm | PASS | n/a |  |
| trusted ingress no auth v1 models still requires relayna auth | PASS | 401 | missing_authorization |
| direct responses accepts litellm bearer without relayna key | PASS | n/a |  |
| wildcard literal chatcompletion reaches litellm | PASS | 404 |  |
| wildcard literal response reaches litellm | PASS | 404 |  |
| wildcard literal embedding reaches litellm | PASS | 404 |  |
| wildcard rerank reaches litellm | PASS | 400 | 400 |

## Provider Capture Behind LiteLLM

| Request | Authorization seen by mock provider | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
| POST /v1/chat/completions | Bearer sk-upstream | no | no |
| POST /v1/chat/completions | Bearer sk-upstream | no | no |
| POST /v1/responses | Bearer sk-upstream | no | no |
| POST /v1/embeddings | Bearer sk-upstream | no | no |
| POST /v1/chat/completions | Bearer sk-upstream | no | no |
| POST /v1/responses | Bearer sk-upstream | no | no |

## LiteLLM Front-Door Capture

| Request | Authorization from Gateway | x-litellm-api-key from Gateway | x-litellm-key from Gateway | Client credential leaked? |
| --- | --- | --- | --- | --- |
| POST /v1/chat/completions |  |  | Bearer sk-key | no |
| POST /v1/chat/completions |  |  | Bearer sk-client | no |
| GET /v1/models?source=wildcard |  |  | Bearer sk-key | no |
| POST /v1/responses |  |  | Bearer sk-project | no |
| POST /v1/embeddings |  |  | Bearer sk-provider | no |
| POST /v1/chatcompletion |  |  | Bearer sk-provider | no |
| POST /v1/response |  |  | Bearer sk-provider | no |
| POST /v1/embedding |  |  | Bearer sk-provider | no |
| POST /v1/rerank |  |  | Bearer sk-provider | no |
| GET /ui/ |  |  | Bearer sk-provider | no |
| POST /v1/chat/completions |  |  | Bearer sk-provider | no |
| GET /ui/ |  |  | Bearer sk-provider | no |
| GET /user/info |  |  | Bearer sk-provider | no |
| GET /global/spend/logs |  |  | Bearer sk-provider | no |
| GET /key/info |  |  | Bearer sk-provider | no |
| POST /v1/responses |  |  | Bearer sk-client | no |

Observed LiteLLM credential precedence:
`sk-key -> sk-client -> sk-key -> sk-project -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-provider -> sk-client`

## Wildcard Coverage

The current branch routes managed canonical calls through LiteLLM, can switch a
canonical route to direct LiteLLM passthrough, and forwards enabled wildcard
`/v1/*` calls while preserving path and query.

The browser-safe LiteLLM UI path is also covered: unauthenticated
`/admin-ui/litellm-ui/` is rejected, while the operator-authenticated path
reaches the real LiteLLM `/ui/` through Gateway with only the server-side
LiteLLM credential forwarded.

The trusted-ingress LiteLLM UI path is covered with Entra and Apigee front-door
checks disabled: unauthenticated `/ui/` and the UI support endpoint
`/user/info` reach real LiteLLM with only the server-side LiteLLM credential,
while unauthenticated `/v1/models` still fails at Gateway auth.

The literal alias probes below reached real LiteLLM and were rejected there with
404 or 400 responses, proving they were not stopped by the Gateway router:

- `/v1/chatcompletion`
- `/v1/response`
- `/v1/embedding`
- `/v1/rerank`

## Screenshot Artifacts

- `screenshots/01-admin-ui-providers-litellm-mapping.png`
- `screenshots/02-admin-ui-project-mapping-control.png`
- `screenshots/03-real-env-report-overview.png`
- `screenshots/04-real-env-credential-capture.png`
- `screenshots/05-real-litellm-issue-64-report.png`
- `screenshots/06-admin-ui-litellm-passthrough-controls.png`
- `screenshots/07-admin-ui-route-mode-controls.png`
- `screenshots/08-litellm-ui-proxy-real-env.png`
- `screenshots/09-real-env-issue-66-report.png`
- `screenshots/66-issue-68-real-litellm-evidence.png`
- `screenshots/67-issue-68-real-env-dashboard.png`
- `screenshots/68-issue-68-trusted-ingress-litellm-ui.png`

## Raw Results

See `results.json`.
