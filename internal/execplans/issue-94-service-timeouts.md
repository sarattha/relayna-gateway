# Issue 94 Service Timeout Controls and Responses

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Operators should be able to see and update each registered service's persisted
`timeout_ms` directly from the Admin UI Routes view. A client whose upstream
request reaches that timeout before response headers are committed should
receive Relayna Gateway's stable JSON error envelope with HTTP 504 and
`upstream_timeout`, rather than Pingora's empty HTTP 502 response. Usage and
debug records should use status 504 for that terminal pre-commit failure. If a
stream has already committed its response, Gateway should leave the committed
response alone, terminate the failed stream, and retain timeout evidence in the
debug trace.

## Progress

- [x] (2026-07-18 09:32Z) Inspected issue 94, its empty comment thread, the latest release tag, the Admin UI Routes and Services flows, the service persistence model, and Pingora's proxy failure lifecycle.
- [x] (2026-07-18 09:38Z) Chose and checked out `codex/issue-94-service-timeouts` from `v0.1.19` / `origin/main`.
- [x] (2026-07-18 09:57Z) Added focused proxy failure classification, structured JSON response behavior, terminal status/debug evidence, and unit tests.
- [x] (2026-07-18 10:00Z) Added the registered-service timeout form to Routes, reused the existing service PATCH API, extended UI and service tests, regenerated assets, and updated operator documentation.
- [x] (2026-07-18 10:21Z) Passed focused checks, Computer Use and isolated real-environment validation, the mandatory verification stack, the release build, and the Admin UI test suite.
- [x] (2026-07-18 10:32Z) Committed and pushed the focused branch, opened ready-for-review PR #95, confirmed every GitHub check passed, and monitored the first Codex review cycle. The reviewer returned a clean thumbs-up with no review threads to fix, reply to, or resolve.
- [x] (2026-07-18 11:14Z) Prepared patch release `0.1.20`: bumped all workspace crates, added release notes, updated current-release and deployment documentation, refreshed Admin UI version indicators and generated assets, and passed release metadata, Admin UI, strict docs, and mandatory verification checks.

## Surprises & Discoveries

- Observation: Pingora's `fail_to_proxy` hook is asynchronous and runs only
  after the proxy engine has exhausted retry handling, while
  `error_while_proxy` runs earlier and can request a retry.
  Evidence: `pingora-proxy-0.8.0/src/proxy_trait.rs` documents and implements
  the two hooks in that order.
