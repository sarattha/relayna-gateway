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
- [x] User clarified repository-wide coverage: all-features Rust workspace line coverage 95.02% (31,028 lines; 1,545 missed). Full regression suite and Computer Use desktop/mobile QA passed.
- [x] Complete mandatory verification stack from clean worktree; all 345 Nextest tests and all security checks passed.
- [x] Open PR #116 and request Codex review.
- [x] Verify and push 0.1.35 version, changelog and docs in the same PR.
- [ ] Complete Codex re-review after the second UI finding.

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

All three behaviors are implemented in PR #116. Codex identified one scoped-query
performance issue, fixed in `a1142a1`, and a failed-project empty-state issue.
The latter now shows explicit unavailable states for project activity and charts.
The corrected implementation passes CI, the complete mandatory local stack,
Computer Use desktop/mobile QA and 95.02% complete Rust workspace line coverage.
Frontend regressions and coverage scope are documented separately. Version 0.1.35,
changelog, docs and generated assets are verified for the same PR. No merge,
release tag, schema migration or production data change is performed.

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

PR: https://github.com/sarattha/relayna-gateway/pull/116. Codex review requested
on implementation commit `74f6e8a`. The 0.1.35 metadata/docs update passed frontend
regressions, asset build, release metadata validation and strict MkDocs build;
its complete mandatory stack and workspace build also passed in the clean worktree.

### Codex review correction

Codex P2 review comment 3970624301 identified inventory-wide mapping reads and
per-row label queries on the spend path. Reuse the existing scoped runtime lookup
and return scope with its credential from one SQL query. This changes only an
internal Rust lookup value; HTTP contracts, persisted schemas and proxy credential
selection remain unchanged. No migration or compatibility shim is needed. Add
store regressions for key precedence, disabled-key project fallback, disabled
project/no-context absence, plus an API fixture that fails if inventory enumeration
is attempted. Re-run coverage and the mandatory stack after this correction.

The correction is committed as `a1142a1`; the review thread is resolved and
Codex re-review requested. The complete mandatory stack and workspace build passed
with this correction and prepared 0.1.35 metadata. One parallel coverage attempt
hit an existing gateway process startup deadline during heavy Docker VM load;
rerun coverage in isolation, preserving all assertions and the 95% threshold.

Final isolated coverage after `a1142a1` passed: 95.02% lines (31,028 total, 1,545
missed), regions 92.25%, functions 92.16%, with `--fail-under-lines 95`.

Codex re-review completed at 2026-09-09 16:33 UTC; its second finding appeared
after the completed status and required the additional UI correction.
The verified 0.1.35 update is the final release-metadata commit in this PR.

The second correction passed frontend tests, rebuilt assets, Computer Use desktop
and 390px failure/retry checks, and the full mandatory stack. Rust production code
is unchanged by this UI correction; the measured 95.02% Rust coverage remains
applicable. Re-review the new UI commit and inspect all unresolved threads before
claiming the review is clear.
