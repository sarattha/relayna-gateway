# LiteLLM Passthrough Configuration

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators need to configure direct LiteLLM passthrough behavior for all
supported OpenAI and Anthropic endpoints without changing code. The gateway
should let them set request payload size, response payload size, route timeout,
and virtual-key policy limits that protect long-context Codex and other model
traffic. The Admin UI should expose these controls clearly, and tests should
prove that configured limits are enforced for OpenAI and Anthropic passthrough
routes.

## Progress

- [x] (2026-07-04) Created implementation branch
  `codex/litellm-passthrough-config`.
- [x] (2026-07-04) Loaded required `$karpathy`,
  `$implementation-strategy`, design manifesto, PLANS.md, and Admin UI
  guidance.
- [x] (2026-07-04) Established compatibility boundary at latest release tag
  `v0.1.15`.
- [x] (2026-07-04) Inspect current route settings, LiteLLM passthrough handling, policy
  limits, Admin UI provider forms, migrations, and tests.
- [x] (2026-07-04) Implement persisted configuration for direct LiteLLM passthrough across
  OpenAI and Anthropic endpoints.
- [x] (2026-07-04) Add Admin UI controls and regenerate checked-in static assets.
- [x] (2026-07-04) Add focused Rust and Admin UI tests.
- [x] (2026-07-04) Run required verification, including `$code-change-verification`.
- [x] (2026-07-04) Build a Docker image, test in a real local environment, and capture
  screenshots for inspection.
- [x] (2026-07-04) Update version, CHANGELOG, and documentation.
- [x] (2026-07-04) Create a review-ready PR and start a 5-minute review-comment monitor.

## Surprises & Discoveries

- Observation: The current `/v1/responses` LiteLLM route defaults to a 1 MiB
  route body cap via `RouteMatch::litellm`.
  Evidence: `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/routing.rs`.
- Observation: Direct LiteLLM passthrough for canonical OpenAI/Anthropic routes
  uses the route settings tables, while wildcard passthrough uses the singleton
  `litellm_passthrough_settings` row.
  Evidence: `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`.
- Observation: The Docker fixture passes with `/v1/responses` long-context
  payloads after raising the route request cap to 8 MiB, and the Admin UI shows
  the persisted wildcard/OpenAI/Anthropic timeout and body limits.
  Evidence:
  `/Users/jobz/Works/relayna-gateway/internal/test-reports/litellm-real-passthrough/report.md`
  and screenshots `69`, `70`, and `71`.

## Decision Log

- Decision: Treat this as compatibility-sensitive because it changes proxy
  limits, admin API payloads, persisted settings, and operator-facing UI.
  Rationale: These surfaces are explicitly listed as compatibility-sensitive in
  `AGENTS.md` and `$implementation-strategy`.
  Date/Author: 2026-07-04 / Codex.

- Decision: Use additive configuration fields and migrations rather than
  replacing existing route settings in place.
  Rationale: The latest release tag is `v0.1.15`; direct passthrough and route
  settings may already be persisted by deployed operators. Additive fields
  preserve existing behavior when unset while allowing larger payloads where
  needed.
  Date/Author: 2026-07-04 / Codex.

## Outcomes & Retrospective

Work is in progress. Implementation, local Docker validation, screenshots,
release metadata validation, Admin UI tests, the full repository verification
stack, PR creation, and review monitoring are complete. The PR remains open for
review and merge.

## Context and Orientation

Relayna Gateway accepts external requests authenticated by Relayna virtual keys
and forwards OpenAI-compatible and Anthropic-compatible traffic to LiteLLM.
Direct LiteLLM passthrough mode bypasses gateway request rewriting but still
authenticates virtual keys, applies route/key policy, counts body bytes, and
records usage. A virtual key is the public Relayna credential presented by a
client; a key policy is the per-key or route policy that restricts routes,
models, providers, streaming, tools, token counts, payload sizes, budgets, and
rates.

Relevant files:

- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/routing.rs`
  resolves supported OpenAI and Anthropic routes into `RouteMatch` values with
  timeout and max body defaults.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/route_settings.rs`
  defines route mode and LiteLLM passthrough settings.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/policies.rs`
  evaluates key and route policy limits such as max request and response bytes.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-proxy/src/pingora_plane.rs`
  enforces route limits and LiteLLM passthrough behavior in Pingora callbacks.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-store/` owns PostgreSQL
  migrations and persistence for route settings and passthrough settings.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs` exposes
  Admin UI JSON APIs.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/` is the Vite
  Admin UI source of truth.
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/`
  contains generated checked-in UI assets.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.15`. This change affects
public proxy behavior, admin API payloads, and PostgreSQL-persisted settings.
The implementation should be additive and default-preserving: existing route
and passthrough records keep their current behavior unless operators set new
limits. Migrations should be idempotent.

## Plan of Work

First inspect the existing route and passthrough model to identify the smallest
place to add route-level request and response payload settings for all direct
LiteLLM passthrough endpoints. Then add persistence and defaulting so OpenAI
and Anthropic routes can use configured request payload size, response payload
size, and timeout without losing existing key policy enforcement.

Next extend Admin UI APIs and the Admin UI provider/settings view so operators
can edit the new configuration. Keep secrets write-only, use existing compact
panel/form/table classes, and rebuild static assets with the documented
commands.

Finally add tests at the core/store/API/proxy/UI layers as needed, run focused
commands while iterating, run the required verification stack, exercise a Docker
environment, capture screenshots, update version/docs/changelog, open a PR,
and start the review monitor.

## Concrete Steps

Work from repository root:

    cd /Users/jobz/Works/relayna-gateway
    git switch -c codex/litellm-passthrough-config
    cargo test -p gateway-core
    cargo test -p gateway-proxy
    npm run build:admin-ui
    npm test
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

Docker and PR commands will be filled in after inspecting the repository's
existing Docker and release workflow.

## Validation and Acceptance

Acceptance requires evidence that:

- Operators can configure direct LiteLLM passthrough limits for all supported
  OpenAI and Anthropic endpoints.
- Configured request payload size changes the gateway's 413 threshold for long
  Codex `/v1/responses` payloads.
- Configured response payload size is enforced without fully buffering
  streaming responses.
- Virtual key policy settings can still impose stricter request/response/token
  limits than route defaults.
- Admin UI displays and saves the settings with no secret leakage.
- Tests cover OpenAI and Anthropic direct LiteLLM passthrough routes.
- A local Docker image can run the changed gateway and expose the Admin UI
  settings.
- Screenshots capture the Admin UI settings for review.
- PR is open, review-ready, and monitored for Codex review comments.

## Idempotence and Recovery

Migrations must be safe to rerun through existing migration tooling. Failed
tests can be rerun after fixing the failing area. Admin UI generated assets
should always be regenerated from `crates/gateway-api/admin-ui/`; do not hand
edit generated assets. Docker containers should be stopped and recreated if
environment variables or migrations change.

## Artifacts and Notes

Artifacts to attach or reference before completion:

- Admin UI screenshots.
- Docker image tag or build output.
- PR URL: https://github.com/sarattha/relayna-gateway/pull/79.
- Review monitor automation: `monitor-pr-79-codex-review-comments`.
- Review monitor sub-agent: `019f2c37-d593-7362-b8eb-8325717d31a7`
  (`Leibniz`).

## Interfaces and Dependencies

Final interfaces should include persisted route or passthrough configuration
for request payload bytes, response payload bytes, timeout, and virtual-key
policy interactions. Existing `/admin-ui`, `/admin-ui/app.js`, and
`/admin-ui/app.css` asset contracts must remain stable.
