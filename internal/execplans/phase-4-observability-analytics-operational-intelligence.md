# Phase 4 Observability, Analytics, and Operational Intelligence

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators and Relayna Studio need deeper visibility into gateway traffic, cost,
latency, errors, denials, guardrails, providers, policy decisions, and traces.
After this change the gateway exposes production-safe Prometheus metrics,
trace-aware usage/debug records, and analytics-ready admin APIs protected by
Phase 1 operator scopes.

## Progress

- [x] (2026-05-23 00:00Z) Read repository contributor rules, design manifesto,
  implementation strategy, production freeze guard, and verification skill.
- [x] (2026-05-23 00:00Z) Established freeze baseline `v0.0.14` and ran the
  freeze perimeter test before edits; it passed.
- [x] (2026-05-23 00:00Z) Implemented bounded telemetry metrics and tracing
  helpers.
- [x] (2026-05-23 00:00Z) Added additive usage/debug trace columns, indexes,
  query filters, and
  analytics response fields.
- [x] (2026-05-23 00:00Z) Expanded admin usage UI views without weakening
  operator RBAC.
- [x] (2026-05-23 00:00Z) Updated docs for metrics, tracing, analytics
  filters, and cardinality.
- [x] (2026-05-23 00:00Z) Ran required verification and updated this plan
  with outcomes.

## Surprises & Discoveries

- Observation: The current proxy already validates and propagates `traceparent`
  headers to upstream requests.
  Evidence: `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`
  stores `ctx.traceparent` and inserts it into upstream headers.
- Observation: Existing usage APIs already enforce `usage:read` and
  `usage:export` scopes.
  Evidence: `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs`
  routes usage reads through `admin_query(..., SCOPE_USAGE_READ, ...)` and
  exports through `SCOPE_USAGE_EXPORT`.

## Decision Log

- Decision: Treat Phase 4 as additive against the released `v0.0.14` perimeter.
  Preserve existing route names and response meanings, add fields and indexes
  rather than changing existing semantics.
  Rationale: The requested goal expands observability and analytics; no
  existing client-visible behavior needs removal.
  Date/Author: 2026-05-23, Codex.
- Decision: Keep metric labels bounded to route, provider, status class,
  decision/reason class, circuit state, stream mode, and guardrail metadata;
  never use request IDs, raw keys, prompt text, raw service routes, or
  trace IDs as metric labels.
  Rationale: Prometheus metrics must stay production-safe.
  Date/Author: 2026-05-23, Codex.

## Outcomes & Retrospective

Implemented additive Phase 4 observability and analytics support. The gateway
now emits bounded Prometheus metric families for request/upstream/guardrail
duration, first-token latency, active requests/streams, auth failures, denials,
fallbacks, and circuit states. The proxy preserves `traceparent`, derives trace
IDs for usage/debug records, and records trace-aware debug bundles without raw
prompt or credential data. Admin usage analytics gained run ID, trace ID, and
minimum-cost filters, expensive request and denial summary fields, CSV trace ID
export, and an RBAC-protected unused keys endpoint. The embedded admin UI shows
summary cards plus project, key, service, provider, model, and unused-key views.

Compatibility impact is additive against `v0.0.14`: one new admin route, one
new migration, additive response fields, and additive CSV column. No existing
route meaning, error code, Redis key format, or provider proxy semantics was
removed or renamed.

Verification passed:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `node tests/admin-ui.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `bash .codex/skills/code-change-verification/scripts/run.sh`

## Context and Orientation

Relayna Gateway is a Rust gateway for AI traffic. The Pingora proxy records
usage events for successful and failed LLM/provider requests. The Axum control
API exposes admin routes under `/admin-ui/admin/*` and static admin UI assets.
PostgreSQL stores `usage_events`, `guardrail_execution_events`, provider health
state, and request debug bundles. The `gateway-telemetry` crate currently
renders Prometheus text manually and initializes JSON tracing.

Key files:

- `/Users/jobz/Works/relayna-gateway/crates/gateway-telemetry/src/lib.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/usage.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/observability.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-store/src/postgres.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/app.js`
- `/Users/jobz/Works/relayna-gateway/docs/operations.md`
- `/Users/jobz/Works/relayna-gateway/docs/admin-portal.md`
- `/Users/jobz/Works/relayna-gateway/tests/freeze-v0.0.14-perimeter.test.mjs`

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. The implementation is
additive: new metrics, spans, query filters, fields, and indexes are added
without changing existing API response meanings, route names, status codes,
Redis key formats, or provider proxy semantics. The production freeze perimeter
test must be updated only for additive migration inventory changes.

Touched freeze surfaces: telemetry fields and metric labels, PostgreSQL
migrations/indexes/usage fields, admin API response shapes, admin UI endpoint
usage, and docs.

## Plan of Work

Add telemetry helpers in `gateway-telemetry` for bounded counters, gauges, and
histograms. Instrument proxy request phases with tracing spans and record
request, upstream, guardrail, and first-token latency. Preserve traceparent and
extract the trace ID into usage/debug records.

Add an additive PostgreSQL migration for `usage_events.trace_id`,
`request_debug_bundles.trace_id`, and analytics indexes. Extend usage query
filters and summaries to include run IDs, denials, guardrail blocks, fallback
rate, expensive requests, unused keys, and trace IDs in exports.

Expose new admin usage breakdowns through existing RBAC-protected routes and
render the admin Usage view with more actionable analytics. Update docs and
freeze tests. Finish with the required verification stack.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.0.14-perimeter.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance requires low-cardinality Prometheus output with the new metric
families, trace-aware usage/debug records, RBAC-protected analytics APIs and
exports, admin UI analytics cards/tables, documentation of metric labels and
filters, and all required verification commands passing.

## Idempotence and Recovery

SQL migrations are additive and use `IF NOT EXISTS` for columns and indexes
where possible. Metrics are in-process counters and reset on process restart.
Failed verification can be rerun after fixes. If a migration fails locally,
rerun from a clean test database or inspect SQLx migration history before
retrying.

## Artifacts and Notes

Pre-edit freeze perimeter result: all checks passed on 2026-05-23.

## Interfaces and Dependencies

Metrics must avoid high-cardinality labels. Usage APIs continue to use
`usage:read`; exports continue to use `usage:export`. Trace IDs are derived from
valid W3C `traceparent` headers and are stored as the 32-hex-character trace-id
segment.
