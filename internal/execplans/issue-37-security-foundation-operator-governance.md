# Issue 37 Security Foundation and Operator Governance

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md`. Product intent comes from GitHub issue #37,
`internal/design-manifesto.md`, and `internal/codex-roadmap-goals.md`.

## Purpose / Big Picture

Strengthen Relayna Gateway's operator security baseline. After this change,
admin APIs require explicit operator scopes, admin mutations leave append-only
audit evidence, worker token checks avoid direct string equality, and public
error responses retain stable machine-readable codes and request IDs.

## Progress

- [x] (2026-05-22 23:00 +07) Confirmed latest release tag and freeze baseline
      are `v0.0.14`.
- [x] (2026-05-22 23:00 +07) Ran the v0.0.14 freeze perimeter test before
      edits.
- [x] (2026-05-22 23:00 +07) Read implementation strategy, production freeze,
      code verification, design, and planning guidance.
- [x] (2026-05-22 23:00 +07) Add scoped operator authorization and tests.
- [x] (2026-05-22 23:00 +07) Add append-only audit event persistence, API,
      and tests.
- [x] (2026-05-22 23:00 +07) Add constant-time worker token verification and
      tests.
- [x] (2026-05-22 23:00 +07) Update docs and freeze perimeter.
- [x] (2026-05-22 23:00 +07) Run required verification.

## Surprises & Discoveries

- Observation: structured error bodies already include `code`, `message`, and
  `request_id`.
  Evidence: `crates/gateway-core/src/errors.rs` defines `ErrorEnvelope`.

- Observation: operator tokens already use Argon2 hashes and a one-active-token
  index.
  Evidence:
  `crates/gateway-store/migrations/20260510000200_phase_7_operator_tokens.sql`.

- Observation: `mkdocs` is not installed in this local shell.
  Evidence: `mkdocs build --strict` returned `zsh:1: command not found:
  mkdocs`.

## Decision Log

- Decision: Use additive compatibility against `v0.0.14`.
  Rationale: This phase touches released admin auth, routes, error codes,
  PostgreSQL schema, and proxy auth. Existing route paths, `op_live_` token
  format, and error envelope fields should remain valid.
  Date/Author: 2026-05-22 / Codex.

- Decision: Store operator `roles` and `scopes` on `operator_tokens`, with
  existing/bootstrap tokens defaulting to owner plus wildcard scope.
  Rationale: This binds tokens to governance metadata without breaking existing
  single-operator deployments.
  Date/Author: 2026-05-22 / Codex.

- Decision: Add an audit read endpoint rather than changing existing admin
  response shapes.
  Rationale: The audit surface is additive and lets Relayna Studio consume
  audit history without altering existing mutation responses.
  Date/Author: 2026-05-22 / Codex.

## Outcomes & Retrospective

Implemented. Operator tokens now carry roles and scopes, admin authorization can
deny valid tokens that lack the required scope, audit events are persisted and
queryable, worker token checks use constant-time comparison, and the freeze
perimeter pins the additive route, error code, and migration. Documentation now
describes operator scopes, audit events, and worker token handling.

Required verification passed: `node tests/freeze-v0.0.14-perimeter.test.mjs`,
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace --all-features`, and
`bash .codex/skills/code-change-verification/scripts/run.sh`. The admin UI
static test also passed. Docs build was not run because `mkdocs` is not
installed locally.

## Context and Orientation

Admin APIs live in `crates/gateway-api/src/app.rs` under `/admin-ui/admin/*`.
Operator token types live in `crates/gateway-core/src/operators.rs`, with
PostgreSQL persistence in `crates/gateway-store/src/postgres.rs`. Worker token
trust for Relayna workers is in
`crates/gateway-proxy/src/pingora_plane.rs`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. Touched freeze surfaces:
admin route inventory, operator token auth behavior, public error codes,
PostgreSQL migration inventory, proxy worker-token behavior, docs, and tests.
Compatibility impact is additive: preserve existing admin routes, token
prefixes, and error envelope fields; add scoped denial errors, audit events,
and a new audit read route.

## Plan of Work

Add operator role/scope types and have token verification return an operator
authorization context. Add an additive migration for operator roles/scopes and
`audit_events`. Replace admin checks with scope-aware checks. Record audit
events for admin mutations with actor token ID, action, target type, target ID,
before/after JSON, request ID, IP, user agent, and timestamp. Add an audit read
endpoint protected by an auditor scope. Replace worker token direct equality
with constant-time comparison. Update docs and freeze perimeter tests.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.0.14-perimeter.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance requires tests proving allowed and denied admin scopes, audit
insertion and reads, worker token exact/missing/malformed/mismatch cases,
stable structured error codes, and freeze perimeter updates for intentional
additive surfaces.

## Idempotence and Recovery

The migration must use `IF NOT EXISTS` or additive `ALTER TABLE` statements so
startup migration retries are safe. Audit insertion failures should fail the
admin mutation only when persistence is unavailable, not leak secrets. If
verification fails, fix the failure and rerun the full relevant stack.

## Artifacts and Notes

Pre-edit freeze test passed on 2026-05-22.

## Interfaces and Dependencies

New public/admin concepts:

- Operator role strings such as `owner`, `admin`, `security_admin`,
  `key_manager`, `billing_viewer`, `usage_viewer`, `service_manager`,
  `guardrail_manager`, and `read_only_auditor`.
- Operator scope strings such as `keys:create`, `keys:disable`,
  `keys:rotate`, `policies:update`, `guardrails:update`, `usage:read`,
  `usage:export`, `providers:update`, `services:update`, `settings:update`,
  and `operators:manage`.
- Audit event rows for admin mutation and audit export/read workflows.
