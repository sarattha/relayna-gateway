# Project, Virtual Key, and Service Usage Hierarchy

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md` at the repository root.

## Purpose / Big Picture

After this change, an operator can open the Admin UI Usage view and inspect
which registered services each project consumes through each Relayna virtual
key. The view groups usage as Project -> Virtual Key -> Service and reports the
same request, failure, token, latency, and cost evidence used by the existing
usage dashboard. Existing project, key, service, provider, status, route, and
time filters continue to constrain the hierarchy.

The behavior is observable in the additive
`GET /admin-ui/admin/usage/dashboard` response and in a compact expandable
Admin UI hierarchy. The implementation uses one aggregate PostgreSQL query and
does not issue one query per project, key, or service.

## Progress

- [x] (2026-08-18 13:46Z) Read repository, Admin UI, implementation strategy,
  verification, publishing, and review-handling guidance.
- [x] (2026-08-18 13:46Z) Confirmed a clean `main` checkout at release tag
  `v0.1.26` and created branch `agent/usage-project-key-services`.
- [x] (2026-08-18 13:55Z) Added the project/key/service aggregation and
  additive dashboard field.
- [x] (2026-08-18 14:02Z) Added the expandable Admin UI hierarchy and
  regenerated embedded assets.
- [x] (2026-08-18 14:08Z) Added backend, database-backed, and frontend
  regression tests.
- [x] (2026-08-18 14:12Z) Bumped the workspace release target to `0.1.27` and updated the changelog
  and operator/release documentation.
- [x] (2026-08-18 14:18Z) Exercised the real Gateway UI with Computer Use
  against disposable PostgreSQL and Redis data on desktop and at 390px wide.
- [x] (2026-08-18 14:19Z) Ran focused tests, the complete mandatory
  verification stack, release build and metadata checks, and strict docs build.
- [ ] Commit, push, open a ready-for-review pull request, and wait for the first
  Codex review.
- [ ] Address all actionable first-review findings, reply on each thread,
  resolve them, rerun relevant checks, and update the pull request.

## Surprises & Discoveries

- Observation: The released usage event and export shapes already carry
  `project_id`, `key_id`, and `service_name` together.
  Evidence: `UsageExportRow` in
  `crates/gateway-core/src/observability.rs` and the released PostgreSQL usage
  queries at tag `v0.1.26` expose all three fields.
- Observation: The current Usage UI can filter one virtual key and then inspect
  the service breakdown, but it cannot compare project/key/service
  combinations in one view.
  Evidence: `usage()` and `loadUsage()` in
  `crates/gateway-api/admin-ui/src/main.ts` render independent project, key,
  and service breakdown tables.
- Observation: The user-level Cargo advisory checkout contained untracked
  duplicate advisory files, so an unmodified `cargo audit` could not parse it.
  Evidence: the mandatory verifier failed before scanning dependencies, while
  the same audit passed against a fresh advisory checkout under `/tmp`.
- Observation: The fresh audit identified `RUSTSEC-2026-0258` in transitive
  `h2` 0.4.14.
  Evidence: after updating the lockfile to `h2` 0.4.16, Cargo audit and Trivy
  both reported zero blocking tracked-workspace vulnerabilities.
- Observation: Trivy initially entered the intentionally ignored
  `design-prototypes/` directory and reported local prototype packages that
  are not part of the Git repository.
  Evidence: `git check-ignore` attributes the whole directory to `.gitignore`;
  the final repository-scoped scan excluded it and Semgrep independently
  confirmed that it scans only tracked files.

## Decision Log

- Decision: Preserve the existing usage dashboard route and fields and add one
  new breakdown collection.
  Rationale: The latest release tag is `v0.1.26`, so the dashboard response is
  a released public admin contract. An additive field preserves existing
  callers.
  Date/Author: 2026-08-18 / Codex.
- Decision: Reuse persisted usage events and do not add a database migration.
  Rationale: The required dimensions are already recorded together; the
  feature only needs aggregation and presentation.
  Date/Author: 2026-08-18 / Codex.
- Decision: Return flat project/key/service aggregate rows and shape them into
  the visual hierarchy in the Admin UI.
  Rationale: Flat rows preserve a simple additive wire shape, support exact
  filtering and sorting in one SQL query, and avoid a server-side nesting
  abstraction used by only one client.
  Date/Author: 2026-08-18 / Codex.
- Decision: Treat the repository production freeze as waived for this feature
  and version bump.
  Rationale: The user explicitly authorized breaking the production freeze on
  2026-08-18. Release safety checks remain mandatory.
  Date/Author: 2026-08-18 / Codex.
- Decision: Include the `h2` 0.4.16 lockfile update in release `0.1.27`.
  Rationale: the vulnerability is in the tracked runtime dependency graph, the
  fixed patch release preserves the public contract, and the user authorized
  the release change despite the production freeze.
  Date/Author: 2026-08-18 / Codex.

## Outcomes & Retrospective

The implementation delivers one additive flat dashboard collection and an
expandable Project -> Virtual key -> Service view using safe key prefixes. It
requires no schema migration and preserves every existing dashboard field and
filter. Regression coverage proves aggregation, filtering, response shape,
safe label resolution, generated assets, and responsive disclosure styling.

Computer Use validated the actual Gateway with 425 usage events across two
projects, three keys, and three services: the displayed request, failure,
token, latency, and cost values matched the API, project filtering reduced five
combinations to three, and the hierarchy remained usable at desktop and 390px
width. The disposable Gateway, PostgreSQL, and Redis processes were stopped
and their seeded data was removed after validation.

The complete mandatory stack passed: Rust formatting, clippy, workspace tests,
Cargo audit and deny, cargo-machete, nextest, Trivy, Gitleaks, and Semgrep. The
workspace release build, Admin UI build and tests, `v0.1.27` metadata validator,
strict MkDocs build, and diff whitespace check also passed. Publication and
first-review handling remain in progress.

## Context and Orientation

A Relayna virtual key is the only credential external callers use with the
Gateway. Every authenticated request creates a usage event that attributes the
request to a virtual key and, when applicable, a project and registered
service. `crates/gateway-core/src/observability.rs` defines usage query and
response types. `crates/gateway-store/src/postgres.rs` owns PostgreSQL usage
aggregations. `crates/gateway-api/src/app.rs` wires the dashboard endpoint and
contains the in-memory test implementation. The Admin UI source of truth is
`crates/gateway-api/admin-ui/src/`; generated assets under
`crates/gateway-api/src/static/admin-ui/` must be regenerated rather than
edited manually. Static UI contract tests live in `tests/admin-ui.test.mjs`.

The hierarchy is analytical, not an authorization mapping. It reports the
project and key attribution captured on each historical usage event, so later
changes to a key's ownership do not rewrite historical consumption.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.26`; the released
`GET /admin-ui/admin/usage/dashboard` response gains an additive breakdown
field while every existing field, route, filter, and usage-event schema remains
unchanged. No compatibility shim or migration is needed.

