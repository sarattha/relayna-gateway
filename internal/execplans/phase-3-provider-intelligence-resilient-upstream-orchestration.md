# Phase 3 Provider Intelligence and Resilient Upstream Orchestration

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Transform Relayna Gateway from a static proxy into an intelligent provider
routing and resilience control plane. After this work, gateway operators can
define provider selection constraints, inspect persisted health and circuit
state, retrieve redacted request debug bundles by request ID, and manage service
registry import snapshots with preview, activation, history, and rollback.

## Progress

- [x] (2026-05-23 12:00 +07) Read `internal/design-manifesto.md`, `PLANS.md`,
      `$implementation-strategy`, `$production-freeze-guard`, and
      `$code-change-verification`.
- [x] (2026-05-23 12:00 +07) Confirm latest release tag is `v0.0.14` and run
      the v0.0.14 freeze perimeter before editing.
- [x] (2026-05-23 12:00 +07) Add core routing strategy, provider health,
      circuit breaker, fallback, debug bundle, and import snapshot models.
- [x] (2026-05-23 12:00 +07) Add PostgreSQL persistence for provider health
      state, debug bundles, and service registry snapshots.
- [x] (2026-05-23 12:00 +07) Add admin APIs for provider health state, debug
      bundles, service import preview/activation/version history/rollback.
- [x] (2026-05-23 12:00 +07) Add proxy selection telemetry and redacted debug
      bundle capture.
- [x] (2026-05-23 12:00 +07) Add tests and docs for routing, fallback,
      circuits, debug redaction, and import rollback.
- [x] (2026-05-23 12:00 +07) Run required verification.

## Surprises & Discoveries

- Observation: Existing code already has direct OpenAI-compatible fallback to
  LiteLLM for retry-safe status and proxy errors.
  Evidence: `RelaynaPingoraProxy::activate_provider_fallback` and tests in
  `crates/gateway-proxy/src/pingora_plane.rs`.

- Observation: Provider health currently exists as usage-derived aggregates,
  not durable operator-managed health and circuit state.
  Evidence: `UsageQueryStore::provider_health` groups `usage_events` by
  provider or service name.

- Observation: Service registry import and sync already preserve local runtime
  fields but do not expose versioned snapshots or rollback.
  Evidence: `AdminServiceStore::import_studio_service` and
  `sync_studio_service` both call `upsert_studio_service`.

## Decision Log

- Decision: Use `v0.0.14` as the compatibility boundary and make public route,
  schema, and telemetry changes additive.
  Rationale: `AGENTS.md` defines `v0.0.14` as the production freeze baseline.
  Existing proxy paths and credential handling must remain stable.
  Date/Author: 2026-05-23 / Codex.

- Decision: Keep routing intelligence in `gateway-core` as framework-agnostic
  types and decisions, then let proxy/API/store crates persist or apply those
  decisions.
  Rationale: This follows package ownership and keeps Axum/Pingora specifics out
  of the core decision logic.
  Date/Author: 2026-05-23 / Codex.

- Decision: Capture redacted debug bundles with hashes and decision traces, not
  request or response bodies.
  Rationale: Acceptance requires operator debugging without exposing secrets or
  full prompts.
  Date/Author: 2026-05-23 / Codex.

## Outcomes & Retrospective

Implemented. Core provider routing strategies, fallback policy, circuit state,
provider health state, debug bundle, and service import snapshot models are in
place. PostgreSQL persistence, additive admin APIs, proxy debug bundle capture,
active health check execution, admin portal controls, and provider
selection/fallback/circuit telemetry were added. Docs now cover routing
strategies, fallback, circuit behavior, provider health fields, debug bundle
redaction, and service import rollback.

