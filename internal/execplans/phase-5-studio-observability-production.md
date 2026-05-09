# Phase 5 Relayna Studio, Observability, and Production Readiness

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Expose gateway and runtime data to Relayna Studio and harden Relayna Gateway
for production operation. Studio should display usage, cost, provider health,
task lifecycle, task-level cost, logs, traces, and failures. Operators should
deploy the gateway on AKS with safe secrets, probes, metrics, and graceful
failure behavior.

## Progress

- [ ] Confirm Phases 1 through 4 are complete.
- [x] (2026-05-09 18:40 +07) Establish compatibility boundary for usage queries, telemetry fields,
      metrics names, log fields, deployment config, and task observability.
- [x] (2026-05-09 18:40 +07) Add usage and dashboard query APIs.
- [ ] Add task observability APIs or proxy behavior.
- [x] (2026-05-09 18:40 +07) Add provider and internal service health query API.
- [ ] Add OpenTelemetry propagation and spans.
- [x] (2026-05-09 18:40 +07) Add Prometheus metrics endpoint and baseline counters.
- [ ] Add structured JSON logging and redaction coverage.
- [x] (2026-05-09 18:40 +07) Add Kubernetes production deployment resources.
- [ ] Add reliability hardening for timeouts, retries, backpressure, limits,
      graceful shutdown, and error taxonomy.
- [ ] Run `$code-change-verification` and deployment validation.

## Surprises & Discoveries

- Observation: Studio-facing usage APIs can be served from existing
  `usage_events` with additive token, service, and fallback columns.
  Evidence: Phase 5 query implementation uses PostgreSQL aggregation over
  `usage_events`.

## Decision Log

- Decision: Metrics and logs must avoid high-cardinality raw user input.
  Rationale: Observability should help operations without leaking prompts,
  keys, provider payloads, or unbounded labels.
  Date/Author: 2026-05-08 / Codex.
- Decision: Expose internal service health alongside provider health.
  Rationale: Phase 4 service passthrough makes `/summary`, `/translation`, and
  other internal services operational dependencies visible to Studio.
  Date/Author: 2026-05-09 / Codex.

## Outcomes & Retrospective

Started. Protected usage summary, timeseries, breakdown, provider/service
health, `/metrics`, and Kubernetes resources are implemented. OpenTelemetry
span propagation, redaction-specific tests, graceful shutdown validation, and
deployment dry-run validation remain.

## Context and Orientation

Phase 5 turns gateway data into Studio-facing visibility and production
operational readiness.

Important terms:

- Relayna Studio: the visibility and control UI that consumes gateway usage,
  provider health, and task lifecycle data.
- Provider health: availability, latency, error rate, timeout, fallback, and
  model-level status data for upstream providers.
- OpenTelemetry: trace propagation and spans across gateway, providers, Redis,
  PostgreSQL, and Relayna runtime.
- Production readiness: deployment resources, probes, secrets, graceful
  shutdown, resource limits, and reliable failure behavior.

Expected areas:

- `crates/gateway-api/`: usage APIs, task observability APIs, provider health
  endpoint, metrics endpoint, readiness/liveness behavior, and error taxonomy.
- `crates/gateway-core/`: usage aggregation, provider health calculations,
  reliability policy, and response shaping.
- `crates/gateway-store/`: usage query implementation, task usage reads, and
  provider health storage if needed.
- `crates/gateway-telemetry/`: metrics, tracing, structured logging,
  redaction, and correlation fields.
- Deployment files: container, Kubernetes, monitoring, secret, autoscaling, and
  ingress or Gateway API manifests.
- `tests/`: usage query, task observability, metrics, traces, redaction,
  readiness, and shutdown tests.

## Compatibility Boundary

Compatibility boundary: compare usage query responses, task observability
responses, metrics names, trace attributes, log fields, configuration, and
deployment manifests against the latest release tag. Relayna Studio consumes
these shapes, so additive changes are preferred once released.

