# Usage Pricing and Admin UI Issues

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md`.

## Purpose / Big Picture

Implement open issues #80 through #89 in one integrated branch. Operators should
be able to configure internal-service pricing rules, audit usage cost sources,
filter and inspect Usage data more effectively, export data cleanly, and use a
lower-fanout Usage dashboard. The final result is one draft PR with all required
Rust and Admin UI verification passing.

## Progress

- [x] (2026-07-04 18:41Z) Created integration branch
  `feat/usage-pricing-admin-ui`.
- [x] (2026-07-04 18:41Z) Created this ExecPlan before implementation work.
- [x] (2026-07-04 19:18Z) Ran conflict-free parallel workers for #80 and #82.
- [x] (2026-07-04 19:20Z) Integrated worker changes into the branch.
- [x] (2026-07-04 19:21Z) Ran focused checks for integrated #80/#82
  changes: `cargo test -p gateway-core services::tests` and `npm test`.
- [x] (2026-07-04 20:09Z) Implemented #81 service pricing rules across
  core, store, proxy, API memory store, migration, and Admin UI JSON textarea.
- [x] (2026-07-04 20:09Z) Implemented #89 usage cost metadata with nullable
  columns, event fields, proxy metadata population, and JSON/CSV export fields.
- [x] (2026-07-04 20:09Z) Implemented #83-#88 Usage dashboard, filters,
  recent events, latency labels, exports, and autocomplete improvements.
- [x] (2026-07-04 20:09Z) Regenerated Admin UI static assets with
  `npm run build:admin-ui`.
- [x] (2026-07-04 22:09+07:00) Ran final verification:
  `npm run build:admin-ui`, `npm test`, and
  `bash .codex/skills/code-change-verification/scripts/run.sh`.
- [x] (2026-07-04 22:15+07:00) Pushed
  `feat/usage-pricing-admin-ui` and opened draft PR
  https://github.com/sarattha/relayna-gateway/pull/90.
- [ ] Run final verification and open one draft PR.

## Surprises & Discoveries

- Observation: latest release compatibility boundary is `v0.1.15`.
  Evidence: `git tag -l 'v*' --sort=-v:refname | head -n1`.
- Observation: current `HEAD` includes post-release LiteLLM passthrough config
  changes in Admin UI, gateway API, proxy, and store files.
  Evidence: `git diff --name-status v0.1.15...HEAD`.

## Decision Log

- Decision: use additive PostgreSQL migrations and additive admin API fields and
  routes.
  Rationale: public admin APIs, persisted schemas, and usage event shapes are
  compatibility-sensitive against `v0.1.15`.
  Date/Author: 2026-07-04 / Codex.
- Decision: run only two initial implementation workers in parallel: #80 and
  #82.
  Rationale: #80 is service validation and #82 is Admin UI Usage time controls;
  later issues overlap heavily in Usage store/API/UI files.
  Date/Author: 2026-07-04 / Codex.
- Decision: keep existing individual Usage endpoints while adding dashboard,
  event, and filter-value endpoints.
  Rationale: this preserves existing clients and keeps new behavior additive.
  Date/Author: 2026-07-04 / Codex.

## Outcomes & Retrospective

Implementation, verification, branch push, and draft PR publication are
complete.

## Context and Orientation

Relayna Gateway is the public AI traffic control plane. Gateway API owns Axum
admin routes and the checked-in Admin UI static asset contract. Gateway core
owns service validation, policy, usage, and cost semantics. Gateway store owns
PostgreSQL migrations and SQL access. Gateway proxy owns Pingora request
handling, policy/budget checks, provider calls, and usage recording.

Relevant implementation areas:

- `crates/gateway-core/src/services.rs` for service pricing types,
  validation, and resolver helpers.
- `crates/gateway-core/src/usage.rs` and `observability.rs` for usage event and
  query/export response types.
- `crates/gateway-store/src/postgres.rs` and migrations for persistence and
  Usage queries.
- `crates/gateway-proxy/src/pingora_plane.rs` for resolved request costs,
  policy/budget checks, and usage recording.
- `crates/gateway-api/src/app.rs` for admin Usage routes and CSV output.
- `crates/gateway-api/admin-ui/src/main.ts` for Admin UI behavior; generated
  files under `crates/gateway-api/src/static/admin-ui/` must come from
  `npm run build:admin-ui`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.15`.

Public admin API changes must be additive. Existing individual Usage endpoints
and export endpoints remain available. PostgreSQL changes use forward additive
migrations with nullable/defaulted columns so old rows remain readable. Existing
exact-match Usage filter semantics remain unchanged; autocomplete only helps
operators supply exact values.

## Plan of Work

First, fix #80 by moving fixed-cost patch validation to the final merged
service state in store/API test coverage while preserving strict create
validation. In parallel, implement #82 in the Admin UI by adding preset/custom
time controls, interval selection, and query serialization for existing Usage
calls.

Second, implement #81 service pricing rules. Add the core rule type,
validation, JSON Pointer exact-match resolver, persistence, API responses, and
Admin UI JSON textarea. Wire resolved fixed costs into proxy policy and budget
checks before upstream calls; passthrough and none modes affect final usage
cost recording.

Third, implement #89 usage cost metadata. Add nullable usage event metadata
columns and fields, populate cost source/mode/rule consistently, and include
the metadata in JSON and CSV exports.

Fourth, implement the Usage page and API improvements: average latency fields
and labels, consolidated dashboard endpoint, breakdown sorting/limit controls,
request-level events endpoint/table, export download/copy controls, and typed
filter/autocomplete support.

Finally, regenerate Admin UI static assets, run the full verification stack,
push the integration branch, and open a draft PR.

## Concrete Steps

Work from `/Users/jobz/Works/relayna-gateway`.

    git switch feat/usage-pricing-admin-ui
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

For final publication:

    git status --short --branch
    git push -u origin feat/usage-pricing-admin-ui
    gh pr create --repo sarattha/relayna-gateway --draft --base main --head feat/usage-pricing-admin-ui

## Validation and Acceptance

Behavioral acceptance:

- Service fixed-cost patches validate the final effective state.
- Services can store and return pricing rules, and resolved fixed rule costs
  drive policy and budget checks.
- Usage rows can explain cost source, cost mode, and pricing rule while old rows
  with null metadata still load.
- Usage dashboard can be loaded through one consolidated endpoint without
  removing older endpoints.
- Usage page supports time ranges, average latency labels, top-N breakdowns,
  recent request rows, export downloads/copy actions, and typed filter inputs.
- Admin UI generated static files match source.

Required verification:

- `npm run build:admin-ui`
- `npm test`
- `bash .codex/skills/code-change-verification/scripts/run.sh`

## Idempotence and Recovery

All migrations are additive and can be applied once by SQLx. If a worktree
worker conflicts with the integration branch, discard only that worker branch
after confirming its diff is either merged or intentionally superseded. Do not
reset the integration branch or revert unrelated user work. Regenerate Admin UI
assets from source after any UI merge conflict.

## Artifacts and Notes

Sub-agent work should report changed files and tests run. The integrator reviews
and merges only conflict-free diffs into this branch.

## Interfaces and Dependencies

New admin API routes are additive and require `SCOPE_USAGE_READ` where they
expose Usage data. Usage export CSV gains appended cost metadata columns.
Service pricing rules are stored as JSONB on service registrations. No new
external services are required beyond the existing Rust workspace, PostgreSQL
test setup, Node/Vite Admin UI build, and repository verification tools.
