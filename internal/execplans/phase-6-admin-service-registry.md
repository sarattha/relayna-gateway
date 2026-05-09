# Phase 6 Admin and Studio Service Registry

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Let Relayna Gateway register, import, inspect, update, disable, and delete
internal service passthrough targets through protected admin APIs and by syncing
services already registered in Relayna Studio. Operators should not need to
restart the gateway with a single shared `INTERNAL_SERVICE_BASE_URL`. After
this phase, Relayna Gateway should route `/summary`, `/translation`, `/ocr`,
`/embeddings`, and `/services/{service_name}/*` to service-specific upstreams
with per-service credentials, limits, pricing, and health visibility.

## Progress

- [x] Confirm current Phase 3 through 5 gateway-only service passthrough
      foundations exist for service route names, `allowed_services`, usage
      service attribution, and provider/service health.
- [x] Establish compatibility boundary for service registry APIs, PostgreSQL
      schema, service route matching, credential handling, and usage fields.
- [x] Add persistent service registry schema.
- [x] Add protected admin service registry APIs.
- [x] Add Relayna Studio service import and sync behavior.
- [x] Load active service registrations in the proxy path.
- [x] Route static and wildcard service passthrough through registered service
      upstreams.
- [x] Enforce key policy against registered service names.
- [x] Record usage, cost, latency, fallback count, and provider health by
      registered service.
- [x] Add registry, routing, credential, policy, and usage tests.
- [x] Run `$code-change-verification` and record results.

## Surprises & Discoveries

- No `v*` release tag exists locally, so the existing
  `INTERNAL_SERVICE_BASE_URL` behavior is treated as unreleased branch-local
  shortcut behavior and is replaced directly by registry-based routing.
- The current repository has unit/API coverage but no live PostgreSQL-backed
  store integration harness, so service persistence is covered by additive SQL
  migration plus compile-time store wiring in this pass.

## Decision Log

- Decision: Admin service registration is a gateway control-plane feature, not
  Relayna runtime task integration.
  Rationale: Operators need to register synchronous passthrough services such
  as summary and translation without coupling this phase to async task
  submission.
  Date/Author: 2026-05-09 / Codex.

- Decision: Store only service credential references or encrypted/opaque secret
  material; never return service tokens through admin read APIs.
  Rationale: The gateway owns internal credentials and must not leak service
  tokens to external clients or Studio consumers.
  Date/Author: 2026-05-09 / Codex.

- Decision: Relayna Studio can be a service catalog source, but Gateway remains
  the enforcement point.
  Rationale: Studio may already know service identity and metadata, while the
  gateway must still own upstream routing, credential injection, policy checks,
  budgets, usage, and fail-closed behavior.
  Date/Author: 2026-05-09 / Codex.

- Decision: Phase 6 Studio import/sync is request-body driven through protected
  admin APIs, not an outbound Gateway-to-Studio catalog client.
  Rationale: This delivers deterministic import/sync semantics without adding
  Studio client configuration, auth, retry, and availability behavior to the
  gateway runtime in this phase.
  Date/Author: 2026-05-10 / Codex.

- Decision: Store service credentials as opaque gateway-owned Postgres secret
  material for the MVP and redact them from every admin read/list response.
  Rationale: Per-service credential injection is required for routing now;
  KMS/envelope encryption can be added later without exposing raw credentials
  through public API shapes.
  Date/Author: 2026-05-10 / Codex.

- Decision: Remove the environment-based internal service upstream shortcut
  from startup config.
  Rationale: No released compatibility tag exists, and Phase 6 makes durable
  service registrations the source of truth for internal service upstreams.
  Date/Author: 2026-05-10 / Codex.

## Outcomes & Retrospective

Implemented the first Phase 6 pass: core service registry contracts,
PostgreSQL registry migration and store methods, protected admin service APIs,
request-body Studio import/sync, dynamic proxy service lookup, registered
credential injection, wildcard path rewriting, service policy enforcement, and
redacted admin responses.

Verification: `bash .codex/skills/code-change-verification/scripts/run.sh`
passed on 2026-05-10. The script ran `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo test --workspace --all-features`.

## Context and Orientation

Phase 4 introduced gateway-owned service passthrough routes and a single
environment-configured internal service upstream. Phase 6 replaces that MVP
shortcut with a durable service registry that can be managed locally through
Gateway admin APIs and populated from services already registered in Relayna
Studio.

Important terms:

- Service registration: durable config describing a gateway-owned service
  passthrough target, such as `summary`, `translation`, `ocr`, or a custom
  `/services/{service_name}/*` service.
