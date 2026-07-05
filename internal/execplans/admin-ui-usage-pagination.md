# Admin UI Usage Pagination

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators using `/admin-ui` need the Usage view to stay scannable when recent
requests and time series contain many rows. After this change, the Recent
requests, Timeseries, and Service timeseries sections each have paging controls
so the page no longer renders all log-like rows at once.

## Progress

- [x] (2026-07-05 09:45Z) Created branch `codex/admin-ui-usage-pagination`.
- [x] (2026-07-05 09:46Z) Read repository UI, compatibility, and verification guidance.
- [x] (2026-07-05 09:58Z) Add backend usage pagination fields and page metadata.
- [x] (2026-07-05 10:01Z) Add Admin UI pagination controls and state.
- [x] (2026-07-05 10:03Z) Add focused Rust and Admin UI tests.
- [x] (2026-07-05 10:10Z) Run verification and test on the real Docker/Admin UI environment.
- [x] (2026-07-05 10:14Z) Commit, push, and create a PR.

## Surprises & Discoveries

- Observation: `/admin-ui/admin/usage/events` already supports `limit`,
  `offset`, and `has_more`; dashboard `timeseries` and `service_timeseries`
  are currently unpaginated.
  Evidence: `gateway-store/src/postgres.rs` uses `limit + 1` for usage events
  and returns bare vectors for both dashboard time series.

- Observation: The Admin UI package exposes `npm run build`, while the root
  repository uses `node tests/admin-ui.test.mjs` for static UI verification.
  Evidence: `crates/gateway-api/admin-ui/package.json` has no `test` script.

## Decision Log

- Decision: Add backend-backed pagination for dashboard time series and reuse
  existing events pagination for Recent requests.
  Rationale: UI-only slicing would still download huge time series arrays.
  Date/Author: 2026-07-05 / Codex.

- Decision: Preserve existing dashboard fields and default unpaginated behavior
  unless explicit time series pagination params are supplied.
  Rationale: `/admin-ui/admin/usage/dashboard` is a released admin API surface.
  Date/Author: 2026-07-05 / Codex.

## Outcomes & Retrospective

Implemented additive backend pagination for dashboard time series and wired the
Admin UI Usage view so Recent requests, Timeseries, and Service timeseries each
page independently. Applying usage filters resets all three pagers to the first
page.

Verification passed with the full repository stack and a real Docker-backed
gateway serving `/admin-ui` on port 19081. The live API returned two-row
time series pages with `has_more: true` when requested, and the browser UI
showed 50-row paged sections with independent next/previous behavior.

## Context and Orientation

The Admin UI source of truth is
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/src/main.ts`.
The generated static assets are served from
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/`.
The usage admin API is served by `gateway-api` and backed by usage query code in
`gateway-core/src/observability.rs` and PostgreSQL query code in
`gateway-store/src/postgres.rs`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.17`; preserve existing
`/admin-ui/admin/usage/dashboard` fields and response defaults. Additive query
params and metadata are acceptable without a compatibility shim.

## Plan of Work

Extend `UsageQuery` with `timeseries_limit`, `timeseries_offset`,
`service_timeseries_limit`, and `service_timeseries_offset`, and extend the
dashboard response with page metadata for both time series lists.

Update PostgreSQL store queries so time series functions return one extra row
when paginated, compute `has_more`, and keep existing unpaginated behavior when
the new limits are absent.

Update the Usage UI to track offsets for recent requests, time series, and
service time series. Add compact Previous/Next controls to each table section,
reset offsets when filters are applied, and pass the correct query params.

Regenerate Admin UI static assets from the Vite source package.

## Concrete Steps

Run commands from `/Users/jobz/Works/relayna-gateway` unless noted:

    npm --prefix crates/gateway-api/admin-ui run build
    node tests/admin-ui.test.mjs
    cargo test -p gateway-api usage_dashboard
    bash .codex/skills/code-change-verification/scripts/run.sh

For real environment verification, rebuild and run the Docker image, then open
`http://127.0.0.1:19081/admin-ui` and verify the Usage view pagers against the
local Postgres/Redis-backed gateway.

## Validation and Acceptance

Acceptance criteria:

- Recent requests shows only the selected page and can navigate next/previous.
- Timeseries shows only the selected page and can navigate next/previous.
- Service timeseries shows only the selected page and can navigate next/previous.
- Applying filters resets all three pagers to the first page.
- Existing dashboard consumers still receive `timeseries` and
  `service_timeseries` fields.
- Required Rust, Admin UI, and real-environment checks pass.

## Idempotence and Recovery

All code edits are local and can be rerun. If Docker ports `19080` or `19081`
are occupied by an old gateway container, stop only the gateway container and
leave the dev Postgres and Redis containers running.

## Artifacts and Notes

Verification commands passed:

    npm --prefix crates/gateway-api/admin-ui run build
    node tests/admin-ui.test.mjs
    cargo test -p gateway-api usage_dashboard
    bash .codex/skills/code-change-verification/scripts/run.sh

Real environment checks passed against Docker image
`relayna-gateway:codex-usage-pagination` running as
`relayna-gateway-usage-pagination`:

- `GET http://127.0.0.1:19081/admin-ui/readyz` returned ready.
- `GET /admin-ui/admin/usage/dashboard?interval=hour&timeseries_limit=2&service_timeseries_limit=2`
  returned `timeseries_page.has_more: true` and
  `service_timeseries_page.has_more: true`.
- Playwright verified Usage sections render pagers, Timeseries can move to
  rows 51-100 independently, and Apply resets all pagers to rows 1-50.

## Interfaces and Dependencies

New optional query params:

- `timeseries_limit`
- `timeseries_offset`
- `service_timeseries_limit`
- `service_timeseries_offset`

New dashboard metadata fields:

- `timeseries_page`
- `service_timeseries_page`
