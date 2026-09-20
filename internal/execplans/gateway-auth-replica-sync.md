# Synchronize persisted gateway authentication across replicas

This living ExecPlan follows `PLANS.md` in `/Users/jobz/Works/relayna-gateway`.

## Purpose / Big Picture

Changes saved through Settings must reach every gateway pod sharing PostgreSQL
without a rollout. Currently startup loads persisted settings, while PATCH only
refreshes the serving process's shared verifier. Add a five-second reconciliation
loop per process, keeping request admission free of extra database reads.

## Progress

- [x] (2026-09-20) Trace startup, PATCH, shared runtime and database source.
- [x] (2026-09-20) Implement serialized refresh and unchanged-verifier preservation.
- [x] (2026-09-20) Prove replica convergence, failures/recovery and concurrent refresh ordering.
- [x] (2026-09-20) Run full verification, >98% changed Rust coverage and docs; prepare existing draft PR update.

## Compatibility Boundary

Latest release v0.1.37. Preserve stored settings, API responses, deployment
variables and authentication semantics. This implements the user's requested
live propagation of Admin-saved settings. No schema or wire changes are needed.
Environment-only Entra, owner verifier and portal sign-in deployment settings
remain startup configuration; changing Kubernetes env/Secrets still requires
restarting those processes. Persisted gateway settings retain precedence.

## Context and Orientation

`crates/gateway-api/src/main.rs` loads settings and shares one runtime between
control API and proxy. `app.rs` PATCH writes through gateway-store then updates
only that runtime. `crates/gateway-core/src/auth_settings.rs` owns effective
settings resolution and atomic verifier snapshots. PostgreSQL's singleton row
is authoritative; no notification channel or additional dependency is needed.

## Decision Log

- 2026-09-20: Poll every five seconds with a bounded read attempt. Retain the
  last valid configuration on database/validation failure and retry; never fall
  back to disabled authentication on errors. This is eventual convergence,
  not simultaneous activation, and an unavailable pod may keep its prior policy.
- Serialize source-read plus runtime-install per pod for both PATCH and refresh,
  preventing a slow older poll from overwriting the completed local save. Skip
  identical runtime updates to preserve JWKS caches and existing verifier Arcs.
- Own both refresh workers in a Tokio task set within the control runtime;
  dropping that task set aborts them on control shutdown.
  Active requests retain their already acquired admission snapshot.

## Plan of Work

Add core refresh coordination and no-op update handling. Add a bounded periodic
worker in gateway-api and wire it into production startup/shutdown. Route PATCH
through the same refresh coordination. Add focused tests with a controllable
store and a real two-process PostgreSQL integration regression. Update operator
docs to distinguish shared gateway settings from process environment settings.

## Validation and Acceptance

PATCH one process; requests on another must enforce the new Entra policy within
the refresh interval plus I/O time, without restarting either. Verify repeated
updates, disabling, malformed tokens and safe failure/recovery. Unit tests verify
unchanged verifier identity and serialized refresh order. Run
`bash .codex/skills/code-change-verification/scripts/run.sh` with local PostgreSQL
and Redis; run LLVM coverage and the existing changed-production-line checker,
requiring >98%. This task has no UI change requiring Computer Use.

## Idempotence and Recovery

No migration. Reverting code restores startup-only behavior. Integration tests
use isolated databases and terminate only their child gateway processes. Test
fixtures never alter the user's demo settings. Logs must not contain config
payloads, JWTs, provider credentials or Apigee secrets.

## Surprises & Discoveries

The Admin GET reads PostgreSQL, so it can display current settings while that
pod's proxy still enforces an old in-memory verifier. The documentation currently
instructs operators to roll every replica after an Admin-saved change.

## Outcomes & Retrospective

Focused tests pass. Two separate gateway processes enable Entra, change the
audience from either writer, reject old/malformed JWTs and disable Entra without
restarting. A timed worker test covers database errors, timeout and recovery;
core tests prove preserved verifier identity and serialized source reads.
The final ten-command verification passed with 377/377 nextest tests and zero
skips after worker lifecycle/log refinement. Log:
`/tmp/auth-sync-verification-final.log`. Final format/Clippy checks also passed. LLVM coverage passes: 44/44 changed production executable lines for
this feature and 206/206 for the complete PR, both 100% against the >98% gate.
The complete workspace coverage run was followed by focused worker/process runs
for the final source. See `internal/test-reports/gateway-auth-replica-sync-coverage.json`.
Strict MkDocs build passes. No UI source was changed.
