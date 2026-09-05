# Shared request investigation and per-attempt timing

This living ExecPlan follows PLANS.md.

## Purpose
Operators opening a request from Usage or Traffic should see the same structured summary, context, failures, timing, policy and sanitized diagnostics. Network timings must distinguish reused connections and absent instrumentation from zero time.

## Progress
- [x] Inspected Usage/debug/Traffic models, storage, frontend drawers and Pingora 0.8 connection callbacks.
- [x] Add backward-readable timing/context fields and instrument resolution, TCP/TLS, headers/body/token milestones.
- [x] Build shared drawer, exact request correlation and meaningful frontend/regression coverage.
- [x] Run mandatory verification and actual local HTTP/TLS/stream/browser checks; rebuild demo.

## Compatibility boundary
Latest release tag is v0.1.31; the working branch begins at merged 0.1.32. Preserve all existing route responses and storage columns. Add defaulted fields to existing Traffic JSON and a defaulted internal traffic ID to usage diagnostics. Old records deserialize without fabricated timings. Metadata-only additions; no credentials or payload bodies exposed. User explicitly authorized both UI and instrumentation steps.

## Decision Log
- Use Pingora's socket-creation hook plus connection digest timestamps for TCP; TCP-to-TLS established timestamps for TLS. Do not count pool lookup as TCP or reuse a prior connection's timings.
- Resolve hostnames asynchronously under the configured timeout; literal IPs skip DNS. Preserve SNI/Host and certificate verification. Store per-attempt timing, bounded to the same diagnostic retention scale as the timeline.
- First body byte and first content token are separate. Observe bounded SSE metadata without holding up or modifying streamed chunks.
- Snapshot the debug bundle and usage metadata into each completed Traffic record so reused client request IDs cannot mix policy/cost details from different requests. Link new Usage rows via internal traffic ID; legacy or absent evidence is explicit.
- The real HTTPS fixture exposed that Pingora was built with no TLS backend. Enable its Rustls backend (consistent with the workspace's HTTP clients) with certificate verification; this corrects HTTPS transport and makes TLS timing measurable. Existing URLs and HTTP behavior are preserved. Connection/proxy errors must resolve Pingora's conditional retry decision before returning.
- Rustls pulls Pingora's required `rustls-pemfile` 2.2 wrapper. Added a narrowly scoped maintenance-advisory exception, following existing dependency policy, with a 2026-10-05 revisit date. [RUSTSEC-2025-0134](https://rustsec.org/advisories/RUSTSEC-2025-0134.html) identifies an unmaintained wrapper around `rustls-pki-types` and has no patched version. No vulnerability gate is broadly disabled.
- Preserve the earlier failure-button spacing fix. Use repository Karpathy, implementation-strategy, code-change-verification and pr-draft-summary skills.

## Surprises & Discoveries
Pingora currently resolves hostnames synchronously inside HttpPeer construction; connection digests expose TCP and TLS completion timestamps, but the socket hook is required to measure the TCP start accurately. Existing Usage timestamps are completion times. Existing debug bundles are indexed by client request ID, which can be reused; they are insufficient for exact correlation alone.

## Plan of Work
Extend core traffic models, populate fields in Pingora callbacks and terminal recording, and implement a reusable frontend renderer/mount for Traffic and Usage. Keep old health debug lookup usable through the same renderer. Add tests for legacy JSON, repeated IDs, reused/plain/TLS connections, partial SSE frames and missing measurements. Document timing definitions and retention limits.

## Validation and Acceptance
Frontend build/tests and complete mandatory verification script pass. Local proxy requests demonstrate fresh/reused HTTP, TLS and streaming measurements, pre-upstream failure, and matching Usage/Traffic investigation. Desktop and narrow-screen drawers remain usable with keyboard focus and sanitized data.

## Outcomes & Retrospective
Both implementation steps are complete. Usage and Traffic share a structured investigation renderer, exact internal-ID correlation, allowlisted snapshots, copy actions, responsive timing cards, and missing-data explanations. Added real DNS/TCP/TLS and response/content milestones without changing streamed chunks. Preserved the failure-button spacing fix.

The final verification script passed (including 331 nextest tests), as did the explicit PostgreSQL/Redis proxy integration, frontend build/tests, workspace build, and an additional scan of new source files. Real HTTPS tests cover certificate verification; a local end-to-end TLS fixture confirmed fresh/reused connections and body-versus-content timing. Fixed the exposed missing TLS backend, undecided retry panic, and integration startup race (the control API can bind before the proxy listener).

The local demo was rebuilt and restarted on ports 20381/20384/20385 with its existing realistic sample dataset and live generator. Evidence and timing limitations are recorded in `internal/test-reports/admin-ui-request-investigation.md`.

Subsequent operator testing added shared button spacing, a viewport-height sticky sidebar with bottom session controls, and explicit export-field labels and explanations. These frontend checks are recorded in `internal/test-reports/admin-ui-button-spacing.md`. The user requested a draft PR for the combined follow-up branch on 2026-09-05; prepare it against `main` without a release bump or merge.
