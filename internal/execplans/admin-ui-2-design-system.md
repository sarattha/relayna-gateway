# Admin UI 2.0 Design System

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Relayna Gateway operators should get a denser, more polished Admin UI 2.0 that
keeps every existing operator workflow while introducing a reusable frontend
foundation. The new UI should translate Boomi-inspired API governance themes
into gateway operations: discovery and cataloging for services/routes/projects,
risk and guardrail visibility, standards and policy control, and monitoring
through overview, usage, and health screens.

The outcome remains served from `/admin-ui`, with no backend route or response
shape changes. Static assets are still bundled by `gateway-api`; the source of
truth becomes a Vite + TypeScript admin UI source package that builds those
static files.

## Progress

- [x] (2026-05-23T16:16:08Z) Established freeze baseline and current admin UI
      serving model.
- [x] (2026-05-23T16:16:08Z) Ran the v0.0.14 freeze perimeter test before
      editing; it passed.
- [x] Add Vite + TypeScript frontend source package that emits the existing
      static asset filenames.
- [x] Move admin portal workflow logic into the frontend source package without
      changing admin API contracts.
- [x] Implement Admin UI 2.0 design tokens, shell, component classes, and
      governance-focused visual hierarchy.
- [x] Update static tests to pin the new design-system and build-tooling
      expectations.
- [x] Run frontend, freeze, and gateway verification.

## Surprises & Discoveries

- Observation: The current portal is a static HTML/CSS/JS bundle embedded with
  `include_str!` from `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui`.
  Evidence: `gateway-api` serves only `index.html`, `app.css`, and `app.js`
  under `/admin-ui`.
- Observation: The production freeze baseline is `v0.0.14`.
  Evidence: `git tag -l 'v*' --sort=-v:refname | head -n1` returned
  `v0.0.14`.
- Observation: The Codex Browser/Playwright screenshot path is not available in
  this session.
  Evidence: `tool_search` exposed Node REPL rather than Browser tools, and the
  Node runtime reported `Module not found: playwright`.
- Observation: The Vite dev server serves the redesigned Admin UI source at
  `/admin-ui/`.
  Evidence: `curl -I http://127.0.0.1:4173/admin-ui/` returned `200 OK`.

## Decision Log

- Decision: Preserve `/admin-ui`, `/admin-ui/app.js`, and `/admin-ui/app.css`
  as the deployed asset contract.
  Rationale: The admin UI is a frozen operator-facing surface; changing asset
  routes is unnecessary for the redesign.
  Date/Author: 2026-05-23 / Codex.

- Decision: Use direct frontend replacement without backend compatibility
  shims.
  Rationale: The change is limited to build tooling and static admin UI assets;
  admin API routes, response shapes, auth, schemas, Redis state, and proxy
  behavior remain unchanged.
  Date/Author: 2026-05-23 / Codex.

## Outcomes & Retrospective

Implemented Admin UI 2.0 as a Vite + TypeScript source package that builds the
existing checked-in static assets served by `gateway-api`. The deployed
`/admin-ui` surface now has a governance-grouped shell, redesigned login and
header chrome, design tokens, status colors, dense panels, table/form/modal
component styling, and responsive rules. Existing admin workflows and endpoint
strings remain preserved in the generated bundle.

Verification passed:

- `npm run build:admin-ui`
- `npm test`
- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `bash .codex/skills/code-change-verification/scripts/run.sh`
- `npm audit --omit=dev`

Visual browser screenshots were not captured because the Browser tool was not
available in this session and the Node runtime did not have Playwright
installed. The Vite dev server was started and `/admin-ui/` returned `200 OK`
with the redesigned shell markup.

## Context and Orientation

The Admin UI is currently a static operator console embedded in
`gateway-api`. `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs`
serves `/admin-ui`, `/admin-ui/app.js`, and `/admin-ui/app.css` using
`include_str!`. The existing JavaScript calls protected
`/admin-ui/admin/*` endpoints for projects, keys, providers, routes, services,
guardrails, usage, health, settings, and operator token rotation.

## Compatibility Boundary

Freeze baseline: `v0.0.14`.

Touched freeze surfaces: admin UI static assets and build/test tooling.

Compatibility impact: frontend-only replacement with no intended public API,
auth, schema, Redis, proxy, streaming, telemetry, config, or route inventory
change. The v0.0.14 freeze perimeter test must continue to pass.

## Plan of Work

Add `/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui` as the
frontend source package. Configure Vite so `npm run build:admin-ui` emits
`index.html`, `app.js`, and `app.css` into the existing static asset directory.

Move the current workflow logic into TypeScript source with permissive typing
first, preserving function names and endpoint strings pinned by tests. Introduce
a small component helper module for reusable shell/page metadata, status tone
classification, and display helpers.

Replace the CSS with Admin UI 2.0 design tokens and components: dense shell,
left navigation grouped by governance domains, compact page header, metric
tiles, panels, forms, tables, modals, badges, notices, and responsive layouts.
Keep text readable and controls stable across desktop and mobile.

Update static tests so they verify the Vite/TypeScript source package,
generated static asset contract, core design-system tokens/classes, and existing
admin API usage.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    node tests/freeze-v0.0.14-perimeter.test.mjs
    npm install
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance criteria:

- `/admin-ui` remains the operator UI entrypoint.
- The generated static asset filenames remain `index.html`, `app.js`, and
  `app.css`.
- Every existing admin view remains reachable and continues calling the same
  protected admin APIs.
- Raw virtual keys, operator tokens, provider credentials, and Studio tokens are
  still handled as write-only or show-once values.
- Tests cover the design-system source, generated asset contract, freeze
  perimeter, and Rust workspace.

## Idempotence and Recovery

The frontend build can be rerun safely; it regenerates only the checked-in
static admin UI assets. If `npm install` is interrupted, remove incomplete
`node_modules` and rerun it. If generated static files drift from source, rerun
`npm run build:admin-ui` and inspect the diff before committing.

## Artifacts and Notes

The Boomi reference is used only as product inspiration: discovery, risk,
standards/compliance, and monitoring/control. Relayna keeps its own visual
identity and operator-focused density.

## Interfaces and Dependencies

New build command: `npm run build:admin-ui`.

No new backend route, environment variable, database migration, Redis key,
provider behavior, or admin API response shape is introduced by this plan.
