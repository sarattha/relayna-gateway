# Service Endpoint Failure Monitoring

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain it in accordance with `PLANS.md`.

## Purpose / Big Picture

Administrators need to identify which registered service operation produced a
failed request. After this change, the Usage view, usage APIs, and exports show
the HTTP method, query-free upstream-relative path, matched OpenAPI template,
and numeric status code for authenticated registered-service traffic. Endpoint
aggregates group templated operations while retaining a concrete fallback for
services without a synced catalog.

## Progress

- [x] (2026-08-06 00:00Z) Confirmed clean `main`, latest release `v0.1.22`, and approved production-freeze exception.
- [x] (2026-08-06 00:00Z) Created `codex/service-endpoint-failure-monitoring`.
- [x] (2026-08-06 16:10Z) Added endpoint usage metadata, migration, query surfaces, and focused tests.
- [x] (2026-08-06 16:10Z) Added Admin Usage filters, endpoint breakdown, recent-row detail, generated assets, and the isolated real-environment harness.
- [x] (2026-08-06 16:13Z) Ran focused core, proxy, API, and Admin UI tests plus the complete mandatory verification stack; all passed.
- [ ] Publish the ready PR and address the first Codex review.
- [ ] Build and verify the isolated real environment with Computer Use.
- [ ] Prepare `0.1.23`, run release checks, and finalize the PR.

## Surprises & Discoveries

- Observation: Existing usage rows store `/services/*` plus `service_name`, but
  neither usage nor debug records preserve the method or concrete endpoint.
  Evidence: `UsageEvent` has no endpoint fields and `Route::ServiceWildcard`
  serializes as `/services/*`.
- Observation: The repository's existing real-LiteLLM Compose fixture already
  demonstrates the expected image-first test style, but it does not exercise
  registered service usage. A smaller isolated service fixture avoids unrelated
  provider dependencies.
  Evidence: `internal/test-reports/litellm-real-passthrough/run.sh` tests only
  LiteLLM route modes while the new harness targets ports `19280..19282`.

## Decision Log

- Decision: Add nullable fields and additive API response/query fields instead
  of replacing the released usage shape.
  Rationale: `v0.1.22` is the released boundary; additive storage preserves old
  rows and clients while implementing the freeze-approved feature.
  Date/Author: 2026-08-06 / Codex.
- Decision: Persist uppercase method, query-free upstream-relative concrete
  path, and the most-specific synced OpenAPI template. Display and aggregate by
  template with path fallback.
  Rationale: This balances useful fallback diagnostics with bounded catalog
  aggregation and avoids query-string leakage.
  Date/Author: 2026-08-06 / Codex.
- Decision: Keep existing failure semantics (`status_code >= 400`) and do not
  add an upstream-versus-Gateway origin split or endpoint Prometheus labels.
  Rationale: The user selected all authenticated routed failures; endpoint
  labels would create avoidable metric-cardinality risk.
  Date/Author: 2026-08-06 / Codex.

## Outcomes & Retrospective

Pending implementation and verification.

## Context and Orientation

`gateway-core` owns usage and OpenAPI endpoint matching types. `gateway-proxy`
constructs usage events after proxy completion. `gateway-store` persists and
queries PostgreSQL usage data. `gateway-api` exposes admin usage endpoints and
embeds the Vite Admin UI source and generated assets.

An effective endpoint is `endpoint_template` when a synced OpenAPI operation
matches the request method and relative path, otherwise `endpoint_path`.
Historical and non-service rows have neither value and are excluded from the
endpoint breakdown.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.22`. The PostgreSQL
`usage_events` schema and admin usage response/export shapes are released.
Use an idempotent forward migration with nullable columns, append CSV columns,
and make all new query/response fields additive. Do not backfill information
that was never recorded. The user explicitly approved breaking the production
freeze for this feature; no existing proxy route or response behavior changes.

## Plan of Work

Extend usage types with method/path/template metadata and add a core helper
that returns the most-specific OpenAPI operation for a method and relative
path. Capture query-free endpoint context before proxy URI rewriting and attach
it to every successful or failed usage event after route and key resolution.

Add nullable PostgreSQL columns and a partial expression index. Extend insert,
event-page, export, filter, and breakdown queries. Add exact `method`,
`endpoint`, and `status_code` usage query parameters and expose endpoint
breakdowns as `METHOD /path`.

Update the Admin Usage view with filters, suggestions, endpoint aggregates,
and recent-request detail. Rebuild checked-in assets. Add a self-contained
Docker harness with PostgreSQL, Redis, Gateway, and a deterministic service;
verify it through API assertions and the local Admin UI using Computer Use.

After the first Codex review is addressed, bump active release metadata to
`0.1.23`, document the feature and migration, run release validation, and
leave a ready unmerged PR with passing checks.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    npm ci
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh
    python3 scripts/validate-release-metadata.py v0.1.23
    docker build -t relayna-gateway:0.1.23 .

The release pass also runs the documented audit, deny, machete, nextest,
filesystem security, image security, and strict MkDocs checks when their tools
are available.

## Validation and Acceptance

A templated service success and failure must store the same method/template
with their concrete relative paths and exact status codes. An unlisted service
endpoint must fall back to its concrete query-free relative path. Exact method,
endpoint, and status-code filters must constrain summary, event, export, and
breakdown results consistently. The Admin UI must show the failing endpoint and
numeric code at desktop and narrow widths without exposing credentials or
query parameters.

The final branch must pass generated-asset tests, mandatory Rust verification,
the isolated Docker harness, Computer Use visual verification, release
metadata validation, security/documentation checks, the final image build and
scan, PR CI, and have no unresolved first-review Codex threads.

## Idempotence and Recovery

The migration uses `IF NOT EXISTS`. The real-environment harness uses a unique
Compose project and named local image, and cleanup targets only that project's
containers and volumes. Failed verification is fixed and rerun from the start.
No production deployment, merge, release tag, or published image is performed.

## Interfaces and Dependencies

`UsageEvent` and `UsageExportRow` gain optional `http_method`,
`endpoint_path`, and `endpoint_template`. `UsageQuery` gains optional `method`,
`endpoint`, and integer `status_code`. `UsageDashboardBreakdowns` gains
`endpoints`. Existing admin usage routes carry these additive shapes; no new
route or environment variable is required.
