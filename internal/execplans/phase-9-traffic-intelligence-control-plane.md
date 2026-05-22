# Phase 9 Traffic Intelligence and Control Plane Depth

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; compatibility-sensitive work
must use `$implementation-strategy` and `$production-freeze-guard` before
runtime edits.

## Purpose / Big Picture

Make Relayna Gateway more powerful as the public AI traffic control plane by
improving streaming reliability, spend safety, provider choice, fallback
routing, token-aware limits, billing-grade usage, and guardrail execution. After
this phase, clients should get lower-latency streamed responses and more
reliable provider access, while operators can enforce token and budget controls,
inspect accurate usage, export billing data, and attach stronger guardrail
policy to virtual keys.

This phase intentionally focuses on these next capabilities:

- True streaming proxy and lifecycle metrics.
- Budget reservation for concurrent requests.
- Direct provider routing.
- Fallback routing and provider health-aware routing.
- Token-per-minute limits.
- Usage export and billing-grade analytics.
- Guardrail pipeline expansion.

Relayna runtime task submission is out of scope for this phase unless a later
decision explicitly expands scope.

## Progress

- [x] (2026-05-20 00:00 +07) Create the internal next-phase plan covering the
      requested feature set.
- [x] (2026-05-22 00:00 +07) Confirm the latest release tag and compare all planned public behavior
      against the v0.0.9 production freeze perimeter.
- [x] (2026-05-22 00:00 +07) Run `$implementation-strategy` before editing streaming, routing, budget,
      policy, usage, Redis, telemetry, schema, or admin API behavior.
- [x] (2026-05-22 00:00 +07) Run `$production-freeze-guard` before any public route, exported API,
      config, schema, Redis key, policy, usage, telemetry, or admin UI change.
- [ ] Implement streaming proxy hardening and lifecycle metrics.
- [ ] Implement budget reservation and token-per-minute enforcement. Completed:
      TPM Redis key format, stable `token_rate_limit_exceeded` error, proxy
      enforcement, request token estimation, and budget reservation for any
      route with a configured preflight estimated cost. Remaining: Redis-backed
      concurrency integration tests.
- [ ] Implement direct provider routing with safe credential handling.
- [ ] Implement fallback and provider health-aware routing.
- [ ] Implement usage export and billing-grade analytics.
- [ ] Expand the guardrail pipeline and audit surface.
- [x] (2026-05-22 00:00 +07) Run `$code-change-verification` before marking runtime, migration, test,
      packaging, or build/test changes complete.

## Surprises & Discoveries

- Observation: None yet.
  Evidence: This plan was newly drafted before implementation.

- Observation: `tpm_limit` already existed in key policy persistence and admin
  responses, but the proxy enforced only request-per-minute limits.
  Evidence: `KeyPolicy` contains `tpm_limit`; `RelaynaPingoraProxy` previously
  called only `check_request_rate_limit`.

- Observation: Budget reservation support already existed in Redis, but the
  proxy reserved only streaming requests.
  Evidence: `RedisControlState::reserve_budget` existed, and proxy reservation
  was guarded by `ctx.is_streaming`.

## Decision Log

- Decision: Exclude Relayna runtime task submission from this phase.
  Rationale: The requested scope included streaming, budgets, direct providers,
  fallback routing, token limits, usage analytics, and guardrails, but did not
  include runtime task submission. Keeping task submission separate makes this
  phase large but still coherent around traffic governance.
  Date/Author: 2026-05-20 / Codex.

- Decision: Treat v0.0.9 as the production freeze baseline for all planned
  public behavior changes.
  Rationale: `AGENTS.md` defines Relayna Gateway v0.0.9 as the freeze baseline,
  and this phase touches compatibility-sensitive behavior across streaming,
  policy, usage, Redis state, telemetry, routing, and admin surfaces.
  Date/Author: 2026-05-20 / Codex.

