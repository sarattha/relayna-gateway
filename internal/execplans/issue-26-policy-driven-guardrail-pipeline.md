# Issue 26 Policy-Driven Guardrail Pipeline

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent comes from
issue #26, "feat: add policy-driven guardrail pipeline for LLM gateway
requests", and the gateway boundaries come from `internal/design-manifesto.md`.

## Purpose / Big Picture

Add a gateway-owned guardrail pipeline so Relayna virtual key policy can require
safety checks on LLM traffic without relying on callers to opt in. After the
feature is implemented, an operator can attach mandatory guardrails such as
`pii-redact` or `prompt-injection-check` to a key, route, model, or default
policy; a client request that omits `guardrails` still receives those
mandatory controls; and any client-requested guardrails are additive only and
validated against policy.

The first complete client-visible outcome should be non-streaming
OpenAI-compatible requests where:

- pre-call guardrails can inspect, block, warn, or modify JSON request bodies;
- `guardrails` is removed before forwarding to LiteLLM, direct providers, or
  OpenAI-compatible services;
- post-call guardrails can inspect, block, warn, or modify JSON responses;
- the response includes `x-relayna-request-id` and
  `x-relayna-applied-guardrails`;
- sanitized guardrail results are recorded for operator and Studio visibility.

Streaming-aware `during_call` guardrails, external guardrail providers, and
Redis-backed encrypted PII mappings are later phases because the current
Pingora proxy path streams the original request body and only keeps a 64 KiB
prefix for policy and usage extraction.

## Progress

- [x] (2026-05-16 16:43 +0700) Read issue #26 through GitHub, including
  motivation, proposed design, runtime flow, examples, and acceptance criteria.
- [x] (2026-05-16 16:43 +0700) Read `internal/design-manifesto.md`,
  `PLANS.md`, and `$implementation-strategy` guidance.
- [x] (2026-05-16 16:43 +0700) Inspected the current policy, proxy, usage,
  error, migration, and admin API structure relevant to guardrails.
- [x] (2026-05-16 16:43 +0700) Established a phased implementation strategy and
  compatibility boundary for the feature.
- [x] (2026-05-16 16:52 +0700) Phase 0: Added an inactive reusable Pingora
  body rewrite helper with request/response buffering tests and header helpers.
- [x] (2026-05-16 16:52 +0700) Phase 1: Added guardrail core domain types,
  deterministic resolver, in-memory executor, stable errors, and focused tests.
- [x] (2026-05-16 17:14 +0700) Phase 2: Added persisted guardrail registry,
  key-level policy, execution events, admin key policy fields, and
  virtual-key-authenticated guardrail discovery/test APIs.
- [x] (2026-05-16 17:14 +0700) Phase 3: Wired non-streaming proxy guardrail
  execution, request/response JSON rewriting, `x-relayna-applied-guardrails`,
  execution event persistence, streaming fail-closed behavior, and built-in
  `pii-redact`.
- [x] (2026-05-16 18:06 +0700) Phase 4: Added admin guardrail catalog,
  execution list and summary APIs, embedded Admin UI visibility, low-cardinality
  telemetry, docs, and release-hardening notes.
- [x] (2026-05-16 18:06 +0700) Phase 5: Added `during_call` support for
  `pii-redact`, streaming chunk redaction with holdback, custom HTTP guardrail
  registration and execution, and guardrail mapping config knobs.
- [ ] Run `$code-change-verification` before implementation is marked complete.

## Surprises & Discoveries

- Observation: The current Pingora proxy path does not retain a full request
  body for mutation. It records only `ctx.body_prefix` up to 65,536 bytes in
  `crates/gateway-proxy/src/pingora_plane.rs`.
  Evidence: `request_body_filter` increments `ctx.body_bytes_seen` and appends
  only to `body_prefix`; upstream receives the original chunks unless a later
  filter changes them.

- Observation: The current policy surface is key-centric. It supports route,
  model, provider, service, streaming, tools, rate-limit, and budget fields, but
  no guardrail fields and no team policy abstraction.
  Evidence: `crates/gateway-core/src/policies.rs` defines `KeyPolicy`, and
  `crates/gateway-store/migrations/20260509000100_phase_2_policy_keys_limits_budget.sql`
  defines the released `key_policies` table.

