# Phase 1 Core Proxy MVP

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Create the minimum working Relayna Gateway that accepts authenticated
OpenAI-compatible generation requests and forwards them to LiteLLM. A client
should call `POST /v1/chat/completions` or `POST /v1/responses` with a Relayna
virtual key, receive the upstream response, and leave behind a usage event
operators can query later.

Relayna virtual keys are the only external client credential. The gateway must
strip client credentials, inject internal LiteLLM credentials, record request
correlation data, and avoid logging prompt bodies by default.

## Progress

- [x] (2026-05-08 23:59 +07) Confirm current repository state and latest
      release tag before runtime edits. Working tree was clean; no `v*` release
      tag was present.
- [x] (2026-05-08 23:59 +07) Create the initial Rust workspace and
      crate/module boundaries.
- [x] (2026-05-08 23:59 +07) Add configuration loading for required Phase 1
      settings.
- [x] (2026-05-08 23:59 +07) Add PostgreSQL persistence for virtual key lookup
      and usage inserts.
- [x] (2026-05-08 23:59 +07) Add framework-agnostic authentication, route, and
      usage core logic.
- [x] (2026-05-08 23:59 +07) Add Axum health/readiness control API.
- [x] (2026-05-08 23:59 +07) Add proxy handling for
      `/v1/chat/completions` and `/v1/responses`, including a Pingora
      `ProxyHttp` service implementation in `gateway-proxy` and an Axum-hosted
      tested path in `gateway-api`.
- [x] (2026-05-08 23:59 +07) Add request IDs, structured errors, tracing, and
      graceful shutdown.
- [x] (2026-05-08 23:59 +07) Add unit and black-box mock-upstream tests.
      Remaining: manual smoke test against local PostgreSQL, Redis, and
      LiteLLM-compatible upstream.
- [x] (2026-05-08 23:59 +07) Run `$code-change-verification` and record
      results.

## Surprises & Discoveries

- Observation: The local default Rust toolchain was `rustc 1.59.0`, which was
  too old for the selected 2026 gateway dependencies.
  Evidence: `rustup update stable` moved the toolchain to `rustc 1.95.0`.
- Observation: Pingora and Axum are represented as separate framework
  boundaries. The tested runnable binary currently hosts health/readiness and
  proxy routes through Axum, while `gateway-proxy` contains the Pingora
  `ProxyHttp` implementation for the proxy plane.
  Evidence: `crates/gateway-api/src/app.rs` black-box tests pass for both
  generation routes; `crates/gateway-proxy/src/pingora_plane.rs` compiles
  through clippy and tests.

## Decision Log

- Decision: Treat Phase 1 as the first released public route and schema shape
  unless a release tag proves otherwise.
  Rationale: There is no Rust workspace in the current repo, so the first
  implementation establishes the compatibility baseline for future phases.
  Date/Author: 2026-05-08 / Codex.
- Decision: Include `POST /v1/responses` alongside
  `POST /v1/chat/completions` in Phase 1 generation routing.
  Rationale: Responses is the current OpenAI generation API for new clients,
  while Chat Completions remains required for existing OpenAI-compatible
  clients.
  Date/Author: 2026-05-08 / Codex.
- Decision: Keep Redis as a Phase 1 readiness dependency but do not add active
  counters yet.
  Rationale: Rate limits and budgets are Phase 2 scope, but preserving the
  deployment shape now avoids a later required configuration change.
  Date/Author: 2026-05-08 / Codex.

## Outcomes & Retrospective

Implemented the initial Rust workspace, configuration, PostgreSQL schema,
framework-agnostic auth/routing/usage core, Axum health/readiness API,
LiteLLM proxy forwarding, Pingora proxy service implementation, usage
recording, and tests. `$code-change-verification` passed on 2026-05-08.

Remaining operational gap: run the manual smoke test with local PostgreSQL,
Redis, and a LiteLLM-compatible upstream.

## Context and Orientation

Relayna Gateway is the public control plane for AI traffic. In Phase 1 it is
not yet a full policy system; it proves the secure proxy foundation.

Important terms:

- Virtual key: an external Relayna API key in the form `rk_live_xxx`.
- LiteLLM upstream: the internal OpenAI-compatible backend selected for Phase
  1 proxy traffic.
- Usage event: a durable record of a request with request ID, key, project,
  route, model when available, provider, status, latency, and timestamp.
- Gateway core: framework-agnostic Rust logic for auth, routing, usage, policy,
  rate-limit, budget, and pricing decisions.

Expected areas:

- `crates/gateway-api/`: Axum control API for `/healthz`, `/readyz`, errors,
  request IDs, middleware, and shutdown wiring.
- `crates/gateway-core/`: virtual key validation, route resolution, usage
  event construction, and plain Rust decision types.