- Decision: Add TPM as an additive freeze surface with a new Redis key and
  stable error code instead of reusing the RPM error code.
  Rationale: operators and clients need to distinguish request count throttles
  from token throughput throttles, and the freeze perimeter should pin the new
  key format and public error name.
  Date/Author: 2026-05-22 / Codex.

## Outcomes & Retrospective

Partially implemented. TPM enforcement is active in the proxy, budget
reservation now applies to all requests with configured preflight cost, and the
freeze perimeter pins the new TPM Redis key and error code. Remaining Stage 1,
3, 4, 5, and 6 items are still open.

Verification on 2026-05-22: `node tests/freeze-v0.0.9-perimeter.test.mjs`,
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace --all-features`, and
`bash .codex/skills/code-change-verification/scripts/run.sh` all passed.

## Context and Orientation

Relayna Gateway is the public governance layer for AI traffic. Clients present
Relayna virtual keys, and the gateway authenticates the key, evaluates policy,
routes the request to LiteLLM, a direct provider, or an internal service, records
usage, and exposes operator controls through admin APIs and the embedded admin
portal.

Important terms:

- Virtual key: a Relayna-owned external API key in the `rk_live_xxx` format.
  Clients use this key instead of provider credentials.
- Key policy: per-key limits and permissions for routes, models, providers,
  services, streaming, tools, token limits, budgets, and guardrails.
- Streaming proxy: a proxy path that forwards streamed provider chunks to the
  client as they arrive without buffering the complete response body.
- Budget reservation: a Redis-backed hold for estimated spend before forwarding
  a request, reconciled after actual usage is known.
- Token-per-minute limit: a policy limit that restricts token throughput in a
  rolling minute window, not only request count.
- Direct provider routing: gateway-managed routing to provider endpoints using
  internal credentials, with client credentials stripped.
- Fallback routing: switching to an allowed alternate provider or backend after
  a safe failure class such as connection failure, timeout, or provider
  unavailability.
- Usage export: operator or Studio-facing extraction of usage records in a
  billing-friendly format with stable filters and totals.
- Guardrail pipeline: ordered pre-provider and post-provider checks that can
  redact, block, audit, or call external policy services.

Expected areas:

- `/Users/jobz/Works/relayna-gateway/crates/gateway-core`: policy evaluation,
  route resolution, feature extraction, token estimation, rate limits, budgets,
  fallback classification, guardrail decisions, usage construction, and billing
  summaries.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy`: Pingora streaming,
  direct provider passthrough, upstream credential injection, response handling,
  fallback execution, cancellation handling, and usage finalization.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api`: admin APIs, usage
  export endpoints, provider health endpoints, guardrail management, and admin
  portal updates.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-store`: PostgreSQL
  migrations, usage query persistence, provider configs, guardrail execution
  records, Redis budget reservations, Redis TPM counters, and health state.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-telemetry`: metrics, traces,
  log fields, redaction, stream lifecycle metrics, fallback metrics, and
  guardrail metrics.
- `/Users/jobz/Works/relayna-gateway/tests`: freeze perimeter tests,
  admin-ui tests, proxy integration tests, streaming tests, provider stubs,
  Redis budget/rate-limit tests, and usage export tests.

## Compatibility Boundary

Compatibility boundary: v0.0.9 production freeze baseline. This phase changes
or extends compatibility-sensitive surfaces including streaming behavior,
policy decisions, rate-limit behavior, budget Redis keys, provider routing,
usage event fields, telemetry fields, admin APIs, admin UI assumptions, and
PostgreSQL schema.

Use additive changes by default:

- Preserve existing `/v1/chat/completions`, `/v1/responses`, `/admin/*`, and
  `/admin-ui` behavior unless a compatibility decision explicitly updates the
  freeze perimeter tests.
- Preserve released error codes and response shapes. New errors for TPM,
  budget reservation, provider fallback exhaustion, export validation, and
  guardrail failures must have stable names and documented status codes.
- Preserve existing usage records. Add new usage fields through migrations with
  backward-safe reads and clear defaults.
- Document Redis key formats and TTLs for new budget reservation, TPM, provider
  health, and fallback state before release.
- Keep provider credentials, LiteLLM credentials, operator tokens, raw virtual
  keys, prompt bodies, and guardrail secrets out of responses, logs, traces, and
  metrics.

Before runtime work begins, record the actual latest release tag used for
comparison and run:

    cd /Users/jobz/Works/relayna-gateway
    git tag -l 'v*' --sort=-v:refname | head -n1
    node tests/freeze-v0.0.9-perimeter.test.mjs

## Plan of Work

Start with streaming proxy hardening because every later feature needs accurate
lifecycle boundaries for usage, budget reconciliation, and guardrail timing.
Review `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`
and related core usage modules. Ensure `stream: true` requests are detected,
server-sent events are forwarded chunk by chunk, cancellation and timeout paths
finalize usage, and telemetry reports stream start, first chunk, completion,
abort, and duration.

Add budget reservation and token-per-minute enforcement next. Extend
`/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/budgets.rs`,
`/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/rate_limits.rs`, and
`/Users/jobz/Works/relayna-gateway/crates/gateway-store/src/redis.rs` so each
request can estimate token and cost impact before forwarding, reserve budget,
increment TPM counters atomically, and reconcile or release reservations after
success, failure, timeout, or cancellation.

Implement direct provider routing after spend controls are reliable. Extend
route resolution and provider config handling so selected routes can target a
direct OpenAI-compatible provider using internal credentials. The proxy must
strip client credentials, inject only the configured internal credential,
preserve safe request path/query data, apply route policy, apply route pricing,
and record provider/model/cost usage.

Implement fallback and provider health-aware routing once direct providers are
available. Add provider health signals, safe retry classification, fallback
chain selection, final provider attribution, and operator-visible fallback
metrics. Retry only safe failure classes and do not retry request classes that
could duplicate side effects without an explicit idempotency decision.

Add usage export and billing-grade analytics after usage fields and provider
attribution settle. Add admin APIs that export filtered usage by project, key,
provider, model, service, route, task, status, and time range. Provide stable
CSV and JSON formats, total cost fields, token totals, failure counts, fallback
counts, and guardrail action counts. Update the admin portal only after the API
shape is stable.

Expand the guardrail pipeline last, because it depends on streaming lifecycle,
usage attribution, provider routing, and telemetry. Add explicit pre-provider
and post-provider stages, latency budgets, failure policy behavior, external
HTTP guardrail execution controls, output restoration rules, audit drilldowns,
and policy presets. Ensure guardrails can block or redact without leaking
provider credentials or prompt content.

## Concrete Steps

Use focused tests while iterating, but finish with the full verification stack.
Commands are run from the repository root:

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    node tests/freeze-v0.0.9-perimeter.test.mjs
    cargo test -p gateway-core
    cargo test -p gateway-proxy
    cargo test -p gateway-store
    cargo test -p gateway-api
    node tests/admin-ui.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

If documentation, admin UI, or MkDocs pages are updated, also run:

    mkdocs build --strict

## Validation and Acceptance

This phase is accepted when all of the following are true:

- Streaming requests deliver chunks to clients before the upstream response is
  complete, and no path buffers complete streamed responses by default.
- Client disconnects, upstream disconnects, timeouts, and upstream errors
  finalize usage and release budget reservations.
- Stream lifecycle metrics expose active streams, first-token latency, stream
  duration, completion count, abort count, and timeout count.
- Budget reservations prevent concurrent overspend and reconcile actual cost
  after success, failure, and cancellation.
- TPM limits reject over-limit traffic with a stable error code and retry hint
  when available.
- Direct provider routes forward only sanitized headers and gateway-owned
  provider credentials.
- Fallback routing uses only configured fallback chains and safe failure
  classes, and usage records show final provider and fallback count.
- Provider health state is visible to operators and can influence fallback
  decisions without adding high-cardinality metric labels.
- Usage export APIs produce stable JSON and CSV output with filters, totals,
  token counts, costs, status counts, and fallback/guardrail counts.
- Guardrail pipeline stages can run before and after provider calls, enforce
  fail-open or fail-closed behavior, record audit events, and preserve
  credential redaction.
- The v0.0.9 freeze perimeter test passes, or intentional updates include a
  compatibility note in this plan and in the relevant PR.

Required tests:

- Unit tests for streaming detection, stream lifecycle event construction,
  token estimation, TPM decisions, budget reservation reconciliation, fallback
  classification, provider route selection, usage export filters, and guardrail
  stage decisions.
- Integration tests with stub streaming upstreams that delay chunks and prove
  incremental delivery.
- Disconnect and timeout tests that prove reservation cleanup and failure usage
  insertion.
- Redis-backed tests for atomic TPM and budget reservation behavior.
- Provider passthrough tests proving client credentials are stripped and
  internal credentials are never returned.
- Fallback tests for connection failure, timeout, selected upstream 5xx
  responses, fallback exhaustion, and final provider attribution.
- Usage export tests for JSON, CSV, time range filters, project/key/model
  filters, totals, and empty result sets.
- Guardrail tests for pre-provider block, pre-provider redact, post-provider
  block, HTTP guardrail timeout, fail-open, fail-closed, and audit persistence.

## Idempotence and Recovery

All migrations must be additive or use forward migrations for corrections.
Never edit a migration that may already have been applied outside a disposable
local database. For local failures, recreate only the local test database or add
a corrective migration.

Redis tests must isolate keys by test namespace. If a run is interrupted, remove
only keys with the test prefix or flush only a disposable local Redis database.
Document new Redis key formats, TTLs, and cleanup behavior before release.

Streaming and fallback integration tests must use bounded timeouts. If a local
stub server remains after an interrupted run, stop that process and rerun the
focused test before the full workspace test.

Provider and guardrail credentials used in tests must be dummy values. Do not
place real credentials in fixtures, logs, snapshots, docs, or admin UI tests.

If the admin UI is updated and a visual or static test fails, keep API contract
fixes separate from UI rendering fixes so compatibility review remains clear.

## Artifacts and Notes

Expected successful stream lifecycle:

    request accepted
    policy allowed
    budget reserved
    upstream connected
    first chunk observed
    chunks forwarded incrementally
    upstream completed
    usage finalized
    reservation reconciled
    metrics recorded

Expected fallback usage facts:

    original_provider = "openai-primary"
    final_provider = "openai-secondary"
    fallback_count = 1
    fallback_reason = "upstream_timeout"
    status_code = 200

Expected TPM behavior:

    estimated_input_tokens + reserved_output_tokens <= key_policy.tpm_limit
    over-limit requests return a stable token rate-limit error
    successful requests reconcile estimated tokens with actual usage

## Interfaces and Dependencies

End-state interfaces must include:

- Stable policy fields for RPM, TPM, daily budget, monthly budget, streaming,
  tools, provider allowlists, route allowlists, and guardrail policy.
- Redis keys and TTLs for request rate limits, token rate limits, budget
  reservations, daily budget counters, monthly budget counters, and provider
  health state.
- Usage event fields for prompt tokens, completion tokens, total tokens,
  estimated cost, provider, final provider, fallback count, status, latency,
  stream state, guardrail action, route, key, and project.
- Admin APIs for provider health, usage summary, usage breakdown, usage
  timeseries, usage export, provider configuration, route configuration, key
  policy, and guardrail policy.
- Metrics for request count, latency, active streams, first-token latency,
  stream aborts, provider fallback, provider health, guardrail actions, budget
  reservation failures, and TPM rejections.
- Documentation updates for any new environment variables, admin endpoints,
  Redis keys, policy fields, usage export formats, and operator workflows.
