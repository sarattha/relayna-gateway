# Implement the approved Admin UI 3.0

This living ExecPlan follows `PLANS.md`. Root: `/Users/jobz/.codex/worktrees/9b27/relayna-gateway`.

## Purpose / Big Picture

Migrate the production Vite/TypeScript console to the approved design in `internal/design/admin-ui-3/index.html`. Operators see results before creation forms, retain project context through monitoring, and investigate requests in drawers. Preserve all existing admin and owner operations and exact server authorization. A virtual key is a scoped gateway credential; provider credentials remain write-only, virtual credentials show once. Live traffic is a bounded process-local stream; durable usage and saved diagnostic history are separate sources.

## Progress

- [x] (2026-09-05) Prepared 0.1.32 metadata, changelog, current UI docs/screenshots and contributor guidance. All major remaining workflow checks completed through authorized Chrome; Computer Use retry timed out. Full mandatory script and release build pass.
- [ ] Verify final PR head and fresh Codex review, then merge under explicit user authorization and pause monitoring.

- [x] (2026-09-05) Second review: defer Usage drilldown filters until accepted navigation; clear scope after deleting its selected project. Unit tests cover cancel/pending/accept and unrelated project selection; real browser reproductions pass. Required format/Clippy/workspace tests pass; upstream audit blocker unchanged.

- [x] (2026-09-05) Extended browser testing uncovered lost DOM currentTarget after identity-delete confirmation. Captured binding IDs before awaiting; service/project confirmation and cancellation tests pass, with browser readback. Frontend and required format/Clippy/workspace tests pass; upstream audit blocker remains.

- [x] (2026-09-05) Review fixes pushed at 932c282, both addressed threads resolved with evidence and fresh review requested; all CI checks pass. Extended guardrail, Settings, Studio import/sync and service-workload lifecycle coverage. Remaining matrix items stay in the QA report.

- [x] (2026-09-05) First Codex review findings fixed: highest reliability signal and atomic workspace navigation. Unit and dual-role browser regressions pass; review replies/resolution follow the pushed fix.

- [x] (2026-09-05) Confirmed approved artifacts and isolated branch `codex/admin-ui-3` at `42aa804`.
- [x] (2026-09-05) Read repository guidance, audit and required implementation skills; latest release `v0.1.31`.
- [x] Validate correctness findings and implement request lifecycle, readiness and status fixes with regression checks.
- [x] Implement shell, contextual workflows, monitoring scope and drawers while preserving all 18 surfaces.
- [x] Build isolated gateway environment and record admin/owner and responsive Chrome coverage; Computer Use blocked by locked Mac. Additional interaction matrix gaps are explicitly tracked in the QA report.
- [x] Regenerate assets; frontend, format, Clippy, workspace tests and independent remaining checks pass.
- [x] Full verification script passes after preserving and replacing the contaminated local advisory cache. The earlier upstream diagnosis was incorrect.
- [x] Commit 03cd5f3 pushed; draft PR #111 opened, Codex review requested. Dedicated 10-minute heartbeat owns remaining verification and review.
- [ ] Address Codex review with evidence, replies and resolution; keep PR draft while material verification gaps remain. Merge only after final version, documentation, checks and review support landing.

## Surprises & Discoveries

- Initial PR CI caught an unnecessary TypeScript package import in the new test harness. The production helper is JavaScript-compatible; load its actual source directly to preserve dependency-free CI testing.

- Computer Use first timed out, then reported the Mac locked; authorized Chrome verification continued.
- The older local image lacked current Traffic endpoints. Switched to the current native gateway binary for Live/History verification, reusing only isolated development dependencies.
- Browser tests exposed a null inventory-heading lookup, nested Usage tabs, focus restoration after debug loading, a stale key-edit drawer, invalid HTML pattern escaping, mismatched Traffic scope, and indefinite Usage loading on failure; fixed and checked these paths.
- The full verification script passes formatting, Clippy and workspace tests, then fails loading upstream RustSec data with duplicate ID RUSTSEC-2026-0244. Remaining checks were executed independently without weakening the audit rule.

The branch was created and selected by task setup between inspection and `git switch`; verified it points to this checkout and starting commit. Docker is available. Existing frontend checks are mostly source assertions; behavioral browser coverage must be added. The production UI already supports workflows that the approved prototype labels conceptual.

## Decision Log

