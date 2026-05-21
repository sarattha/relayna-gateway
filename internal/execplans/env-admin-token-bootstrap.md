# Environment Admin Token Bootstrap

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Operators can seed the first Relayna Gateway admin/operator token from an
environment variable during the first pod startup against a fresh database. Once
an active token exists in PostgreSQL, startup ignores later environment variable
changes. The only supported way to change the active admin token after bootstrap
is the existing Admin portal/operator-token rotation endpoint.

## Progress

- [x] (2026-05-21 00:00Z) Read repository guidance, design manifesto, and skill workflows.
- [x] (2026-05-21 00:00Z) Ran the v0.0.9 freeze perimeter test before editing; it passed.
- [x] (2026-05-21 00:00Z) Identified latest release tag as `v0.0.10`.
- [x] (2026-05-21 00:00Z) Implement env-seeded bootstrap behavior and focused tests.
- [x] (2026-05-21 00:00Z) Update freeze env inventory, deployment manifest, and operator docs.
- [x] (2026-05-21 00:00Z) Run available non-Rust checks.
- [x] (2026-05-21 00:00Z) Bump current release target to `0.0.11` and update release docs.
- [ ] Run Rust formatting and code-change verification once `cargo` is available.

## Surprises & Discoveries

- Observation: `scripts/run-gateway.sh` still exports `GATEWAY_ADMIN_TOKEN`, but
  runtime config no longer reads it.
  Evidence: `rg GATEWAY_ADMIN_TOKEN` only found script/docs history before this
  change.
- Observation: Operator tokens must match the existing `op_live_...` format
  because lookup and verification derive a fixed prefix.
  Evidence: `gateway_core::operator_token_prefix` rejects raw tokens without
  the `op_live_` prefix.
- Observation: This environment does not have the Rust toolchain installed.
  Evidence: `cargo fmt --all --check` and the code-change-verification script
  failed with `cargo: command not found`; `which cargo` and `which rustfmt`
  returned no path.

## Decision Log

- Decision: Use `GATEWAY_ADMIN_TOKEN` as an optional bootstrap-only environment
  variable.
  Rationale: The name already appears in local scripts and matches operator
  language. It is additive and only affects fresh databases with no active
  operator token.
  Date/Author: 2026-05-21 / Codex.
- Decision: Require the env token to be a valid Relayna operator token string
  (`op_live_...`) instead of accepting arbitrary bearer strings.
  Rationale: Preserves the current token lookup/verification model and avoids
  adding a second token format.
  Date/Author: 2026-05-21 / Codex.
- Decision: Do not print env-provided raw tokens on startup.
  Rationale: The operator already supplied the secret through deployment
  configuration, and printing it would expand log exposure.
  Date/Author: 2026-05-21 / Codex.

## Outcomes & Retrospective

Implemented the bootstrap-only `GATEWAY_ADMIN_TOKEN` path. Startup now checks
for an existing active operator token before parsing the env token, so env
changes are ignored when the database already has an active token. Fresh
databases use the env token when present, or generate and print a token when
absent. The current release target is now `0.0.11`, with release docs and
deployment examples updated to match. Freeze, release metadata, and admin UI
static tests pass. Rust formatting, clippy, and workspace tests remain blocked
because `cargo` is not installed in this environment.

## Context and Orientation

`crates/gateway-api/src/main.rs` starts the process, connects PostgreSQL, runs
store migrations through `PostgresStore::connect`, then calls
`bootstrap_operator_token`. `crates/gateway-store/src/postgres.rs` inserts an
operator token only when no active row exists. Raw operator tokens are never
stored; `crates/gateway-core/src/operators.rs` hashes raw material and requires
the `op_live_` token prefix.

`crates/gateway-api/src/config.rs` owns environment loading. Deployment
defaults live in `deploy/kubernetes/relayna-gateway.yaml`. Freeze protection
for env names lives in `tests/freeze-v0.0.9-perimeter.test.mjs`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.10`; touched freeze surfaces
are operator-token authentication behavior and environment variables. The
change is additive for fresh databases. Existing active database tokens remain
authoritative, and existing rotation behavior remains the only post-bootstrap
change path. No PostgreSQL schema or route shape changes are planned.

## Plan of Work

Add `gateway_admin_token: Option<String>` to runtime config and read
`GATEWAY_ADMIN_TOKEN` as optional. Change startup bootstrap to use the env token
material when present and to generate a random token only when the env token is
absent. Keep the store's existing conditional insert so active database rows
ignore env changes.

Add focused unit coverage for token material selection and config loading.
Update the freeze env inventory to include the new optional env var. Update
Kubernetes and docs to state that `GATEWAY_ADMIN_TOKEN` seeds only a fresh DB
and later changes are ignored until rotating through the Admin portal.

## Concrete Steps

    cd /home/sarattha/relayna-gateway
    node tests/freeze-v0.0.9-perimeter.test.mjs
    cargo fmt --all --check
    bash .codex/skills/code-change-verification/scripts/run.sh
    node tests/admin-ui.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.11
    git diff --check

## Validation and Acceptance

Acceptance criteria:

- Fresh database with `GATEWAY_ADMIN_TOKEN=op_live_...` stores that token hash
  as the initial active operator token.
- Fresh database without `GATEWAY_ADMIN_TOKEN` still generates and prints one
  random operator token.
- Database with an active operator token ignores `GATEWAY_ADMIN_TOKEN` changes.
- Admin portal rotation remains the only supported post-bootstrap token change
  path.
- Freeze perimeter test and gateway verification stack pass when the Rust
  toolchain is available.

## Idempotence and Recovery

The store insert is idempotent because it only inserts when no active
`operator_tokens` row exists and a unique partial index enforces one active
token. Failed local runs can be retried. If a disposable local database was
seeded with the wrong token, reset that local database or rotate the token
through the Admin portal.

## Artifacts and Notes

Successful checks:

    node tests/freeze-v0.0.9-perimeter.test.mjs
    node tests/admin-ui.test.mjs
    git diff --check

Blocked checks:

    cargo fmt --all --check
    bash .codex/skills/code-change-verification/scripts/run.sh

Both blocked checks failed because `cargo` is not installed.

## Interfaces and Dependencies

`GATEWAY_ADMIN_TOKEN` is an optional environment variable. When set, it must be
a valid Relayna operator token string beginning with `op_live_` and long enough
for the existing lookup prefix.
