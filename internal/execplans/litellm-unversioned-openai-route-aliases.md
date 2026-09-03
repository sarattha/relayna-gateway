# LiteLLM Unversioned OpenAI Route Aliases

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Relayna Gateway clients should be able to send `POST /chat/completions` and
`POST /responses` through the same governed LiteLLM routes as
`POST /v1/chat/completions` and `POST /v1/responses`. Operators should continue
to manage one canonical route setting for each operation, including its
`direct_litellm_passthrough` mode, while the proxy preserves the client-requested
path when forwarding to LiteLLM.

## Progress

- [x] (2026-09-03 12:42Z) Read the repository instructions, design manifesto,
  `PLANS.md`, and the required implementation, verification, and PR-summary
  skills.
- [x] (2026-09-03 12:42Z) Confirmed the latest release boundary is `v0.1.30`,
  inspected routing and passthrough behavior, and identified an unrelated
  pre-existing `Cargo.lock` modification to preserve.
- [x] (2026-09-03 12:45Z) Added the two aliases to canonical core route
  resolution while retaining canonical `/v1/...` identities.
- [x] (2026-09-03 12:47Z) Added focused route and proxy-process regression
  coverage; core tests passed and the process test compiled but skipped its
  service-backed exercise because `DATABASE_URL` is unavailable.
- [x] (2026-09-03 12:45Z) Updated user-facing LiteLLM passthrough and current
  feature documentation.
- [x] (2026-09-03 12:59Z) Passed focused core and proxy tests, the complete
  mandatory Relayna Gateway verifier, and the additional all-feature workspace
  build against the clean released lockfile.
- [x] (2026-09-03 12:59Z) Reviewed the final diff, confirmed it is whitespace
  clean, preserved unrelated user work, and prepared the required PR draft
  handoff.
- [x] (2026-09-03 13:24Z) Built fresh local Gateway, development OIDC, and
  mock-upstream images and started the isolated Compose stack healthy.
- [x] (2026-09-03 13:30Z) Used Computer Use to authenticate as the development
  Gateway Administrator, configured both canonical routes for direct LiteLLM
  passthrough, and proved both aliases and canonical paths reach the local
  upstream without path rewriting.
- [x] (2026-09-03 13:41Z) Ran all 316 tests under LLVM coverage with PostgreSQL
  and Redis enabled; changed runtime executable line coverage is 100% (43/43).
- [x] (2026-09-03 13:45Z) Passed the complete mandatory Relayna Gateway
  verification stack against the final patch and released lockfile.
- [x] (2026-09-03 13:48Z) Committed and pushed the feature without the
  unrelated `Cargo.lock` edit and opened pull request #109.
- [ ] Monitor pull request #109 checks and its first Codex review.
- [ ] Address all actionable Codex review comments, reply inline, resolve the
  review threads, and rerun verification.

## Surprises & Discoveries

- Observation: Canonical route mode, policy, runtime limits, and usage identity
  are selected from the resolved `Route` enum rather than the literal request
  path.
  Evidence: `Route::resolve_match` in `crates/gateway-core/src/routing.rs` and
  canonical route handling in `crates/gateway-proxy/src/pingora_plane.rs`.
- Observation: The proxy preserves canonical LiteLLM request paths and rewrites
  only `/providers/openai/*` direct-provider paths.
  Evidence: `direct_openai_path_and_query` is applied only to
  `Route::DirectOpenAi` in `crates/gateway-proxy/src/pingora_plane.rs`.
- Observation: The process-level proxy test is environment-gated and skipped
  its runtime assertions because `DATABASE_URL` is not set in this workspace.
  Evidence: `cargo test -p gateway-api --test proxy_process_integration --
  --nocapture` compiled and passed with the documented skip message.
- Observation: The user's pre-existing `Cargo.lock` edit downgrades `chacha20`
  from `0.10.2` to yanked `0.10.0`, so the repository's `cargo deny` step fails
  in the primary checkout independently of this feature.
  Evidence: the initial `Cargo.lock` diff predates this work and `cargo deny`
  reported the yanked crate; the unchanged release lock passed in the isolated
  verification worktree.
