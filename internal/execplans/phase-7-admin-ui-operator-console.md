# Phase 7 Admin UI and Operator Console

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Add a bundled operator console for Relayna Gateway. Operators should be able to
sign in with a gateway-owned operator token, manage virtual keys and service
registrations, inspect usage and health, and rotate the operator token without
running raw curl commands.

Phase 7 also replaces the unreleased environment-only `GATEWAY_ADMIN_TOKEN`
workflow with database-backed operator tokens. On first startup, Gateway
generates one random operator token, stores only its Argon2 hash and lookup
prefix in PostgreSQL, and prints the raw token once.

## Progress

- [x] Establish compatibility boundary for admin auth, operator token storage,
      bundled UI routes, and operator-facing API behavior.
- [x] Add additive PostgreSQL schema for operator tokens.
- [x] Add operator token generation, hashing, prefix lookup, verification, and
      rotation contracts.
- [x] Bootstrap one operator token on first startup when no active token exists.
- [x] Replace environment-token admin middleware with database-backed operator
      token verification.
- [x] Add authenticated operator token rotation endpoint.
- [x] Serve bundled Admin UI assets from the control-plane listener.
- [x] Add UI screens for overview, keys, services, usage, health, sign-in,
      sign-out, and token rotation.
- [x] Add tests for operator token primitives, admin API auth, token rotation,
      static UI serving, and one-time raw token behavior.
- [x] Run `$code-change-verification` and record results.

## Surprises & Discoveries

- No existing frontend package or build pipeline exists in this repository, so
  Phase 7 uses static HTML/CSS/JS assets embedded with `include_str!`.
- No `v*` release tag exists locally, so the `GATEWAY_ADMIN_TOKEN` requirement
  is treated as unreleased branch-local behavior and replaced directly.
- Real-container testing verified startup bootstrap, database-backed admin
  auth, Redis-backed rate limiting, service passthrough, usage persistence,
  token rotation, and bundled UI rendering with the new operator token model.

## Decision Log

- Decision: Bundle the operator UI in `gateway-api` and serve it from
  `/admin-ui`.
  Rationale: The gateway already owns the protected admin APIs and control
  listener, and the MVP does not need a separate frontend deployment.
  Date/Author: 2026-05-10 / Codex.

- Decision: Use database-backed operator bearer tokens instead of the
  environment admin token.
  Rationale: Operators need a persistent sign-in token that survives restarts
  without storing raw token material in configuration or PostgreSQL.
  Date/Author: 2026-05-10 / Codex.

- Decision: Generate one bootstrap token only when no active operator token
  exists, print it once, and store only hash plus lookup prefix.
  Rationale: This provides first-start usability while preserving one-way
  secret handling. Lost-token recovery is a manual database operation to revoke
  or remove active operator tokens before restart.
  Date/Author: 2026-05-10 / Codex.

- Decision: Keep the Admin UI static for MVP.
  Rationale: Static assets avoid adding Node tooling, package management, and
  frontend build configuration to the Rust gateway workspace for this phase.
  Date/Author: 2026-05-10 / Codex.

## Outcomes & Retrospective

Implemented the first Phase 7 pass: operator token migration, core token
material and verification helpers, Postgres bootstrap/verify/rotate behavior,
database-backed admin middleware, `/admin/operator-token/rotate`, bundled
static UI assets, and tests for token and UI behavior.

Verification: `bash .codex/skills/code-change-verification/scripts/run.sh`
passed on 2026-05-10. The script ran `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo test --workspace --all-features`.

Real service verification: started Gateway without `GATEWAY_ADMIN_TOKEN`
against local Docker containers `relayna-postgres` and `redis` using a
disposable `relayna_gateway_phase7_test` database, applied all migrations, and
used a local HTTP stub service. Verified first-start operator token bootstrap,
`/readyz`, `/admin-ui` serving without token leakage, invalid-token rejection,
service create, key create, internal service passthrough, Redis-backed RPM
rejection, usage by service from Postgres, operator token rotation, old-token
rejection, and new-token acceptance. Rendered `/admin-ui` in headless Chrome
through Playwright and verified the sign-in screen loads without embedding the
bootstrap token. The temporary gateway, stub service, and test database were
removed after the run.

## Context and Orientation

Expected areas:

- `crates/gateway-core/`: operator token material, hashing, verification, and
  public response types.
- `crates/gateway-store/`: PostgreSQL operator token schema and store methods.
- `crates/gateway-api/`: startup bootstrap, admin auth middleware, token
  rotation, and static UI serving.
- `internal/mvp-phase-roadmap.md`: Phase 7 roadmap entry.

## Compatibility Boundary

Compatibility boundary: compare admin auth, static UI routes, PostgreSQL
operator token schema, and operator-facing API response shapes against the
latest release tag before editing.

No local `v*` release tag exists, so replacing `GATEWAY_ADMIN_TOKEN` is treated
as a direct replacement of unreleased behavior. The `operator_tokens` table is
durable once deployed and must evolve through additive migrations after review.

## Validation and Acceptance

Phase 7 is accepted when:

- First startup generates exactly one active operator token when none exists.
- Restart with an existing active operator token does not generate another raw
  token.
- Admin APIs reject missing, malformed, invalid, disabled, and revoked operator
  tokens.
- Operators can rotate the token and the old token stops working.
- `/admin-ui` serves the bundled operator console without embedding secrets.
- The UI can sign in, manage keys, manage services, inspect usage, inspect
  health, and rotate the operator token.
- Raw virtual keys and rotated operator tokens are shown only once.
- Service credentials remain write-only and are never displayed.

## Idempotence and Recovery

The operator token migration is additive. If the first printed token is lost,
operators can recover by directly revoking, disabling, or deleting active rows
in `operator_tokens`, then restarting Gateway to trigger bootstrap creation of
a new token.
