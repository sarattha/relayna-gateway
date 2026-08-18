# Project-owner monitoring with Entra

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

After this change, an administrator can grant an active Entra portal member
Owner or Viewer access to an exact Relayna project. That member can use a
read-only project dashboard and owner API that expose only usage events whose
persisted `project_id` matches the assignment. A managed identity can use the
same existing `gateway.monitor.read` Entra application role for project
monitoring, but it must also match an enabled exact project binding stored by
Relayna.

## Progress

- [x] (2026-08-18 00:00Z) Reviewed the design manifesto, existing service-owner flow, Entra role contract, project model, usage query model, and Admin UI design system.
- [x] (2026-08-18 00:00Z) Chose an additive compatibility strategy against release `v0.1.26`.
- [x] (2026-08-18 08:00Z) Added project member and managed-identity access models, an additive migration, PostgreSQL implementations, and durable store coverage.
- [x] (2026-08-18 08:00Z) Added administrator grant APIs and read-only `/owner/v1/projects/*` APIs with exact server-side project scoping and authorization regressions.
- [x] (2026-08-18 08:00Z) Added project-owner portal navigation, assignment controls, managed-identity controls, project dashboard, and sanitized request detail UI.
- [x] (2026-08-18 08:00Z) Updated Entra/operator documentation, release metadata, and the `v0.1.28` changelog.
- [x] (2026-08-18 08:00Z) Regenerated checked-in Admin UI assets; passed frontend, Rust, dependency, static-analysis, and real-browser verification. The repository-wide Trivy scan retains a documented pre-existing failure in an unchanged design-prototype lockfile.
- [x] (2026-08-18 09:10Z) Addressed the first Codex review: project-attributed debug bundles now fail closed on mismatches, and hashless Entra sign-in returns defer to the membership-aware owner landing view.
- [x] (2026-08-18 09:20Z) Addressed a delayed duplicate Codex review by scoping guardrail summary, event, and export joins to the persisted virtual key and project as well as request ID.
- [x] (2026-08-18 16:30Z) Added and ran an isolated Docker Compose inspection stack with an Entra-shaped project-owner persona, mock upstream, idempotent seeded usage, and shared-role managed-identity binding.

## Surprises & Discoveries

- Observation: Service Owner and Viewer currently receive the same read-only API capability; the role is returned for display and future policy differentiation.
  Evidence: `require_owner_service_access` authorizes either membership role and every `/owner/v1/services/*` handler is read-only.
- Observation: Usage queries already support an exact `project_id` filter.
  Evidence: `gateway_core::UsageQuery` contains `project_id: Option<Uuid>` and PostgreSQL usage queries bind that filter.
- Observation: The local default RustSec advisory checkout had duplicate advisory ID `RUSTSEC-2026-0244` and could not be parsed.
  Evidence: `cargo audit` passed against a freshly fetched database under `/private/tmp/relayna-rustsec-audit-db` with the repository's configured ignores.
- Observation: Trivy now reports four high-severity npm advisories in `design-prototypes/service-owner-monitoring/package-lock.json`.
  Evidence: The lockfile is unchanged from `HEAD`; Cargo dependencies report zero vulnerabilities, and the production Admin UI has its own passing dependency tests. The prototype was left untouched to avoid mixing unrelated dependency maintenance into this feature.
- Observation: Request IDs are client-supplied and debug bundles use the request ID as a global upsert key, so checking only the already-scoped usage row cannot prove a bundle belongs to that project.
  Evidence: The first Codex review identified the collision; debug bundles now persist the authenticated key's `project_id`, and project details expose a bundle only on an exact attribution match.
- Observation: A browser-visible localhost OIDC issuer cannot advertise that same localhost token and JWKS address to a containerized Gateway.
  Evidence: The development issuer now keeps public authorization/logout endpoints on its configured issuer while optionally advertising an internal token/JWKS base URL; its regression test covers the split endpoints.

## Decision Log

- Decision: Reuse `gateway.monitor.read` for both service and project monitoring managed identities.
  Rationale: The Entra role is a coarse read-only monitoring capability; Relayna's exact resource binding remains the fine-grained authorization boundary. A new Entra role would add tenant provisioning work without adding a meaningful capability boundary.
  Date/Author: 2026-08-18 / Codex.
- Decision: Add separate project membership and project managed-identity binding tables instead of changing released service binding rows into a polymorphic schema.
  Rationale: Additive tables preserve the released `service_memberships` and `managed_identity_bindings` contracts and avoid nullable target columns or data rewrites.
  Date/Author: 2026-08-18 / Codex.
- Decision: Scope project dashboards by persisted usage-event `project_id`, not by `project_service_links`.
  Rationale: A service may be shared across projects; service linkage is policy/catalog metadata, while usage attribution is the security-safe project boundary.
  Date/Author: 2026-08-18 / Codex.
- Decision: Add nullable project attribution to debug bundles and treat missing attribution as ineligible for project-owner display.
  Rationale: Existing bundles predate project-owner access and cannot be safely inferred from a globally keyed request ID. Nullable attribution preserves existing operator access while new virtual-key traffic can be matched exactly.
  Date/Author: 2026-08-18 / Codex, after Codex review.
