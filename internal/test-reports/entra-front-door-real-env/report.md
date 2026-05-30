# Entra Front Door Real Environment Test Report

Generated: 2026-05-29T17:00:16.757Z

Overall result: **PASS**

## Environment

- Gateway proxy: `http://gateway:8080`
- Gateway control: `http://gateway:8081`
- Mock OIDC issuer: `http://mock-app:4000/oauth`
- Audience: `api://relayna-gateway-review`
- Tenant: `relayna-review-tenant`
- Relayna key header: `X-Relayna-Key`
- Trusted Apigee header mode: `true`

## Checks

| Check | Result | Status | Error code |
| --- | --- | ---: | --- |
| direct valid jwt and relayna key | PASS | 200 |  |
| responses valid jwt and relayna key | PASS | 200 |  |
| direct provider route valid jwt and relayna key | PASS | 200 |  |
| builtin internal summary valid jwt and relayna key | PASS | 200 |  |
| service wildcard valid jwt and relayna key | PASS | 200 |  |
| service wildcard missing jwt fails before upstream | PASS | 401 | missing_entra_authorization |
| missing jwt fails before upstream | PASS | 401 | missing_entra_authorization |
| wrong audience rejected by gateway | PASS | 401 | invalid_entra_audience |
| expired token rejected by gateway | PASS | 401 | expired_entra_token |
| missing scope rejected by gateway | PASS | 403 | insufficient_entra_authorization |
| invalid signature rejected by gateway | PASS | 401 | invalid_entra_token |
| invalid relayna key rejected after jwt | PASS | 401 | invalid_virtual_key |
| default header is x relayna key not x aih api key | PASS | 401 | missing_authorization |
| apigee revalidation path forwards after edge validation | PASS | 200 |  |
| apigee edge rejects wrong audience | PASS | 401 | apigee_verify_jwt_failed |
| apigee trusted header path forwards with signed identity | PASS | 200 |  |
| apigee trusted header tamper rejected by gateway | PASS | 401 | untrusted_apigee_identity |
| upstream receives only internal provider credentials | PASS | n/a |  |

## Upstream Credential Capture

| Path | Upstream authorization | Client credential leaked? | Apigee identity leaked? |
| --- | --- | --- | --- |
| /v1/chat/completions | Bearer sk-litellm-review-service-key | no | no |
| /v1/responses | Bearer sk-litellm-review-service-key | no | no |
| /v1/chat/completions | Bearer sk-direct-openai-review-service-key | no | no |
| /summary | Bearer sk-internal-summary-review-service-key | no | no |
| /execute | Bearer sk-internal-review-service-key | no | no |
| /v1/chat/completions | Bearer sk-litellm-review-service-key | no | no |
| /v1/chat/completions | Bearer sk-litellm-review-service-key | no | no |

## Screenshot Artifacts

- `screenshots/entra-review-dashboard.jpg`
- `screenshots/entra-review-results-json.jpg`

## Raw Results

See `results.json`.
