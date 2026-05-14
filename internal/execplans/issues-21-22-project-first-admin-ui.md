# Issues 21-22 Project-First Admin UI and Ownership

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This plan follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators can use the admin portal without the Studio import modal overflowing
at common viewport sizes, and they can manage Gateway ownership from Projects
first. A Project can link multiple Services from the Project page. Project-owned
virtual keys inherit access to those linked Services, while Individual virtual
keys choose Services directly. Usage views expose Project, Key, Service, and
route-level breakdown entry points.

## Progress

- [x] (2026-05-14 15:15Z) Read repo instructions, design manifesto, GitHub
  Issues #21 and #22, and current admin UI/backend structure.
- [x] (2026-05-14 15:20Z) Established compatibility boundary and plan.
- [x] (2026-05-14 15:45Z) Add PostgreSQL migration and core API types for key ownership and service
  link tables.
- [x] (2026-05-14 15:52Z) Update store/API policy reads and admin key/service/project methods.
- [x] (2026-05-14 16:02Z) Update admin UI forms, menu order, Project service linking, modal
  scrolling, and Usage breakdowns.
- [x] (2026-05-14 16:10Z) Add/update tests and run required verification.

## Surprises & Discoveries

- Observation: the current branch already has Projects, Services, Keys, and
  Usage in the static admin UI, but Services directly store one optional
  `project_id` and virtual keys require manual route/service policy inputs.
  Evidence: `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/app.js`.
- Observation: `cargo check` passed before test fixtures were updated because
  the stale `project_id: Uuid` initializers only lived in test-only code.
  Evidence: `cargo test --workspace --all-features` caught and the fixtures now
  use `Option<Uuid>` and include the new response fields.
- Observation: service-derived permissions must expand both service names and
  route policy entries; otherwise linked Services would still fail the route
  gate before the service-name gate.
  Evidence: `PostgresStore::policy_for_key` now appends canonical service
  routes or `/services/*` for linked Service route patterns.

## Decision Log

- Decision: Compatibility boundary is latest release tag `v0.0.6`; admin
  project/service UI and service registry behavior are branch-local post-tag
  work, but PostgreSQL migrations are durable once applied locally. Add a
  forward migration and update readers/writers directly without preserving the
  old service-owned project-link UI as a primary path.
  Rationale: Issue #22 explicitly asks to make Project the ownership/control
  surface and remove per-key manual route-pattern assignment from normal flows.
  Date/Author: 2026-05-14 / Codex.

## Outcomes & Retrospective

Implemented Issues #21 and #22 across the admin UI, gateway core API models,
PostgreSQL store, and migration layer. The Studio import modal now constrains
wide tables inside a scrollable modal body. The admin console menu order is
Overview, Providers, Services, Routes, Projects, Keys, Usage, Health. Projects
now expose multi-Service linking, virtual keys expose Project vs Individual
ownership, Individual keys select Services directly, and policy reads derive
service permissions from Project or key service links before falling back to
legacy policy fields. Usage now offers Project, Key, Service, and route filters
with Project/Key/Service breakdown tables.

Validation passed:

    node --test tests/admin-ui.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Context and Orientation

The static admin portal lives in
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui`.
Axum admin routes live in
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs`. Core request
and response types live in `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src`.
PostgreSQL storage and migrations live in
`/Users/jobz/Works/relayna-gateway/crates/gateway-store`.

A virtual key is the external Relayna credential. A key policy decides which
routes, providers, models, Services, streaming, tools, rate limits, and budgets
are allowed. A Service is a registered route pattern and upstream target for
internal-service proxy traffic. A usage event records one request and must
contain enough dimensions for aggregation.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.6`; branch-local admin project
and service UX can be replaced directly. Persisted PostgreSQL state needs a
forward migration that backfills existing service `project_id` links into the
new Project-to-Service link table and keeps existing keys project-owned.

## Plan of Work

Add ownership and service-link data structures to core admin and project types.
Add a migration creating `project_service_links` and `key_service_links`, plus
key ownership columns. Update PostgresStore to persist links, derive
`allowed_services` for policy reads, and expose Project link patching through
the existing Project admin API.

Update the admin UI menu order, Studio import modal layout, Project service
linking controls, key ownership controls, and Usage filter/breakdown tables.
Keep manual route/provider/model/budget controls available for advanced policy,
but make Service selection the normal route-permission control.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    node --test tests/admin-ui.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

Then run the repository `$code-change-verification` workflow.

## Validation and Acceptance

Acceptance is proven when the admin UI tests cover the new menu order, modal
scrolling, project service checkboxes, key ownership selector, and usage
breakdown tabs, and Rust tests compile/pass for the changed admin API and policy
resolution behavior.

## Idempotence and Recovery

The migration uses `IF NOT EXISTS` and backfills links idempotently. If
verification fails, rerun focused tests after fixes, then rerun the full
verification stack from a clean command.

## Artifacts and Notes

GitHub Issues:

- #21: Studio import modal content overflows; modal content should scroll and
  action buttons remain usable.
- #22: Project-first ownership, Project-to-Service links, key ownership choice,
  service-derived route permissions, usage breakdowns, and menu order.

## Interfaces and Dependencies

New API shape should be additive where practical:

- `AdminKeyCreate.owner_type`: `project` or `individual`.
- `AdminKeyCreate.project_id`: required for project-owned keys, absent for
  individual keys.
- `AdminKeyCreate.service_names`: selected Services for individual keys.
- `AdminKeyResponse.owner_type`, `project_id`, and `service_names`.
- `ProjectPatchRequest.service_names`: replaces Project linked Services.
- `ProjectResponse.service_names`: current linked Services.
