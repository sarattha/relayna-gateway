# Issue 38 Policy Governance, Key Lifecycle, and Explainability

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md`. Product intent comes from GitHub issue #38,
`internal/design-manifesto.md`, and `internal/codex-roadmap-goals.md`.

## Purpose / Big Picture

Make Relayna Gateway a stronger policy decision point. After this change,
operators can create safer keys from presets, see lifecycle/risk metadata,
simulate a policy decision before issuing a key, and rely on deterministic
effective-policy behavior that combines global, project, team, key, route, and
model layers.

## Progress

- [x] (2026-05-23 13:00 +07) Confirmed current branch, clean worktree, and
      latest release/freeze baseline `v0.0.14`.
- [x] (2026-05-23 13:00 +07) Ran pre-edit
      `node tests/freeze-v0.0.14-perimeter.test.mjs`.
- [x] (2026-05-23 13:00 +07) Inspected current policy, admin key, proxy, and
      persistence surfaces.
- [x] (2026-05-23 16:20 +07) Added deterministic effective policy resolver,
      lifecycle/risk fields, policy versions, safe presets, persistence
      migration, focused core/API tests, and admin response fields.
- [x] (2026-05-23 16:20 +07) Added proxy request/response byte policy checks
      and stable `response_body_too_large` error code.
- [x] (2026-05-23 16:20 +07) Added `POST /admin-ui/admin/policy/simulate`
      and Admin UI controls for presets, lifecycle fields, risk limits, and
      simulation.
- [x] (2026-05-23 16:20 +07) Updated docs, Admin UI static tests, and freeze
      perimeter for additive migration and error code changes.
- [x] (2026-05-23 16:35 +07) Added successful-auth `last_used_at`
      updates, stale-key auto-disable before auth, daily request/token cap
      enforcement from usage rows, and per-request input/output/cost cap checks.
- [x] (2026-05-23 16:40 +07) Ran required verification:
      `node tests/freeze-v0.0.14-perimeter.test.mjs`,
      `node tests/admin-ui.test.mjs`, and
      `bash .codex/skills/code-change-verification/scripts/run.sh`.
- [x] (2026-05-23 17:05 +07) Added first-class inherited policy-layer Admin
      APIs/UI, context-aware runtime lookup for route/model layers, team-aware
      simulation, and unsaved policy-patch simulation.

## Surprises & Discoveries

- Observation: current policy is per-key only and already defaults to
  `/v1/chat/completions`, `/v1/responses`, LiteLLM, no streaming, and no tools.
  Evidence: `crates/gateway-core/src/policies.rs`.

- Observation: request body limits already exist at route/service level, but
  not as effective policy fields, and response limit uses the same route cap.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs` and
  `crates/gateway-core/src/routing.rs`.

- Observation: SQLx tuple decoding only supports tuples up to its implemented
  arity, so the widened policy read path uses `sqlx::query` plus named columns
  instead of a long `query_as` tuple.
  Evidence: focused `cargo check -p gateway-store --all-features` failure and
  fix.

- Observation: lifecycle metadata needed runtime wiring, not only response
  fields. Successful virtual-key authentication now updates `last_used_at`;
  stale-key auto-disable runs before returning a matching stored key.
  Evidence: `crates/gateway-core/src/auth.rs` and
  `crates/gateway-store/src/postgres.rs`.

## Decision Log

- Decision: Preserve released key policy fields and add new policy/lifecycle
  fields as nullable/additive columns.
  Rationale: `v0.0.14` is the compatibility boundary. Existing keys must keep
  the same defaults unless operators explicitly set stricter policy.
  Date/Author: 2026-05-23 / Codex.

- Decision: Implement policy inheritance as a framework-agnostic core resolver
  over plain policy layers, with the store persisting global/project/team/route/
  model layers and runtime lookup applying route/model layers when request
  context is available.
  Rationale: This keeps the resolver framework-agnostic while making inherited
  governance observable and operator-manageable now.
  Date/Author: 2026-05-23 / Codex.

- Decision: Use neutral defaults for policy-layer JSON.
  Rationale: Empty inherited layers should not accidentally narrow routes,
  providers, streaming, or tools; only explicit fields should tighten behavior.
  Date/Author: 2026-05-23 / Codex.

- Decision: Make safe presets an Admin API create-time field, not a separate
  key creation endpoint.
  Rationale: This is additive and keeps raw-key one-time return semantics on
  the existing `/admin-ui/admin/keys` route.
  Date/Author: 2026-05-23 / Codex.

- Decision: Add `response_body_too_large` as a new stable error code.
  Rationale: request byte caps already use `request_body_too_large`; response
  byte caps need a distinct structured error for simulator/proxy diagnostics.
  Date/Author: 2026-05-23 / Codex.

## Outcomes & Retrospective

Implemented the main Phase 2 policy governance surface: deterministic
effective-policy resolution, additive policy/lifecycle schema, safe key presets,
policy simulation, Admin UI controls, inherited policy-layer management, proxy
request/response limit enforcement, last-used/stale-key lifecycle behavior,
daily cap checks, documentation, and freeze perimeter updates.

## Context and Orientation

Policy types live in `crates/gateway-core/src/policies.rs`, admin key request
and response types in `crates/gateway-core/src/admin.rs`, proxy enforcement in
`crates/gateway-proxy/src/pingora_plane.rs`, and PostgreSQL persistence in
`crates/gateway-store/src/postgres.rs`. Admin UI is the static app under
`crates/gateway-api/src/static/admin-ui/`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. Touched freeze surfaces:
admin routes and response fields, PostgreSQL migrations, public error codes if
new size errors are needed, policy decisions, proxy request/response handling,
and Admin UI contracts. Compatibility impact should be additive: existing keys
continue to evaluate with existing defaults; stricter behavior applies only
when new policy fields or presets are configured.

## Plan of Work

Add core policy-layer and effective-policy types with deterministic merge
semantics. Extend key policy/admin types with lifecycle/risk and size fields.
Persist additive columns and read/write them through existing admin key APIs.
Use the effective policy in proxy checks, enforce request/response size fields,
add a simulator admin route that returns the auth, route, merge, guardrail,
rate-limit, budget, and final decision trace, expose safe presets in the Admin
UI, and document the behavior.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.0.14-perimeter.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance requires tests for policy merge edge cases, preset key creation,
simulator outputs, lifecycle metadata in admin responses, request size
enforcement, response size enforcement, freeze perimeter updates, and the full
verification stack.

## Idempotence and Recovery

Migrations must be additive and idempotent. If migration application fails,
fix the migration and rerun tests from a fresh database where possible. If the
simulator diverges from proxy behavior, move shared decision construction into
gateway-core instead of duplicating logic in API/proxy.

## Artifacts and Notes

Pre-edit freeze perimeter passed on 2026-05-23.

## Interfaces and Dependencies

New surfaces planned:

- `preset` on admin key creation.
- Lifecycle/risk fields in admin key policy/response metadata.
- Effective policy layer and simulation request/response types in
  gateway-core.
- `POST /admin-ui/admin/policy/simulate`.
- Admin UI controls for presets, lifecycle metadata, and simulation.
