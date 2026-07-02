# Anthropic LiteLLM Route Support

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Relayna Gateway should support Claude and Claude Code clients through
Anthropic-compatible LiteLLM passthrough routes in the same governed/direct
route model currently used for OpenAI-compatible routes. Operators should see
OpenAI and Anthropic route families as distinct, recognizable sections in the
Admin UI and be able to enable, disable, and switch direct LiteLLM passthrough
mode for each canonical route. Clients should be able to call Anthropic routes
such as `/v1/messages` and `/v1/messages/batches` through Gateway without
opening unrelated service or control-plane routes.

## Progress

- [x] (2026-07-02 +07) Read repository instructions, `internal/design-manifesto.md`, `PLANS.md`, `SKILLS.md`, and the required `$karpathy`, `$implementation-strategy`, `$code-change-verification`, `$pr-draft-summary`, and `$github` skills.
- [x] (2026-07-02 +07) Confirmed clean `main`, latest release boundary `v0.1.14`, and created branch `codex/anthropic-litellm-routes`.
- [x] (2026-07-02 +07) Add core route definitions and settings for Anthropic-compatible routes.
- [x] (2026-07-02 +07) Add PostgreSQL migration and store/API support for Anthropic route settings.
- [x] (2026-07-02 +07) Update Pingora proxy direct LiteLLM handling for Anthropic-compatible routes.
- [x] (2026-07-02 +07) Update Admin UI source, generated assets, and UI tests with clear OpenAI and Anthropic sections and logos.
- [x] (2026-07-02 +07) Add focused Rust tests, Admin UI tests, and real-environment passthrough coverage.
- [x] (2026-07-02 +07) Run full verification, release metadata validation,
  Admin UI tests, and the real LiteLLM environment harness.
- [ ] Commit, push, open draft PR, mark ready, and start review-comment monitoring.

## Surprises & Discoveries

- Observation: `v0.1.14` already contains the OpenAI route settings and direct LiteLLM passthrough model.
  Evidence: `git show v0.1.14:crates/gateway-core/src/route_settings.rs`.
- Observation: The current Admin UI route controls are OpenAI-specific in names and endpoints.
  Evidence: `crates/gateway-api/admin-ui/src/main.ts` calls `/admin-ui/admin/openai-routes`.
- Observation: The real LiteLLM harness already exists and can cover the new Anthropic route by adding a `claude-review` model and `/v1/messages` fixture checks.
  Evidence: `internal/test-reports/litellm-real-passthrough/run.sh` and `mock-provider/server.mjs`.
- Observation: The final verification stack exposed advisory drift in
  transitive lockfile entries.
  Evidence: Updating `quinn-proto` and `anyhow` cleared blocking audit and
  deny checks, and `.codex/skills/code-change-verification/scripts/run.sh`
  then passed.

## Decision Log

- Decision: Implement Anthropic route support additively rather than replacing existing OpenAI route settings.
  Rationale: OpenAI-compatible route behavior is a released public surface in `v0.1.14`; existing route IDs, admin endpoints, and modes should keep working.
  Date/Author: 2026-07-02 +07 / Codex.
- Decision: Treat native Anthropic routes as LiteLLM-backed canonical routes, not direct provider routes.
  Rationale: The user asked for Claude/Anthropic API support for direct LiteLLM routes like OpenAI-compatible routes; Gateway should still own auth, policy, rate limits, budgets, and credential translation.
  Date/Author: 2026-07-02 +07 / Codex.
- Decision: Include Claude Code message batch routes in the canonical
  Anthropic family.
  Rationale: Claude Code can require `/v1/messages/batches`,
  `/v1/messages/batches/*`, `/v1/messages/batches/*/results`, and
  `/v1/messages/batches/*/cancel` in addition to `/v1/messages`.
  Date/Author: 2026-07-02 +07 / Codex.

## Outcomes & Retrospective

Implemented Anthropic-compatible LiteLLM route governance for Claude Messages,
Message Batches, token counting, and model listing. Operators can manage the
new route family through Admin API endpoints and a separate Anthropic Claude
section in Admin UI.

Validation passed:

- `python3 scripts/validate-release-metadata.py v0.1.15`
- `npm run build:admin-ui`
- `npm test`
- `bash internal/test-reports/litellm-real-passthrough/run.sh`
- `bash .codex/skills/code-change-verification/scripts/run.sh`

The real passthrough report recorded overall `PASS`, including a direct
Anthropic `/v1/messages` request using a LiteLLM bearer credential without a
Relayna key.

## Context and Orientation

`crates/gateway-core/src/routing.rs` defines public proxy route matching and
backend classification. Existing OpenAI-compatible canonical LiteLLM routes are
`/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings`.

