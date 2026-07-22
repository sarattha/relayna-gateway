# OpenAPI Endpoint Discovery and Pricing

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

After this change, an operator can point a registered internal service at its
relative `/openapi.json` document, preview the discovered HTTP operations, and
explicitly sync a durable endpoint catalog and endpoint pricing rules into
Relayna Gateway. Standard Relayna runtime, task-observability, DLQ,
failed-task, and health endpoints default to cost mode `none`; an operator can
change any discovered endpoint to `fixed` or `passthrough`. Billable endpoints
continue to support request-body pricing selectors, so OCR `POST /ocr` can use
the service default while `engine=docint` resolves to its existing higher
price. OpenAPI is a control-plane discovery input only and is never fetched on
the proxy request path.

## Progress

- [x] (2026-07-22 02:20Z) Inspected released service pricing, routing, budget,
  Admin API/UI, and PostgreSQL contracts and the deployed OCR OpenAPI 3.1
  document in `vm-machine01` namespace `common`.
- [x] (2026-07-22 02:30Z) Established the `v0.1.20` compatibility boundary and
  selected an additive migration and API design.
- [x] (2026-07-22 04:05Z) Implemented OpenAPI types, validation, endpoint template matching, Relayna
  endpoint classification, and endpoint-aware pricing resolution in
  `gateway-core`.
- [x] (2026-07-22 04:15Z) Added persisted OpenAPI snapshots and endpoint pricing rules through an
  additive PostgreSQL migration and store mappings.
- [x] (2026-07-22 04:35Z) Added secure preview/sync Admin API endpoints with audit events and no
  redirects, no credential forwarding, same-origin relative paths, timeout,
  JSON content-type, and document-size enforcement.
- [x] (2026-07-22 04:50Z) Applied endpoint pricing in the Pingora service path before budget
  reservation and reconcile body selectors afterward.
- [x] (2026-07-22 05:20Z) Added Admin UI preview, sync, drift visibility, and per-endpoint pricing
  controls, then regenerate checked-in assets.
- [x] (2026-07-22 05:40Z) Added core, proxy, API, Admin UI, and migration coverage and updated
  operator documentation.
- [x] (2026-07-22 09:55+07) Ran focused tests, Admin UI tests, workspace coverage, the mandatory full verification stack,
  and a fresh Docker real-environment test against the deployed OCR contract.
- [x] (2026-07-22 10:10+07) Committed and pushed PR #96, requested and monitored
  the first Codex review, addressed both actionable comments, and reran the
  complete mandatory verification stack. Review replies and thread resolution
  follow the review-fix push.

## Surprises & Discoveries

- Observation: The deployed OCR service exposes OpenAPI 3.1 at
  `/openapi.json`, with 21 paths and 22 operations. Only `POST /ocr` accepts
  multipart OCR work; the remaining operations are health, task visibility,
  Relayna runtime, DLQ, or failed-task operations.
  Evidence: Read-only `kubectl exec deploy/ocr-service-api` inspection in the
  `common` namespace on `vm-machine01`.
- Observation: The OCR specification has no tags, security declarations, or
  vendor pricing extensions, so billability cannot be inferred from OpenAPI
  alone.
  Evidence: The deployed 21 KB document reports no operation-level `x-*`
  extensions.
- Observation: Current budget preflight reserves the maximum service/body-rule
  fixed cost before body selectors are reconciled. Without endpoint scoping, a
  free Relayna endpoint could reserve the unrelated OCR `docint` ceiling.
  Evidence: `prepare_service_cost_for_ctx` and
  `service_preflight_estimated_cost` in the proxy/core pricing path.
- Observation: The Admin UI package builds through Vite but its existing
  TypeScript source does not pass a standalone `tsc --noEmit` invocation; the
  baseline contains broad DOM and inferred-object type errors outside this
  feature.
  Evidence: `npx tsc -p crates/gateway-api/admin-ui/tsconfig.json --noEmit`
  reported pre-existing errors throughout `main.ts` and the design system,
  while the production Vite build and static UI test suite pass.