- Observation: Latest release tag `v0.0.8` already contains the current
  `KeyPolicy` shape and `key_policies` table, so persisted policy changes need
  an additive migration rather than rewriting existing columns.
  Evidence: `git show v0.0.8:crates/gateway-core/src/policies.rs` and the
  `v0.0.8` phase 2 migration match the local policy fields.

- Observation: Pingora request body filters must suppress intermediate chunks
  with empty `Bytes`, not `None`.
  Evidence: Pingora's HTTP/1 proxy path treats `None` as upstream end-of-body;
  `gateway-proxy::body_rewrite` tests now lock the empty-chunk suppression
  behavior for later guardrail proxy integration.

- Observation: Virtual-key-authenticated Axum guardrail routes need real
  verifiable key hashes in tests, not placeholder hashes.
  Evidence: The test fixture now builds `StoredVirtualKey` with
  `VirtualKeyMaterial::from_raw` so `Authenticator` verifies the supplied raw
  key.

## Decision Log

- Decision: Implement guardrails in phases, with non-streaming JSON
  request/response mutation first and streaming or chunk-level guardrails later.
  Rationale: Issue #26 asks for pre-call and post-call modification, but the
  gateway design manifesto requires streaming proxy paths not to buffer full
  LLM responses. A phased plan lets mandatory safety controls ship for
  non-streaming traffic without weakening streaming guarantees.
  Date/Author: 2026-05-16 / Codex.

- Decision: Keep the guardrail engine in `gateway-core`, persistence in
  `gateway-store`, public/control routes in `gateway-api`, and proxy execution
  in `gateway-proxy`.
  Rationale: This follows the repository ownership model: core owns policy
  decisions, store owns durable schemas, API owns routes, and proxy owns
  upstream request/response handling.
  Date/Author: 2026-05-16 / Codex.

- Decision: Treat `GET /v1/guardrails` and `POST /v1/guardrails/test` as
  additive public gateway routes protected by Relayna virtual keys unless the
  implementation review chooses an admin-only variant.
  Rationale: The issue places the endpoints under `/v1`, and callers may need
  to discover which optional guardrails they can request. Protection prevents
  exposing operator policy details to unauthenticated callers.
  Date/Author: 2026-05-16 / Codex.

- Decision: Store sanitized guardrail execution records separately from
  `usage_events` at first, while linking by `request_id`, `key_id`,
  `project_id`, route, model, and provider.
  Rationale: Guardrail results can be multi-row per request and include modes,
  actions, failure policies, and block reasons. Keeping them separate avoids
  bloating the existing usage event contract while preserving Studio join keys.
  Date/Author: 2026-05-16 / Codex.

- Decision: Phase 1 includes stable guardrail error variants but no persistence,
  public routes, proxy enforcement, or built-in PII redaction.
  Rationale: This keeps Phase 1 decision-complete for later integration without
  changing active request behavior.
  Date/Author: 2026-05-16 / Codex.

- Decision: Seed `pii-redact` as available but not default-on.
  Rationale: Preserves existing key behavior until operators opt in through
  key-level guardrail policy.
  Date/Author: 2026-05-16 / Codex.

- Decision: Guarded streaming requests fail closed in Phase 3.
  Rationale: Streaming-aware `during_call` inspection is intentionally deferred
  to Phase 5, and mandatory guardrails must not be silently bypassed.
  Date/Author: 2026-05-16 / Codex.

- Decision: Phase 4 exposes operator visibility through admin APIs and the
  embedded static Admin UI before adding Studio-specific dashboards.
  Rationale: The current repository already has operator-token protected admin
  routes and a bundled UI, while Studio query contracts can build on the same
  sanitized execution store later.
  Date/Author: 2026-05-16 / Codex.

- Decision: Phase 5 starts with a generic custom HTTP guardrail provider
  contract and keeps Presidio, Azure, and OpenAI-specific connectors out of
  this phase.
  Rationale: A single HTTP contract gives operators a useful extension point
  without baking vendor-specific behavior into the first streaming iteration.
  Date/Author: 2026-05-16 / Codex.