- `crates/gateway-proxy/`: Pingora proxy service and shared LiteLLM forwarding
  support for `/v1/chat/completions` and `/v1/responses`, upstream request
  construction, credential stripping, and LiteLLM forwarding.
- `crates/gateway-store/`: PostgreSQL models, migrations, key lookup, and
  usage insert behavior.
- `crates/gateway-telemetry/`: tracing setup, redaction, and request
  correlation helpers.
- `tests/`: black-box gateway behavior tests where practical.

## Compatibility Boundary

Compatibility boundary: latest release tag must be checked before runtime
edits with `git tag -l 'v*' --sort=-v:refname | head -n1`. If no released
gateway route/schema exists, Phase 1 may define the initial baseline directly.

Public surfaces created in this phase are compatibility-sensitive once
released: `/v1/chat/completions`, `/v1/responses`, `/healthz`, `/readyz`,
structured error responses, request correlation headers, required environment
variables, PostgreSQL `api_keys`, `usage_events`, and `route_policies`
schemas, and usage event fields.

## Plan of Work

Start by creating the Cargo workspace and package boundaries listed above.
Implement configuration loading with required environment variables:
`DATABASE_URL`, `REDIS_URL`, `LITELLM_BASE_URL`, `LITELLM_SERVICE_KEY`,
`GATEWAY_BIND_ADDR`, and `LOG_LEVEL`.

Add initial PostgreSQL migrations and store code for virtual key lookup and
usage event inserts. Store key prefixes and hashes only. Use `argon2` for
secret verification.

Build core types for authenticated key context, route resolution,
upstream target, usage event construction, and gateway errors. Keep these
types independent of Axum and Pingora request types.

Add the Axum control API with health and readiness endpoints, shared request ID
handling, structured error responses, tracing, and graceful shutdown.

Add proxy paths for `POST /v1/chat/completions` and `POST /v1/responses`.
Validate the Relayna virtual key before forwarding, strip client
`Authorization`, inject the LiteLLM service credential, add Relayna correlation
headers, forward the request, preserve relevant response headers, and map
upstream timeouts or connection failures to stable gateway errors.

Record usage events for both successful and failed requests. Capture request
ID, key ID, project ID, route, model when present in JSON, provider `litellm`,
status code, latency, and timestamp. Do not log full prompts by default.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Observed result on 2026-05-08:

    code-change-verification: all commands passed.

While iterating, use focused package tests once packages exist, then finish
with the full verification stack.

## Validation and Acceptance

Phase 1 is accepted when:

- A valid `rk_live_xxx` key can call `POST /v1/chat/completions` through the
  gateway and receive the LiteLLM response.
- A valid `rk_live_xxx` key can call `POST /v1/responses` through the gateway
  and receive the LiteLLM response.
- Missing, invalid, expired, and disabled keys are rejected before any upstream
  call.
- Client credentials are stripped and internal LiteLLM credentials are never
  returned to clients.
- Usage events are inserted for success and failure.
- Logs contain `request_id` and do not include full prompt payloads by default.
- Core authentication, route, and usage logic are unit tested without Axum or
  Pingora request objects.

Required tests:

- Unit tests for key parsing, key verification, route resolution, credential
  stripping decisions, usage event construction, and error mapping.
- Integration or black-box tests for valid proxying, invalid auth, upstream
  timeout, upstream connection failure, and usage insertion.
- Manual smoke test against a seeded PostgreSQL key and LiteLLM-compatible
  upstream.

## Idempotence and Recovery

All Rust checks are safe to rerun. If migrations partially apply in a local
database, reset the local development database or add a forward migration that
repairs the state; do not rewrite migrations after they are shared or released.

If Redis counters or readiness state become stale during testing, flush only
the local development Redis database. Never run destructive database or Redis
commands against shared environments.

If an interrupted proxy test leaves a server running, stop the local process,
confirm ports are free, and rerun the focused test before the full verification
stack.

## Artifacts and Notes

Sample client request:

    POST /v1/chat/completions
    Authorization: Bearer rk_live_xxx
    Content-Type: application/json

    {"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}

Sample Responses API request:

    POST /v1/responses
    Authorization: Bearer rk_live_xxx
    Content-Type: application/json

    {"model":"gpt-4o-mini","input":"ping"}

Expected upstream correlation headers:

    X-Relayna-Request-Id: <request id>
    X-Relayna-Key-Id: <key id>
    X-Relayna-Project-Id: <project id>

## Interfaces and Dependencies

Phase 1 depends on PostgreSQL, Redis configuration, and a LiteLLM or
OpenAI-compatible upstream. Redis may be configured before active counters are
used so later phases can add rate limits without changing deployment shape.

The end state includes a Cargo workspace, gateway crates or modules, required
environment variables, initial PostgreSQL schemas, health/readiness endpoints,
`POST /v1/chat/completions`, `POST /v1/responses`, Relayna request correlation
headers, and a usage event writer.
