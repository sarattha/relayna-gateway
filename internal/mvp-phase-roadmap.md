# Relayna Gateway MVP Phase Roadmap

This internal roadmap turns the MVP design manifesto into an implementation
checklist for contributors. The source of truth for product intent, architecture
principles, and phase scope remains `internal/design-manifesto.md`; this file
adds execution gates for planning, review, and handoff.

## How to Use This Roadmap

- Use the matching phase below before starting gateway work, then create a
  dedicated ExecPlan from `PLANS.md` for multi-step features, refactors,
  architecture changes, or compatibility-sensitive work. Phase-level starting
  ExecPlans live under `internal/execplans/`.
- Use `$implementation-strategy` before changing runtime behavior, public route
  shapes, authentication, policy decisions, usage event fields, configuration,
  PostgreSQL schemas, Redis state, proxy behavior, streaming behavior,
  telemetry fields, or Relayna runtime contracts.
- Use `$code-change-verification` before marking Rust runtime, test, migration,
  packaging, or build/test behavior changes complete.
- Treat each phase as complete only when the deliverables, acceptance gates,
  verification gates, and security or compatibility review points are all
  satisfied.
- Do not use this roadmap to expand scope beyond the design manifesto. If this
  file and the manifesto disagree, update the roadmap to follow the manifesto.

## Phase 1 - Core Proxy MVP

ExecPlan: `internal/execplans/phase-1-core-proxy-mvp.md`

Objective: establish Relayna Gateway as the public AI API entry point by
accepting authenticated OpenAI-compatible generation requests and forwarding
them to LiteLLM while recording usage.

Deliverables:

- Server foundation:
  - [x] Create the Rust workspace with gateway API, core, proxy, store, and
        telemetry boundaries preserved even if the MVP starts compact.
  - [x] Add a Pingora proxy path for `/v1/*` traffic and an Axum control API for
        health and readiness.
  - [x] Add `/healthz`, `/readyz`, structured error responses, shared request
        ID handling, tracing layers, and graceful shutdown.
  - [x] Keep authentication, routing, usage, policy, budget, and rate-limit
        decisions independent of Pingora and Axum request types.
- Configuration and persistence:
  - [x] Load required environment settings for database, Redis, LiteLLM, proxy
        bind address, control bind address, LiteLLM service key, and log level.
  - [x] Add the initial PostgreSQL schema for virtual keys, usage events, and
        route policies.
  - [x] Store only key prefixes and hashes, never raw Relayna virtual keys.
- Authentication and routing:
  - [x] Accept `Authorization: Bearer rk_live_xxx` Relayna virtual keys.
  - [x] Reject missing, malformed, invalid, expired, and disabled keys.
  - [x] Attach authenticated key and project context to the request lifecycle.
  - [x] Route `POST /v1/chat/completions` and `POST /v1/responses` to LiteLLM.
- Proxy and accounting:
  - [x] Strip client credentials before the upstream call.
  - [x] Inject the internal LiteLLM service credential and Relayna correlation
        headers.
  - [x] Forward method, path, query string, JSON body, upstream status, response
        body, and relevant content-type headers.
  - [x] Handle upstream timeouts and connection errors with stable gateway
        errors.
  - [x] Insert usage events for successful and failed requests with request ID,
        key, project, route, model when available, provider, status, latency,
        and timestamp.

Acceptance gates:

- [x] A valid Relayna virtual key can call `/v1/chat/completions` and
      `/v1/responses` through the gateway and receive the LiteLLM response.
- [x] Invalid, expired, disabled, and missing keys are rejected before any
      upstream provider call.
- [x] LiteLLM and provider credentials are never returned to the client.
- [x] Usage rows are inserted for both success and failure paths.
- [x] Logs include request IDs and do not include full prompts by default.
- [x] Core decision logic can be unit tested without constructing Pingora or
      Axum request objects.

Verification gates:

- [x] Unit tests cover key validation, route resolution, credential stripping,
      usage event construction, and error mapping.
- [ ] Integration or black-box tests cover valid proxying, invalid auth,
      upstream timeout, upstream connection failure, and usage insertion.
- [ ] Manual smoke test uses a seeded key and LiteLLM-compatible upstream for
      `POST /v1/chat/completions`.
