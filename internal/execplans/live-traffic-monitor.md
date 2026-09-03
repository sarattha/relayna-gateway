# Live traffic monitor and failure diagnostics

This living ExecPlan follows `PLANS.md` in `/Users/jobz/Works/relayna-gateway`.

## Purpose / Big Picture

Operators can watch requests arriving at the Pingora gateway, inspect upstream
attempts and failure stages, and search completed diagnostics in Monitor → Traffic.
Unauthenticated requests and database failures must remain observable. Client
status is separate from stream outcome. No bodies, credentials, query strings, or
raw transport error messages are captured.

## Progress

- [x] (2026-09-03) Reviewed implementation, design manifesto and UI guidance;
  created `codex/live-traffic-monitor`; saved pre-existing Cargo.lock diff outside repo.
- [x] (2026-09-03) Implemented bounded lifecycle capture, failure classification and persistence.
- [x] (2026-09-03) Added authorized live/history APIs and responsive Traffic page.
- [x] (2026-09-03) Added passing real-process regression coverage; Computer Use confirmed desktop table, details, pause/resume, disconnect, mobile layout, saved history across restart and request-ID filtering.
- [x] (2026-09-03) Bumped to 0.1.31, updated changelog/docs, passed the full mandatory verification stack and workspace build on the committed runtime snapshot.
- [x] (2026-09-03) Opened PR #110. The first Codex review completed at 16:34:58 UTC with a thumbs-up and no findings; there were no threads requiring fixes, replies or resolution.

## Surprises & Discoveries

The current final proxy callback skips requests without a route/key. Usage and
debug writes discard errors. Incoming request logs are emitted only after route
configuration succeeds. Early terminal usage is saved before the error response.
The proxy and control API run in the same process on separate runtimes.
Body admission errors previously sent 503 but left the upstream exchange running,
delaying terminal diagnostics. Returning an error ends the exchange; HTTP/1 error
responses must disable Pingora session reuse (which generates Connection: close)
to prevent reuse races; setting only the header is overwritten by Pingora. Existing
migrations inspect constraint names globally, so process regressions use isolated
databases rather than isolated schemas (test PostgreSQL role needs CREATEDB).

## Decision Log

- 2026-09-03: Compatibility boundary is v0.1.30. Preserve public upstream
  bodies/status semantics; add correlation headers to gateway errors. Add nullable
  usage diagnostics and a separate diagnostic table with nullable identity, since
  unauthenticated traffic is not billable usage. Existing rows remain readable.
- 2026-09-03: Use a bounded process-local journal for SSE and PostgreSQL for
  completed history across instances. Explicitly label live instance scope, boot
  identity, retention gaps and reconnection. Never depend on Redis for diagnostics
  of Redis failure. Structured terminal logs survive failed database writes.
- 2026-09-03: Keep authorization on new admin APIs using existing usage-read
  scope; periodically expire live connections for reauthorization. Diagnostics
  are metadata only and do not change streaming body flow.

## Outcomes & Retrospective

Implementation and local verification are complete. The committed runtime was
verified in an isolated worktree so the pre-existing local Cargo.lock downgrade
could remain untouched and excluded from the PR. All GitHub checks passed for
the reviewed commit. The first Codex review reported no findings. A subsequent
test-only update covers Redis minute-boundary timing and explicitly checks live
records and process logs for client/operator/provider credential leakage.

## Context and Orientation

`crates/gateway-proxy/src/pingora_plane.rs` owns the request callbacks and upstream
attempts. `gateway-core` owns plain lifecycle types and usage metadata. The shared
monitor retains bounded snapshots; `gateway-store` persists terminal diagnostics.
`gateway-api/src/app.rs` authorizes admin operations and serves SSE/history. UI
source is `crates/gateway-api/admin-ui/`; regenerate static files with npm build.

## Plan of Work

First add lifecycle types and additive PostgreSQL migration; instrument arrival,
authentication, policy, limits, connection, forwarding preparation, response and
terminal outcome. Preserve known error categories in usage and report every
failed write through structured logs and the live record. Add history filtering
and a one-way SSE feed, then implement the Traffic view using existing components.
Validate early rejection, capacity/control-state 503, upstream 503/transport
failure, stream abort after 200, retention gaps, authorization and redaction.

## Concrete Steps

Run focused cargo tests during implementation. Build UI and run npm tests. Use
isolated local PostgreSQL/Redis and synthetic upstream responses for regression
and Computer Use QA. Run `bash .codex/skills/code-change-verification/scripts/run.sh`
and `cargo build --workspace --all-features`, then validate release metadata.
Push only task changes and create a review-ready PR using the repository template.

## Validation and Acceptance

Every HTTP request that reaches the proxy lifecycle has an arrival record and
terminal diagnostic even without route/key. Every failed persistence operation
has a safe warning and visible recording state. Upstream attempts have distinct
outcomes. A streamed response can show HTTP 200 and failure simultaneously.
Unauthorized callers cannot read diagnostics. Reconnects disclose missing data
and instance changes. History survives process restarts when database writes
succeed. Live retention and database outage limitations are documented.

## Idempotence and Recovery

Migration is additive and applied by the existing SQLx migrator. Repeated terminal
diagnostic writes use an internal unique ID. Do not roll back unrelated lockfile
changes. Test services/data remain isolated from production. On failed checks,
fix the failure and rerun the complete mandatory script.

## Artifacts and Notes

Computer Use results are recorded in `internal/test-reports/live-traffic-monitor/README.md`.
Local screenshots are ignored by the repository's existing policy. Record final
verification results, PR URL and review outcome here as completed.

The full verification script passed: fmt, Clippy, cargo test, audit, deny,
machete, Nextest (325 passed, zero skipped), Trivy, Gitleaks and Semgrep.
An earlier Nextest run reported one non-failing process-exit warning in the existing
`admin_guardrail_catalog_requires_operator_token` test; the final rerun on `b8e82b6`
passed all 325 tests without that warning. The audit command used
a clean `--db` path because the default local advisory cache contained an
untracked duplicate advisory; exclusions were not expanded. Workspace build,
Admin UI build/tests and release metadata validation passed as well.

PR: https://github.com/sarattha/relayna-gateway/pull/110. Codex reviewed `a6654e2`;
GitHub's review-thread query returned no threads and the Codex connector added a
thumbs-up. The review follow-up automation was paused after this result.

## Interfaces and Dependencies

Add `traffic` core types, `TrafficStore` for terminal records/history, and
`/admin-ui/admin/traffic/live` plus `/admin-ui/admin/traffic/history`. Journal and
timeline bounds are constants, no unbounded channels. Usage gets optional
failure stage/code/source and separate outcome, upstream status and instance ID.