- Observation: Service body selectors are intentionally service-wide. A
  billable endpoint therefore reserves the highest fixed selector ceiling,
  even when its final base price is lower; only an endpoint explicitly set to
  `none` skips that ceiling.
  Evidence: The Docker environment allowed free `GET /events/feed` under a
  `$0.001` per-request cap, denied it after the endpoint changed to `$0.02`
  while the OCR selector ceiling remained `$0.50`, and allowed it under a
  `$0.60` cap before recording the final `$0.02` cost.
- Observation: A matched body selector can intentionally have no display name;
  using an absent name to infer that no body rule matched incorrectly relabels
  its usage with the endpoint operation ID.
  Evidence: The first Codex review of PR #96 identified the ambiguity, and a
  proxy regression test now distinguishes match presence from rule naming.

## Decision Log

- Decision: Preserve all existing pricing behavior when no endpoint pricing
  rule matches.
  Rationale: Service pricing and its persisted JSON shape are released in
  `v0.1.20`; additive fallback is backward compatible.
  Date/Author: 2026-07-22 / Codex.
- Decision: Use explicit preview then sync operations. Never fetch OpenAPI on a
  user request.
  Rationale: Pricing must remain available when documentation is unavailable,
  and drift must not silently change billing.
  Date/Author: 2026-07-22 / Codex.
- Decision: Store a compact endpoint snapshot and endpoint rules as additive
  JSONB columns on the existing service registration.
  Rationale: Runtime already loads a service registration as one pricing
  snapshot; this avoids a new database lookup per proxied request while keeping
  the patch scoped.
  Date/Author: 2026-07-22 / Codex.
- Decision: A matched endpoint in cost mode `none` is authoritative and skips
  body selectors. For other matched endpoints, body selectors may override the
  endpoint base price.
  Rationale: Relayna default endpoints must remain free, while `POST /ocr` must
  retain `engine=docint` pricing.
  Date/Author: 2026-07-22 / Codex.
- Decision: OpenAPI discovery accepts only a relative absolute path such as
  `/openapi.json`, uses the registered upstream origin, does not forward the
  service credential, disables redirects, refuses non-JSON/oversized content,
  and does not resolve external references.
  Rationale: This bounds SSRF and credential-exfiltration risk without adding
  an arbitrary URL fetcher.
  Date/Author: 2026-07-22 / Codex.

## Outcomes & Retrospective

The implementation now discovers the deployed OCR service's 22 OpenAPI
operations, persists a method/path catalog, defaults its 21 Relayna operations
to `none`, keeps `POST /ocr` billable, and composes endpoint pricing with the
existing multipart `engine=docint` selector. Preview/sync is authenticated,
audited, bounded, redirect-free, same-origin, and credential-free.

Workspace coverage ran 243 tests successfully and reported 67.11% total line
coverage, including 91.92% for `gateway-core/src/services.rs`. The mandatory
verification stack passed formatting, clippy, workspace tests, audit, deny,
machete, nextest (246 tests), Trivy, Gitleaks, and Semgrep. The production Vite
build and 30 static Admin UI tests passed.

A fresh Docker image and isolated PostgreSQL/Redis/OCR stack applied migration
`20260722000100`, synced all 22 endpoints, recorded free Relayna traffic with
no cost, recorded multipart `docint` at `$0.50`, and recorded an operator-
changed `/events/feed` price at `$0.02`. Chrome verified the real Admin portal
at 1440×1000 and 390×844 with no console errors or page-level mobile overflow.

The first Codex review produced two actionable findings. The proxy now keeps an
anonymous matched body rule anonymous instead of inheriting the endpoint
operation ID, and the Admin UI service PATCH now persists an edited OpenAPI
source path. Both fixes have regression coverage, and the mandatory verification
stack passed again after the changes.

Remaining limitation: endpoint rules and body selectors are separate layers;
body selectors remain service-wide for compatibility, so every billable
endpoint reserves their highest fixed ceiling at preflight. An explicit
endpoint mode of `none` bypasses that ceiling.

## Context and Orientation

`crates/gateway-core/src/services.rs` owns registered-service types,
validation, body selector pricing, and route utilities. Add endpoint catalog,
endpoint pricing, OpenAPI preview/sync request/response types, Relayna default
classification, template matching, and compatible pricing composition there.

