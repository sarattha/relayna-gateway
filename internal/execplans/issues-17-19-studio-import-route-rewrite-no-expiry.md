# Issues 17-19 Studio Import, Route Rewrite, and No-Expiry Keys

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This plan follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators should be able to import Relayna Studio services into Gateway from the
Admin portal without copying Studio IDs by hand, service route aliases should
forward to the expected upstream path, and virtual keys should have an explicit
non-expiring mode in the Admin UI and API.

## Progress

- [x] (2026-05-13T14:52Z) Read issues #17, #18, #19, paired Studio issue #72,
  `internal/design-manifesto.md`, and the implementation/verification skills.
- [x] (2026-05-13T14:52Z) Established compatibility boundary: latest release
  tag is `v0.0.5`; changes are additive or preserve current released behavior.
- [x] (2026-05-13T14:52Z) Implement Studio catalog configuration, client, preview route, import
  mapping, Admin UI picker, route alias rewrite, no-expiry UI, docs, and tests.
- [x] (2026-05-13T14:52Z) Run `$code-change-verification`; fmt, clippy, and
  full workspace tests all passed.

## Surprises & Discoveries

- Studio issue #72 proposes `GET /studio/gateway/services` with a top-level
  `services` array and gateway-safe metadata. Gateway can target that endpoint
  first and optionally tolerate the existing `/studio/services` shape where
  field names differ.
- Current Postgres Studio upsert overwrites route pattern, project, and cost
  hints on re-import. Issue #17 requires preserving Gateway-owned runtime/local
  fields by default.
- Current proxy route lookup can match a persisted wildcard alias, but rewrite
  still strips only `/services/{service_name}`.
- Admin API already models `expires_at: null` distinctly on patch via
  `Option<Option<DateTime<Utc>>>`; the UI needs an explicit create/edit control
  and tests should lock the API behavior.
- Adding the Studio catalog client required `reqwest`; `Cargo.lock` expanded
  accordingly.

## Decision Log

- Decision: Additive admin API route `GET /admin/studio/services`; keep
  existing `POST /admin/services/import` route and extend its request shape with
  optional Studio catalog hints.
  Rationale: Existing callers keep working while the Admin portal gains a real
  picker.
  Date/Author: 2026-05-13 / Codex.
- Decision: Preserve existing service runtime/local fields on Studio re-import,
  including route pattern, project link, enabled state, upstream URL,
  credential, allowed methods, limits, fallback services, and cost settings.
  Rationale: Gateway remains the enforcement and runtime owner; Studio metadata
  is only a catalog source.
  Date/Author: 2026-05-13 / Codex.
- Decision: Rewrite persisted wildcard aliases by the matched service
  `route_pattern`, but leave exact patterns unchanged.
  Rationale: This satisfies issue #18 while preserving exact route semantics.
  Date/Author: 2026-05-13 / Codex.

## Outcomes & Retrospective

Implemented all three issue outcomes. Gateway now has optional Studio catalog
configuration and a protected `GET /admin/studio/services` preview endpoint.
The Admin portal opens an import picker, maps selected Studio catalog rows into
the existing import endpoint, and imported/re-imported Studio services preserve
Gateway-owned runtime fields. Persisted wildcard route aliases now rewrite
upstream paths by the matched route pattern while exact patterns remain
unchanged. Virtual key create/edit flows expose explicit `No expiration`
controls and list non-expiring keys unambiguously.

Verification passed with `.codex/skills/code-change-verification/scripts/run.sh`
and `node tests/admin-ui.test.mjs`.

## Context and Orientation

Gateway API lives in `/Users/jobz/Works/relayna-gateway/crates/gateway-api`.
The Axum admin routes are in `src/app.rs`; Admin UI assets are under
`src/static/admin-ui`. Gateway core service models and validation are in
`crates/gateway-core/src/services.rs`. Durable Postgres service and key logic is
in `crates/gateway-store/src/postgres.rs`. Pingora proxy path rewriting is in
`crates/gateway-proxy/src/pingora_plane.rs`.

A virtual key is the Relayna client credential stored hashed in Gateway. A
service registration maps a Gateway route pattern to an internal service
upstream. A Studio import records Studio-owned catalog identity
(`studio_service_id`, name/status hints) while Gateway owns routability,
credentials, policy, budgets, and usage.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.5`. The plan adds a new admin
catalog route, keeps the existing import route, keeps `expires_at: null` API
semantics, and fixes alias rewriting without changing exact route behavior.
Persisted schema changes are not expected.

## Plan of Work

Add Studio catalog types and mapping helpers in gateway-core. Add API config for
`RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN`, a small HTTP catalog
client in gateway-api, and a protected `GET /admin/studio/services` endpoint.
Extend `POST /admin/services/import` to accept mapped catalog records.

Adjust Postgres and test memory-store import upsert so re-imports preserve
Gateway-owned runtime fields while updating Studio identity/name metadata only.

Update proxy rewrite logic to strip the wildcard prefix from the matched
persisted `route_pattern`; keep canonical `/services/{service_name}` rewriting.

Update Admin UI keys with explicit "No expiration" create/edit controls and
non-expiring display text. Update services UI so "Import from Studio" opens a
catalog picker, imports selected rows, and reports Studio configuration errors.

Update docs and tests, then run the gateway verification stack.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

The repository wrapper `.codex/skills/code-change-verification/scripts/run.sh`
runs these in fail-fast order.

## Validation and Acceptance

Acceptance is proven by Rust tests for Studio catalog mapping, import
idempotence and runtime-field preservation, route alias rewrite with query
preservation, and key create/patch expiration behavior. UI tests should verify
the Studio picker affordance and no-expiry controls are present. Docs should
name the required Studio environment variables and failure behavior when Studio
is unavailable.

## Idempotence and Recovery

All import operations are idempotent by `studio_service_id`. If a test or format
step fails, fix the smallest failing area and rerun the full verification
script. Because this plan does not add migrations, recovery does not require
database rollback.

## Artifacts and Notes

Issue #17: Gateway Admin portal Studio import picker.
Issue #18: persisted wildcard route alias path rewrite.
Issue #19: explicit no-expiration virtual key option.
Studio issue #72: proposed `GET /studio/gateway/services` export contract.

## Interfaces and Dependencies

New environment variables:

- `RELAYNA_STUDIO_BASE_URL`: optional Studio backend base URL.
- `RELAYNA_STUDIO_TOKEN`: optional bearer token for the Studio catalog request.

New Admin route:

- `GET /admin/studio/services`: protected by operator token; returns importable
  Studio catalog rows and mapped Gateway import payloads.
