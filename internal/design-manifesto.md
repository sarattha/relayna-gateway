Relayna Gateway MVP Design Manifesto

0. Mission

Build relayna-gateway as a Rust-based AI API gateway and control plane for Relayna.

The gateway must become the single public entry point for:

* LLM requests
* provider routing
* virtual key authentication
* route/model policy control
* rate limiting
* budget enforcement
* usage tracking
* async task submission into Relayna
* observability for Relayna Studio

Relayna Gateway should not be only a reverse proxy. It is the governance, metering, and routing layer for AI traffic.

Relayna itself remains the runtime/task execution layer.

External Client / SDK / Studio
        |
        v
relayna-gateway
        |
        ├── LiteLLM / OpenAI-compatible providers
        ├── Direct provider passthrough
        ├── Internal Relayna APIs
        └── Relayna task runtime

⸻

Core Architecture Principles

1. Gateway owns identity

All external requests must use Relayna virtual keys.

Do not expose:

* LiteLLM master key
* LiteLLM virtual keys
* provider API keys
* internal service tokens

The client should only know:

Authorization: Bearer rk_live_xxx

Gateway translates this into internal credentials.

⸻

2. Gateway owns policy

Gateway must decide:

* which key can call which route
* which key can use which model
* which key can access which provider
* whether streaming is allowed
* whether tool calling is allowed
* whether task execution is allowed
* whether budget remains
* whether rate limit is exceeded

⸻

3. Gateway owns usage tracking

Every request must produce a usage event.

Minimum fields:

request_id
key_id
project_id
route
model
provider
input_tokens
output_tokens
estimated_cost
latency_ms
status_code
created_at

Usage must be queryable later by Relayna Studio.

⸻

4. Gateway should stream, not buffer

For LLM streaming and passthrough endpoints, avoid buffering large request/response bodies.

Preferred behavior:

Client <- streaming bytes <- Gateway <- streaming bytes <- Provider

⸻

5. Relayna Gateway and Relayna Runtime must integrate cleanly

Gateway is the public control plane.

Relayna is the execution runtime.

External users -> relayna-gateway
relayna-gateway -> relayna internal task API
relayna workers -> relayna-gateway for metered LLM/provider calls

⸻

Recommended Rust Stack

Use this baseline stack unless there is a strong reason not to.

Proxy framework      pingora
Control API          axum
Async runtime        tokio
Middleware           tower / tower-http
HTTP client          pingora upstreams, reqwest or hyper for non-proxy calls
Database             PostgreSQL + sqlx
Cache/counters       Redis
Serialization        serde / serde_json
Auth hashing         argon2
Tracing              tracing
Metrics              Prometheus exporter
Observability        OpenTelemetry
Error handling       thiserror / anyhow

Framework boundary:

* Pingora owns the high-throughput proxy plane:
    * /v1/chat/completions
    * future OpenAI-compatible passthrough routes
    * LiteLLM upstream calls
    * direct provider passthrough
    * streaming request/response handling
    * upstream retries, timeouts, load balancing, and connection reuse
* Axum owns the control-plane API:
    * /admin-ui/healthz
    * /admin-ui/readyz
    * /admin-ui/admin/keys
    * /admin-ui/admin/keys/{key_id}
    * /admin-ui/admin/keys/{key_id}/usage
    * future policy, usage, and operator APIs
* gateway-core must stay framework-agnostic.
    * Authentication, policy, route resolution, usage construction, budget
      checks, rate-limit decisions, and pricing logic must not depend directly
      on Pingora or Axum request types.
* For the first MVP, an axum-only proxy path is acceptable only as a temporary
  bootstrap shortcut. The design target is Pingora for proxy traffic.

⸻

Repository Structure

Recommended initial structure:

relayna-gateway/
  Cargo.toml
  crates/
    gateway-api/
      src/
        main.rs
        app.rs
        routes/
        admin.rs
        health.rs
        middleware/
        errors.rs
    gateway-core/
      src/
        auth/
        policy/
        routing/
        budget/
        rate_limit/
        usage/
        pricing/
    gateway-proxy/
      src/
        server.rs
        session.rs
        litellm.rs
        passthrough.rs
        streaming.rs
        upstreams.rs
        internal.rs
    gateway-store/
      src/
        postgres.rs
        redis.rs
        models.rs
        migrations/
    gateway-telemetry/
      src/
        tracing.rs
        metrics.rs

