# Front Door Penetration Test Report

Generated: 2026-05-30T16:35:15.905Z

Overall result: **PASS**

## Environment

- LiteLLM upstream: `http://litellm:4000`
- LiteLLM image: `docker.io/litellm/litellm:latest`
- Mock OIDC issuer: `http://mock-provider:4000/oauth`
- Audience: `api://relayna-gateway-pentest`
- Tenant: `relayna-pentest-tenant`
- Required scope: `gateway.invoke`
- Allowed group: `relayna-pentest-group`

## Front Doors

| Path | Proxy URL | Control URL |
| --- | --- | --- |
| No EntraID | `http://gateway-no-entra:8080` | `http://gateway-no-entra:8081` |
| EntraID | `http://gateway-entra:8080` | `http://gateway-entra:8081` |
| Apigee trusted header | `http://gateway-apigee:8080` | `http://gateway-apigee:8081` |

## Summary

- Checks: 27
- Passed: 27
- Failed: 0
- Provider captures behind LiteLLM: 10

## Attack Checks

| Check | Result | Status | Error code | Notes |
| --- | --- | ---: | --- | --- |
| no entra v1 chat completions passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| no entra v1 responses passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| no entra v1 embeddings passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| entra v1 chat completions passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| entra v1 responses passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| entra v1 embeddings passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| apigee v1 chat completions passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| apigee v1 responses passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| apigee v1 embeddings passes to litellm | PASS | 200 |  | LiteLLM forwarded to mock provider |
| no entra missing authorization rejected | PASS | 401 | missing_authorization |  |
| no entra x relayna key without authorization rejected | PASS | 401 | missing_authorization | No-Entra mode must not accept the Entra header as a bypass |
| entra missing jwt rejected | PASS | 401 | missing_entra_authorization |  |
| entra legacy relayna authorization bypass rejected | PASS | 401 | malformed_entra_authorization |  |
| entra expired token rejected | PASS | 401 | expired_entra_token |  |
| entra wrong audience rejected | PASS | 401 | invalid_entra_audience |  |
| entra missing scope rejected | PASS | 403 | insufficient_entra_authorization |  |
| entra tampered signature rejected | PASS | 401 | invalid_entra_token |  |
| apigee missing identity proof rejected | PASS | 401 | missing_entra_authorization |  |
| apigee missing signature rejected | PASS | 401 | untrusted_apigee_identity |  |
| apigee bad signature rejected | PASS | 401 | untrusted_apigee_identity |  |
| apigee missing scope rejected | PASS | 403 | insufficient_entra_authorization |  |
| apigee client jwt header does not break trusted path | PASS | 200 |  |  |
| alias embedding path rejected before litellm | PASS | 404 | unsupported_route |  |
| no entra rerank path rejected before litellm | PASS | 404 | unsupported_route |  |
| entra rerank path rejected before litellm | PASS | 404 | unsupported_route |  |
| apigee rerank path rejected before litellm | PASS | 404 | unsupported_route |  |
| no client or front door credentials reached provider | PASS | n/a |  | Mock provider only saw the LiteLLM provider credential |

## Provider Capture Behind LiteLLM

| Request | Authorization seen by mock provider | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
| POST /v1/chat/completions | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/responses | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/embeddings | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/chat/completions | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/responses | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/embeddings | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/chat/completions | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/responses | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/embeddings | Bearer sk-local-provider-pentest-key | no | no |
| POST /v1/chat/completions | Bearer sk-local-provider-pentest-key | no | no |

## Interesting Findings

- Canonical `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` pass through all three front-door paths to LiteLLM.
- Direct Relayna-key auth remains isolated to the no-Entra path and does not bypass Entra mode.
- Entra rejects missing, expired, wrong-audience, missing-scope, and tampered-signature JWTs before LiteLLM.
- Trusted Apigee mode rejects missing proof, missing signature, bad signature, and missing scope before LiteLLM.
- LiteLLM/mock provider only receives the internal provider credential; Relayna keys, Entra JWTs, and Apigee proof headers do not reach the provider.
- Alias `/v1/embedding` and unsupported `/v1/rerank` still return `unsupported_route` before reaching LiteLLM.

## Screenshot Artifacts

- `screenshots/01-dashboard.png`
- `screenshots/02-attack-checks.png`
- `screenshots/03-provider-capture.png`
- `screenshots/04-interesting-findings.png`

## Raw Results

See `results.json`.
