# WebSocket and verified identity diagnostics

Maintain this plan under PLANS.md. Work remains on PR #119's feature branch.

## Purpose

Admins can inspect live WebSocket duration, directional frame bytes, frame counts,
activity, timeout estimates and session expiry in Traffic, and retain terminal
measurements in Usage. Both investigation drawers show bounded verified Entra
claims and the endpoint policy applied at request time, without raw credentials.

## Progress

- [x] (2026-09-16) Inspect Traffic, Usage, WebSocket hooks and identity verification.
- [x] Add backward-readable diagnostic models and capture runtime metadata.
- [x] Add shared Traffic/Usage presentation with live countdowns and protocol labels.
- [x] Verify persistence, redaction, counters, failures, and multi-turn E2E diagnostics.
- [x] Run UI and mandatory checks; visually inspect and capture screenshots.
- [x] Update docs and draft PR #119; implementation pushed as 0ac9b3c.

## Compatibility Boundary

Latest release is v0.1.36. Add optional serde-default fields to RequestDiagnostics,
which already persists as JSON in usage_events and Traffic records. Older records
remain readable without a migration. No auth or socket wire behavior changes.

## Context and Plan of Work

Core traffic.rs defines metadata shared by live Traffic and terminal Usage.
Proxy accessa_runtime.rs verifies endpoint identity and admits sockets; Pingora
body hooks see frame bytes and accessa_transport.rs parses bounded frame headers.
UI investigation.ts is shared by Traffic and Usage. Add typed metadata there,
throttle live publications, and use the existing terminal persistence path.

## Decision Log

- 2026-09-16: Record an allowlist of verified claims only. Failed verification
  records required policy and failure code, without decoding untrusted claims.
- 2026-09-16: Capture byte counts at proxy frame hooks, not TLS/network packet
  totals; label estimates accordingly. Idle read/write limits and frame-triggered
  expiry checks do not promise a universal hard connection deadline.
- 2026-09-16: Keep claims bounded and mark truncation. Do not store JWTs, nonce,
  email, display name, prompts, frame payloads or close reason text.

## Surprises & Discoveries

Socket streams currently return before ordinary response body counters and Traffic
updates. Usage already stores diagnostic JSON, allowing additive persisted fields.

## Validation and Acceptance

Test old JSON reads, claim limits/redaction, frame counts across split frames,
upgraded and rejected handshakes, expiry/failure state, terminal persistence and
live updates in the existing real-gateway multi-turn Accessa E2E. UI tests cover
both drawers and countdown semantics. Run npm test, npm run build:admin-ui,
strict docs build and .codex/skills/code-change-verification/scripts/run.sh with
PostgreSQL/Redis. Inspect desktop/mobile UI and save screenshots.

## Outcomes & Retrospective

Runtime and both shared investigation views implemented. The extended mock-chain E2E checks live counters across five conversation turns plus heartbeats, terminal Usage persistence, failed identity redaction, and signed Apigee identity snapshots. Changed production Rust line coverage is 145/145 (100%) against bac55aa. Desktop Computer Use and 390px mobile screenshots are saved in internal/test-reports/accessa/. Live updates coalesce between polls in a bounded map; countdowns are explicitly estimates. No new migrations or token/payload capture.

The complete mandatory verification script passed (363 Nextest tests, zero skipped), including Cargo tests, formatting, Clippy, audit, deny, machete, Trivy, Gitleaks and Semgrep. Admin UI build/regression tests and strict docs build passed.

## Release and landing follow-up (2026-09-17)

User authorized the version bump, ready-for-review transition, monitoring and
merge when ready. Prepare 0.1.37 across workspace/lockfile, generated UI, deployment
examples, changelog and current documentation; retain historical release notes.
Rerun mandatory verification and the release build before pushing. GitHub requires
one approving review and code-owner review; do not bypass this protection.

- [x] Prepare 0.1.37 release metadata and operator documentation.
- [x] Validate 0.1.37: mandatory stack (363 Nextest tests), full workspace build, UI build/tests, release metadata and strict docs all passed.
- [ ] Push and mark PR #119 ready for review.
- [ ] Monitor checks/review and merge the verified head when eligible.

Decision: this is release metadata and documentation only; no additional runtime
contract or migration change. Final acceptance is a merged PR after successful
checks and required approval.