- Observation: the current shared Pingora `respond_error` helper serializes the
  Gateway JSON envelope but delegates headers to Pingora's generic error
  response, which does not set `Content-Type: application/json`.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs` calls
  `respond_error_with_body`; Pingora's `gen_error_response` adds length and
  cache headers but no content type.
- Observation: Studio re-import already preserves service runtime fields,
  including the existing persisted timeout, because its conflict update does
  not overwrite `timeout_ms`.
  Evidence: `PostgresStore::upsert_studio_service` and
  `upsert_studio_service_in_tx` update Studio metadata while leaving timeout and
  other Gateway-owned runtime columns intact.
- Observation: the gateway-api in-memory test store's service patch helper did
  not update `timeout_ms`, even though PostgreSQL and the production API do.
  Evidence: the extended Studio re-import preservation test initially observed
  the default 60000 after patching to 123456; bringing the test double in line
  with PostgreSQL made the focused test pass.
- Observation: a fresh database without an explicit global policy layer
  synthesizes the released LiteLLM/chat default layer, which intentionally
  intersected the isolated service-key policy into a deny.
  Evidence: the policy simulator reported `deny=true` and an excluded
  `internal-service` provider. The disposable test database used an explicit
  neutral global layer before runtime validation; product code was unchanged.
- Observation: Pingora calls `logging` twice when `fail_to_proxy` writes the
  terminal response itself, and the existing `terminal_usage_recorded` guard
  was checked but not set in the normal logging path.
  Evidence: the first real 504 produced two usage rows with the same request
  ID. Setting the existing guard before persistence and repeating the request
  produced exactly one row.

## Decision Log

- Decision: preserve the existing `service_registrations.timeout_ms` column,
  `ServicePatchRequest`, and `PATCH /admin-ui/admin/services/{service_name}`;
  add no timeout alias, new route, or migration.
  Rationale: issue 94 explicitly requires one persisted source of truth and the
  released API already validates the desired `1..=600000` boundary.
  Date/Author: 2026-07-18 / Codex.
- Decision: classify Pingora connect, TLS handshake, read, and write timeout
  error types only in `fail_to_proxy`, after any Pingora retry or configured
  provider fallback has ended.
  Rationale: this matches Pingora's lifecycle and avoids rewriting upstream
  HTTP 502/503 responses or non-timeout connection failures.
  Date/Author: 2026-07-18 / Codex.
- Decision: production-freeze compatibility is intentionally crossed for the
  terminal timeout status/body change, with the user's explicit authorization.
  Rationale: `v0.1.19` currently ships the empty 502 behavior that issue 94 asks
  to replace. Unaffected upstream and connection behavior remains compatible.
  Date/Author: 2026-07-18 / Codex.
- Decision: make the shared Gateway-error response helper explicitly emit
  `application/json`, exact content length, and `private, no-store` rather than
  adding a timeout-only response writer.
  Rationale: every call site already serializes the same JSON envelope, and a
  shared header builder keeps the wire contract consistent without duplicating
  timeout-specific response code.
  Date/Author: 2026-07-18 / Codex.
- Decision: publish issue 94 as patch release `v0.1.20`.
  Rationale: `v0.1.19` is the latest published tag, and the requested version
  bump packages one focused timeout-control and error-contract change without
  changing the existing minor-release API surface or requiring migration.
  Date/Author: 2026-07-18 / Codex.

## Outcomes & Retrospective

Implementation is complete and focused checks pass. Computer Use saved a real
service timeout from 800 ms to 250 ms in the running Admin UI, confirmed the
success notice and persisted reload value, and inspected both desktop and a
narrow browser viewport; the Routes table remained usable through its existing
horizontal overflow behavior.

An isolated Gateway backed by PostgreSQL and Redis then called a real delayed
HTTP upstream. A pre-response read timeout returned HTTP 504 in 0.576 seconds
with `Content-Type: application/json`, the `upstream_timeout` envelope, and the
caller-supplied request ID. PostgreSQL contained exactly one matching failure
usage row with status 504 and a debug selection trace containing
`timeout_ms=250` and `terminal_error=upstream_timeout`. An SSE response that
committed its first event remained HTTP 200 and closed at the timeout without a
replacement response; its single usage row remained 200 and its debug trace
recorded the timeout. An immediate upstream-originated 503 passed through with
its original body and one 503 usage row.

The mandatory workspace verification stack, release build, Admin UI tests, and
every GitHub pull request check passed. The repository's first automated Codex
review completed with a thumbs-up and no review submissions or inline threads,
so no feedback fix, reply, or resolution mutation was necessary. PR #95 is
ready for human review.

Release metadata now targets `0.1.20` across Cargo packages, the Admin UI,
deployment manifests, README, and current operator/release documentation. The
new changelog section records the structured 504 compatibility change, stream
behavior, duplicate-usage fix, and secret-handling invariants. Historical
`v0.1.19` changelog and compatibility-boundary references remain unchanged.

## Context and Orientation

`crates/gateway-proxy/src/pingora_plane.rs` implements the Pingora data plane.
`upstream_peer` applies a matched route's timeout to connection, total
connection, read, and write operations. `error_while_proxy` handles established
upstream failures before retry decisions finish. `fail_to_proxy`, currently
inherited from Pingora, writes the generic terminal error. `logging` writes the
usage event and debug bundle after the response or terminal error.

`crates/gateway-api/admin-ui/src/main.ts` is the source for the Admin UI. The
Routes view already embeds inline configuration forms for canonical OpenAI and
Anthropic routes. Registered service rows currently display route metadata but
not their persisted timeout. The Services form already updates the same timeout
through the service PATCH endpoint. Vite emits checked-in assets under
`crates/gateway-api/src/static/admin-ui/`.

`crates/gateway-core/src/services.rs` defines and validates service create and
patch requests. `crates/gateway-store/src/postgres.rs` persists the timeout and
preserves Gateway-owned runtime values during Studio re-import. No schema or
wire-format change is necessary.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.19`, which is also the branch
base. The public timeout failure response is an intentional released behavior
change authorized by the user: terminal pre-commit upstream timeouts become
structured 504 responses. Existing service API fields, database schema,
upstream-originated status passthrough, non-timeout 502 behavior, and committed
stream status semantics remain unchanged.

