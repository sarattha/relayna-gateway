# Resolve Issues 12-15: Admin Projects, Providers, Routes, and Cost Summaries

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Operators can manage projects and provider configuration from the embedded
admin portal, link virtual keys and services to project UUIDs, choose structured
providers and internal routes, and see numeric cost summaries instead of `n/a`
when aggregates are zero. This resolves GitHub issues #12, #13, #14, and #15.

## Progress

- [x] (2026-05-12 00:00Z) Inspected open GitHub issues and current admin API/UI/store shape.
- [x] (2026-05-12 00:00Z) Chose additive schema/API implementation against latest release tag `v0.0.4`.
- [x] (2026-05-12 00:00Z) Add projects and provider configuration schema, traits, stores, and routes.
- [x] (2026-05-12 00:00Z) Fix cost aggregate null handling.
- [x] (2026-05-12 00:00Z) Update admin UI for projects, providers, selectors, service route choices, and cost-mode help.
- [x] (2026-05-12 00:00Z) Add/update tests and documentation.
- [x] (2026-05-12 00:00Z) Run mandatory verification stack.

## Surprises & Discoveries

- Observation: usage aggregate SQL uses `SUM(estimated_cost)` without `COALESCE`, so empty or all-null groups return `null`.
  Evidence: `crates/gateway-store/src/postgres.rs` usage summary, key summary, project summary, timeseries, and breakdown queries.
- Observation: service routes are partly persisted but runtime resolution starts from static `Route::resolve_match`, so arbitrary persisted route patterns need a store-backed path lookup.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs` resolves static routes before fetching service registrations.

## Decision Log

- Decision: Use additive migrations and keep env LiteLLM configuration as a fallback.
  Rationale: Admin API and PostgreSQL schema are compatibility-sensitive; latest release tag is `v0.0.4`.
  Date/Author: 2026-05-12 / Codex.
- Decision: Keep provider secrets write-only and expose only `credential_configured`.
  Rationale: Matches existing service credential handling and the gateway identity/security model.
  Date/Author: 2026-05-12 / Codex.
- Decision: Resolve dynamic service routes through persisted `route_pattern` and treat them as `Route::ServiceWildcard` for policy.
  Rationale: Allows selected admin routes to work without expanding the public route enum for every service.
  Date/Author: 2026-05-12 / Codex.

## Outcomes & Retrospective

Implemented additive admin project and provider configuration APIs, portal
views, service project linking, provider selectors, persisted service route
resolution, and zero-cost usage aggregates. The mandatory verification script
and admin UI test pass.

## Context and Orientation

Admin APIs are wired in `crates/gateway-api/src/app.rs` through the `GatewayData`
trait. PostgreSQL-backed stores live in `crates/gateway-store/src/postgres.rs`.
Core admin DTOs and traits are exported from `crates/gateway-core/src/lib.rs`.
The embedded admin portal is static HTML/CSS/JS under
`crates/gateway-api/src/static/admin-ui/`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.4`. Changes are additive for
admin HTTP routes and PostgreSQL schema. Existing key, service, usage, route,
and env-based LiteLLM behavior must keep working.

## Plan of Work

Add project and provider admin DTOs/traits in gateway-core, implement them in
PostgresStore, and wire them into gateway-api routes. Add schema migration for
projects, service project links, and provider configs. Update cost aggregate
SQL to emit `0.0` instead of `null` for empty/all-null cost sums.

Update Pingora service routing to consult persisted service route patterns when
static route resolution does not match. Resolve enabled LiteLLM provider config
from the DB during request filtering and fall back to env config when absent.

Update the admin portal with Projects and Providers views, project selectors in
Keys and Services, controlled provider policy selectors, route pattern choices,
and cost-mode help. Update docs and tests.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Issues #13-#15 are accepted when overview, key usage, and usage breakdown cost
fields render `$0.0000` for zero aggregate costs and real costs for populated
usage rows.

Issue #12 is accepted when operators can CRUD projects, link keys/services to
projects, configure provider endpoint/secrets without secret exposure, choose
providers through controlled UI controls, select/persist internal route patterns,
and read cost-mode explanations in UI/docs.

## Idempotence and Recovery

The migration uses `IF NOT EXISTS` and backfills projects from existing key
project IDs. If verification fails, fix the failing command and rerun the full
verification script from the repository root.

## Artifacts and Notes

GitHub issues: #12, #13, #14, #15.

## Interfaces and Dependencies

New admin routes: `/admin/projects`, `/admin/projects/{project_id}`,
`/admin/providers`, `/admin/providers/{provider_id}`,
`/admin/providers/{provider_id}/enable`, and
`/admin/providers/{provider_id}/disable`.
