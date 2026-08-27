# LiteLLM Rerank Route Support

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Relayna Gateway clients should be able to submit reranking requests to
LiteLLM through `POST /rerank`, `POST /v1/rerank`, or `POST /v2/rerank` while
retaining Gateway authentication, policy, rate-limit, budget, usage, and
operator route controls. Operators should see one canonical `/v1/rerank`
route setting in Admin UI and can enable, disable, configure runtime limits,
or select direct LiteLLM passthrough mode for all three aliases together.

## Progress

- [x] (2026-08-27 11:08Z) Read repository instructions, design manifesto,
  `PLANS.md`, `SKILLS.md`, and the required implementation, verification, PR,
  and Computer Use skills.
- [x] (2026-08-27 11:08Z) Confirmed clean `main`, fetched `origin`, verified
  latest release boundary `v0.1.29`, and created
  `feat/litellm-rerank-routes` from `origin/main`.
- [x] (2026-08-27 11:27Z) Added the core route identity and all three public
  POST aliases.
- [x] (2026-08-27 11:27Z) Added the PostgreSQL migration, policy parsing,
  Admin API, and Admin UI support.
- [x] (2026-08-27 11:30Z) Added focused unit, store, API, proxy, UI, and
  real-environment regression
  coverage.
- [x] (2026-08-27 11:29Z) Rebuilt Admin UI assets and passed its Node test
  suite.
- [x] (2026-08-27 15:54Z) Passed the mandatory Relayna Gateway verification
  stack from formatting through Semgrep.
- [x] (2026-08-27 11:40Z) Exercised the Admin UI in the real local environment
  through Computer Use at desktop and 390x844 responsive widths.
- [x] (2026-08-27 15:55Z) Reviewed the final diff, confirmed it is whitespace
  clean, and prepared the required PR draft handoff.

## Surprises & Discoveries

- Observation: LiteLLM exposes three rerank aliases that share one handler:
  `/rerank`, `/v1/rerank`, and `/v2/rerank`.
  Evidence: LiteLLM `litellm/proxy/rerank_endpoints/endpoints.py` route
  decorators.
- Observation: Relayna's generic `LiteLlmPassthrough` fallback can allow these
  paths, but deliberately bypasses normal Gateway governance.
  Evidence: `bypass_gateway_governance_for_passthrough` in
  `crates/gateway-proxy/src/pingora_plane.rs`.
- Observation: Docker BuildKit could not resolve the pinned Dockerfile frontend
  because the local Docker credential/network lookup timed out.
  Evidence: two `run.sh` attempts failed before build execution while resolving
  `docker/dockerfile:1.7`; the test itself was recovered with the already-cached
  LiteLLM, PostgreSQL, Redis, and Node images plus the freshly built local
  gateway binary.
- Observation: The disposable PostgreSQL volume retained an older bootstrap
  operator token even though the environment specified the expected test token.
  Evidence: the first real UI login returned `invalid_operator_token`; disabling
  only the stale active token and restarting the isolated gateway caused the
  configured test token to bootstrap and the UI login to succeed.
- Observation: The user's existing cargo-audit checkout contained unrelated
  untracked advisory files, including a duplicate `RUSTSEC-2026-0244`.
  Evidence: the first audit attempt stopped on the duplicate ID; a pristine
  temporary clone at the same upstream commit contained only the canonical
  `gettext-rs` advisory and allowed the unchanged verification sequence to
  pass. The dirty user cache was not modified.

## Decision Log

- Decision: Represent all three public aliases as one
  `Route::LiteLlmRerank` identity with canonical string `/v1/rerank`.
  Rationale: One policy and operator setting should govern the semantic rerank
  operation, while the proxy already records the actual request endpoint
  separately and preserves the incoming upstream URI.
  Date/Author: 2026-08-27 / Codex.
- Decision: Implement rerank as a canonical governed LiteLLM route rather than
  extending the generic wildcard allowlist.
  Rationale: Canonical routing retains Relayna policy, rate-limit, budget,
  guardrail, credential translation, and usage behavior.
  Date/Author: 2026-08-27 / Codex.
- Decision: Use a forward PostgreSQL migration and preserve every existing
  route row, ID, mode, and public path from `v0.1.29`.
  Rationale: The released route-settings schema is durable external state;
  rerank support is additive and the user explicitly authorized exceeding the
  production freeze.
  Date/Author: 2026-08-27 / Codex.

## Outcomes & Retrospective

Implemented one governed `Route::LiteLlmRerank` identity with canonical policy
path `/v1/rerank`. `POST /rerank`, `POST /v1/rerank`, and `POST /v2/rerank`
all resolve to that identity, keep the incoming path when forwarded, and reject
unsupported methods. A forward SQLx migration expands the released route
constraints and seeds the enabled operator row. Policy parsing, simulation,
runtime settings, Admin APIs, Admin UI route controls, usage filters, docs, and
the real-environment harness now understand rerank.

Focused Rust tests passed for routing, route settings, proxy governance,
database-backed Admin behavior, and the full gateway process. Admin UI assets
were regenerated and the Node UI suite passed. The cached-image real
environment ran the freshly built gateway against PostgreSQL, Redis, real
LiteLLM, and a mock provider: every alias returned HTTP 200 in managed mode,
the original alias reached the provider, and `/v2/rerank` also returned HTTP
200 in direct LiteLLM passthrough mode with a LiteLLM bearer credential.