- Observation: The machine's existing RustSec checkout contains duplicate
  advisory ID `RUSTSEC-2026-0244` from unrelated untracked files.
  Evidence: the initial audit failed while a pristine temporary checkout loaded
  successfully and completed the required audit.
- Observation: The local seed used obsolete pricing-rule JSON fields, causing
  the Admin UI Routes view to fail while decoding registered services.
  Evidence: the route tables loaded after updating the fixture to the current
  `ServicePricingRule` and `ServiceEndpointPricingRule` wire shapes and
  reseeding the local database.
- Observation: The process integration test inherited operator-selected route
  modes and relied on an ambient global policy row.
  Evidence: the live coverage run initially exercised direct bearer semantics;
  after the test established managed mode and a neutral project-scoped layer,
  its complete proxy workflow passed against live PostgreSQL and Redis.
- Observation: Parallel Redis integration tests can race while globally
  rehydrating all budgeted keys.
  Evidence: one rehydration assertion failed in the parallel coverage run,
  passed alone, and the complete suite passed with `--test-threads=1`.

## Decision Log

- Decision: Resolve `/chat/completions` and `/v1/chat/completions` to
  `Route::ChatCompletions`, and `/responses` and `/v1/responses` to
  `Route::Responses`, retaining the `/v1/...` value from `Route::as_str()`.
  Rationale: This matches the existing rerank alias design, preserves one
  policy and operator setting per semantic operation, and makes both managed
  and direct LiteLLM modes behave consistently.
  Date/Author: 2026-09-03 / Codex.
- Decision: Make the change additive without database or configuration changes.
  Rationale: The released canonical route IDs and stored paths remain unchanged;
  only new public request aliases are accepted.
  Date/Author: 2026-09-03 / Codex.

## Outcomes & Retrospective

Implemented `POST /chat/completions` and `POST /responses` as additive aliases
for the existing canonical OpenAI-compatible routes. Both aliases resolve to
the same route enum, operator setting, policy path, runtime limits, usage label,
credential handling, and managed/direct mode as their `/v1/...` counterparts.
The proxy continues to preserve the literal client path upstream.

Core tests cover both aliases and unsupported methods. An unconditional proxy
test proves that each alias selects `direct_litellm_passthrough` through its
canonical route setting. The process-level proxy coverage includes both aliases
and asserts upstream path preservation against live PostgreSQL and Redis.

Fresh local images were built and started. Computer Use authenticated through
the development OIDC flow and saved direct passthrough mode for both canonical
route settings. Requests to `/chat/completions`, `/v1/chat/completions`,
`/responses`, and `/v1/responses` all returned 200, with the mock upstream
reporting the exact corresponding request path. The local seed fixture was
updated to current pricing-rule shapes so this Admin UI workflow is repeatable.

The instrumented workspace suite passed all 316 tests serially. LLVM reports
100% changed runtime executable line coverage (43/43), exceeding the 95% gate;
the broader pre-existing workspace baseline is 92.11% line coverage.

The mandatory verifier passed formatting, Clippy, workspace tests and doc tests,
cargo-audit, cargo-deny, cargo-machete, 316 Nextest tests, Trivy, Gitleaks, and
Semgrep in a disposable worktree containing only this patch and the released
`Cargo.lock`. `cargo build --workspace --all-features` also passed. The clean
worktree was necessary to preserve the user's unrelated lockfile edit, which
currently causes `cargo deny` to reject yanked `chacha20 0.10.0` in the primary
checkout. No implementation gaps remain.

## Context and Orientation

`crates/gateway-core/src/routing.rs` converts an HTTP method and request path
into a framework-independent route identity. `crates/gateway-proxy/src/pingora_plane.rs`
uses that identity to load the route's operator mode, limits, policy, upstream,
and usage label. In direct LiteLLM passthrough mode, a non-Relayna bearer
credential is delegated to LiteLLM after credential translation; Relayna keys
retain Gateway governance.

