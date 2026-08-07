# Service Endpoint Monitoring Real-Environment Report

Generated: 2026-08-06T17:08:04.473Z

Overall result: **PASS**

Image: `relayna-gateway:0.1.23`

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

**PASS** — Google Chrome was used against the exact final
`relayna-gateway:0.1.23` image to confirm:

- the embedded Admin shell reports `v0.1.23`;
- the Usage view renders endpoint request/success/failure breakdowns;
- `POST /jobs/{job_id}` groups the successful and 503 calls under the synced
  OpenAPI template;
- `POST /unlisted/fail-500` remains visible through the concrete-path fallback;
- recent rows display method, effective endpoint, and numeric 200/500/503
  statuses.

Earlier in the same isolated workflow, Computer Use also verified the failure
filter and responsive narrow-width presentation. That pass exposed the
flattened numeric `status_code` deserialization edge case; after the fix, the
rebuilt final image passed the API regression test and all harness assertions.

Local screenshot evidence (intentionally ignored by Git):

- `screenshots/usage-failures-desktop.jpeg`
- `screenshots/usage-failures-endpoints-desktop.jpeg`
- `screenshots/usage-failures-recent-desktop.jpeg`
- `screenshots/usage-failures-narrow.jpeg`