Computer Use verified login plus the Monitor Overview/Usage, Discover Routes,
and Govern Keys views. The Routes view displayed one enabled `rerank` row at
`/v1/rerank` with mode and runtime-limit controls on desktop and at a 390x844
responsive viewport. The isolated services, test credential state, and volumes
were removed after verification.

The mandatory verifier passed formatting, Clippy, all workspace tests and doc
tests, cargo-audit, cargo-deny, cargo-machete, 314 Nextest tests, Trivy,
Gitleaks, and Semgrep. `cargo build --workspace --all-features` also passed.
There are no known implementation gaps.

## Context and Orientation

`crates/gateway-core/src/routing.rs` resolves public request paths into a
framework-independent `Route` and LiteLLM backend selection.
`crates/gateway-core/src/route_settings.rs` maps canonical route identities to
operator-facing IDs and runtime configuration. A Relayna virtual key is the
only client credential used by governed requests; the proxy replaces it with
the internally selected LiteLLM credential.

`crates/gateway-proxy/src/pingora_plane.rs` is the streaming request plane. It
applies route enablement, policy, rate limits, budget checks, usage recording,
and upstream credential translation before forwarding the original path and
query to LiteLLM.

`crates/gateway-store/src/postgres.rs` parses persisted key policies and reads
route settings. PostgreSQL migrations under
`crates/gateway-store/migrations/` constrain and seed canonical route rows.
`crates/gateway-api/src/app.rs` exposes operator APIs and contains in-memory
test support. Admin UI source is under `crates/gateway-api/admin-ui/`; generated
checked-in assets are under `crates/gateway-api/src/static/admin-ui/`.

The canonical policy route will be `/v1/rerank`. Calling `/rerank` or
`/v2/rerank` resolves to the same policy identity, but usage endpoint fields
and the LiteLLM upstream request preserve the actual alias.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.29`. Existing public routes,
route IDs, policy values, route modes, PostgreSQL rows, and API response shapes
remain unchanged. Rerank support is additive. The released
`openai_route_settings` constraints require a forward migration that adds the
`rerank` ID and `/v1/rerank` canonical row without mutating prior migrations.

## Plan of Work

Add `Route::LiteLlmRerank` and resolve POST requests for `/rerank`,
`/v1/rerank`, and `/v2/rerank` to it. Reject other methods and use
`/v1/rerank` as the canonical route string.

Extend route settings with the `rerank` ID and include it in existing canonical
LiteLLM route lookups. Add a new SQLx migration that expands released table
constraints and seeds an enabled row. Teach stored policy parsing, policy
simulation, and API test stores to understand `/v1/rerank`.

Update Admin UI route defaults and tests so rerank appears in route controls
and usage filters, then regenerate checked-in assets from TypeScript source.

Add unit tests for alias resolution and method rejection, store/API tests for
the new route row and controls, proxy tests proving governed routing, and the
real LiteLLM harness coverage proving every alias reaches the upstream unchanged.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo fmt --all --check
    cargo test -p gateway-core routing
    cargo test -p gateway-api openai_route
    cargo test -p gateway-proxy rerank
    npm run build:admin-ui
    npm test
    bash internal/test-reports/litellm-real-passthrough/run.sh
    bash .codex/skills/code-change-verification/scripts/run.sh

Use the local Admin UI URL produced by the real-environment harness and inspect
the Routes and Usage views with Computer Use at desktop and mobile widths.

## Validation and Acceptance

Success requires all of the following:

- POST requests to `/rerank`, `/v1/rerank`, and `/v2/rerank` resolve to
  `Route::LiteLlmRerank`, select LiteLLM, and preserve the incoming upstream
  path.
- Unsupported methods on each alias return the stable unsupported-route error.
- The canonical `/v1/rerank` policy value governs every alias; keys without it
  are denied.
- The Admin API and Admin UI expose one `rerank` route setting with existing
  enablement, direct-passthrough, timeout, and body-limit controls.
- Relayna client credentials never reach LiteLLM; the configured internal
  credential is injected.
- Usage uses canonical route `/v1/rerank` and actual endpoint paths remain
  queryable.
- Admin UI source and generated assets agree, UI tests pass, and Computer Use
  confirms the route is visible and usable without layout regressions.
- The mandatory Rust formatting, Clippy, and workspace test sequence passes.

## Idempotence and Recovery

The migration is applied once by SQLx and uses an idempotent seed insert. Local
tests and Admin UI builds are safe to rerun. If the real-environment harness is
interrupted, rerun its documented cleanup or run script; do not delete user
data or reset the working tree. Failed checks must be fixed and the complete
required verification sequence rerun.

## Artifacts and Notes

Real LiteLLM results are expected under
`internal/test-reports/litellm-real-passthrough/`. UI evidence will be described
in Outcomes & Retrospective without storing credentials or sensitive screen
content.

## Interfaces and Dependencies

The new core interface is `Route::LiteLlmRerank`, canonical route string
`/v1/rerank`, and route setting ID `rerank`. It uses the existing
`managed_by_gateway` and `direct_litellm_passthrough` modes. No new environment
variables, credentials, provider formats, response shapes, or dependencies are
introduced.
