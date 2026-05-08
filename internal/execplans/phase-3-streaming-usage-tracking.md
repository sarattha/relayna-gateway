# Phase 3 Streaming and Accurate Usage Tracking

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`; checklist gates are summarized
in `internal/mvp-phase-roadmap.md`.

## Purpose / Big Picture

Support production-grade LLM streaming without buffering complete responses in
memory, and improve usage/cost accounting for both streamed and non-streamed
requests. Clients should receive server-sent event chunks as they arrive, while
operators can observe first-token latency, active streams, stream aborts, final
usage, and reconciled cost.

## Progress

- [ ] Confirm Phase 1 and Phase 2 behavior is complete.
- [ ] Establish compatibility boundary for streaming, usage, Redis budget
      reservation state, and telemetry fields.
- [ ] Add streaming request detection.
- [ ] Add Pingora SSE passthrough without full response buffering.
- [ ] Add stream lifecycle telemetry and metrics.
- [ ] Add disconnect and timeout handling.
- [ ] Add usage extraction fallback order and pricing.
- [ ] Add budget reservation and reconciliation.
- [ ] Add streaming, disconnect, and accounting tests.
- [ ] Run `$code-change-verification` and record results.

## Surprises & Discoveries

- None yet.

## Decision Log

- Decision: Treat streaming as a first-class proxy path, not a buffered
  convenience mode.
  Rationale: The design manifesto prioritizes streaming over buffering and
  requires high-concurrency safety.
  Date/Author: 2026-05-08 / Codex.

## Outcomes & Retrospective

Not started.

## Context and Orientation

Phase 3 builds on authenticated proxying, policy checks, Redis state, and
budget enforcement. Streaming traffic must preserve the provider's incremental
delivery behavior while still producing a complete usage record after the
stream ends.

Important terms:

- Streaming proxy: a proxy path that forwards chunks as they arrive without
  collecting the full provider response.
- First-token latency: time from accepted gateway request to first upstream
  content chunk.
- Budget reservation: a Redis-backed hold for estimated maximum request cost
  that is reconciled when final usage is known.
- Usage extraction fallback: ordered strategies for determining tokens and
  cost when upstream metadata is partial or absent.

Expected areas:

- `crates/gateway-proxy/`: streaming session handling, SSE passthrough,
  disconnect handling, and timeout behavior.
- `crates/gateway-core/`: streaming detection, usage extraction decisions,
  pricing fallback, and budget reservation lifecycle.
- `crates/gateway-store/`: Redis reservation keys and PostgreSQL usage updates.
- `crates/gateway-telemetry/`: active stream metrics, first-token latency,
  stream duration, and abort counters.
- `tests/`: streaming chunk timing, cancellation, timeout, usage, and budget
  reconciliation coverage.

## Compatibility Boundary

Compatibility boundary: compare streaming behavior, usage event fields, Redis
budget key formats, and telemetry names against the latest release tag before
editing. Once released, cancellation behavior, final status recording, usage
fields, and metric names are compatibility-sensitive.

Prefer additive telemetry fields and forward-compatible usage event changes.
For Redis reservation state, document key format, TTL, and cleanup behavior in
the implementation plan before release.

## Plan of Work

Add detection for OpenAI-compatible requests with `stream: true`. Route those
requests through Pingora streaming behavior that forwards SSE chunks to the
client as they arrive and preserves content type.

Implement lifecycle tracking for stream start, first chunk received, client
disconnect, upstream completion, upstream error, and stream completion. Record
active streams, first-token latency, stream duration, and stream abort counts.

Handle client disconnects, upstream disconnects, and timeouts without panics,
leaked tasks, unreleased budget reservations, or missing usage records.

Add usage extraction in this order: upstream usage field, LiteLLM response
metadata, tokenizer-based estimation, and flat route pricing fallback. Persist
prompt tokens, completion tokens, total tokens, estimated cost, provider,
model, latency, and final status.

Add budget reservation before forwarding streaming requests. Estimate maximum
cost, reserve budget, reconcile actual cost on completion, and release unused
reservation on failure or cancellation.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    git status --short
    git tag -l 'v*' --sort=-v:refname | head -n1
    cargo test -p gateway-proxy
    cargo test -p gateway-core
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Use focused tests with stub streaming upstreams while iterating. Finish with
full workspace verification.

## Validation and Acceptance

Phase 3 is accepted when:

- Streaming chat completions pass through chunk by chunk.
- A client receives chunks before the upstream response is complete.
- Client disconnect and upstream disconnect paths do not crash the gateway.
- First-token latency, active stream count, duration, and abort metrics are
  observable.
- Usage and estimated cost are recorded after stream completion and on failure.
- Budget reservation prevents concurrent overspend and releases unused holds.
- No implementation path buffers full streamed responses by default.

Required tests:

- Unit tests for stream request detection, lifecycle event construction, usage
  fallback order, and budget reservation reconciliation.
- Integration tests using a stub SSE upstream that delays chunks to prove
  incremental forwarding.
- Disconnect tests covering client cancellation, upstream cancellation,
  timeout, reservation release, and failure usage insertion.

## Idempotence and Recovery

Streaming tests must use bounded timeouts so interrupted test runs do not hang.
If a local server remains running after a failed test, stop it and rerun the
focused streaming test.

Budget reservation tests should isolate Redis keys by test prefix. If a run is
interrupted, delete only those local reservation keys or flush only the local
test Redis database.

If a migration extends usage fields for token/cost data, rerun it only against
local test databases until reviewed. Shared migrations must be corrected with
forward migrations.

## Artifacts and Notes

Lifecycle events expected during a successful stream:

    stream_started
    first_chunk_received
    upstream_completed
    stream_completed

Lifecycle events expected during client cancellation:

    stream_started
    first_chunk_received
    client_disconnected

## Interfaces and Dependencies

Phase 3 depends on Phase 2 policy and budget decisions. It adds or finalizes
streaming proxy behavior, stream lifecycle metrics, usage extraction logic,
token/cost estimation fallback, and Redis budget reservation state.