- Decision: Keep the inspection environment additive under `deploy/local` with localhost-only alternate ports and named volumes.
  Rationale: The user can inspect the real compiled UI and authorization path without changing production defaults or colliding with existing PostgreSQL, Redis, or Gateway services.
  Date/Author: 2026-08-18 / Codex.

## Outcomes & Retrospective

Project-owner monitoring is implemented as a parallel, read-only owner surface
without weakening the existing service-owner boundary. Human access is granted
per project, workload access combines the shared `gateway.monitor.read` Entra
application role with an exact enabled Relayna project binding, and every usage
read is overwritten server-side with the route project UUID.

Regression coverage proves same-project success, cross-project concealment,
malicious query-filter replacement, durable PostgreSQL access state, and one
development OIDC workload token using the shared role for both a service and a
project. The compiled Admin UI was also exercised in Chrome at desktop and a
390 by 844 responsive viewport, including the sanitized request-detail drawer.

The local inspection stack builds this branch into a release image and runs it
with PostgreSQL, Redis, a development OIDC issuer, and a mock upstream. Its
seeded Analytics Project Owner sees exactly one project containing 168 usage
events, 16 failures, two services, two models, version transitions, guardrail
actions, and project-attributed sanitized debug bundles. The same development
managed identity reads that project with `gateway.monitor.read` and an exact
project binding.

All formatting, Clippy, workspace tests, Nextest (307 tests), cargo-audit with a
fresh database, cargo-deny, cargo-machete, Gitleaks, and Semgrep checks passed.
Trivy's only failure is the unchanged historical design-prototype lockfile
recorded above; the changed Rust and production Admin UI surfaces are clean.

## Context and Orientation

The browser portal authenticates humans with Microsoft Entra OIDC and stores an
opaque session in `portal_sessions`. `service_memberships` grants an active
portal member access to one registered service. Managed identities present an
Entra access token containing `gateway.monitor.read`, then Relayna additionally
matches tenant, client, optional object ID, and exact service in
`managed_identity_bindings`.

Projects are stored in `projects`, usage events carry an optional `project_id`,
and usage reads accept `gateway_core::UsageQuery`. The Axum control plane is in
`crates/gateway-api/src/app.rs`; access models and the store trait are in
`crates/gateway-core/src/access.rs`; PostgreSQL access is in
`crates/gateway-store/src/postgres.rs`; the source Admin UI is under
`crates/gateway-api/admin-ui/` and generated assets are checked in under
`crates/gateway-api/src/static/admin-ui/`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.26`. Existing public
`/owner/v1/services/*` routes, portal session fields, service membership tables,
managed-identity binding tables, configuration, and `gateway.monitor.read` role
semantics remain valid. The change uses a forward PostgreSQL migration, adds
new response fields and routes, and does not rewrite existing durable rows.

## Plan of Work

Add project access types and store methods beside the service equivalents. Add
an idempotent migration for `project_memberships` and
`managed_identity_project_bindings`, then implement PostgreSQL CRUD and exact
authorization queries.

Extend administrator APIs to grant/revoke project memberships and manage
project workload bindings. Extend the session and member responses additively
with project memberships. Add `/owner/v1/projects` listing, dashboard, events,
request-details, and export routes. Every handler overwrites `query.project_id`
with the route project UUID before reading usage.

Extend the existing owner workspace and administration views using current
Admin UI 2.0 components. Add project navigation/cards/dashboard and allow the
Members and Managed identities views to configure project access. Regenerate
the deployed static assets.

Update operations, database, Entra integration, README, and changelog text to
describe the shared app role and exact project binding requirement.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

An administrator can add and remove a project membership for an Entra member.
The session response exposes it, and the user can enter the owner workspace and
open only assigned projects. A project owner dashboard returns aggregates and
sanitized request rows only for its route project ID, even if a caller supplies
a different `project_id` query parameter. A request ID belonging to another
project returns the same not-found response as a missing request.

A managed-identity token with `gateway.monitor.read` plus an enabled exact
project binding succeeds. Missing role, mismatched tenant/client/object ID,
disabled binding, or another project fails closed. Existing service-owner tests
continue to pass unchanged.

## Idempotence and Recovery

The migration uses `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT
EXISTS`; rerunning migrations is safe. The new tables reference members and
projects with cascading deletes, so removing either parent cleans up only its
new access rows. Existing tables are not rewritten. Failed frontend builds can
be rerun; generated assets are rebuilt only from the Vite source package.

## Artifacts and Notes

The project dashboard intentionally aggregates usage attributed to project
keys. It does not infer ownership from service links.

## Interfaces and Dependencies

The final API includes additive administrator routes beneath
`/admin-ui/admin/members/{member_id}/projects/{project_id}` and project managed
identity routes, plus read-only owner routes beneath
`/owner/v1/projects/{project_id}`. Both human and workload authorization use
the existing owner Entra verifier; workload tokens require
`gateway.monitor.read` and an exact Relayna project binding.