- Service upstream: the internal HTTP base URL that receives sanitized
  passthrough requests.
- Service credential: the gateway-owned token or secret reference injected
  into upstream service requests after client credentials are stripped.
- Service route policy: per-key allowlist that controls which registered
  service names a virtual key may use.
- Studio service: a service definition created in Relayna Studio that Gateway
  can import or periodically sync into its local registry.
- Local override: Gateway-owned fields that may differ from Studio metadata,
  such as upstream URL, credential reference, enabled state, timeout, body
  limit, cost mode, and fallback services.

Expected areas:

- `crates/gateway-api/`: protected admin routes for creating, reading,
  updating, disabling, deleting, importing, and syncing service registrations.
- `crates/gateway-core/`: service registration request/response types, route
  resolution contracts, Studio import contracts, service policy decisions,
  validation, and error shapes.
- `crates/gateway-proxy/`: dynamic service upstream lookup, path rewriting,
  credential injection, fallback classification, and usage attribution.
- `crates/gateway-store/`: PostgreSQL service registry schema and reads/writes.
- `crates/gateway-telemetry/`: service registration audit fields, provider
  health labels, and redaction helpers.
- `tests/`: admin service lifecycle, passthrough routing, policy denial,
  credential stripping, disabled service, and usage recording tests.

## Compatibility Boundary

Compatibility boundary: compare service registry routes, request and response
shapes, Studio sync contracts, PostgreSQL schema, service name matching,
service credential handling, and usage/provider-health fields against the
latest release tag before editing.

If no `v*` release tag exists, treat current service passthrough config as
unreleased branch-local behavior. Use additive PostgreSQL migrations because
service registrations are durable once deployed.

Service admin read responses must never include raw internal service tokens.
If credentials are stored directly for local MVP usage, expose only
`credential_configured: true` and last-updated metadata.

Studio-sourced service metadata may be imported into Gateway, but Studio must
not become the runtime authorization decision point. Gateway must cache enough
local service state to fail closed when Studio is unavailable.

## Plan of Work

Add a PostgreSQL `service_registrations` table keyed by service name. Include
Studio service ID when applicable, route pattern, upstream base URL, enabled
flag, allowed methods, timeout, body-size limit, cost mode, fixed estimated
cost, credential reference or credential secret, optional fallback service
names, service source, sync status, last synced timestamp, created/updated
timestamps, and disabled timestamp.

Add protected admin APIs:

    POST /admin/services
    GET /admin/services
    GET /admin/services/{service_name}
    PATCH /admin/services/{service_name}
    DELETE /admin/services/{service_name}
    POST /admin/services/{service_name}/disable
    POST /admin/services/{service_name}/enable
    POST /admin/services/import
    POST /admin/services/sync
    GET /admin/services/{service_name}/sync-status

Use the existing admin bearer token middleware behavior. Validate service names
as lowercase URL-safe identifiers. Reject duplicate service names, invalid base
URLs, unsupported methods, invalid limits, and attempts to return raw secrets.

Add Relayna Studio import/sync behavior. Gateway should accept a Studio service
reference and import the Studio-owned metadata into the local registry. For MVP,
support explicit admin-triggered import/sync; periodic background sync can be a
later enhancement if needed. The sync contract should map Studio fields into
Gateway fields without requiring Studio to provide internal credentials.
Gateway-local credential references and runtime limits must survive Studio
metadata refresh unless the admin explicitly overwrites them.

Studio sync precedence:

- Studio owns display metadata, Studio service ID, default route pattern,
  service category, and optional default pricing hints.
- Gateway owns enabled state, upstream base URL, credential reference or
  secret, timeout, body size limit, fallback services, and fail-closed runtime
  behavior.
- Admin patches can override imported metadata locally; each override should be
  visible in read responses as `source: "gateway_override"` or equivalent.

