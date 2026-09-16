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
- [x] Commit, push and open draft PR #119: https://github.com/sarattha/relayna-gateway/pull/119.

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

Implementation, multi-hop E2E and Computer Use save/readback are complete. Changed production Rust coverage is 611/615 executable lines (99.35%). The complete mandatory verification stack passed, including 357 Nextest tests with zero skips, Trivy, Gitleaks and Semgrep. UI tests, strict docs build and release metadata checks passed. Published as draft PR #119 on codex/accessa-channel-websockets.

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

## Follow-up: endpoint protocol labels

User requested visible HTTP / WS / HTTP + WS labels and clarification of registration.
Display saved transport support in Services, Routes and the service editor, with
an explicit WS label on the exact GET run path. Require an app binding, matching
prefix and GET before advertising WS; discovery and ordinary services are HTTP.
SSE remains HTTP. This changes presentation only; no API/schema/runtime changes.

- [x] Add and regression-test protocol labels; rebuild static assets. `npm test` and the Admin UI build pass.
- [x] Verify Services, Routes and the editor with Computer Use; capture `protocol-labels.png` and `protocol-settings.png`.
- [x] Mandatory verification stack passed in sequence (357 Nextest tests, zero skipped); publish this follow-up on draft PR #119.

## Follow-up: multi-turn chatbot E2E

- [x] Extend the existing real-gateway/mock-chain harness with five successful
  conversational turns on one BFF WebSocket, then replay all five after reconnect
  and execute a sixth contextual follow-up.
- [x] Assert ordered and correlated accepted/token/completed events, distinct
  admissions, exact agent history, and no agent execution for replay or denied
  turns. Agent memory is deliberately fixture-owned, not gateway-owned.
- [x] Mandatory verification passed in sequence: 359 Nextest tests, zero skipped,
  and all configured security checks. Publish this follow-up on draft PR #119.

Decision: this is test-only; no runtime, protocol, migration or compatibility
changes. Deterministic prompts model itinerary follow-ups without a live LLM.

## Follow-up: Routes alignment

The Routes configuration fields inherited bottom alignment while guidance text
varied in length. Keep Mode on its own row, top-align the three numeric fields,
and place Save below them. Give identity controls consistent spacing and prevent
badge wrapping. Use a compact single-field layout for service timeouts and a
readable minimum width inside horizontally scrollable tables on narrow screens.
This is presentation-only and preserves existing form handlers and API contracts.

- [x] Update source styles/markup, regenerate assets, and pass UI build/tests.
- [ ] Verify desktop/mobile with Computer Use and capture screenshots.

## Follow-up: opaque drawer action bars

- [x] Replace translucent shared action-bar backgrounds with the opaque surface.
- [x] Match sticky footer bounds to drawer padding at desktop and mobile sizes,
  covering the bottom and side gaps through which scrolled text was visible.
- [x] Build Admin UI, run npm tests, and rebuild the gateway successfully.
- [x] Inspect Services, Providers, Virtual keys, Projects and Workload identities
  using the in-app browser after Computer Use lost its Chrome target. Verify
  service footer at 390px and desktop; save screenshots. Inspect Routes desktop
  alignment and Monitor navigation as well. No configuration writes performed.

This is shared CSS only; no runtime/API changes. Short forms retain natural
button placement; long forms keep their actions pinned without transparency.
