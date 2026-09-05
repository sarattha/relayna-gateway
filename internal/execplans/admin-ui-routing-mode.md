# Request routing mode and passthrough metering labels

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Operators can distinguish LiteLLM passthrough from gateway-managed requests even
when both use the same canonical endpoint and provider. Passthrough token/cost
cells say “Not metered by gateway”; absent historical evidence says “Not recorded”.

## Progress

- [x] Inspect request routing, terminal usage, diagnostics persistence and UI rendering.
- [x] Choose additive diagnostics metadata with backward reads.
- [x] (2026-09-05) Implement mode recording, structured terminal logs and shared UI labels.
- [x] (2026-09-05) Test managed, wildcard/canonical passthrough, missing identity and old records.
- [x] (2026-09-05) Rebuild local demo, inspect desktop/mobile UI and verify the workspace.
- [x] (2026-09-05) Update documentation and prepare verified draft PR #112 follow-up.

## Context and Orientation

Worktree `/Users/jobz/.codex/worktrees/9b27/relayna-gateway`, branch
`codex/admin-ui-3-followup-fixes`. `gateway-core/src/traffic.rs` defines diagnostics;
`gateway-proxy/src/pingora_plane.rs` classifies requests and records Usage/Traffic.
Diagnostics already persist as JSON, so no SQL column is required. Frontend source
is `crates/gateway-api/admin-ui/src/` (main, traffic, investigation, design system).
The native local demo uses control port 20384, proxy 20385 and UI proxy 20381.

## Compatibility Boundary and Decisions

Latest release tag: v0.1.31. This user-authorized addition preserves route values,
provider names, credentials, forwarding, billing and usage-identity requirements.
Add optional `diagnostics.routing_mode` with `managed_by_gateway` or
`litellm_passthrough`. Old JSON remains readable and missing/unrecognized modes
remain unknown. Do not infer mode from provider, route, cost source, or current
configuration: these do not prove the mode of a historical request.

Only classify gateway-managed requests once a route match has been selected;
earlier failures may remain unknown. The existing passthrough flag is authoritative
for wildcard, canonical direct and trusted-ingress requests. Record mode before
terminal Usage snapshots as well as Traffic updates. Add a bounded routing mode
field to terminal structured logs, with no high-cardinality metric labels.

## Plan of Work

Add diagnostic metadata and serialization/backward-read coverage. Populate it in
normal Traffic updates and early error snapshots. Share frontend mode and usage
formatters across Usage, Traffic and request investigation. Keep numeric values
in APIs/exports unchanged; labels are presentation only. Update tooltip definitions
and operator documentation. Run focused tests, then the complete verification
script and frontend build/tests. Restart only the local native demo to load the
new runtime, preserving data. Exercise representative managed/passthrough requests
and inspect saved/live UI at desktop and 390 px.

## Acceptance

Canonical and wildcard passthrough have the same explicit badge. Their token/cost
cells say “Not metered by gateway” even without a Usage row. Managed numeric zero
remains zero; missing managed/old records remain “Not recorded”. An ordinary
service using passthrough cost pricing is not mislabeled as LiteLLM passthrough.
The full Rust verification stack, frontend checks and browser checks pass.

## Surprises & Discoveries

Usage requires a Relayna key; direct LiteLLM credentials can produce Traffic
without Usage. Routing mode therefore belongs in shared request diagnostics,
not only in Usage metadata. Service passthrough pricing is a different feature.

## Outcomes & Retrospective

Implemented and verified the additive mode field in diagnostics, terminal logs,
Usage, Traffic and shared investigation. Managed, canonical passthrough, wildcard
upstream failure and keyless passthrough were exercised against the local mock;
saved history retained their modes after original routing settings were restored.
Legacy rows keep unknown mode and numeric zero is preserved. Frontend, full
runtime build and complete verification stack passed (332 nextest tests). Chrome
desktop/mobile checks passed; Computer Use was blocked by the locked Mac. Evidence:
`internal/test-reports/admin-ui-routing-mode.md`.

## Idempotence and Recovery

No migration or release bump. Regenerate static assets from Vite source. Restore
any temporarily changed local demo routing settings after emitting test requests.
Keep the existing draft PR and preserve its prior changes.
