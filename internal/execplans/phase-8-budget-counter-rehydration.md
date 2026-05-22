# Phase 8 Budget Counter Rehydration

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Make daily and monthly budget enforcement resilient to Redis loss without
turning Redis into a second durable database. Operators should be able to
restart Gateway pods, replace Redis, or recover from an empty Redis instance
without losing historical usage cost or silently allowing large budget
overspend.

After this phase, PostgreSQL `usage_events` remains the durable source of truth
for usage and estimated cost. Redis remains the fast budget enforcement cache.
Gateway rehydrates Redis budget counters from PostgreSQL for active keys and
keeps those counters reconciled enough that budget enforcement resumes from the
already-recorded spend after Redis recovery.

## Progress

- [x] (2026-05-10 00:00 +07) Draft Phase 8 ExecPlan for budget counter
      rehydration and Redis recovery behavior.
- [x] (2026-05-22 00:00 +07) Establish compatibility boundary for Redis budget key formats,
      PostgreSQL usage queries, startup behavior, and operator-visible recovery
      semantics.
- [x] (2026-05-22 00:00 +07) Add PostgreSQL usage-cost aggregation by key for current UTC day and
      current UTC month.
- [x] (2026-05-22 00:00 +07) Add Redis budget counter seed or set methods that preserve existing TTL
      semantics.
- [x] (2026-05-22 00:00 +07) Rehydrate budget counters on gateway startup after PostgreSQL and Redis
      connections are available.
- [x] (2026-05-22 00:00 +07) Add optional periodic budget reconciliation for active budgeted keys.
- [ ] Add tests covering empty Redis recovery, nonzero historical usage, and
      no-op behavior for keys without budget limits. Completed: focused unit
      coverage for UTC budget windows. Remaining: Redis/PostgreSQL integration
      coverage with disposable services.
- [x] (2026-05-22 00:00 +07) Run `$code-change-verification` and record results.

## Surprises & Discoveries

- Observation: Current usage cost history is durable because proxy completion
  writes `estimated_cost` to PostgreSQL `usage_events`.
  Evidence: `crates/gateway-store/src/postgres.rs` inserts
  `UsageEvent.estimated_cost_usd` into `usage_events.estimated_cost`.
- Observation: Current budget enforcement reads Redis counters only.
  Evidence: `crates/gateway-store/src/redis.rs` reads `budget:daily:*` and
  `budget:monthly:*` keys in `RedisControlState::check_budget`.

- Observation: Redis readiness must be checked even when no budgeted keys need
  seeding.
  Evidence: startup now calls `RedisReadiness::ready` before budget
  rehydration, so an empty seed set cannot hide an unavailable Redis instance.

## Decision Log

- Decision: Do not implement a generic periodic Redis dump to PostgreSQL.
  Rationale: PostgreSQL already owns durable usage events and cost history.
  Persisting all Redis counters would add write volume and schema complexity
  while preserving state that is mostly cache/window state.
  Date/Author: 2026-05-10 / Codex.

- Decision: Rehydrate Redis budget counters from PostgreSQL usage events.
  Rationale: Budget spend is business-critical enforcement state. PostgreSQL
  usage events are the durable ledger, while Redis is the low-latency cache used
  by the proxy request path.
  Date/Author: 2026-05-10 / Codex.

- Decision: Keep RPM and TPM rate-limit counters Redis-only by default.
  Rationale: Short-window rate limits can reset after Redis loss with lower
  business risk than budget counters. Persisting them would increase write
  volume and operational complexity.
  Date/Author: 2026-05-10 / Codex.

- Decision: Overwrite current daily and monthly Redis budget counters from the
  PostgreSQL ledger during startup and periodic reconciliation.
  Rationale: rehydration must converge after Redis loss or stale cache state,
  while reservation keys remain request-local and are not reconstructed from
  durable usage history.
  Date/Author: 2026-05-22 / Codex.

## Outcomes & Retrospective

Implemented startup and periodic budget counter rehydration. PostgreSQL now
computes current UTC day/month spend for active budgeted keys, Redis can seed
the existing budget keys with existing TTLs, and gateway startup checks Redis
readiness before rehydrating counters and starting proxy traffic. Remaining
gap: disposable Redis/PostgreSQL integration tests for full recovery behavior.