For MVP, this can start as one crate, but the design should allow splitting later.

Intended ownership:

* gateway-api exposes axum control-plane routes and should call gateway-core for
  decisions.
* gateway-proxy exposes Pingora proxy services and should call gateway-core for
  authentication, policy, routing, and usage accounting.
* gateway-core must contain plain Rust types that can be used from both axum
  handlers and Pingora proxy callbacks.
* gateway-store owns PostgreSQL and Redis access.
* gateway-telemetry owns tracing, metrics, OpenTelemetry, redaction, and
  correlation helpers.

⸻

Phase 1 — Core Proxy MVP

Goal

Create the minimum working Relayna Gateway that can accept authenticated OpenAI-compatible requests and forward them to LiteLLM.

This phase proves:

* the Rust gateway can receive external requests
* API keys can be validated
* requests can be routed
* usage events can be recorded
* LiteLLM can be treated as one backend, not the control plane

Ultimate Goal

Establish the foundation of Relayna Gateway as the public AI API entry point.

At the end of this phase, users should be able to call:

POST /v1/chat/completions
POST /v1/responses

using a Relayna virtual key, and the gateway should forward the request to LiteLLM.

Scope

Implement:

- Pingora proxy service for OpenAI-compatible traffic
- Axum control API service for health/readiness
- health endpoint
- basic config loading
- Relayna virtual key validation
- Postgres key lookup
- route resolver
- forwarding to LiteLLM
- request ID generation
- basic usage event insert
- basic structured logging

Checklist

Server foundation

