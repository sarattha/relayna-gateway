# Fix PR #126 pricing attribution, ledger precision and Foundry edits

This living ExecPlan follows `PLANS.md`. Maintain Progress, Surprises &
Discoveries, Decision Log and Outcomes & Retrospective throughout the work.

## Purpose / Big Picture

Resolve issues #127, #128 and #129. Operators can reprice only provably matched
historical rules, cannot configure service charges that disappear in the ledger,
and can edit Foundry services with a single database connection. Prepare 0.1.43,
open a review PR and monitor CI and Codex feedback without merging it.

## Progress

- [x] (2026-10-08) Read design manifesto, implementation/verification/PR skills,
  PLANS.md and Admin UI rules; created fix/pr126-pricing-and-foundry from main.
- [x] (2026-10-08) Implement pricing fingerprints and migration with regression coverage.
- [x] (2026-10-08) Enforce eight-decimal service pricing in Rust/UI and add boundary coverage.
- [x] (2026-10-08) Reuse Foundry transaction and test single-connection/concurrent edits.
- [x] (2026-10-08) Update version, changelog and operator/release documentation to 0.1.43.
- [x] (2026-10-08) Run focused tests, UI/browser checks and mandatory stack.
  Rust fmt/Clippy/workspace tests/build, audit/deny/machete and nextest pass;
  414 tests pass, zero skipped. Full script stops on seven pre-existing Trivy
  HIGH findings in an unchanged ignored prototype lockfile. Semgrep and
  Gitleaks history (399 commits) and staged scans pass.
- [ ] Commit, push, open/link PR, monitor checks and address review feedback.

## Surprises & Discoveries

- Latest release tag remains v0.1.41 after fetching tags. PR #126 is merged
  into main and prepares 0.1.42, but the usage ledger is durable state already.
- The existing verification script includes dependency/security scanners. PR
  #126 documented pre-existing Trivy findings in the design prototype lockfile.
- PostgreSQL queues same-row waiters on tuple locks as well as transaction-ID
  locks. The concurrency regression waits for pool saturation and a proven
  row-lock wait, rather than expecting every waiter on the same transaction ID.
- PostgreSQL float8-to-numeric casts round to 15 significant digits; a valid
  near-limit service price can overflow. Persist the decimal representation
  through text/numeric and decode repricing JSON costs directly as numeric.
  Enable serde_json float_roundtrip so saved JSON pricing reads do not round a
  near-limit accepted price up to the unsupported boundary.
- Three unrelated untracked security documents belong to the user; exclude them.

## Decision Log

- Decision: Record a versioned SHA-256 fingerprint of the selected rule's name
  and selector (body pointer/value or endpoint method/template), excluding price.
  Add a nullable usage column without backfill. Skip historical named-rule
  records without proof, including legacy endpoints; no request bodies or raw
  selector values are added to usage. Preserve original budget charges.
  Rationale: before/after configuration alone cannot prove per-event provenance.
  Date/Author: 2026-10-08 / Codex.
- Decision: Restrict service charges to eight decimal places and below USD 1e12,
  matching numeric(20,8), in backend validation and native UI controls. Keep
  ledger precision and Redis formats unchanged. Date/Author: 2026-10-08 / Codex.
- Decision: Execute Foundry provider validation on the already-held transaction
  connection, preserving existence/type checks. Date/Author: 2026-10-08 / Codex.

## Outcomes & Retrospective

All three fixes and 0.1.43 release/documentation updates are implemented.
The seven focused PostgreSQL/Redis tests pass, including large-price round-trips.
Workspace and nextest validation pass (414 tests, zero skipped). UI build/tests,
50 React tests, TypeScript, release metadata and strict documentation build pass.
Fresh migration and prior-schema upgrade/idempotence checks leave legacy proof
NULL and costs intact. Native controls and the real API accept 1e-8 and reject
1e-9; the dialog fits a 390px viewport. T3 screenshots were unavailable.

The local full verification script stops at seven pre-existing HIGH prototype
findings; Cargo.lock and the root package-lock have zero HIGH/CRITICAL findings.
The ignored prototype is excluded from commits. Semgrep and Gitleaks history
(399 commits) and staged scans pass. The branch is committed and pushed;
PR publication and review monitoring are in progress.

## Context and Orientation

`gateway-core/src/services.rs` resolves request pricing and historical costs.
`gateway-proxy/src/pingora_plane.rs` carries resolved pricing into UsageEvent.
`gateway-store/src/postgres.rs` persists usage and atomically edits services and
historical reporting costs, keeping original charges for budget enforcement.
Foundry services bind to Azure providers validated during edits. Admin UI source
is `crates/gateway-api/admin-ui/`; rebuild its embedded assets with npm.

## Compatibility Boundary

Latest release tag v0.1.41. Historical repricing is post-tag behavior, but
PostgreSQL usage data must survive upgrades. Add a nullable fingerprint column
with no inferred backfill; old rows remain readable and unproven rule rows stay
unchanged. No public response, credential, virtual-key or Redis format changes.
Tightened validation accepts existing representable prices and rejects precision
beyond durable accounting. Upgrade all writers before relying on fingerprints.

## Plan of Work

Add fingerprints to resolved pricing and internal usage events, persist them
through an additive migration, and require recorded proof in historical rule
resolution. Add core, proxy and PostgreSQL regression tests for selector changes,
legacy rows, endpoint attribution, repeated repricing and preserved budgets.
Validate service amounts centrally; constrain only corresponding UI controls.
Run provider lookup in patch_service through its transaction and exercise a
one-connection pool plus bounded concurrent edits. Bump workspace/lockfile, UI
labels, deployment examples and release documents to 0.1.43.

## Concrete Steps

From repository root, use disposable PostgreSQL/Redis containers with unique
names and ports. Run focused Rust pricing tests and dependency-backed store
integration tests, then npm build/test/typecheck and strict documentation checks.
Run `bash .codex/skills/code-change-verification/scripts/run.sh`, followed by
`cargo build --workspace --all-features` and release metadata validation.
Commit only task files, push branch, create review-ready PR referencing all three
issues, register it with T3 and monitor checks/reviews on each pushed head.

## Validation and Acceptance

A selector-changing new-only edit followed by historical price-only edits must
leave old-selector and legacy records unchanged while repricing proven current
selectors. Original budget charges remain fixed. Named endpoint attribution
cannot be confused with body rules. USD 0, 1e-8 and 0.0002 are valid; 1e-9 and
other excess precision are invalid. Live and recovered budget charges agree.
A Foundry PATCH completes with max_connections=1, while invalid bindings roll
back and concurrent edits terminate. Required Rust, UI and docs checks pass;
report any externally blocked or pre-existing scanner failures accurately.

## Idempotence and Recovery

Migration is additive and uses IF NOT EXISTS. Keep its column on rollback.
Do not fabricate fingerprints for old rows. Repricing retains the first original
charge. Tests use unique service/key identifiers and disposable dependencies;
remove only containers created for this task. Preserve unrelated user files.

## Artifacts and Notes

Source review: https://github.com/sarattha/relayna-gateway/pull/126
Issues: #127 (P1), #128 (P2), #129 (P2).

## Interfaces and Dependencies

Internal ResolvedServiceCost/UsageEvent gain an optional fingerprint; public
usage APIs remain unchanged. PostgreSQL gains usage_events.pricing_rule_fingerprint.
Existing SHA-256 dependency suffices. No new configuration or dependency required.
