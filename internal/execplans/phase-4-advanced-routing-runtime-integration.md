# Phase 4 Advanced Routing, Passthrough, and Relayna Runtime Integration

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Expand Relayna Gateway beyond LiteLLM so it becomes the universal AI traffic
entry point for LLMs, direct providers, internal services, and Relayna task
submission. External clients and Relayna workers should use the gateway for
metered provider access, with usage attributable to keys, projects, tasks, and
runs.

## Progress

- [ ] Confirm Phases 1 through 3 are complete.
- [x] (2026-05-09 18:40 +07) Establish compatibility boundary for route config, passthrough behavior,
      task request shapes, worker headers, and usage attribution fields.
- [x] (2026-05-09 18:40 +07) Add route resolver support for static and wildcard routes.
- [ ] Add per-route timeout, body size, auth mode, and cost mode handling.
- [x] (2026-05-09 18:40 +07) Add OpenAI-compatible direct provider upstream selection.
- [x] (2026-05-09 18:40 +07) Add internal service route matching, policy, upstream selection, and usage attribution.
- [ ] Add Relayna task submission and task status/events proxy behavior.
- [ ] Add worker-to-gateway authentication and task/run usage attribution.
- [ ] Add provider fallback chains for safe error classes.
- [ ] Run `$code-change-verification` and record results.

## Surprises & Discoveries

- Observation: Phase 4 runtime task integration is intentionally deferred by
  product decision; gateway-only direct provider and internal service
  passthrough are the active scope.
  Evidence: User selected "focus on relayna-gateway only" during planning.

## Decision Log

- Decision: Route decisions remain in framework-agnostic core logic.
  Rationale: Pingora, Axum, internal service calls, and task submission all
  need the same policy and routing decisions without framework coupling.
  Date/Author: 2026-05-08 / Codex.
- Decision: Include internal service passthrough as first-class Phase 4 scope
  for `/summary`, `/translation`, `/ocr`, `/embeddings`, and `/services/*`.
  Rationale: These are gateway-owned service routes, not Relayna runtime task
  APIs, and need the same auth, policy, budget, and usage controls.
  Date/Author: 2026-05-09 / Codex.

## Outcomes & Retrospective

Started. Core route matching, provider/service policy fields, optional
OpenAI-compatible and internal service upstream config, credential injection,
and usage attribution are implemented. Per-route timeout/body-size config,
fallback retry execution, worker-token attribution, and Relayna runtime APIs
remain.

## Context and Orientation

Earlier phases establish key authentication, policy, budgets, usage, and
streaming. Phase 4 broadens what the gateway can route to and introduces
Relayna runtime integration.

Important terms:

- Direct provider passthrough: forwarding non-standard provider API requests
  after sanitizing user headers and injecting internal provider credentials.
- Internal service route: a gateway route to trusted internal AI services such
  as OCR, summarization, embeddings, document handlers, or sandbox controllers.
- Relayna runtime integration: gateway behavior that creates Relayna tasks and
  proxies task status/events.
- Worker-to-gateway call: a Relayna worker calling the gateway for metered LLM
  or provider usage attributed to a task and run.

Expected areas:

- `crates/gateway-core/`: route matching, backend type, auth mode, cost mode,
  fallback decision, task attribution, and worker auth decisions.
- `crates/gateway-proxy/`: direct provider passthrough and fallback routing.
- `crates/gateway-api/`: task submission, task status, task events, and
  internal service control routes as needed.
- `crates/gateway-store/`: route config persistence or loading, task usage
  attribution fields, and pricing data.
- `crates/gateway-telemetry/`: route/provider/task/run correlation fields.
- `tests/`: passthrough, task runtime, worker call, fallback, and attribution
  tests.

## Compatibility Boundary

Compatibility boundary: compare route config, passthrough routes, task APIs,
worker headers, usage event fields, and provider routing behavior against the
latest release tag. Once released, these are durable integration contracts for
external clients, Relayna Studio, Relayna workers, and Relayna runtime.

Prefer additive route and usage fields where existing consumers may depend on
current shapes. Do not add compatibility shims for unreleased branch-local
route experiments unless explicitly requested.

## Plan of Work

Extend the route resolver to support static and wildcard route matches with
backend type, upstream URL, auth mode, cost mode, timeout, and body size limit.
Keep the matching and policy decisions in gateway core.

Add direct provider passthrough routes. Sanitize incoming headers, strip user
credentials, inject provider credentials, preserve path and query data, enforce
route permission, apply route-level pricing, and record request status,
latency, provider, model or route, and cost.

Add internal service routing with internal service credentials, correlation
headers, policy checks, usage tracking, and cost tracking when configured.

Add Relayna task submission. Validate the key, check task permission, estimate
cost, reserve budget, call Relayna runtime with request, project, key, task
type, and input metadata, then return a task ID. Add task status and task event
proxy behavior.

Add worker-to-gateway authentication and support task/run attribution headers.
Attribute LLM and provider usage to task and run identifiers.

Add provider fallback chains. Retry only safe error classes, never blindly
retry non-idempotent task creation, and record the final provider used.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo test -p gateway-core
    cargo test -p gateway-proxy
    cargo test -p gateway-api
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Use stub providers and a stub Relayna runtime for focused integration tests.
Finish with the full verification stack.

## Validation and Acceptance

Phase 4 is accepted when:

- Gateway supports direct provider passthrough without exposing provider
  credentials.
- Gateway supports internal service routes with internal authentication.
- Gateway can submit Relayna tasks and return task IDs.
- Gateway can proxy task status and task events.
- Relayna workers can call the gateway for metered LLM/provider usage.
- Usage can be attributed to key, project, task, and run context.
- Route-level pricing and fallback provider attribution work.

Required tests:

- Unit tests for route matching, auth mode selection, cost mode selection,
  provider credential injection, task attribution, worker auth decisions, and
  fallback retry classification.
- Integration tests for direct provider passthrough, internal service routing,
  task submission, task status/events proxying, worker calls, and usage
  attribution.
- Manual verification with stub providers and stub Relayna runtime, avoiding
  real provider secrets.

## Idempotence and Recovery

Stub providers and runtime services should use local ports and deterministic
fixtures so tests can rerun safely. If a local stub remains running after a
failed test, stop it and rerun the focused integration test.

Task submission tests must use idempotent stub runtime behavior or unique task
fixtures. If a test creates durable local task records, reset only the local
test database.

Fallback tests must avoid real provider calls and should assert retry counts
and final provider attribution from deterministic stub responses.

## Artifacts and Notes

Task creation metadata sent to Relayna runtime should include:

    request_id
    project_id
    key_id
    task_type
    input

Worker attribution should include task and run context without exposing worker
tokens or provider credentials to external clients.

## Interfaces and Dependencies

Phase 4 depends on completed auth, policy, budget reservation, usage tracking,
and streaming foundations.

The end state includes route resolver extensions, passthrough routes, internal
service routes, Relayna task submission, task status/events proxy behavior,
worker authentication, task-aware usage events, route-level pricing, and
provider fallback attribution.