Extend route resolution so built-in static service routes map to registered
service names:

    /summary -> summary
    /translation -> translation
    /ocr -> ocr
    /embeddings -> embeddings
    /services/{service_name}/* -> {service_name}

In the proxy path, resolve the service registration after virtual-key
authentication and before upstream selection. Reject missing or disabled
services with stable gateway errors. Enforce key policy `allowed_services`
against the resolved registration name.

Forward service requests by stripping client credentials and injecting the
registered service credential. Preserve the client path for static service
routes and rewrite `/services/{service_name}/*` to the suffix path by default.
Add Relayna correlation headers and service metadata headers.

Record usage with service name, route, model when present, provider
`internal-service`, status, latency, estimated cost, fallback count, task/run
context when present, and final status. Include registered services in
provider/service health and Studio usage query results.

Expose sync status so Studio and operators can see whether a Gateway service is
locally created, imported from Studio, successfully synced, stale, or blocked
because required Gateway-owned runtime fields are missing.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo test -p gateway-core
    cargo test -p gateway-store
    cargo test -p gateway-api
    cargo test -p gateway-proxy
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Use local PostgreSQL and Redis for integration tests when available. Apply the
service registry migration only to local/test databases until reviewed.

## Validation and Acceptance

Phase 6 is accepted when:

- Operators can create, inspect, update, disable, enable, and delete service
  registrations through protected admin APIs.
- Gateway can import or sync service registrations that already exist in
  Relayna Studio.
- Studio-imported services are not routable until required Gateway-owned
  runtime fields are configured.
- Raw internal service credentials are accepted only on create/update and are
  never returned by read/list APIs.
- `/summary`, `/translation`, `/ocr`, `/embeddings`, and
  `/services/{service_name}/*` route to registered upstreams.
- Disabled or missing services fail closed before upstream calls.
- Key policy can allow one registered service while denying another.
- Client credentials are stripped and registered internal credentials are
  injected upstream.
- Usage and provider/service health are attributed to the registered service.
- Sync status identifies local, Studio-imported, stale, and incomplete
  registrations.
- Existing environment-based internal service config is either removed as
  unreleased branch-local behavior or kept only as an explicit development
  fallback documented in the Decision Log.

Required tests:

- Unit tests for service name validation, admin request validation, response
  redaction, route-to-service mapping, Studio import mapping, and policy
  allow/deny decisions.
- Store tests for service create/list/get/patch/disable/enable/delete and
  duplicate handling, Studio service ID uniqueness, sync status, and local
  override preservation.
- API tests for admin auth, raw credential one-way behavior, invalid payloads,
  disabled service states, import/sync requests, incomplete Studio-imported
  services, and stable response shapes.
- Proxy tests with stub services proving path rewriting, header stripping,
  credential injection, missing/disabled service rejection, and usage
  attribution.

## Idempotence and Recovery

Service registry migrations must be additive and safe to rerun locally. If a
local test creates service rows, use unique service names per test or clean only
those service names afterward.

Admin create and patch APIs must be retry-safe for clients. Duplicate service
creates should return a stable conflict error instead of creating multiple
registrations.

Studio import/sync should be idempotent. Re-importing the same Studio service
ID must update the existing registration instead of creating a duplicate.
Failed syncs should keep the last known good local registration and mark sync
status as stale or failed.

If credential storage format changes after review, add a forward migration and
backward-compatible read path for already-created local registrations.

## Artifacts and Notes

Example create request:

    POST /admin/services
    Authorization: Bearer <admin-token>

    {
      "name": "summary",
      "route_pattern": "/summary",
      "upstream_base_url": "http://summary.internal:8080",
      "allowed_methods": ["POST"],
      "credential": "internal-summary-token",
      "timeout_ms": 60000,
      "max_body_bytes": 1048576,
      "cost_mode": "fixed",
      "estimated_cost_usd": 0.01,
      "enabled": true
    }

Example read response omits raw credentials:

    {
      "name": "summary",
      "route_pattern": "/summary",
      "upstream_base_url": "http://summary.internal:8080",
      "allowed_methods": ["POST"],
      "credential_configured": true,
      "timeout_ms": 60000,
      "max_body_bytes": 1048576,
      "cost_mode": "fixed",
      "estimated_cost_usd": 0.01,
      "enabled": true
    }

Example Studio import request:

    POST /admin/services/import
    Authorization: Bearer <admin-token>

    {
      "studio_service_id": "svc_01HX...",
      "name": "translation",
      "route_pattern": "/translation",
      "category": "language",
      "default_pricing": {
        "cost_mode": "fixed",
        "estimated_cost_usd": 0.02
      }
    }

Example imported response requiring Gateway runtime config:

    {
      "name": "translation",
      "studio_service_id": "svc_01HX...",
      "source": "studio",
      "sync_status": "incomplete",
      "enabled": false,
      "credential_configured": false,
      "missing_runtime_fields": [
        "upstream_base_url",
        "credential"
      ]
    }

## Interfaces and Dependencies

Phase 6 depends on Phase 4 service route matching, key policy
`allowed_services`, Phase 5 service health and usage query behavior, PostgreSQL
migrations, Redis-backed control state, existing admin bearer-token protection,
and the Relayna Studio service catalog contract.

The end state includes durable service registry APIs, service-specific upstream
selection, Studio service import/sync, service credential injection, policy
enforcement by registered service name, usage attribution, and Studio-visible
service health.