Metrics must preserve stable names and bounded labels after release. Logs and
traces may add safe fields, but must not expose raw keys, prompts, provider
credentials, internal service tokens, or unbounded payload fields.

## Plan of Work

Add usage query APIs for summary, by-key, by-project, by-model, by-provider,
by-task, and timeseries views. Include cost, error rate, and latency views over
time while preserving stable response shapes for Studio.

Add task observability APIs or proxy behavior for task status, events, usage,
and logs. Ensure Studio can show task timelines, LLM calls inside tasks,
provider calls inside tasks, per-task cost, worker failures, and artifact
links.

Add provider health tracking for availability, latency, error rate,
model-level error rate, fallback count, and timeout count. Expose provider
health and metrics endpoints.

Add OpenTelemetry trace propagation and spans for gateway requests, upstream
calls, Redis, PostgreSQL, and Relayna task calls. Add Prometheus metrics listed
in the manifesto.

Add JSON structured logging with request ID, key ID, project ID, route,
provider, and redacted sensitive fields. Ensure logs use key IDs, not raw keys,
and do not log prompts by default.

Add production deployment resources for AKS: Deployment, Service, ConfigMap,
Secret, autoscaling, PodDisruptionBudget, monitoring resources, and ingress or
Gateway API exposure. Configure non-root containers, probes, resource
requests/limits, secret-based config, TLS or mTLS support, and optional network
policies.

Harden reliability behavior: upstream timeouts, retry policy, backpressure,
max body size, max concurrent streams, connection pool tuning, graceful
shutdown that waits for streams within configured limits, and clear error
taxonomy. Add circuit breaker design or implementation if provider health data
shows it is needed.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo test -p gateway-api
    cargo test -p gateway-core
    cargo test -p gateway-telemetry
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Run deployment validation with the repository's chosen Kubernetes tooling once
manifests exist. If no tooling exists yet, add documented validation commands
as part of this phase.

## Validation and Acceptance

Phase 5 is accepted when:

- Relayna Studio can display gateway usage, cost, errors, latency, provider
  health, and task-level cost.
- Metrics are exposed and scrapeable by Prometheus.
- Structured logs and traces correlate gateway, provider, Redis, PostgreSQL,
  and Relayna runtime activity.
- Gateway is deployable on AKS without hardcoded secrets.
- Production failure modes produce stable errors, metrics, and logs.
- Graceful shutdown preserves in-flight request and stream behavior within
  configured limits.

Required tests:

- Unit tests for usage aggregation, dashboard response shaping, telemetry
  labels, log redaction, provider health calculations, and error taxonomy.
- Integration tests for usage query APIs, task observability APIs, metrics
  exposure, trace propagation, health/readiness behavior, and graceful
  shutdown.
- Deployment validation for Kubernetes manifests, probes, secret wiring,
  resource settings, monitoring resources, and ingress or Gateway API config.

## Idempotence and Recovery

Usage and dashboard tests should use deterministic timestamps and isolated
projects so repeated runs produce stable aggregates. If local usage data is
left behind, reset only the local test database.

Metrics and tracing tests should avoid depending on global process state where
possible. If a test installs a global subscriber or exporter, isolate it or
run it serially.

Deployment validation should be dry-run capable before applying to a cluster.
If a local cluster apply fails, delete only resources in the test namespace and
rerun validation.

## Artifacts and Notes

Prometheus metrics required by the manifesto:

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

Production resources expected by the manifesto:

    Deployment
    Service
    ConfigMap
    Secret
    HorizontalPodAutoscaler
    PodDisruptionBudget
    ServiceMonitor
    Ingress or Gateway API

## Interfaces and Dependencies

Phase 5 depends on completed usage tracking, task attribution, route/provider
tracking, streaming metrics, Redis, PostgreSQL, and Relayna runtime
integration.

The end state includes Studio-facing usage and task observability APIs,
provider health, Prometheus metrics, OpenTelemetry traces, structured redacted
logs, Kubernetes deployment resources, and hardened production reliability
behavior.
