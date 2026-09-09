# Admin UI reliability and LiteLLM spend

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Protect newly issued virtual keys from accidental dismissal, make Overview
usable with production usage volumes, and show LiteLLM-reported spend for mapped
Relayna keys. Deliver one reviewed PR with release notes and regression evidence.

## Progress

- [x] (2026-09-09) Read contributor, design, implementation and verification guidance; created `codex/admin-ui-reliability-litellm-cost` from clean main.
- [x] Located show-once dialog dismissal and excessive Overview dashboard aggregation.
- [x] Complete dialog regression tests.
- [x] Reduce Overview work and verify delayed/error responses.
- [x] Implement bounded, authenticated LiteLLM spend reads and clear attribution UI.
- [x] User clarified repository-wide coverage: all-features Rust workspace line coverage 95.01% (31,029 lines; 1,548 missed). Full regression suite and Computer Use desktop/mobile QA passed.
- [ ] Complete mandatory verification stack.
- [ ] Update version, changelog and docs; open one PR and address Codex review.

## Surprises & Discoveries

Overview fetches the full usage dashboard despite using only summary, timeseries
and projects. PostgreSQL builds its many sections sequentially. The UI cancels
finite requests after eight seconds. Production latency itself is not yet measured.
LiteLLM `/key/info` provides its current spend counter, which can reset and can
include traffic outside Gateway. It cannot honestly be labeled gateway-only or
arbitrarily filtered by Gateway's date range.

## Decision Log

- 2026-09-09: Keep existing released APIs and persisted usage intact. Add cost
  visibility separately from estimated usage and budget enforcement. User has
  authorized these behavior changes and the combined PR.
- 2026-09-09: Show-once secrets require explicit Close; other dialogs retain
  backdrop and Escape behavior. Preserve focus trapping/restoration.

## Outcomes & Retrospective

All three behaviors implemented. Coverage target passed for the complete Rust workspace, without path exclusions. Frontend regression suite and Computer Use QA passed; frontend coverage is not part of the LLVM metric. Mandatory verification and PR review remain in progress.

## Context and Orientation

Source UI: `crates/gateway-api/admin-ui/src/main.ts`; generated assets:
`crates/gateway-api/src/static/admin-ui`. API handlers live in
`crates/gateway-api/src/app.rs`, PostgreSQL aggregation in
`crates/gateway-store/src/postgres.rs`, and mapping lookup contracts in
`crates/gateway-core/src/provider_configs.rs`. A virtual key is a Relayna client
credential. A LiteLLM mapping supplies the upstream credential for a key or project.

## Compatibility Boundary

Latest local release tag: v0.1.33. Preserve routes, usage shapes, credentials,
database and Redis state. New spend reads are additive and never write counters.
Admin asset URLs remain unchanged. No schema migration planned.

## Plan of Work

Implement and test dialog protection first. Reduce Overview to its needed data,
with a bounded analytics timeout and independent stale/unavailable handling.
Read mapped LiteLLM spend server-side using configured authentication, with
bounded timeout/body and no upstream secrets/errors returned to the browser.
Expose spend with scope, timestamp and reset/shared-spend caveats. Then validate
and prepare the release and combined PR.

## Concrete Steps

From repository root run `npm run build:admin-ui`, `npm test`, focused regression
tests and coverage, then `bash .codex/skills/code-change-verification/scripts/run.sh`
and `cargo build --workspace --all-features`. Exercise a disposable local UI with
Computer Use. Inspect GitHub checks and Codex review after pushing the PR.

## Validation and Acceptance

Backdrop clicks and Escape cannot dismiss a new key; Copy and Close still work.
Overview does not request unused dashboard sections; delays beyond eight seconds
can succeed, navigation cancels old work, failed sources are not shown as zero.
Spend reads authorize usage access and handle absent/shared mappings, zero spend,
upstream failures, malformed data and secret redaction. Record actual coverage
scope and results; do not claim repository-wide coverage from focused tests.

## Idempotence and Recovery

Builds regenerate assets from source. Use disposable test services and fixtures.
Rerun checks after fixing failures. Do not alter production data or merge the PR.

## Artifacts and Notes

Record QA, measured coverage and review results here as work proceeds.

## Interfaces and Dependencies

Reuse existing provider runtime lookup and administrator usage authorization.
LiteLLM source: https://docs.litellm.ai/docs/proxy/virtual_keys (spend tracking).
No additional production dependency planned.

### Verification evidence (2026-09-09)

`cargo llvm-cov --workspace --all-features --summary-only` passed with 95.01%
line coverage. Regions: 92.24%; functions: 92.20%. PostgreSQL and Redis tests
used disposable services on ports 21432 and 21379. `npm test` passed, including
an actual HTTP response delayed 8.1 seconds, cancellation, stale-cache scoping,
show-once dismissal, spend zero/error/shared attribution and existing regressions.

Computer Use tested Safari desktop and 390px responsive layouts against a
synthetic local fixture: Overview, key inventory, spend modal, Escape protection,
Copy success and explicit Close. Coordinate clicks were unavailable in the
Computer tool; backdrop dismissal was verified by automated regression. No live
provider spend or production performance measurement is claimed. The backend
spend tests use local HTTP fixtures, including credential forwarding/redaction,
redirect rejection, malformed and oversized bodies.

The first mandatory stack passed formatting, Clippy, Cargo tests, audit, deny,
machete and all 345 Nextest tests, then Trivy found 14 high-severity dependencies
in ignored `design-prototypes/service-owner-monitoring/package-lock.json`.
That pre-existing prototype is outside the PR. Preserve it and rerun the entire
stack from a clean temporary worktree of the committed branch.
