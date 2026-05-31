# LiteLLM Real Passthrough Test Report

Generated: 2026-05-31T16:32:08.793Z

Overall result: **PASS**

PASS: canonical /v1/chat/completions, /v1/responses, and /v1/embeddings pass through to LiteLLM; literal aliases /v1/chatcompletion, /v1/response, /v1/embedding, and /v1/rerank remain unsupported.

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
| canonical responses passes to litellm | PASS | 200 |  |
| canonical embeddings passes to litellm | PASS | 200 |  |
| apigee trusted header chat passes to litellm | PASS | 200 |  |
| upstream receives no client credentials | PASS | n/a |  |
| litellm front door receives custom header only | PASS | n/a |  |
| litellm key mapping precedes project mapping | PASS | n/a |  |
| disabled key mapping falls back to project mapping | PASS | n/a |  |
| disabled project mapping falls back to provider default | PASS | n/a |  |
| requested literal chatcompletion path | PASS | 404 | unsupported_route |
| requested literal response path | PASS | 404 | unsupported_route |
| requested literal embedding path | PASS | 404 | unsupported_route |
| requested rerank path | PASS | 404 | unsupported_route |

## Provider Capture Behind LiteLLM

| Request | Authorization seen by mock provider | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
| POST /v1/chat/completions | Bearer sk-upstream | no | no |
| POST /v1/responses | Bearer sk-upstream | no | no |
| POST /v1/embeddings | Bearer sk-upstream | no | no |
| POST /v1/chat/completions | Bearer sk-upstream | no | no |

## LiteLLM Front-Door Capture

| Request | Authorization from Gateway | x-litellm-api-key from Gateway | Client credential leaked? |
| --- | --- | --- | --- |
| POST /v1/chat/completions |  | sk-key | no |
| POST /v1/responses |  | sk-project | no |
| POST /v1/embeddings |  | sk-provider | no |
| POST /v1/chat/completions |  | sk-provider | no |

Observed LiteLLM credential precedence:
`sk-key -> sk-project -> sk-provider -> sk-provider`

## Interesting Finding

The current branch routes `/v1/chat/completions`, `/v1/responses`, and
`/v1/embeddings` to LiteLLM. The singular or alias paths still return
`unsupported_route` before reaching LiteLLM:

- `/v1/chatcompletion`
- `/v1/response`
- `/v1/embedding`
- `/v1/rerank`

The Gateway also has an internal-service `/embeddings` route, but it is not a
LiteLLM passthrough route.

## Screenshot Artifacts

- `screenshots/01-admin-ui-providers-litellm-mapping.png`
- `screenshots/02-admin-ui-project-mapping-control.png`
- `screenshots/03-real-env-report-overview.png`
- `screenshots/04-real-env-credential-capture.png`

## Raw Results

See `results.json`.
