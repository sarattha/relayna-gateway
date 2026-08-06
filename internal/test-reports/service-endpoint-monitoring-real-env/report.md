# Service Endpoint Monitoring Real-Environment Report

Generated: 2026-08-06T16:48:50.647Z

Overall result: **PASS**

Image: `relayna-gateway:endpoint-monitoring-test`

| Check | Result |
| --- | --- |
| templated success recorded | PASS |
| templated failure recorded | PASS |
| fallback failure recorded | PASS |
| endpoint breakdown aggregates template | PASS |
| endpoint breakdown uses path fallback | PASS |
| exact filters find failure | PASS |
| csv appends endpoint columns | PASS |

## Computer Use Evidence

**PASS** — Google Chrome was used against the rebuilt local image to:

- sign in to the isolated Admin portal;
- filter Usage to failed requests;
- confirm endpoint rows for `POST /jobs/{job_id}` and
  `POST /unlisted/fail-500`;
- confirm recent rows display method, effective endpoint, and numeric status
  codes `503` and `500`;
- inspect the same view at a narrow responsive width.

The numeric `status_code=503` path found during visual verification was fixed
and rechecked against the rebuilt image: dashboard, event, route-suggestion,
and endpoint-suggestion requests all returned HTTP 200. A focused API
regression test also covers the flattened filter-values query.

Local screenshot evidence (intentionally ignored by Git):

- `screenshots/usage-failures-desktop.jpeg`
- `screenshots/usage-failures-endpoints-desktop.jpeg`
- `screenshots/usage-failures-recent-desktop.jpeg`
- `screenshots/usage-failures-narrow.jpeg`