`crates/gateway-store/src/postgres.rs` and
`crates/gateway-store/migrations/` own durable service state. Add nullable
OpenAPI source/hash/sync metadata and JSONB endpoint catalog/rules with safe
defaults, then map them into `ServiceRegistration` and `ServiceResponse`.

`crates/gateway-api/src/app.rs` owns authenticated operator routes. Add preview
and sync handlers under `/admin-ui/admin/services/{service_name}/openapi/*`.
The preview fetches and validates the document; sync refetches it, checks the
expected hash, merges existing operator prices for unchanged endpoints,
defaults new Relayna endpoints to `none`, persists the snapshot, and records an
audit event.

`crates/gateway-proxy/src/pingora_plane.rs` owns service request routing,
pricing, policy, and budget reservation. Resolve method plus rewritten upstream
path during header processing, use the matched endpoint price for policy and
budget preflight, skip unrelated body-price ceilings for `none` endpoints, and
apply body selectors only for billable endpoint bases.

`crates/gateway-api/admin-ui/src/main.ts` is the Admin UI source of truth. Add
compact controls in the existing service Usage pricing section and rebuild
`crates/gateway-api/src/static/admin-ui/`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.20`. The public Admin service
response, service pricing JSON, PostgreSQL `service_registrations`, usage cost
metadata, and proxy billing behavior are released surfaces. Preserve existing
fields and semantics, add forward-only nullable/defaulted columns and response
fields, accept legacy payloads unchanged, and fall back to service/body pricing
when no endpoint rule matches.

## Plan of Work

First add plain core types and matching functions with tests. Then add the
migration and store mappings so the new state round-trips. Add secure Admin API
discovery next, followed by proxy pricing/budget integration. Add the Admin UI
workflow and regenerate assets only after the API shape is stable. Finish with
focused and full verification, a fresh Docker image, real OCR OpenAPI discovery
and multipart requests, screenshots, and the requested PR/review workflow.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-core services
    cargo test -p gateway-proxy service_pricing
    cargo test -p gateway-api openapi
    npm --prefix crates/gateway-api/admin-ui ci
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

Build a uniquely tagged image from the final commit, run it with isolated
PostgreSQL/Redis ports and a mock or deployed-compatible OCR upstream, preview
and sync `/openapi.json`, then prove `POST /ocr engine=docint` records `0.5`
while representative Relayna endpoints record no cost and reserve no budget.

## Validation and Acceptance

Existing service payloads and registrations without endpoint rules behave
unchanged. Invalid, cross-origin, redirecting, non-JSON, oversized, or
non-OpenAPI discovery responses fail closed without persisting changes. Preview
does not mutate state. Sync requires a matching preview hash, retains explicit
prices for unchanged endpoints, defaults newly discovered Relayna endpoints to
`none`, and surfaces stale endpoints without silently changing their active
rule. Endpoint template matching is method-aware and deterministic. A `none`
endpoint creates a usage event but no cost/budget reservation. A fixed endpoint
reserves its endpoint/body ceiling and reconciles the final body rule. Admin UI
and API actions require service-update scope and are audited.

## Idempotence and Recovery

The migration uses additive columns with defaults and is safe to rerun through
SQLx migration tracking. Preview is read-only. Repeated sync with the same hash
and endpoints preserves explicit prices. A failed fetch or hash mismatch leaves
the persisted snapshot untouched. Docker validation uses uniquely named
containers and ports and does not alter existing workloads. Production rollout
can retain the previous image because old rows remain readable after the
additive migration.

## Artifacts and Notes

The deployed OCR OpenAPI source inspected during planning:

    service: ocr-service-api.common.svc.cluster.local:8000
    source: /openapi.json
    openapi: 3.1.0
    operations: 22
    billable submission: POST /ocr

## Interfaces and Dependencies

Use existing `reqwest`, `serde_json`, `sha2`, `url`, Axum, SQLx, and Pingora
dependencies. Do not add an OpenAPI code-generation dependency: only the
bounded `openapi`, `info`, and `paths` metadata needed for discovery is parsed.
Keep endpoint matching and pricing framework-agnostic in `gateway-core`; keep
network fetching and operator authorization in `gateway-api`; keep durable
state in `gateway-store`; keep request lifecycle and budget handling in
`gateway-proxy`.