* Create Rust workspace
* Add Pingora proxy service for /v1/* traffic
* Add Axum control API service
* Add /admin-ui/healthz
* Add /admin-ui/readyz
* Add structured error response format
* Add request ID handling shared by Pingora and Axum paths
* Add tracing middleware/layers for both services
* Add graceful shutdown
* Keep auth, policy, route, usage, and budget logic out of framework-specific
  request types

Config

* Support environment variables
* Support config file later
* Required settings:
    * DATABASE_URL
    * REDIS_URL
    * LITELLM_BASE_URL
    * LITELLM_SERVICE_KEY
    * GATEWAY_BIND_ADDR
    * GATEWAY_CONTROL_BIND_ADDR
    * LOG_LEVEL

Database

Create initial schema:

api_keys
usage_events
route_policies

Minimum api_keys fields:

id
key_prefix
key_hash
project_id
owner_id
status
created_at
expires_at

Minimum usage_events fields:

id
request_id
key_id
project_id
route
model
provider
status_code
latency_ms
created_at

Authentication

* Read Authorization: Bearer rk_live_xxx
* Validate prefix
* Hash/check secret
* Reject missing key
* Reject invalid key
* Reject expired key
* Reject disabled key
* Attach key context to request extensions

Routing

* Support /v1/chat/completions
* Resolve route to LiteLLM upstream
* Strip client Authorization header
* Add internal LiteLLM service key
* Add request correlation headers:
    * X-Relayna-Request-Id
    * X-Relayna-Key-Id
    * X-Relayna-Project-Id

Proxy

* Implement proxy path through Pingora
* Forward method
* Forward path
* Forward query string
* Forward JSON body
* Return upstream status code
* Return upstream body
* Preserve relevant content-type headers
* Handle upstream timeout
* Handle upstream connection errors

Usage tracking

* Insert usage event for success
* Insert usage event for failure
* Record latency
* Record model if available in request JSON
* Record provider as litellm
* Record status code
* Never log full prompt by default

Acceptance Criteria

Phase 1 is complete when:

* A client can call /v1/chat/completions through Relayna Gateway
* A client can call /v1/responses through Relayna Gateway
* Invalid keys are rejected
* Valid keys are accepted
* Gateway forwards to LiteLLM through the proxy plane
* Usage events are inserted into Postgres
* Logs include request_id
* No provider keys or LiteLLM master key are exposed to clients
* gateway-core logic can be tested without constructing Pingora or Axum request
  objects

⸻

Phase 2 — Policy, Virtual Keys, Rate Limit, and Budget

Goal

Turn the gateway from a proxy into a real control plane.

This phase adds:

* key policies
* allowed routes
* allowed models
* Redis rate limiting
* monthly/daily budget checks
* admin APIs for key management

Ultimate Goal

A project/team should be able to create keys with controlled access and usage limits.

Example:

Key A:
- can use gpt-4o-mini
- cannot use expensive models
- max 100 requests/min
- max $50/month
- can call chat completions only

Scope

Implement:

- key policy table
- route/model enforcement
- Redis counters
- request-per-minute limit
- simple budget limit
- admin API for creating/revoking keys

Checklist

Key policy

Create key_policies table:

key_id
allowed_routes
allowed_models
allowed_providers
rpm_limit
tpm_limit
daily_budget_usd
monthly_budget_usd
allow_streaming
allow_tools

Policy enforcement

* Check route allowlist
* Check model allowlist
* Reject denied model
* Reject disabled route
* Reject passthrough if not allowed
* Reject streaming if key does not allow streaming
* Add consistent 403 policy_denied error

Rate limiting

Use Redis counters.

Required limits:

requests per minute
tokens per minute later

Initial Redis key pattern:

rl:req:{key_id}:{yyyyMMddHHmm}

Checklist:

* Increment request counter atomically
* Set expiry
* Reject over limit
* Return 429 rate_limit_exceeded
* Include retry hint if possible

Budget

Start simple.

Budget key pattern:

budget:month:{key_id}:{yyyyMM}
budget:day:{key_id}:{yyyyMMdd}

Checklist:

* Load monthly budget from policy
* Load current spend from Redis/Postgres
* Reject if budget exceeded
* Record estimated cost after request
* Update budget counter
* Return 402 budget_exceeded or 403 budget_exceeded

Admin APIs

Add internal/admin APIs:

POST   /admin-ui/admin/keys
GET    /admin-ui/admin/keys/{key_id}
PATCH  /admin-ui/admin/keys/{key_id}
DELETE /admin-ui/admin/keys/{key_id}
GET    /admin-ui/admin/keys/{key_id}/usage

Checklist:

* Create key
* Hash key before storing
* Return raw key only once
* Revoke key
* Disable key
* Update policy
* List usage summary

Security

* Admin APIs protected by internal admin token
* Raw keys never stored
* Raw keys never logged
* Secrets loaded from Kubernetes Secret or env
* Sensitive headers redacted

Acceptance Criteria

Phase 2 is complete when:

* Keys can be created and revoked
* A key can be restricted to specific models
* A key can be restricted to specific routes
* RPM rate limit works across multiple gateway replicas
* Monthly budget check works
* Usage can be queried by key/project
* Raw API keys are never persisted

⸻

Phase 3 — Streaming and Accurate Usage Tracking

Goal

Support production-grade LLM streaming and better cost accounting.

This phase focuses on:

* SSE streaming
* cancellation handling
* first-token latency
* stream completion tracking
* usage extraction
* token estimation fallback

Ultimate Goal

Relayna Gateway should safely support high-concurrency streaming LLM traffic without buffering entire responses in memory.

Scope

Implement:

- streaming proxy
- SSE passthrough
- stream lifecycle metrics
- stream cancellation
- final usage extraction
- token/cost estimator

Checklist

Streaming proxy

* Use Pingora for production streaming proxy paths
* Detect stream: true in OpenAI-compatible request
* Forward streaming response from upstream
* Return chunks to client as they arrive
* Do not buffer full stream
* Preserve SSE content type
* Handle client disconnect
* Handle upstream disconnect
* Handle timeout

Stream lifecycle events

Track:

stream_started
first_chunk_received
client_disconnected
upstream_completed
upstream_error
stream_completed

Metrics:

active_streams
first_token_latency_ms
stream_duration_ms
stream_aborts_total

Usage extraction

Support multiple strategies:

1. usage field from upstream response
2. LiteLLM response metadata
3. tokenizer-based estimation
4. flat per-route pricing fallback

Checklist:

* Extract prompt_tokens
* Extract completion_tokens
* Extract total_tokens
* Estimate tokens if missing
* Calculate estimated cost
* Record cost per provider/model
* Store usage after stream ends

Budget reservation

For streaming/concurrent requests, add reservation logic.

Flow:

1. Estimate max request cost before forwarding
2. Reserve budget
3. Start stream
4. On completion, reconcile actual cost
5. On failure, release unused reservation

Checklist:

* Add budget reservation Redis keys
* Prevent concurrent budget overspend
* Reconcile actual usage
* Release failed request reservation

Memory safety rules

Codex must avoid:

* collecting entire stream into memory
* cloning large request bodies repeatedly
* logging full prompts
* unbounded channels
* unbounded response buffers

Acceptance Criteria

Phase 3 is complete when:

* Streaming chat completion works through the gateway
* Client disconnect does not crash the gateway
* Upstream disconnect produces clean error handling
* First-token latency is recorded
* Active stream count is tracked
* Usage is recorded after stream completion
* Budget reservation prevents concurrent overspend

⸻

Phase 4 — Advanced Routing, Passthrough, and Relayna Runtime Integration

Goal

Expand Gateway beyond LiteLLM.

This phase adds:

* direct provider passthrough
* internal service routing
* Relayna task submission
* route-level pricing
* provider fallback
* route-specific auth modes

Ultimate Goal

Relayna Gateway becomes the universal AI traffic entry point for LLMs, tools, providers, and Relayna tasks.

Scope

Implement routing to:

- LiteLLM
- direct providers
- internal services
- Relayna task runtime

Checklist

Route resolver

Add route config:

routes:
  - pattern: /v1/chat/completions
    backend_type: litellm
    upstream_url: http://litellm:4000/v1/chat/completions
    auth_mode: service_token
    cost_mode: token
  - pattern: /pass/mistral/*
    backend_type: direct_provider
    upstream_url: https://api.mistral.ai
    auth_mode: provider_key
    cost_mode: route_price
  - pattern: /tasks/*
    backend_type: relayna_runtime
    upstream_url: http://relayna-api/internal/tasks
    auth_mode: internal_jwt
    cost_mode: task_price

Checklist:

* Match static routes
* Match wildcard passthrough routes
* Support per-route timeout
* Support per-route body size limit
* Support per-route cost mode
* Support per-route auth mode

Direct provider passthrough

For non-standard APIs:

/pass/{provider}/{path...}

Checklist:

* Sanitize incoming headers
* Remove user auth
* Inject provider auth
* Preserve provider path/query
* Enforce route permission
* Apply route-level pricing
* Track request/latency/status
* Support non-token cost modes

Internal service routing

Use for:

- OCR services
- summarization services
- embedding services
- document handlers
- sandbox controllers

Checklist:

* Inject internal JWT/service token
* Add request correlation headers
* Apply internal route policy
* Track cost if configured
* Track errors

Relayna task integration

Public endpoint:

POST /tasks/{task_type}

Gateway behavior:

1. Validate key
2. Check task permission
3. Estimate task cost
4. Reserve budget
5. Create Relayna task
6. Return task_id

Relayna internal request:

{
  "request_id": "req_123",
  "project_id": "proj_123",
  "key_id": "key_123",
  "task_type": "document-report",
  "input": {}
}

Checklist:

* Create task endpoint
* Map task type to Relayna internal API
* Attach key/project/request metadata
* Store task usage attribution
* Return task ID
* Add task status proxy endpoint
* Add task events proxy endpoint

Worker-to-gateway calls

Relayna workers should call Gateway for metered LLM/provider requests.

Checklist:

* Add internal worker auth
* Support X-Relayna-Task-Id
* Support X-Relayna-Run-Id
* Attribute LLM usage to task
* Attribute provider cost to task
* Add task-aware usage events

Fallback routing

Optional but useful:

primary: LiteLLM Azure OpenAI
fallback: OpenAI
fallback: local vLLM

Checklist:

* Add fallback chain config
* Retry only on safe error classes
* Do not retry non-idempotent task creation blindly
* Record fallback provider
* Record final provider used

Acceptance Criteria

Phase 4 is complete when:

* Gateway supports direct passthrough routes
* Gateway supports internal service routes
* Gateway can submit Relayna tasks
* Relayna workers can call Gateway for metered AI usage
* Usage can be attributed to both key and task
* Route-level pricing works
* Provider auth is never exposed to users

⸻

Phase 5 — Relayna Studio, Observability, and Production Readiness

Goal

Expose gateway and runtime data to Relayna Studio and harden the system for production.

This phase focuses on:

* dashboards
* logs
* traces
* usage analytics
* provider health
* operational safety
* deployment readiness

Ultimate Goal

Relayna Studio should become the unified observability and control UI for:

- API keys
- usage
- cost
- routes
- provider health
- task lifecycle
- worker logs
- artifacts
- failures

Scope

Implement:

- usage query APIs
- dashboard APIs
- metrics
- tracing
- logs
- Kubernetes deployment manifests
- production hardening

Checklist

Usage dashboards

APIs:

GET /usage/summary
GET /usage/by-key
GET /usage/by-project
GET /usage/by-model
GET /usage/by-provider
GET /usage/by-task
GET /usage/timeseries

Checklist:

* Usage by key
* Usage by project
* Usage by provider
* Usage by model
* Cost over time
* Error rate over time
* Latency over time

Task observability

Gateway should expose or proxy:

GET /tasks/{task_id}
GET /tasks/{task_id}/events
GET /tasks/{task_id}/usage
GET /tasks/{task_id}/logs

Checklist:

* Show task status
* Show task timeline
* Show LLM calls inside task
* Show provider calls inside task
* Show cost per task
* Show worker failures
* Show artifact links

Provider health

Track:

provider availability
provider latency
provider error rate
model-level error rate
fallback count
timeout count

Checklist:

* Add provider health endpoint
* Add per-provider metrics
* Add per-model metrics
* Add upstream timeout metrics
* Add circuit breaker later if needed

OpenTelemetry

Checklist:

* Add trace ID propagation
* Add request spans
* Add upstream spans
* Add Redis spans
* Add Postgres spans
* Add Relayna task spans
* Export traces to Tempo/Jaeger

Prometheus metrics

Required metrics:

gateway_requests_total
gateway_request_duration_ms
gateway_upstream_duration_ms
gateway_errors_total
gateway_rate_limit_rejections_total
gateway_budget_rejections_total
gateway_tokens_total
gateway_estimated_cost_total
gateway_active_streams
gateway_stream_aborts_total
gateway_first_token_latency_ms

Checklist:

* Expose /admin-ui/metrics
* Add Kubernetes ServiceMonitor if using Prometheus Operator
* Add Grafana dashboard JSON later

Logging

Checklist:

* JSON structured logs
* Include request ID
* Include key ID, not raw key
* Include project ID
* Include route
* Include provider
* Redact sensitive headers
* Do not log prompts by default
* Add debug mode for safe development only

Production deployment

Kubernetes resources:

Deployment
Service
ConfigMap
Secret
HorizontalPodAutoscaler
PodDisruptionBudget
ServiceMonitor
Ingress/Gateway API

Checklist:

* Container image
* Non-root user
* Readiness probe
* Liveness probe
* Graceful shutdown
* Resource requests/limits
* HPA on CPU and active requests
* Secret-based config
* TLS/mTLS support
* NetworkPolicy if needed

Reliability

Checklist:

* Upstream timeouts
* Retry policy
* Circuit breaker design
* Backpressure handling
* Max request body size
* Max concurrent streams
* Connection pool tuning
* Graceful shutdown waits for streams
* Clear error taxonomy

Acceptance Criteria

Phase 5 is complete when:

* Relayna Studio can display gateway usage
* Relayna Studio can display task-level cost
* Metrics are exposed
* Logs are structured
* Traces are correlated
* Gateway is deployable on AKS
* Secrets are not hardcoded
* Production failure modes are handled cleanly

⸻

Final Product Vision

At the end of all five phases, Relayna Gateway should provide this capability:

One public AI gateway for all Relayna workloads.

It should support:

- API key governance
- LLM routing
- provider passthrough
- task execution submission
- cost tracking
- budget enforcement
- rate limiting
- streaming
- observability
- Relayna Studio integration

The final architecture should look like this:

Client / SDK / Studio
        |
        v
Relayna Gateway
        |
        ├── Auth
        ├── Policy
        ├── Budget
        ├── Rate Limit
        ├── Usage Tracking
        ├── Streaming Proxy
        ├── Route Resolver
        │
        ├── LiteLLM
        ├── Direct Providers
        ├── Internal Services
        └── Relayna Runtime

Relayna Gateway should become the trusted front door.

Relayna Runtime should become the trusted execution layer.

Relayna Studio should become the trusted visibility layer.

Together:

Relayna Gateway = control
Relayna Runtime = execution
Relayna Studio = visibility

⸻

Codex Agent Instruction

When implementing this project, always prioritize:

1. Security before convenience
2. Streaming over buffering
3. Explicit policy over implicit behavior
4. Usage tracking for every request
5. Clear error types
6. Observability from day one
7. Provider keys never exposed to users
8. Raw API keys never stored
9. Gateway as control plane
10. Relayna as execution runtime

Do not build Relayna Gateway as a thin proxy.

Build it as the AI traffic control plane for Relayna.
