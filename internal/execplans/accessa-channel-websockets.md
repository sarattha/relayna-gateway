# Accessa channel WebSockets and endpoint identity policy

This living ExecPlan follows /Users/jobz/Works/relayna-gateway/PLANS.md.

## Purpose / Big Picture

A channel BFF opens a governed WebSocket through Relayna to its Accessa adapter.
The adapter connects to Router; Router owns commands, ownership, deduplication,
replay and agent dispatch. Each new turn requests gateway admission; model and
service calls retain actual charging. Operators configure endpoint-specific
Entra audience, scope, role and group requirements instead of a global union of
audiences. Browser clients never receive channel keys.

## Progress

- [x] (2026-09-15) Reviewed Accessa HTML, Tara masterplan and existing proxy.
- [x] (2026-09-16) Created codex/accessa-channel-websockets from clean main.
- [x] User approved BFF transport, existing Entra header arrangement, per-turn
  admission and breaking the production freeze for this work.
- [x] Implement persisted endpoint access policy and operator forms.
- [x] Implement governed WebSocket transport and per-turn admission.
- [x] Add regression tests and real mock BFF/adapter/Router/agent E2E.
- [x] Measure >98% changed executable Rust line coverage; report exact scope.
- [x] Verify operator UI with Computer Use and save evidence.
- [x] Run mandatory verification and review the diff.
- [ ] Commit, push and open draft PR.

## Surprises & Discoveries

- Pingora 0.8 already implements WebSocket upgrades. Gateway hooks currently
  assume HTTP bodies and require explicit separation from upgraded traffic.
- Persisted service routes already match concrete app/channel prefixes, but
  wildcard forwarding removes their prefix.
- Existing Apigee signed identity lacks audience/expiry fields. New endpoint
  policies must fail closed when required claims are absent, preserving legacy
  behavior only for endpoints without the new policy.

## Decision Log

- Compatibility boundary: v0.1.36. Add JSON endpoint access configuration via a
  forward SQL migration; preserve existing routes and timeout semantics by
  default. New Accessa protocol is unreleased and needs no compatibility shim.
- Reuse service registration for each concrete app/channel binding and the
  channel-only identity endpoint. Require explicit key service allowlisting for
  Accessa bindings. Preserve path; do not rewrite existing service paths.
- Keep Router semantics out of the proxy. Turn admission checks current key,
  policy, request rate and budget eligibility without reserving or billing work
  twice. Transport and paid service usage remain distinct.
- User explicitly authorizes implementing through the production freeze and
  opening a draft PR; no production deployment is part of this task.

## Outcomes & Retrospective

Implementation, multi-hop E2E and Computer Use save/readback are complete. Changed production Rust coverage is 611/615 executable lines (99.35%). The complete mandatory verification stack passed, including 357 Nextest tests with zero skips, Trivy, Gitleaks and Semgrep. UI tests, strict docs build and release metadata checks passed. Publication is pending.

## Context and Orientation

Root: /Users/jobz/Works/relayna-gateway. gateway-core owns plain policy types and
Entra verification; gateway-store owns PostgreSQL registration and Redis
admission state; gateway-proxy/src/pingora_plane.rs owns public traffic. Admin
UI source is crates/gateway-api/admin-ui/src/main.ts, built with npm run
build:admin-ui. Accessa spec is /Users/jobz/Works/accessa-api 2.html. Tara plan is
/Users/jobz/Works/tara-masterplan/tara_multi_channel_architecture_masterplan.html.

## Plan of Work

Add endpoint identity/transport configuration and validate it before writes.
Integrate route-selected identity checks into Pingora authentication, including
Apigee signed claims. Add socket admission, bounded bidirectional relay, timeout
and lifecycle behavior. Implement trusted per-turn admission and wire the mock
Router to it. Add operator fields and docs. Test invalid identity, key binding,
limits, revocation, resume and multiple channels with mock downstream services.

## Validation and Acceptance

Run cargo fmt --all --check, cargo clippy --workspace --all-targets --all-features
-- -D warnings, cargo test --workspace --all-features via the verification skill.
Run npm run build:admin-ui and npm test. Use cargo llvm-cov LCOV with a changed
executable-line gate strictly greater than 98%; preserve denominator and uncovered
line report. E2E must exercise real gateway and separate mock hop servers with
HTTP and WebSocket clients. Computer Use must inspect and operate the UI.

## Idempotence and Recovery

Use isolated local test services, ports and data. Do not alter production.
Migration adds default-empty configuration; rerunning migrations is safe.
Retain existing service behavior when configuration is empty. Connection/session
state must expire or release after disconnect and failure. Rerun the full
verification script after fixes. Commit only task changes.

## Interfaces and Dependencies

Reuse existing Entra verifier, service registry, virtual key lookup, policy,
rate-limit and budget interfaces. No custom agent scheduler or conversation DB
belongs in gateway. Public run socket uses GET .../v1/run; HTTP commands remain
POST .../v1/runs. Per-turn admission is an internal HTTP contract documented with
the implementation. Opaque session credentials must never be logged or sent to
the BFF.

## Artifacts and Notes

Evidence and final coverage commands will be recorded as implementation proceeds.

- Local macOS Pingora tests need `SSL_CERT_FILE=/etc/ssl/cert.pem` to load the system certificate bundle.
- Connection caps apply per instance, including a hard total cap of 4,096. Router owns durable replay and deduplication across reconnects.
- WebSockets skip HTTP body capture and metered-stream counters; actual downstream calls retain billing.

- Mandatory audit found RUSTSEC-2026-0285 in the pre-existing Rustls dependency. Updated Rustls to 0.23.45 (and webpki 0.103.15), plus event-listener 5.4.2 for its reported thread-safety issue. No new audit exemptions.

- Final LLVM coverage run passed all workspace tests with live disposable services: 611/615 changed production executable lines (99.3496%), excluding tests. The four uncovered mappings remain listed in `internal/test-reports/accessa/coverage.json`; no production failure paths were excluded.