Verification on 2026-05-22: `node tests/freeze-v0.0.9-perimeter.test.mjs`,
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace --all-features`, and
`bash .codex/skills/code-change-verification/scripts/run.sh` all passed.

## Context and Orientation

Relayna Gateway currently splits durable and volatile state:

- PostgreSQL stores virtual keys, route policies, usage events, service
  registrations, and operator token hashes.
- Redis stores readiness checks, request rate-limit counters, and daily/monthly
  budget counters.
- `gateway-proxy` records a `UsageEvent` at terminal request completion and
  separately updates Redis budget spend when an estimated cost is available.
- `gateway-store` owns both PostgreSQL access and Redis control-state access.

The failure mode addressed by this phase is specific: if Redis restarts empty,
Gateway still has historical usage cost in PostgreSQL, but budget checks see
daily and monthly Redis counters as zero. That can allow a key to spend past its
configured daily or monthly budget until new Redis counters rebuild from future
traffic.

Key files and modules:

- `crates/gateway-core/src/budgets.rs`: budget key names, budget decisions, and
  `BudgetStore` trait.
- `crates/gateway-core/src/policies.rs`: `KeyPolicy` budget fields,
  `daily_budget_usd` and `monthly_budget_usd`.
- `crates/gateway-core/src/usage.rs`: `UsageEvent` and estimated cost parsing.
- `crates/gateway-store/src/postgres.rs`: durable `usage_events` inserts,
  usage summary queries, and policy lookup.
- `crates/gateway-store/src/redis.rs`: Redis budget counter read/write,
  reservation, reconciliation, and release behavior.
- `crates/gateway-api/src/main.rs`: startup wiring for PostgreSQL, Redis, Axum,
  and Pingora.
- `tests/` and crate-local tests: coverage for startup rehydration and
  budget-denial behavior after Redis loss.

Definitions:

- A virtual key is the Relayna-owned bearer credential presented by clients as
  `Authorization: Bearer rk_live_...`.
- A usage event is the durable PostgreSQL record of one gateway request,
  including key, project, route, provider, status, tokens, latency, and
  estimated cost.
- A budget counter is the Redis value used by request-time budget enforcement
  for one key and one UTC day or month.
- Rehydration means recomputing current day/month spend from PostgreSQL and
  writing the equivalent budget counters into Redis after Redis has lost state
  or after Gateway starts.

## Compatibility Boundary

Compatibility boundary: compare Redis budget key formats, PostgreSQL
`usage_events` reads, gateway startup behavior, and budget denial behavior
against the latest release tag before editing runtime code.

This phase should preserve existing public HTTP routes and response bodies. It
should not change the virtual key format, usage event write shape, or policy
fields. Redis key names should remain compatible:

- `budget:daily:{key_id}:{yyyyMMdd}`
- `budget:monthly:{key_id}:{yyyyMM}`
- `budget:reservation:{key_id}:{request_id}`

The behavior change is operationally additive: after Redis loss, budget checks
should resume from PostgreSQL-derived spend instead of starting from zero.

## Plan of Work

Add a PostgreSQL aggregation method that returns current UTC day and month
estimated spend by key. The query should sum only positive, non-null
`usage_events.estimated_cost` values. It should be scoped to keys with at least
one budget configured when practical, so startup does not scan unnecessary
rows.

Add a Redis method to seed budget counters for a key. The method should set the
current daily and monthly keys to PostgreSQL-derived amounts and apply the same
TTL policy used by `add_budget_spend`: roughly two days for the daily counter
and roughly sixty-two days for the monthly counter. Seeding must be idempotent.

Wire a startup rehydration step in `crates/gateway-api/src/main.rs` after
PostgreSQL and Redis clients are available and before the proxy begins serving
traffic. If rehydration fails because Redis or PostgreSQL is unavailable,
Gateway should fail closed or fail startup consistently with current readiness
expectations rather than serving budgeted traffic with zeroed counters.

Consider an optional periodic reconciliation task. It should run infrequently,
for example every few minutes, recompute active budgeted key spend from
PostgreSQL, and update Redis counters. This protects long-running pods after a
Redis failover that does not restart Gateway. The task must be cancellable via
the existing shutdown flow and must avoid high-cardinality logs.

Add tests that prove:

- Empty Redis plus existing PostgreSQL usage cost rehydrates counters.
- A key already over daily or monthly budget is denied after rehydration.
- A key under budget remains allowed after rehydration.
- Keys without budget limits do not require Redis budget counter seeding.
- Existing Redis reservations are not corrupted by periodic reconciliation.

## Concrete Steps

Use `$implementation-strategy` before editing runtime code because this phase
touches Redis key semantics, startup behavior, and budget enforcement.

Implementation commands:

    cd /Users/jobz/Works/relayna-gateway
    rg -n "BudgetStore|check_budget|add_budget_spend|usage_events|daily_budget" crates tests
    cargo test -p gateway-core budgets
    cargo test -p gateway-store

Final verification:

    cd /Users/jobz/Works/relayna-gateway
    bash .codex/skills/code-change-verification/scripts/run.sh

The verification script must run `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo test --workspace --all-features` successfully before marking Phase 8
implementation complete.

## Validation and Acceptance

Phase 8 is accepted when:

- PostgreSQL remains the durable source of truth for usage cost.
- Redis remains the low-latency budget enforcement cache.
- Restarting Gateway against an empty Redis instance restores current UTC day
  and month budget counters from PostgreSQL before proxy traffic is served.
- A virtual key whose PostgreSQL usage already exceeds its daily budget is
  rejected after Redis rehydration.
- A virtual key whose PostgreSQL usage already exceeds its monthly budget is
  rejected after Redis rehydration.
- Rate-limit counters remain Redis-only unless explicitly changed by a later
  plan.
- Existing `/admin/usage/*` summaries continue to read from PostgreSQL and
  preserve their response shapes.
- Logs and metrics expose whether rehydration succeeded, failed, and how many
  budgeted keys were seeded, without logging virtual keys, prompts, provider
  credentials, or raw request bodies.

## Idempotence and Recovery

Rehydration must be safe to run multiple times. Re-running it should converge
Redis counters to PostgreSQL-derived spend for the current UTC day and month.

If Redis is unavailable at startup, Gateway should not begin serving budgeted
proxy traffic with missing budget counters. If periodic reconciliation fails,
Gateway should log the failure and keep existing request-path budget checks
fail-closed according to current Redis error handling.

If PostgreSQL contains bad historical rows, such as null or negative estimated
costs, aggregation should ignore them. If PostgreSQL is restored from backup,
rehydration should reflect the restored durable ledger and operators should
understand that any lost `usage_events` cannot be reconstructed from Redis.

If stale Redis counters exist from a previous run, rehydration should overwrite
the current daily/monthly budget keys from PostgreSQL-derived totals rather
than incrementing them. Reservation keys should remain per-request and should
not be recreated from PostgreSQL.

## Artifacts and Notes

Current durable usage-cost write path:

    UsageEvent.estimated_cost_usd -> usage_events.estimated_cost

Current Redis budget keys:

    budget:daily:{key_id}:{yyyyMMdd}
    budget:monthly:{key_id}:{yyyyMM}
    budget:reservation:{key_id}:{request_id}

Example aggregation shape:

    SELECT key_id,
           SUM(estimated_cost) FILTER (WHERE created_at >= $day_start) AS daily_spend,
           SUM(estimated_cost) FILTER (WHERE created_at >= $month_start) AS monthly_spend
    FROM usage_events
    WHERE estimated_cost IS NOT NULL
      AND estimated_cost > 0
      AND created_at >= $month_start
    GROUP BY key_id;

This is illustrative. Final implementation should use SQLx and the repository's
existing query style.

## Interfaces and Dependencies

Expected implementation surfaces:

- Add a store-facing query type for budget spend snapshots, likely in
  `gateway-core` or `gateway-store`, containing `key_id`, `daily_spend_usd`,
  and `monthly_spend_usd`.
- Add a PostgreSQL method that aggregates usage cost for current UTC day and
  month, preferably scoped to keys with non-null budget limits.
- Add a Redis method that seeds or overwrites daily and monthly budget counters
  with TTLs matching the existing budget counter behavior.
- Add a startup orchestration function in `gateway-api` that runs after
  `PostgresStore::connect` and Redis client creation.
- Optional: add a background reconciliation task with a configurable interval
  only if the implementation remains simple and observable.

No new public route is required for Phase 8. A future operator endpoint to
manually trigger budget rehydration can be proposed separately if operations
need it.