## Plan of Work

Add a serializable project/key/service breakdown row to gateway-core and expose
it from the existing dashboard breakdown object. In gateway-store, run one
filtered aggregate query grouped by `project_id`, `key_id`, and
`service_name`, applying the existing breakdown ordering and limit semantics.
In the in-memory test store, derive the same rows from filtered export data so
API regression tests exercise the real response shape.

Render the new rows in the Usage view as nested native disclosure sections:
project, then virtual key, then an exact service aggregate table. Resolve IDs
to the project name and safe virtual-key prefix already loaded by the view;
never display raw virtual-key material. Use existing table and escaping helpers
and only add CSS needed for compact disclosure spacing and responsive tables.

Add Rust tests proving correct project/key/service grouping and filter
propagation, plus Admin UI tests proving the hierarchy renderer, safe labels,
and additive response field are consumed. Regenerate embedded Admin UI assets.

Bump the release target from `0.1.26` to `0.1.27`, add a dated changelog entry,
and update current release, deployment, usage, and operator documentation that
describes this feature. Run the repository release metadata validator.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    npm run build:admin-ui
    npm test
    cargo test -p gateway-api usage_dashboard
    cargo test -p gateway-store usage
    python3 scripts/validate-release-metadata.py v0.1.27
    bash .codex/skills/code-change-verification/scripts/run.sh
    mkdocs build --strict

Start the actual Gateway with disposable PostgreSQL and Redis instances seeded
with deterministic project, key, service, and usage rows, then use Computer Use
against Chrome to sign in, open Usage,
expand project and key disclosures, inspect metrics, apply a filter, and check
desktop and narrow-window layouts.

After validation, explicitly stage only files in this feature, commit, push,
open a ready-for-review pull request, and use thread-aware GitHub review reads
to monitor the first Codex review.

## Validation and Acceptance

The dashboard response must include distinct aggregate rows for each unique
project/key/service combination and must apply existing query filters before
grouping. Request, success, failure, token, latency, fallback, and cost totals
must match the source usage events.

The Usage UI must display the hierarchy under human-readable project names and
safe virtual-key prefixes. Expanding one project and key must reveal its
service rows without exposing key secrets. Existing usage tables and filters
must still render and work. The layout must remain usable at desktop and mobile
widths with wide tables scrolling rather than compressing.

The Admin UI build and tests, focused Rust tests, release metadata validation,
full Rust formatting/lint/test stack, strict documentation build, and Computer
Use checks must pass. The pull request must receive its first Codex review; all
actionable threads from that review must be addressed, replied to, resolved,
and supported by rerun checks.

## Idempotence and Recovery

Builds and tests are safe to rerun. Admin UI generation deterministically
replaces checked-in assets from the Vite source. The aggregate query is
read-only and introduces no persisted state. If a check fails, fix the scoped
cause and rerun the complete relevant stack. If PR publication is interrupted,
inspect the current branch, commit, remote tracking branch, and existing PR
before retrying so no duplicate PR is created.

## Artifacts and Notes

Latest release boundary at plan creation:

    v0.1.26 -> 1ce793e Merge pull request #104 from sarattha/codex/entra-single-application

Feature branch at plan creation:

    agent/usage-project-key-services

## Interfaces and Dependencies

At completion, `UsageDashboardBreakdowns` exposes an additive collection of
rows containing `project_id: Option<Uuid>`, `key_id: Uuid`, `service_name:
String`, and `summary: UsageSummary`. `PostgresStore::usage_dashboard` populates
the collection with one SQL aggregate. The in-memory `UsageQueryStore` used by
gateway-api tests produces the same semantics. The Admin UI consumes the field
without adding a new route or JavaScript dependency.
