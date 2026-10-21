# LiteLLM custom-header WebSocket verification

On 2026-10-21, the patched gateway passed **16/16 checks on LiteLLM 1.100.1**
and **16/16 on LiteLLM 1.106.0**. These checks used real LiteLLM applications,
real disposable PostgreSQL databases, real LiteLLM virtual keys and a local
deterministic HTTPS/WSS provider. No paid inference or production keys were used.
The gateway source was based on origin/main d2ff478 (released v0.1.47).

## Result

With `general_settings.litellm_key_header_name: x-litellm-key`, a caller key
restricted to `models: [gpt-6.1-sol]` can exchange Responses events through
Relayna with the provider still configured for custom-header bearer values.
The gateway supplies both the configured custom header and Bearer Authorization
from that same caller key.

The direct controls reproduced the incompatibility on each LiteLLM version:
custom-header-only HTTP succeeds with 200, custom-header-only WebSocket returns
403, and Authorization-only requests fail with HTTP 401 / WebSocket 403. Direct
WebSocket requests carrying both headers open and exchange response events.

Through the patched gateway, HTTP still succeeds; WebSockets exchange
`response.created`, `response.output_text.delta` and `response.completed`.
Two turns on one socket work, including the previous response ID. The response
alias and model selection in the first frame work too. Deltas arrive before
completion: roughly 0.15–0.23 seconds versus 0.30–0.38 seconds in these fixtures.
These are transport observations, not performance benchmarks.

Spoofed custom/alternate credentials and cookies do not override the selected
caller key. A valid custom key cannot rescue an invalid or absent caller bearer.
Luna in the handshake query remains HTTP 403. Luna in the first frame produces
an error frame after 101. Both denials contact no provider. The Sol-only key's
permissions remain intact; this does not explain the separate, uncorrelated
production Luna screenshot.

The correlated two-turn gateway sockets record HTTP 101, three client frames,
seven upstream frames, one upstream attempt, a closed lifecycle, no Relayna key ID and zero Relayna
per-key usage rows. Provider-side assertions confirm only fixture provider
credentials reach the provider, with no caller custom header or Relayna key.
The provider actually receives six `response.create` frames over native sockets
per version (twelve total), proving this is not HTTP inference fallback. The
provider counters in the JSON are cumulative across the sequential version runs.
Full sanitized observations are in [1.100.1 results](results-1.100.1.json) and
[1.106.0 results](results-1.106.0.json).

## Provenance and reproduction

LiteLLM 1.100.1 was the actual PyPI package, checked with
`importlib.metadata.version('litellm')`, installed in
`/tmp/relayna-litellm-11001-repro-venv`. LiteLLM 1.106.0 was the cached official
ARM64 image, run by immutable digest and version-checked inside the container:

    docker.litellm.ai/berriai/litellm@sha256:2e4c0371f3a8c28e4da131d77abaa8341cd3b0c2fcc4d34d7c5900431af7b9d0

Task-owned services were `relayna-ws-custom-pg` (PostgreSQL 16 on localhost
27432), `relayna-ws-custom-redis` (Redis 7 on 27379) and the temporary
`relayna-ws-custom-litellm106` container. Each LiteLLM version and live gateway
had its own fresh database. Prisma `db push --skip-generate` initialized the
disposable LiteLLM schemas; `DISABLE_SCHEMA_UPDATE=true` avoided migration
baselining against them. Gateway startup applied its normal migrations.

The local provider reused the deterministic Responses fixture from
`/tmp/relayna-litellm-live/provider.py` with a new task directory, port 27501
and a fresh two-day certificate covering localhost and host.docker.internal.
It returns a delta followed by completion with intentional 150 ms spacing,
and accepts subsequent response.create messages on the same socket.

Configure two LiteLLM model entries named gpt-6.1-sol and gpt-6-luna, both mapped
to `openai/gpt-4o` at the fixture's HTTPS `/v1` API with a fixture-only provider
credential. Generate a new fixture master key privately, then use `/key/generate`
to issue the Sol-only virtual key. Enable caller-owned LiteLLM forwarding and
the canonical Responses direct route, and enable `access.websocket` on
`/litellm/*` and `/v1/responses`. The active LiteLLM provider uses
`credential_header_mode=custom_header`, `credential_header_name=X-LiteLLM-Key`,
and `credential_header_value_format=bearer`, with a deliberately different
stored provider credential. Keep the configured fallback credential different
too, so neither can accidentally satisfy caller-key checks.

The retained local harness is `/tmp/relayna-ws-custom-live/run.py`; sanitized
results are checked in here. Fixture processes and containers are stopped after
validation; generated private keys, environment files and raw server logs are
removed. The temporary harness is evidence tooling, not a production dependency.

## Repository regression and verification

The committed Pingora E2E now requires both credential headers for custom-header
upgrades, tests raw/bearer values, mixed-case names and the alternate `api-key`
custom name, and verifies text/binary frames across wildcard, canonical and
alias routes. HTTP sends only the custom header; registered-service sockets
continue to use their service credential. Invalid callers and Entra/dedicated-key
requests cannot gain caller socket admission. Credential data stays out of
response headers and Traffic records.

A disposable mutation removing only the compatibility Authorization insertion
made that E2E fail at the first custom-header upgrade with upstream 401. Restoring
the insertion passes. An initial negative-test expectation was corrected from
400 to the existing `malformed_authorization` HTTP 401; runtime errors were not
changed.

The mandatory ordered verification stack passed: fmt, Clippy, all 437 Cargo
tests, audit, deny, machete, all 437 Nextest tests (zero skipped), Trivy,
Gitleaks and Semgrep. Existing audit exceptions and scanner exclusions remain
unchanged. The all-features workspace build, strict MkDocs and v0.1.47 release
metadata check also passed for the original implementation. The user then
authorized v0.1.48 after Codex review completed without findings; release
verification and publication are tracked in the accompanying ExecPlan.

## v0.1.48 release verification

Codex completed review of `921fe68` with no findings
([receipt](https://github.com/sarattha/relayna-gateway/pull/139#issuecomment-6319260430)).
The runtime and regression tests remain unchanged after review. The v0.1.48
versioned tree passes the full ordered verification stack again: 437 Cargo tests,
437 Nextest tests with zero skipped, fmt/Clippy and all required security tools,
using fresh real PostgreSQL/Redis services. The all-features workspace build,
release metadata, strict MkDocs, npm tests, TypeScript and 54 React coverage tests
also pass.

Regenerated Admin UI assets differ only in version labels. Desktop 1440x1000
and mobile 390x844 checks cover login, navigation, Monitor Overview, Discover
Routes, Govern Settings, dialogs and wide tables. Both labels show v0.1.48;
mobile tables scroll locally without page overflow. The earlier real-LiteLLM
matrices cover the identical reviewed runtime patch.

The production Dockerfile builds successfully. Local ARM64 image ID
`sha256:086081d8c5182fc67bacc1ceea2f537937f3f997d40e608d714690e47e2ecc45`
runs as the non-root `relayna` user. Against fresh PostgreSQL/Redis it returns
200 for `/admin-ui/healthz`, `/admin-ui/readyz`, `/admin-ui`, `/admin-ui/app.js`
and `/admin-ui/app.css`, with v0.1.48 labels in the embedded HTML and JavaScript.
Trivy reports no fixable high/critical image findings using the release
pipeline's existing flags. The smoke container is removed after verification.
