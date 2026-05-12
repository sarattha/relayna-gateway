# Admin Portal Persisted Service Routing Test

Date: 2026-05-12

Branch: `codex/fix-issues-#12-#15`

## Environment

- Fresh PostgreSQL database: `relayna_gateway_browser_test_20260512231515`
- Redis: `redis://127.0.0.1:6379`
- Gateway proxy: `http://127.0.0.1:18080`
- Gateway control/admin portal: `http://127.0.0.1:18081/admin-ui`
- Admin session: signed in through the real admin portal UI with a freshly generated operator token.
- Screenshot: `/tmp/relayna-routing-test-20260512231515/admin-routes-view.png`

## Admin Portal UI Actions

1. Created project `Browser Routing QA 20260512231515`.
2. Created three services through the `Services` admin view:

| Service name | Persisted route pattern | Upstream |
| --- | --- | --- |
| `translation-svc` | `/services/translation` | `http://127.0.0.1:19091` |
| `summary-svc` | `/services/internal/summary` | `http://127.0.0.1:19092` |
| `ocr-svc` | `/services/internal1/ocr` | `http://127.0.0.1:19093` |

3. Verified the `Routes` admin view showed all three registered service routes with `POST`, `enabled`, `local`, and `configured` credentials.
4. Created a virtual key through the `Keys` admin view with:
   - Project: `Browser Routing QA 20260512231515`
   - Allowed routes: `/services/*`
   - Allowed providers: `litellm`, `internal-service`
   - Allowed services: `translation-svc,summary-svc,ocr-svc`

## Proxy Request Results

The service names intentionally do not match the first path segment after `/services/`. This verifies runtime lookup uses persisted `route_pattern` rows rather than deriving service identity only from the path.

| Request path | HTTP status | Upstream response service |
| --- | ---: | --- |
| `/services/translation` | 200 | `translation-svc` |
| `/services/internal/summary` | 200 | `summary-svc` |
| `/services/internal1/ocr` | 200 | `ocr-svc` |

Raw request output:

```json
{"path":"/services/translation","status":200,"body":"{\"ok\":true,\"upstream\":\"translation-upstream\",\"url\":\"/services/translation\",\"relayna_service\":\"translation-svc\"}"}
{"path":"/services/internal/summary","status":200,"body":"{\"ok\":true,\"upstream\":\"summary-upstream\",\"url\":\"/services/internal/summary\",\"relayna_service\":\"summary-svc\"}"}
{"path":"/services/internal1/ocr","status":200,"body":"{\"ok\":true,\"upstream\":\"ocr-upstream\",\"url\":\"/services/internal1/ocr\",\"relayna_service\":\"ocr-svc\"}"}
```

## Upstream Forwarding Log

The upstream logger recorded gateway-injected service/project headers and service credentials. Token values are intentionally redacted to prefixes.

```json
{"ts":"2026-05-12T16:23:08.033Z","upstream":"translation-upstream","method":"POST","url":"/services/translation","host_header_present":false,"authorization_present":true,"authorization_prefix":"Bearer translati...","relayna_service":"translation-svc","relayna_project_id":"bba6d263-dce5-443b-a411-39ddc30a1db8","content_type":"application/json","body":"{\"input\":\"qa /services/translation\"}"}
{"ts":"2026-05-12T16:23:08.322Z","upstream":"summary-upstream","method":"POST","url":"/services/internal/summary","host_header_present":false,"authorization_present":true,"authorization_prefix":"Bearer summary-u...","relayna_service":"summary-svc","relayna_project_id":"bba6d263-dce5-443b-a411-39ddc30a1db8","content_type":"application/json","body":"{\"input\":\"qa /services/internal/summary\"}"}
{"ts":"2026-05-12T16:23:08.603Z","upstream":"ocr-upstream","method":"POST","url":"/services/internal1/ocr","host_header_present":false,"authorization_present":true,"authorization_prefix":"Bearer ocr-upstr...","relayna_service":"ocr-svc","relayna_project_id":"bba6d263-dce5-443b-a411-39ddc30a1db8","content_type":"application/json","body":"{\"input\":\"qa /services/internal1/ocr\"}"}
```

## Notes

- `curl` was not available in this shell, so proxy requests were executed with Node's built-in HTTP client.
- The first mock upstream used Node's normal HTTP server and rejected Pingora-forwarded requests before the handler ran because the gateway strips the `Host` header. The mock was replaced with a raw TCP HTTP responder so the test only validated gateway route resolution and forwarding behavior.