- Decision: Streaming `pii-redact` redacts chunks instead of only blocking or
  deferring streaming PII handling.
  Rationale: This preserves streaming behavior for guarded traffic while
  avoiding full-response buffering.
  Date/Author: 2026-05-16 / Codex.

## Outcomes & Retrospective

Phases 0 through 3 are implemented. The codebase now has an inactive bounded
body rewrite helper, framework-agnostic guardrail core types, persisted
guardrail definitions and key policy, virtual-key-authenticated discovery/test
APIs, proxy enforcement for non-streaming JSON requests, request/response body
rewriting, execution event persistence, and built-in `pii-redact`. Rich
operator dashboards, Studio visualization, streaming-aware `during_call`
guardrails, external providers, and Redis-backed encrypted PII mappings remain
Phase 4+ work.

## Context and Orientation

Relayna Gateway is the public entry point for LLM traffic. Clients authenticate
with a Relayna virtual key such as `Authorization: Bearer rk_live_xxx`; the
gateway validates that key, evaluates route/model/provider policy, strips
client credentials, forwards to LiteLLM or direct OpenAI-compatible upstreams,
and records usage.

Current relevant code:

- `crates/gateway-core/src/policies.rs` owns `KeyPolicy`,
  `GenerationFeatures`, policy lookup, and policy evaluation.
- `crates/gateway-core/src/usage.rs` owns `UsageEvent` and request/response
  JSON extraction helpers.
- `crates/gateway-core/src/errors.rs` owns stable gateway error codes and
  public error bodies.
- `crates/gateway-proxy/src/pingora_plane.rs` owns authentication, route
  matching, policy evaluation, rate limit and budget checks, upstream header
  rewriting, response filtering, response body prefix capture, and terminal
  usage recording.
- `crates/gateway-api/src/app.rs` owns Axum health, admin, Studio, usage, and
  static admin UI routes.
- `crates/gateway-store/src/postgres.rs` owns persisted key policy, provider,
  service, usage, and admin reads/writes.
- `crates/gateway-store/migrations/` owns PostgreSQL schema changes.
- `crates/gateway-telemetry/src/lib.rs` owns metrics and redaction helpers.

Definitions:

- A guardrail is a gateway-owned safety control that can inspect a request or
  response and return `allow`, `block`, `modify`, or `warn`.
- A guardrail registry is the catalog of available guardrails, supported modes,
  default-on status, failure policy, and sanitized config schema.
- A guardrail policy is the set of mandatory, optional, and forbidden
  guardrails that applies to a key, route, model, team, or gateway default.
- A guardrail plan is the deterministic ordered list of guardrails selected for
  one request after policy and optional client requests are resolved.
