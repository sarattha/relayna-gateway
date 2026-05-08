# Phase 2 Policy, Virtual Keys, Rate Limit, and Budget

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Turn Relayna Gateway from a secure proxy into a control plane. Operators should
create, revoke, and inspect virtual keys with policy controls for routes,
models, providers, rate limits, streaming/tool permission, and daily or monthly
budget limits.

After this phase, a project can issue a key that can call only approved routes
and models, is limited across gateway replicas, and is rejected when policy or
budget says no.

## Progress

- [x] Confirm Phase 1 behavior and schemas are complete.
- [x] Establish the compatibility boundary against the latest release tag.
- [x] Add key policy persistence and models.
- [x] Add protected admin key APIs.
- [x] Add policy enforcement before upstream calls.
- [x] Add Redis-backed request-per-minute limits.
- [x] Add simple daily and monthly budget checks and spend updates.
- [x] Add usage query behavior for key/project views.
- [x] Add tests for admin, policy, rate-limit, budget, and usage queries.
- [x] Run `$code-change-verification` and record results.

## Surprises & Discoveries

- No `v*` release tags exist, so Phase 2 is implemented as an additive
  unreleased baseline rather than a compatibility shim.
- Pingora exposes a `proxy_upstream_filter` hook that lets policy, Redis RPM,
  and budget checks run after authentication and before upstream connection.
- Phase 2 can check seeded Redis budget counters before upstream calls, but
  accurate cost extraction remains deferred to Phase 3 as planned.

## Decision Log

- Decision: Use explicit deny responses for policy, rate-limit, and budget
  failures instead of falling through to provider errors.
  Rationale: Gateway owns policy and budget decisions, so clients and Relayna
  Studio need stable gateway-originated outcomes.
  Date/Author: 2026-05-08 / Codex.
- Decision: Keep Pingora as the only owner of `/v1/*` generation traffic and
  use Axum only for health/readiness/admin routes.
  Rationale: This preserves the Phase 1 architecture boundary and avoids
  splitting proxy behavior across frameworks.
  Date/Author: 2026-05-09 / Codex.
- Decision: Persist default Phase 2 policy rows for admin-created keys and
  treat missing policy rows as default Phase 1-compatible generation policy.
  Rationale: Existing seeded Phase 1 keys keep working during unreleased local
  development while admin-created Phase 2 keys have durable policy state.
  Date/Author: 2026-05-09 / Codex.

## Outcomes & Retrospective

Implemented the first Phase 2 runtime pass: admin key lifecycle routes,
one-time raw key creation, policy persistence, route/model/provider/feature
policy checks, Redis RPM counters, Redis budget checks, stable denial errors,
and key/project usage summaries.

Verification passed with:

    bash .codex/skills/code-change-verification/scripts/run.sh

Remaining validation before Phase 2 should be considered fully release-ready:
add broader database/Redis black-box coverage when local service fixtures are
available.

## Context and Orientation

Phase 1 validates a virtual key and forwards chat traffic. Phase 2 adds
operator-controlled governance.

Important terms:

- Key policy: durable settings that define allowed routes, models, providers,
  request/token limits, budgets, and feature permissions for a key.
- Rate limit: a Redis-backed request-per-minute decision shared across gateway
  replicas.
- Budget counter: spend state used to reject requests that exceed daily or
  monthly policy.
- Admin API: protected internal control-plane routes for key lifecycle and
  usage inspection.

Expected areas:

- `crates/gateway-api/`: admin routes and auth middleware for internal admin
  access.
- `crates/gateway-core/`: policy decisions, rate-limit decisions, budget
  decisions, admin request/response types, and error taxonomy.
- `crates/gateway-store/`: key policy schema, PostgreSQL key mutations, usage
  summaries, Redis counters, and budget state.
- `crates/gateway-proxy/`: enforcement hooks before upstream calls.
- `tests/`: admin API, policy denial, Redis counter, budget, and usage query
  behavior tests.

## Compatibility Boundary

Compatibility boundary: compare against the latest release tag before editing
Phase 1 public routes, error responses, environment variables, schemas, Redis
state, or usage event fields.

Phase 2 adds compatibility-sensitive surfaces: admin routes, key policy schema,
policy denial errors, rate-limit errors, budget errors, Redis request counter
keys, budget counter keys, and usage query response shapes.

## Plan of Work

Add a key policy table and store models matching the manifesto policy fields:
allowed routes, models, providers, request/token limits, daily and monthly
budget limits, streaming permission, and tool permission.

Add admin routes for creating, reading, updating, revoking, disabling, and
inspecting usage for keys. Protect admin routes with an internal admin
credential. Return a raw virtual key only once at creation, persist only its
hash and prefix, and redact all sensitive material from logs.

Add framework-agnostic policy evaluation in gateway core. Enforce route, model,
provider, passthrough, streaming, and tool decisions before proxying. Return a
stable `policy_denied` error for denials.

Add Redis request-per-minute counters. Increment counters atomically, set
expirations, reject over-limit requests with `rate_limit_exceeded`, and include
retry hints when available.

Add budget checks that load policy limits, compare current spend from Redis
and/or PostgreSQL usage state, reject over-budget requests before upstream
calls, and update spend after requests.

Add usage query behavior by key and project for admin inspection.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo test -p gateway-core
    cargo test -p gateway-store
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Use focused Redis and store tests while iterating, then run the full stack.

## Validation and Acceptance

Phase 2 is accepted when:

- Operators can create, inspect, update, revoke, and disable keys through
  protected admin APIs.
- Raw virtual keys are returned once and are never persisted or logged.
- Keys can be restricted by route, model, provider, streaming, and tool use.
- Request-per-minute limits work across multiple gateway instances or a
  concurrency simulation using shared Redis.
- Daily and monthly budget checks reject over-budget requests.
- Usage can be queried by key and project.

Required tests:

- Unit tests for policy allow/deny decisions, admin key hashing, rate-limit
  decisions, and budget decisions.
- Integration tests for admin create/revoke flows, denied routes, denied
  models, Redis rate limiting, budget rejection, and usage queries.
- Negative tests proving admin APIs fail closed without valid internal admin
  credentials.

## Idempotence and Recovery

Admin create tests should use isolated test projects or database transactions
so reruns do not conflict on unique key prefixes. If local Redis counters
become stale, clear only local test keys with the configured test prefix.

Budget tests should use deterministic key IDs and isolated time windows. If a
test fails after incrementing spend, delete only that test key's local Redis
budget keys or reset the local test database.

Shared migrations must be forward-only after review. If a migration is wrong
before it is shared, replace it in the same branch and document the correction
in this plan's Decision Log.

## Artifacts and Notes

Example policy intent:

    Key A can call chat completions only, use gpt-4o-mini only, make 100
    requests per minute, spend at most $50 per month, and cannot use streaming
    or tools unless explicitly allowed.

Expected denial families:

    policy_denied
    rate_limit_exceeded
    budget_exceeded

## Interfaces and Dependencies

Phase 2 depends on completed Phase 1 authentication, proxying, usage inserts,
PostgreSQL connectivity, and Redis connectivity.

The end state includes admin key routes, key policy persistence, policy
decision logic, Redis rate-limit counters, budget checks, usage summary queries,
and stable denial error shapes.