- 2026-09-05 / User: Explicitly authorized choosing the release bump, updating changelog/documents and merging when ready. This supersedes earlier no-merge instructions; no separate release publication or deployment is authorized. Target 0.1.32 follows the repository's existing patch-release convention.
- 2026-09-05 / Codex: Corrected the audit diagnosis: duplicate advisory was an untracked file in the local cache, not the official repository. Preserved the complete cache in a backup and fetched a clean official copy.

- 2026-09-05 / Codex: User-approved UI 3.0 visual direction supersedes the old UI 2.0 visual rules, while retaining source/build, credential, accessibility and API rules.
- 2026-09-05 / Codex: Preserve released backend contracts at v0.1.31. Implement a frontend migration against existing APIs; do not invent budgets, totals or backend capabilities. No data migration planned.
- 2026-09-05 / Codex: Keep existing navigation identifiers and owner surfaces reachable, even where destinations are visually grouped. Existing links remain valid.

## Outcomes & Retrospective

Production implementation and browser verification are in place. Prototype QA remains separate from production evidence. See `internal/test-reports/admin-ui-3/README.md` for executed checks and explicit limitations. PR preparation is in progress.

## Context and Orientation

Production source is `crates/gateway-api/admin-ui/src/main.ts`, `traffic.ts`, design-system files and `app.css`, with shell `index.html`. Generated assets live in `crates/gateway-api/src/static/admin-ui/` and must be generated, never edited manually. `tests/admin-ui*.mjs` cover contracts and traffic utilities. `deploy/local/` provides PostgreSQL, Redis, development OIDC and mock upstream fixtures. The audit maps 18 current surfaces to UI 3.0 destinations and records 30 prioritized findings.

## Compatibility Boundary

Latest release v0.1.31: preserve `/admin-ui`, `/admin-ui/app.js`, `/admin-ui/app.css`, all admin/owner API routes and authorization, database and Redis formats, request and usage contracts. Visual and local UI state replacement is user-authorized. Keep scope explicit and never broaden invalid owner scope. No new external configuration or backend contracts are required for the planned migration.

## Plan of Work

First fix readiness domain errors, request cancellation through body consumption and stale renders, key lifecycle classification and health semantics. Build common shell and contextual form presentation without discarding existing handlers. Persist monitoring query state and carry project/key context into usage and traffic. Restyle existing functional resource/governance/owner workflows to the approved calm dark/teal design. Add regression checks for reordered requests, unavailable readiness, expired keys and scope. Use disposable services and development identity fixtures for real gateway coverage, recording honest gaps in `internal/test-reports/admin-ui-3/`. Finish verification and PR review cycle.

## Concrete Steps

From repository root run `npm ci`, `npm run build:admin-ui`, `npm test`, then `bash .codex/skills/code-change-verification/scripts/run.sh`. Use isolated Docker project and ports for database/Redis/OIDC/upstream; never reuse production data. Exercise all navigation and mutation workflows with disposable fixtures. Stage intended source, generated assets, tests, approved artifacts and evidence only. Commit and push `codex/admin-ui-3`, create PR via gh, request Codex review, inspect latest-head checks and threads, fix and reply before resolving.

## Validation and Acceptance

All 18 surfaces and major create/edit/delete/confirm/error/empty/loading flows retain capability. Admin, owner, viewer, pending and disabled sessions preserve access boundaries. Phone/tablet/desktop, keyboard focus, tables, charts, imports, exports, live reconnect/history and show-once credentials have recorded coverage. Readiness 503 does not erase healthy monitoring panels. Late navigation responses cannot replace a newer view. Expired keys do not count active. Project/key filters survive refresh and agree with results and exports. Frontend build/tests and Rust format/clippy/workspace tests pass in order. Record blockers instead of claiming unperformed verification.

## Idempotence and Recovery

Use unique local test resources and isolated Docker volumes. Rerun seeds only in the task environment. Do not reset or overwrite unrelated work. Asset generation is repeatable. After failed checks fix and rerun the prescribed sequence. No migrations planned. Keep PR open for review; heartbeat stops only when closed/merged or cancelled.

## Artifacts and Notes

Approved artifacts: `internal/design/admin-ui-3/`; prior design ExecPlan: `internal/execplans/admin-ui-3-design-audit.md`. Coverage and screenshots will be in `internal/test-reports/admin-ui-3/`.

## Interfaces and Dependencies

Retain Vite, TypeScript, Chart.js and Tabler; existing Axum admin/owner endpoints, PostgreSQL, Redis and development OIDC provide functionality. Computer Use uses the supported node_repl and @oai/sky API; if capture fails record the error and continue through the already-authorized Chrome integration.