`crates/gateway-api/tests/proxy_process_integration.rs` starts a real Pingora
proxy against a mock upstream when PostgreSQL and Redis test URLs are available.
It can prove that an alias passes governance and reaches the upstream with its
original path intact. `docs/litellm-passthrough.md` is the operator-facing
description of canonical route precedence and direct mode.

`deploy/local/docker-compose.yml` builds the Gateway, development OIDC issuer,
and mock LiteLLM-compatible upstream into an isolated local stack. The mock
upstream returns the literal request path so local direct-passthrough tests can
prove aliases are not rewritten.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.30`. The change adds public
aliases but preserves the released `/v1/chat/completions` and `/v1/responses`
paths, route IDs, persisted route settings, policy strings, response shapes,
credentials, and streaming behavior. Both aliases intentionally inherit the
canonical route's enablement, mode, limits, policy, and usage identity.

## Plan of Work

Extend the two existing route match arms in
`crates/gateway-core/src/routing.rs` with unversioned aliases, following the
same alternative-pattern form used by rerank. Expand core tests to prove both
paths resolve to their canonical `Route`, expose the existing canonical
`Route::as_str()` value, and reject unsupported methods.

Add the aliases to the full proxy-process route table in
`crates/gateway-api/tests/proxy_process_integration.rs` and assert that the mock
LiteLLM upstream receives the original path. Update
`docs/litellm-passthrough.md` and the current-feature route list to document the
aliases and their shared operator controls.

Extend `deploy/local/mock-upstream.mjs` with compatible chat-completions and
responses handlers for both versioned and unversioned paths. Build and start the
local Compose images, use Computer Use to inspect and configure canonical route
mode, and assert both unversioned paths return their original upstream path.

Run LLVM source coverage for the final Rust regression suite and calculate
coverage over executable lines introduced or modified by this branch relative
to `origin/main`; the result must exceed 95%. Open the PR only after local image,
runtime, coverage, and mandatory verification checks pass. Monitor GitHub until
the first Codex review arrives, address every actionable thread, reply with the
fix and verification evidence, and resolve the thread.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-core routing
    cargo test -p gateway-api --test proxy_process_integration
    ./deploy/local/run.sh --rebuild
    DATABASE_URL=postgres://relayna_gateway:relayna_gateway_local@127.0.0.1:19432/relayna_gateway REDIS_URL=redis://127.0.0.1:19379 cargo llvm-cov --workspace --all-features --lcov --output-path target/coverage.lcov -- --test-threads=1
    bash .codex/skills/code-change-verification/scripts/run.sh

The local Compose service URLs make the process and store integration tests run
instead of taking their documented environment-gated skip path.

## Validation and Acceptance

Success requires `POST /chat/completions` and `POST /responses` to resolve to
the same route identities as their `/v1/...` counterparts. Unsupported methods
must keep returning the stable unsupported-route error. Existing canonical
policy strings and operator route rows must govern both aliases, and the proxy
must preserve the incoming alias path and query when forwarding to LiteLLM.
Formatting, Clippy, and all workspace tests must pass in the mandatory verifier.

## Idempotence and Recovery

All edits and tests are safe to repeat. No migration, persisted state rewrite,
or external service mutation is required. If verification is interrupted, rerun
the focused command or full verifier. Preserve the user's existing
`Cargo.lock` modification and do not reset unrelated working-tree changes.

## Artifacts and Notes

The final diff is limited to core routing, route-focused tests, local inspection
fixtures, operator-facing documentation, and this ExecPlan. No generated assets
or credentials are involved.

## Interfaces and Dependencies

No new Rust type, route setting ID, environment variable, schema, dependency,
or response format is introduced. The public additions are
`POST /chat/completions` and `POST /responses`; their canonical internal route
strings remain `/v1/chat/completions` and `/v1/responses`.