- A PII mapping is request-local state used by `pii-redact` to remember
  placeholders such as `[EMAIL_1] -> original email` for possible post-call
  restoration.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.8`.

This feature touches released compatibility-sensitive surfaces:

- public routes under `/v1`;
- proxy request passthrough semantics for `/v1/chat/completions` and
  `/v1/responses`;
- persisted PostgreSQL policy schema;
- usage and observability data consumed by Relayna Studio;
- provider request bodies and response bodies;
- request and response headers.

Implementation should be additive against `v0.0.8`:

- Preserve existing virtual key format and authentication behavior.
- Preserve existing provider response bodies when no guardrail modifies or
  blocks.
- Preserve existing `usage_events` columns and write behavior.
- Add migrations for guardrail registry, policy, and execution records instead
  of changing existing policy columns in place.
- Add response headers only when meaningful; do not remove existing headers
  except provider-sensitive headers that are already stripped.
- Keep streaming behavior unchanged until the streaming phase explicitly adds
  chunk-aware guardrails.

## Feature Request Analysis

Issue #26 is not just a request-level `guardrails` passthrough. The core
requirement is centralized policy enforcement: mandatory guardrails must apply
even when the client omits `guardrails`, and a client must never be able to
remove or bypass required controls.

The issue asks for five capability groups:

- Policy selection: resolve mandatory, optional, forbidden, and default-on
  guardrails from virtual key, team, route, model, default policy, and optional
  client request fields.
- Execution contract: define modes `pre_call`, `post_call`, and later
  `during_call`; actions `allow`, `block`, `modify`, and `warn`; failure
  policies `fail_closed`, `fail_open`, and `dry_run`.
- Built-in safety: implement `pii-redact` as the first built-in guardrail with
  request-local placeholder mapping for non-streaming traffic.
- Public APIs: expose available guardrails and a way to test guardrails without
  a provider call.
- Observability: record applied guardrails, actions, mode, latency, failure
  policy, block reason, and sanitized metadata for logs, metrics, and Studio.

The highest implementation risk is body mutation in Pingora. The current proxy
path is designed for streaming and low buffering. Pre-call blocking can use
captured prefixes only for simple checks, but correct PII redaction and
`guardrails` field stripping require a full JSON body rewrite for non-streaming
requests. Post-call mutation has the same constraint for non-streaming JSON
responses and is explicitly unsuitable for streamed chunks until a
`during_call` design exists.

## Plan of Work

Phase 0 is a short technical spike. Build a narrow test or prototype around
Pingora `request_body_filter` and `response_body_filter` to prove whether the
gateway can withhold chunks, collect a bounded non-streaming JSON body, emit a
rewritten body at end-of-stream, and update `content-length` or transfer
encoding safely. If Pingora cannot support this shape cleanly, choose a scoped
alternative before implementing: either a dedicated Axum non-streaming proxy
path for guarded JSON routes or a Pingora request buffering adapter that is
covered by black-box tests. Record the result in this ExecPlan before Phase 1
edits proceed.

Phase 1 adds framework-agnostic guardrail types and pure execution logic in
`gateway-core`. Add a new `guardrails` module with `GuardrailMode`,
`GuardrailAction`, `GuardrailFailurePolicy`, `GuardrailDefinition`,
`GuardrailPlan`, `GuardrailContext`, `GuardrailInput`, `GuardrailResult`, and
`GuardrailExecutor` or equivalent traits. Add a deterministic resolver that
combines default-on, mandatory, and allowed client-requested guardrails while
rejecting forbidden or unknown client requests. This phase should include unit
tests proving mandatory guardrails are not removable, optional guardrails are
additive only, forbidden guardrails are rejected, ordering is stable, and dry
run never blocks.

Phase 2 persists guardrail configuration and exposes discovery/test APIs. Add
PostgreSQL migrations for a minimal built-in registry seed, key-level
guardrail policy fields or related tables, and guardrail execution records.
Because team policy does not exist yet, define the storage so team policy can
be added later without blocking key, route, model, and default policy support.
Extend admin key create/patch/response types to show guardrail policy with
redacted/sanitized config only. Add `GET /v1/guardrails` and
`POST /v1/guardrails/test` through `gateway-api`, authenticated by virtual key,
and make responses reflect the caller's optional allowlist without exposing
forbidden internal details.

Phase 3 integrates non-streaming guardrails into `gateway-proxy` and ships
`pii-redact`. Resolve the guardrail plan after authentication, route matching,
route enablement, and normal policy evaluation, but before upstream selection
and provider forwarding. For JSON requests that fit the route body limit and
are not `stream: true`, run pre-call guardrails, strip the client `guardrails`
field, apply request modifications, and forward only the modified provider
payload. Run post-call guardrails for non-streaming JSON provider responses,
apply response modifications, and surface block failures through the existing
gateway error envelope with a new stable error code such as `guardrail_blocked`.
The first `pii-redact` implementation should detect common email, phone, and
simple SSN patterns in `messages`, `input`, and common response text fields;
replace values with stable request-local placeholders; optionally restore
placeholders in post-call output according to policy; and never log raw PII.

Phase 4 completes observability and operator workflow. Insert sanitized
guardrail execution records linked to `request_id`; add tracing fields and
metrics with low-cardinality labels such as guardrail name, mode, action, and
failure policy; update Admin portal or admin APIs enough for operators to see
guardrail catalogs and recent execution summaries; update `docs/`,
`README.md`, and release notes with policy examples, endpoint examples, error
shape, and limitations. Add Studio-oriented query surfaces only if they are
needed by the current Studio integration contract; otherwise keep execution
records queryable from admin APIs first.

Phase 5 covers advanced guardrails after the non-streaming pipeline is
verified. Add streaming-aware `during_call` semantics that inspect chunks
without buffering full responses, Redis-backed encrypted PII mapping with TTL
for retries/multi-replica safety, external HTTP guardrail providers with
timeouts and fail policies, Presidio/Azure/OpenAI safety integrations if
selected, team-level policy once teams exist, policy versioning, and dry-run
simulation/reporting workflows.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    rg -n "request_body_filter|response_body_filter|body_prefix|response_body_prefix" crates/gateway-proxy/src/pingora_plane.rs
    rg -n "KeyPolicy|KeyPolicyPatch|policy_for_key|key_policies|usage_events" crates/gateway-core crates/gateway-store crates/gateway-api

Phase 0 verification commands:

    cargo test -p gateway-proxy body_rewrite
    cargo test -p gateway-proxy pingora

Focused implementation commands by phase:

    cargo test -p gateway-core guardrail
    cargo test -p gateway-store guardrail
    cargo test -p gateway-api guardrail
    cargo test -p gateway-proxy guardrail
    cargo test --workspace --all-features guardrail

Final verification must use `$code-change-verification` because this work
changes Rust runtime code, tests, migrations, and proxy behavior:

    bash .codex/skills/code-change-verification/scripts/run.sh

The final verification script must pass the repository stack in fail-fast
order, including:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

## Validation and Acceptance

Core acceptance:

- A key policy can require `pii-redact` for `/v1/chat/completions` and
  `/v1/responses`.
- A request without a `guardrails` field still applies mandatory guardrails.
- A request with `guardrails: ["output-json-validator"]` adds that guardrail
  only if policy allows it.
- A request cannot disable, remove, shadow, or reorder mandatory guardrails.
- A request that asks for a forbidden guardrail is rejected with a stable
  gateway error body and records a failed usage event.
- The provider never receives the client `guardrails` field.
- The provider never receives raw PII redacted by pre-call `pii-redact`.
- Non-streaming post-call guardrails can modify or block JSON provider
  responses.
- Existing unguarded requests preserve their current behavior and response body
  shape.

PII acceptance:

- `pii-redact` replaces emails in user messages with placeholders before the
  upstream call.
- Request-local mapping is available to post-call execution for the same
  request.
- Policy can choose whether placeholders are restored in output.
- New PII generated in the response is redacted when post-call `pii-redact` is
  enabled.
- Logs, errors, execution records, and metrics do not contain raw request body
  PII or provider credentials.

API acceptance:

- `GET /v1/guardrails` returns available guardrails for an authenticated caller,
  including names, descriptions, modes, default-on status, failure policy, and
  sanitized config schema.
- `POST /v1/guardrails/test` can run allowed guardrails in `pre_call` or
  `post_call` mode without calling a provider.
- Unauthorized guardrail API requests return the existing virtual-key
  authentication error shape.

Observability acceptance:

- Each guarded request records applied guardrails, mode, action, latency,
  failure policy, block reason when present, and sanitized metadata.
- Responses include `x-relayna-applied-guardrails` when at least one guardrail
  is applied.
- Existing usage events still record success and failure terminal outcomes.
- Metrics avoid high-cardinality labels and raw payload values.

Compatibility acceptance:

- Existing `/v1/chat/completions` and `/v1/responses` behavior remains
  unchanged when no guardrail is configured.
- Streaming requests are not buffered or mutated until Phase 5 explicitly adds
  streaming-aware guardrails.
- Existing key policies without guardrail fields continue to load with no
  mandatory guardrails.
- Existing migrations remain intact; new migrations are additive.

## Idempotence and Recovery

Migrations should use `CREATE TABLE IF NOT EXISTS`, additive columns or related
tables, and indexes that can be safely re-applied under SQLx migration
tracking. Default policy rows should be inserted with upsert semantics so
rerunning seeds converges rather than duplicating definitions.

Guardrail execution must be request-local and retry-safe. If a pre-call
guardrail fails with `fail_closed`, return a stable gateway error before any
provider call is made and record sanitized failure metadata. If it fails with
`fail_open`, log and record the failure, then continue with the unmodified
request unless a prior successful guardrail already produced a modified body.
If it is `dry_run`, never block or mutate provider traffic.

If post-call execution fails after the provider responds, `fail_closed` should
return a gateway error instead of unsafe content, while `fail_open` should
return the provider response. In both cases, record usage and guardrail results.

If body rewrite support is partially applied and tests fail, revert only the
guardrail integration changes from this feature branch, not unrelated user
work. Core guardrail resolver and registry code can remain independently
tested while proxy integration is corrected.

## Artifacts and Notes

Issue #26 policy selection rule:

    final_guardrails =
      mandatory_guardrails_from_virtual_key
      + mandatory_guardrails_from_team_policy
      + mandatory_guardrails_from_route_policy
      + mandatory_guardrails_from_model_policy
      + default_on_guardrails
      + allowed_client_requested_guardrails

Phase 1 should implement this as a deterministic ordered set with duplicate
removal by guardrail name and mode. Because team policy does not exist in the
current repository, the initial resolver should accept an optional team policy
input and tests can cover the merge order with in-memory values before durable
team storage exists.

Suggested first migration shape:

    guardrail_definitions:
      name text primary key
      description text not null
      provider text not null
      modes text[] not null
      default_on boolean not null default false
      failure_policy text not null
      config_schema jsonb not null default '{}'::jsonb
      config jsonb not null default '{}'::jsonb
      enabled boolean not null default true
      created_at timestamptz not null default now()
      updated_at timestamptz not null default now()

    key_guardrail_policies:
      key_id uuid primary key references api_keys(id) on delete cascade
      mandatory_guardrails text[] not null default array[]::text[]
      optional_guardrails text[] not null default array[]::text[]
      forbidden_guardrails text[] not null default array[]::text[]
      created_at timestamptz not null default now()
      updated_at timestamptz not null default now()

    guardrail_execution_events:
      id uuid primary key
      request_id text not null
      key_id uuid
      project_id uuid
      route text
      model text
      provider text
      guardrail_name text not null
      mode text not null
      action text not null
      failure_policy text not null
      latency_ms bigint not null
      block_reason text
      metadata jsonb not null default '{}'::jsonb
      created_at timestamptz not null default now()

This migration shape is illustrative. Final implementation should use the
repository's SQLx and admin-store patterns and may normalize route/model
policies into additional tables if the store code stays clearer.

## Interfaces and Dependencies

New or extended core interfaces:

- `gateway_core::guardrails::GuardrailDefinition`
- `gateway_core::guardrails::GuardrailPlan`
- `gateway_core::guardrails::GuardrailContext`
- `gateway_core::guardrails::GuardrailInput`
- `gateway_core::guardrails::GuardrailResult`
- `gateway_core::guardrails::GuardrailRegistryLookup`
- `gateway_core::guardrails::GuardrailPolicyLookup`
- `gateway_core::guardrails::GuardrailExecutionRecorder`
- `gateway_core::guardrails::resolve_guardrail_plan(...)`

New or extended public routes:

- `GET /v1/guardrails`
- `POST /v1/guardrails/test`

New or extended proxy behavior:

- Resolve guardrail plan after normal key and route policy evaluation.
- Strip `guardrails` from provider-bound JSON request bodies.
- Add `x-relayna-applied-guardrails` on guarded responses.
- Return stable gateway errors for guardrail blocks and forbidden guardrail
  requests.

Potential new error codes:

- `guardrail_blocked`
- `guardrail_forbidden`
- `guardrail_unavailable`
- `invalid_guardrail_request`

Potential new docs:

- `docs/guardrails.md`
- `docs/operations.md` guardrail observability section
- `docs/admin-portal.md` guardrail operator workflow after UI work exists