Verification on 2026-05-23: `node tests/admin-ui.test.mjs`,
`node tests/freeze-v0.0.14-perimeter.test.mjs`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace --all-features`, and
`bash .codex/skills/code-change-verification/scripts/run.sh` all passed.

## Context and Orientation

Relayna Gateway accepts Relayna virtual keys, evaluates policy, selects an
upstream provider or internal service, proxies OpenAI-compatible requests,
records usage, and exposes operator controls through `/admin-ui/admin/*`.

Important paths:

- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/routing.rs` owns
  existing route resolution and retry-safe status classification.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`
  owns Pingora proxy execution, upstream credential injection, fallback, and
  usage logging.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-store/src/postgres.rs` owns
  PostgreSQL persistence for admin and usage data.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs` owns Axum
  admin routes and operator authorization.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-telemetry/src/lib.rs` owns
  Prometheus counters and redaction helpers.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. This work touches public
admin APIs, PostgreSQL migrations, provider routing, proxy fallback behavior,
telemetry fields, and debug data. All changes must be additive. Existing proxy
route paths, error codes, virtual key behavior, and upstream credential
stripping/injection must remain stable.

Touched freeze surfaces:

- Public admin route inventory: additive routes only, reflected in freeze tests.
- PostgreSQL migration inventory: additive tables only.
- Provider proxy behavior: fallback remains limited to retry-safe failures.
- Telemetry: additive metrics only with bounded labels.
- Secret handling: debug bundles persist hashes and redacted traces only.

## Plan of Work

Add a new `provider_intelligence` core module with routing strategies
(`priority`, `weighted`, `least_latency`, `least_cost`, `health_aware`,
`budget_aware`, `region_affinity`, and `capability_aware`), circuit states,
fallback policy, health snapshots, provider candidates, and redacted debug
bundle/import snapshot structs. Unit tests should prove strategy ordering,
constraint filtering, circuit exclusion, half-open recovery selection, and
retry-safe fallback classification.

Add PostgreSQL tables for provider health state, request debug bundles, and
service registry snapshots. Implement store traits that read/write these
records and reuse existing service import validation/activation paths.

Expose additive admin endpoints under `/admin-ui/admin/provider-health/state`,
`/admin-ui/admin/debug-bundles/{request_id}`, and
`/admin-ui/admin/services/import/*`. Require existing operator scopes:
`SCOPE_USAGE_READ` for health/debug reads and `SCOPE_SERVICES_UPDATE` for
service import mutations.

Enhance the proxy to create provider selection/fallback trace entries and store
a redacted debug bundle during terminal logging. Record telemetry counters for
provider selections, fallbacks, and circuit transitions.

Update docs for routing strategies, fallback policy, circuit breaker behavior,
provider health fields, debug bundle redaction, and service import rollback.

## Concrete Steps

Commands run from `/Users/jobz/Works/relayna-gateway`:

    node tests/freeze-v0.0.14-perimeter.test.mjs
    cargo test -p gateway-core
    cargo test -p gateway-proxy
    cargo test -p gateway-store
    cargo test -p gateway-api
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance requires:

- Core routing tests prove each requested strategy and constraint.
- Proxy tests prove retry-safe fallback behavior and debug bundle redaction.
- Store/API tests prove persisted provider health, debug bundle lookup, service
  import preview/activation/history, and rollback.
- Freeze perimeter test passes with documented additive route and migration
  changes.
- Full Rust workspace formatting, clippy, and tests pass.

## Idempotence and Recovery

Migrations use `CREATE TABLE IF NOT EXISTS` and additive indexes. API operations
are idempotent where practical: preview does not mutate runtime services,
activation writes a snapshot version, and rollback activates an existing
snapshot version. If tests fail after a partial migration, rerun against a clean
test database or reapply the idempotent migration before retrying.

## Artifacts and Notes

Initial freeze perimeter before edits passed for `v0.0.14` on 2026-05-23.

## Interfaces and Dependencies

New core traits should remain async and object-safe so `PostgresStore`,
`Arc<PostgresStore>`, and API mock stores can implement them. New admin response
shapes must serialize with `snake_case` fields and avoid raw credentials,
request bodies, response bodies, and prompt text.