- [x] Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
      --all-features -- -D warnings`, and `cargo test --workspace
      --all-features`, or use `$code-change-verification`.

Security and compatibility review:

- [x] Compatibility boundary is recorded in the ExecPlan before public route,
      schema, config, or usage event changes.
- [x] Raw Relayna virtual keys, LiteLLM service keys, provider keys, and
      internal service tokens are redacted from logs and responses.
- [x] The initial public route, error shape, correlation headers, and usage
      event fields are documented before release.

## Phase 2 - Policy, Virtual Keys, Rate Limit, and Budget

ExecPlan: `internal/execplans/phase-2-policy-keys-rate-limit-budget.md`

Objective: turn the gateway into a real control plane by enforcing key policy,
route and model access, Redis-backed request limits, simple budgets, and admin
key management.

Deliverables:

- Key policy and admin APIs:
  - [ ] Add key policy persistence for allowed routes, models, providers,
        request/token limits, budget limits, streaming, and tool permissions.
  - [ ] Add protected admin APIs to create, read, update, revoke, disable, and
        inspect usage for virtual keys.
  - [ ] Return raw virtual keys only once at creation time.
  - [ ] Hash raw keys before persistence and prevent raw-key logging.
- Policy enforcement:
  - [ ] Enforce route allowlists before proxying or task submission.
  - [ ] Enforce model and provider allowlists before upstream calls.
  - [ ] Reject disallowed passthrough, streaming, and tool use.
  - [ ] Return stable `policy_denied` errors for denied requests.
- Rate limiting:
  - [ ] Use Redis request-per-minute counters shared across gateway replicas.
  - [ ] Increment counters atomically, set expirations, and reject over-limit
        requests with stable `rate_limit_exceeded` errors.
  - [ ] Include retry hints when the counter state supports them.
- Budget enforcement:
  - [ ] Load daily and monthly budget settings from policy.
  - [ ] Compare current spend using Redis and/or PostgreSQL-backed usage state.
  - [ ] Reject over-budget requests before upstream calls.
  - [ ] Record estimated cost after requests and update budget counters.

Acceptance gates:

- [ ] Operators can create, inspect, update, revoke, and disable keys through
      protected admin APIs.
- [ ] A key can be restricted to specific routes, models, and providers.
- [ ] Request-per-minute limits work across multiple gateway instances.
- [ ] Daily and monthly budget checks reject requests that exceed policy.
- [ ] Usage can be queried by key and project.
- [ ] Raw API keys are never persisted or logged.

Verification gates:

- [ ] Unit tests cover policy allow/deny decisions, admin key hashing,
      rate-limit decisions, and budget decisions.
- [ ] Integration tests cover admin create/revoke flows, denied routes, denied
      models, Redis rate limiting, budget rejection, and usage queries.
- [ ] Multi-instance or simulated-concurrency tests prove Redis counters are
      shared and atomic.
- [ ] Run the full Rust verification stack through `$code-change-verification`.

Security and compatibility review:

- [ ] Admin APIs require an internal admin credential and fail closed when
      missing or invalid.
- [ ] Policy denial, rate-limit, and budget errors have stable status codes and
      response shapes before release.
- [ ] PostgreSQL policy schemas and Redis counter formats are recorded as
      compatibility-sensitive once released.
- [ ] Sensitive headers and credentials remain redacted in logs, traces, and
      error responses.

## Phase 3 - Streaming and Accurate Usage Tracking

ExecPlan: `internal/execplans/phase-3-streaming-usage-tracking.md`

Objective: support production-grade streaming traffic without buffering full
responses, while improving token and cost accounting.

Deliverables:

- Streaming proxy:
  - [ ] Detect OpenAI-compatible streaming requests.
  - [ ] Forward SSE responses through Pingora as chunks arrive.
  - [ ] Preserve streaming response headers and avoid collecting complete
        streams in memory.
  - [ ] Handle client disconnects, upstream disconnects, and stream timeouts.
- Stream lifecycle telemetry:
  - [ ] Track stream start, first chunk, client disconnect, upstream completion,
        upstream error, and stream completion events.
  - [ ] Record active streams, first-token latency, stream duration, and stream
        abort counts.
- Usage extraction and pricing:
  - [ ] Extract token usage from upstream usage fields when present.
  - [ ] Use LiteLLM metadata, tokenizer estimation, or route pricing fallback
        when upstream usage is unavailable.
  - [ ] Persist prompt tokens, completion tokens, total tokens, estimated cost,
        provider, model, latency, and final status.
- Budget reservation:
  - [ ] Estimate maximum request cost before starting a stream.
  - [ ] Reserve budget before forwarding, reconcile actual cost on completion,
        and release unused reservations on failure.
  - [ ] Prevent concurrent streaming requests from overspending the same
        budget.

Acceptance gates:

- [ ] Streaming chat completions pass through the gateway chunk by chunk.
- [ ] Client disconnects and upstream disconnects do not crash the gateway.
- [ ] First-token latency, active stream count, stream duration, and abort
      metrics are observable.
- [ ] Usage and estimated cost are recorded after stream completion.
- [ ] Budget reservation prevents concurrent overspend.
- [ ] Prompt and full response bodies are not logged or buffered by default.

Verification gates:

- [ ] Unit tests cover stream request detection, lifecycle event construction,
      usage extraction fallback order, and reservation reconciliation.
- [ ] Streaming integration tests prove chunks are forwarded incrementally
      without waiting for the full upstream response.
- [ ] Disconnect tests cover client cancellation, upstream cancellation,
      timeout, reservation release, and failure usage insertion.
- [ ] Run `$code-change-verification` after all streaming and accounting
      changes.

Security and compatibility review:

- [ ] Streaming behavior, cancellation semantics, and usage fields are recorded
      as compatibility-sensitive before release.
- [ ] No implementation path uses unbounded channels or unbounded response
      buffers for streams.
- [ ] Budget reservation Redis state has documented keys, TTLs, and recovery
      behavior.
- [ ] Credentials and prompt content remain redacted in stream errors, logs,
      traces, and metrics.

## Phase 4 - Advanced Routing, Passthrough, and Relayna Runtime Integration

ExecPlan: `internal/execplans/phase-4-advanced-routing-runtime-integration.md`

Objective: expand Gateway beyond LiteLLM by supporting direct providers,
internal service routes, Relayna task submission, route pricing, fallback
routing, and worker-to-gateway metering.

Deliverables:

- Route resolver:
  - [ ] Support static and wildcard route matches for LiteLLM, direct provider,
        internal service, and Relayna runtime backends.
  - [ ] Support per-route timeout, body size limit, cost mode, and auth mode.
  - [ ] Keep route matching and policy decisions in framework-agnostic core
        logic.
- Direct provider passthrough:
  - [ ] Support provider passthrough routes described by the manifesto.
  - [ ] Sanitize incoming headers, remove user credentials, inject provider
        credentials, and preserve provider path/query data.
  - [ ] Enforce route permission and apply route-level pricing.
  - [ ] Track request status, latency, provider, model or route, and cost.
- Internal service routing:
  - [ ] Route configured internal AI service requests with internal service
        authentication.
  - [ ] Add request correlation headers and internal route policy checks.
  - [ ] Track usage and cost when configured.
- Relayna runtime integration:
  - [ ] Add task submission behavior that validates keys, checks task
        permission, estimates cost, reserves budget, calls Relayna runtime, and
        returns a task ID.
  - [ ] Attach request ID, key ID, project ID, task type, and input metadata to
        internal task creation requests.
  - [ ] Add task status and task event proxy behavior.
- Worker-to-gateway metering and fallback:
  - [ ] Authenticate Relayna worker calls to the gateway.
  - [ ] Attribute LLM and provider usage to task and run identifiers.
  - [ ] Support safe provider fallback chains without retrying non-idempotent
        task creation blindly.
  - [ ] Record the final provider used after fallback.

Acceptance gates:

- [ ] Gateway supports direct provider passthrough routes without exposing
      provider credentials.
- [ ] Gateway supports internal service routes with service authentication.
- [ ] Gateway can submit Relayna tasks and return task IDs.
- [ ] Task status and task events can be proxied through the gateway.
- [ ] Relayna workers can call the gateway for metered LLM/provider usage.
- [ ] Usage can be attributed to both key/project and task/run context.
- [ ] Route-level pricing and fallback provider attribution work.

Verification gates:

- [ ] Unit tests cover route matching, auth mode selection, cost mode selection,
      provider credential injection, task usage attribution, and fallback
      retry classification.
- [ ] Integration tests cover direct provider passthrough, internal service
      routing, task submission, task status/events proxying, worker calls, and
      usage attribution.
- [ ] Manual tests use stub providers and a stub Relayna runtime to verify
      request shape and credential handling without real secrets.
- [ ] Run `$code-change-verification` after routing and runtime integration
      changes.

Security and compatibility review:

- [ ] Route configuration, passthrough behavior, task request shapes, worker
      headers, and usage event fields are reviewed as compatibility-sensitive.
- [ ] Provider keys, internal service tokens, internal JWTs, and worker tokens
      are never visible to external callers.
- [ ] Fallback retries are limited to safe error classes and do not duplicate
      non-idempotent task creation.
- [ ] Task attribution fields are documented for Relayna Studio consumers
      before release.

## Phase 5 - Relayna Studio, Observability, and Production Readiness

ExecPlan: `internal/execplans/phase-5-studio-observability-production.md`

Objective: expose gateway and runtime data to Relayna Studio and harden the
gateway for production operation.

Deliverables:

- Usage and dashboard APIs:
  - [ ] Add usage summary, by-key, by-project, by-model, by-provider, by-task,
        and timeseries query behavior.
  - [ ] Support cost, error rate, and latency views over time.
  - [ ] Preserve stable query response shapes for Relayna Studio consumers.
- Task observability:
  - [ ] Expose or proxy task status, events, usage, and logs.
  - [ ] Show task timelines, LLM calls inside tasks, provider calls inside
        tasks, per-task cost, worker failures, and artifact links.
- Provider health and telemetry:
  - [ ] Track provider availability, latency, error rate, model-level error
        rate, fallback count, and timeout count.
  - [ ] Expose provider health and metrics endpoints.
  - [ ] Add OpenTelemetry trace propagation and spans for requests, upstream
        calls, Redis, PostgreSQL, and Relayna task calls.
  - [ ] Expose Prometheus metrics listed in the manifesto.
- Logging and production deployment:
  - [ ] Emit JSON structured logs with request ID, key ID, project ID, route,
        provider, and redacted sensitive fields.
  - [ ] Add Kubernetes deployment resources for deployment, service, config,
        secrets, autoscaling, disruption budget, monitoring, and ingress or
        Gateway API exposure.
  - [ ] Configure non-root containers, probes, graceful shutdown, resource
        requests/limits, secret-based config, TLS or mTLS support, and
        optional network policies.
- Reliability hardening:
  - [ ] Define upstream timeouts, retry policy, backpressure behavior, max body
        size, max concurrent streams, connection pool tuning, graceful stream
        shutdown, and a clear error taxonomy.
  - [ ] Add circuit breaker design or implementation when provider health data
        shows it is needed.

Acceptance gates:

- [ ] Relayna Studio can display gateway usage, cost, errors, latency, provider
      health, and task-level cost.
- [ ] Metrics are exposed and usable by Prometheus.
- [ ] Structured logs and traces correlate gateway, provider, Redis,
      PostgreSQL, and Relayna runtime activity.
- [ ] Gateway is deployable on AKS without hardcoded secrets.
- [ ] Production failure modes have stable errors, metrics, and logs.
- [ ] Graceful shutdown preserves in-flight request and stream behavior within
      configured limits.

Verification gates:

- [ ] Unit tests cover usage aggregation, dashboard query shaping, telemetry
      labels, log redaction, provider health calculations, and error taxonomy.
- [ ] Integration tests cover usage query APIs, task observability APIs,
      metrics exposure, trace propagation, health/readiness behavior, and
      graceful shutdown.
- [ ] Deployment validation covers Kubernetes manifests, probes, secret wiring,
      resource settings, and monitoring resources.
- [ ] Run `$code-change-verification` after runtime, telemetry, deployment, or
      test changes.

Security and compatibility review:

- [ ] Usage query responses, metrics names, trace attributes, log fields,
      Kubernetes config, and deployment secrets are reviewed before release.
- [ ] Metrics avoid high-cardinality labels from raw prompts, raw keys, user
      input, or unbounded provider payload fields.
- [ ] Logs include key IDs rather than raw keys and do not log prompts by
      default.
- [ ] Production config fails closed when required secrets or upstream settings
      are missing.

## Cross-Phase Done Definition

- [ ] The phase delivers observable behavior that matches
      `internal/design-manifesto.md`.
- [ ] Public behavior, schemas, configuration, Redis state, usage event fields,
      telemetry fields, and Relayna runtime contracts have an explicit
      compatibility decision in the relevant ExecPlan.
- [ ] Security-sensitive paths prove credentials are stripped, injected only
      server-side, redacted from logs, and never returned to callers.
- [ ] Usage tracking exists for success and failure paths.
- [ ] Tests cover the behavior changed in the phase, including negative cases.
- [ ] Required formatting, linting, tests, and `$code-change-verification`
      checks pass for runtime-impacting work.
- [ ] Documentation and PR notes explain what changed, why, verification run,
      compatibility impact, migration notes if any, and residual risk.