`crates/gateway-core/src/route_settings.rs` defines route setting models,
route IDs, route mode parsing, and LiteLLM wildcard passthrough allowlist
logic. Existing mode values are `managed_by_gateway` and
`direct_litellm_passthrough`.

`crates/gateway-proxy/src/pingora_plane.rs` applies route settings in the
Pingora proxy plane. Direct LiteLLM mode strips or translates client
credentials and records status-only usage.

`crates/gateway-store/src/postgres.rs` persists route settings in PostgreSQL
through migrations under `crates/gateway-store/migrations/`.

`crates/gateway-api/src/app.rs` exposes Admin API route settings endpoints.
The Admin UI source is `crates/gateway-api/admin-ui/src/main.ts` and generated
assets are checked in under `crates/gateway-api/src/static/admin-ui/`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.14`. OpenAI-compatible route
settings, mode values, public proxy routes, and Admin API endpoints are released
surfaces and must remain backward compatible. Anthropic route support is
additive. A PostgreSQL migration should preserve existing rows and seed new
Anthropic route settings without changing existing OpenAI rows.

## Plan of Work

Add Anthropic route variants for `/v1/messages`,
`/v1/messages/count_tokens`, and the Message Batches API routes including
`/v1/messages/batches`. Map them to LiteLLM backend/provider and expose a route
family helper parallel to OpenAI settings.

Extend route settings with Anthropic route IDs and admin/store lookup methods,
or generalize route-setting helpers without breaking existing OpenAI API names.
Add a migration to seed Anthropic route rows and ensure store reads/writes can
serve both OpenAI and Anthropic route families.

Update the proxy so direct LiteLLM passthrough mode applies to Anthropic
canonical routes the same way it applies to OpenAI-compatible canonical routes,
while wildcard LiteLLM passthrough behavior remains unchanged.

Update Admin UI data loading and rendering so the Routes view shows separate
OpenAI and Anthropic sections with recognizable inline logo marks, enablement
controls, and mode controls. Regenerate static assets with
`npm run build:admin-ui`.

Add tests for route resolution, route setting lookups, admin endpoints, proxy
direct passthrough status-only usage, Admin UI rendering, and real-environment
LiteLLM passthrough for Claude-compatible endpoints where local services allow.

Update `CHANGELOG.md`, version metadata, and docs describing supported routes,
operator controls, and compatibility notes.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    cargo fmt --all --check
    cargo test -p gateway-core route
    cargo test -p gateway-api openai_route
    cargo test -p gateway-proxy direct_litellm
    npm --prefix crates/gateway-api/admin-ui run build
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

Final publishing steps, after verification:

    git status -sb
    git add ...
    git commit -m "feat: add anthropic litellm routes"
    git push -u origin codex/anthropic-litellm-routes

## Validation and Acceptance

Success requires:

- `Route::resolve_match` recognizes Anthropic-compatible `/v1/messages`,
  `/v1/messages/count_tokens`, and message batch routes and rejects unsupported
  methods.
- Admin APIs expose OpenAI and Anthropic route families without breaking the
  released `/admin-ui/admin/openai-routes` endpoint.
- Direct LiteLLM passthrough mode can be set for Anthropic routes and forwards
  non-Relayna bearer credentials directly to LiteLLM with credential header
  translation.
- Relayna bearer credentials on Anthropic routes still pass route enablement,
  policy, rate-limit, budget, and credential stripping/injection checks.
- Admin UI shows separate OpenAI and Anthropic route sections with recognizable
  logo marks and no secret rendering.
- Rust workspace verification, Admin UI build/tests, and real-environment
  passthrough checks pass or any external blocker is documented with evidence.
- Version, CHANGELOG, and docs describe the new behavior.
- A PR is created as draft, then marked ready only after verification.

## Idempotence and Recovery

All migrations must be additive and safe to rerun through normal SQLx migration
tracking. If a focused test fails, fix the relevant code and rerun both the
focused command and the final verification script. If local real-environment
services are unavailable, keep unit/integration coverage complete and document
the missing external service evidence before attempting PR readiness.

## Artifacts and Notes

- Real LiteLLM passthrough report:
  `internal/test-reports/litellm-real-passthrough/report.md`.
- Structured real passthrough results:
  `internal/test-reports/litellm-real-passthrough/results.json`.

## Interfaces and Dependencies

New Anthropic route settings must use the existing mode enum values:
`managed_by_gateway` and `direct_litellm_passthrough`. Existing OpenAI route
IDs and endpoints remain stable. Admin UI source remains the source of truth and
generated static assets must be rebuilt, not hand-edited.