## Plan of Work

In `crates/gateway-proxy/src/pingora_plane.rs`, add a small timeout classifier
for Pingora errors and override `fail_to_proxy`. When the terminal error is an
upstream timeout and no response headers are committed, write the existing
`GatewayError::UpstreamTimeout` body with an explicit JSON content type, set a
terminal status hint for logging, and return 504. When headers are already
committed, do not write a replacement response; instead mark the context so the
debug bundle records that the stream ended on an upstream timeout. Delegate all
other errors to the existing Pingora-compatible status logic. Add focused unit
tests for timeout classification, non-timeout exclusions, fallback lifecycle
placement, terminal status selection, and debug trace evidence.

In `crates/gateway-api/admin-ui/src/main.ts`, extend registered service route
rows with a `route-config-form` containing the effective timeout and a Save
button. Attach a submit handler that validates a finite integer in
`1..=600000`, PATCHes only `timeout_ms` to the existing service endpoint, shows
the standard success notice, and reloads Routes so both views reflect the
persisted response. Extend `tests/admin-ui.test.mjs`, update the existing
service API preservation test where useful, and regenerate the checked-in
assets.

In `docs/admin-portal.md`, document the Routes inline service timeout control,
the validation range, the shared persisted value, the structured timeout
response, and the operational warning that a longer synchronous timeout is not
backpressure or asynchronous task submission.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-proxy
    cargo test -p gateway-api admin_service
    npm run build:admin-ui
    npm test
    .codex/skills/code-change-verification/scripts/run.sh

For real-environment proof, start PostgreSQL, Redis, the Gateway, and a local
HTTP upstream that deliberately delays beyond a short persisted service
timeout. Use Computer Use against the actual `/admin-ui/` Routes view to save a
new timeout, then use a real client request to verify the delay yields HTTP 504,
JSON content type, `upstream_timeout`, matching request ID, and persisted usage
and debug evidence. Also inspect desktop and narrow-width Routes layouts.

## Validation and Acceptance

The change is accepted when:

- Routes displays every registered service's current timeout and a permitted
  operator can save a value in `1..=600000` through the existing PATCH API.
- Reloading Routes and opening Services show the same persisted value, and a
  Studio re-import leaves that value unchanged.
- The following request uses the value for connection, total connection, read,
  and write timeouts, as already wired by `upstream_peer`.
- A terminal pre-commit Pingora timeout returns status 504, JSON content type,
  and the stable `upstream_timeout` envelope containing the request ID.
- Usage and debug records use 504 for the pre-commit terminal timeout; a
  committed stream is not rewritten and its debug trace records the timeout.
- Upstream HTTP 502/503 and non-timeout connection failures retain their prior
  behavior.
- Admin UI tests, the Rust workspace format/lint/test stack, and real browser
  validation all pass.

## Idempotence and Recovery

The Admin UI build and all tests are safe to rerun. The real-environment service
registration can be patched repeatedly and removed through the normal Admin
API after validation. No migration or Redis key change is involved. If a local
test service or Gateway process is interrupted, restart it with the same
explicit ports and rerun the request; no production state is involved.

## Artifacts and Notes

The working tree already contained an unrelated untracked
`.tmp-admin-ui-audit/` directory before this work. It must remain untouched and
must not be staged.

## Interfaces and Dependencies

The final implementation continues to use:

- `ServicePatchRequest { timeout_ms: Option<i64>, .. }` with the existing
  `1..=600000` validation.
- `PATCH /admin-ui/admin/services/{service_name}` for persistence and existing
  service-update authorization.
- `GatewayError::UpstreamTimeout` for status 504, code `upstream_timeout`, and
  message `Upstream provider timed out.`.
- Pingora 0.8's `ProxyHttp::fail_to_proxy` as the terminal failure boundary.
- Existing usage events and debug bundles without schema changes.
