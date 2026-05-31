# LiteLLM Credential Mapping

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Operators can configure how Relayna Gateway authenticates to LiteLLM without
exposing LiteLLM keys to clients. Gateway keeps accepting Relayna virtual keys
from clients, then selects an internal LiteLLM credential by Relayna key,
project, or provider default. Operators can also configure LiteLLM to receive
the selected credential in a custom header such as `x-litellm-api-key` instead
of `Authorization: Bearer ...`.

## Progress

- [x] (2026-05-31 15:35Z) Read repo instructions, required skills, design
  manifesto, UI guidance, and current provider/proxy implementation.
- [x] (2026-05-31 15:35Z) Established compatibility boundary as `v0.1.7`;
  initial freeze perimeter test passed before edits.
- [x] (2026-05-31 16:02Z) Add PostgreSQL migration and store/core models for LiteLLM key/project
  credential mappings and provider credential header settings.
- [x] (2026-05-31 16:02Z) Update Pingora proxy credential resolution and header injection.
- [x] (2026-05-31 16:02Z) Add Admin API endpoints and audit redaction for mapping CRUD.
- [x] (2026-05-31 16:02Z) Update Admin UI source and generated static assets.
- [x] (2026-05-31 16:03Z) Add focused tests and run required verification.

## Surprises & Discoveries

- Observation: `provider_configs` already stores write-only upstream
  credentials and allows one enabled LiteLLM config.
  Evidence: `crates/gateway-store/migrations/20260512000100_admin_projects_provider_configs.sql`.
- Observation: current proxy already prefers active DB LiteLLM provider config
  over env fallback.
  Evidence: `RelaynaPingoraProxy` calls `active_litellm_config()` before
  building the LiteLLM upstream.
- Observation: the repository verification script runs additional supply-chain
  and static-analysis tools after Rust checks.
  Evidence: `.codex/skills/code-change-verification/scripts/run.sh`.

## Decision Log

- Decision: Add mappings under the existing provider/admin area instead of a
  standalone settings subsystem.
  Rationale: Provider configs already own upstream base URL and write-only
  credentials.
  Date/Author: 2026-05-31 / Codex.
- Decision: Support key and project mapping only; defer tenant mapping.
  Rationale: Gateway has first-class keys and projects but no durable tenant
  entity for this feature.
  Date/Author: 2026-05-31 / User and Codex.
- Decision: Credential precedence is key mapping, project mapping, then default
  active LiteLLM provider credential.
  Rationale: Most specific operator intent wins while preserving fallback
  compatibility.
  Date/Author: 2026-05-31 / User and Codex.
- Decision: Custom header mode sends only the configured custom header, not
  `Authorization`.
  Rationale: Production front doors may require custom upstream auth headers
  and should avoid duplicate credentials.
  Date/Author: 2026-05-31 / User and Codex.

## Outcomes & Retrospective

Implemented additive LiteLLM credential mapping and provider header controls.
Gateway now resolves LiteLLM credentials by Relayna key, then project, then
provider/default credential. Custom header mode strips client credentials and
sends only the configured LiteLLM header upstream. Admin API/UI responses
remain secret-redacted. The freeze perimeter was updated for additive routes and
the new migration, and the full code-change verification stack passed.

## Context and Orientation

Gateway accepts Relayna virtual keys (`rk_live_...`) from clients and injects
internal credentials when forwarding to LiteLLM. Current defaults come from
`LITELLM_BASE_URL` and `LITELLM_SERVICE_KEY`, and an operator-managed
`provider_configs` row can override the active LiteLLM base URL and credential.

The relevant code lives in:

- `crates/gateway-core/src/provider_configs.rs`: provider config request,
  response, runtime lookup traits.
- `crates/gateway-store/src/postgres.rs`: provider config SQL and runtime
  lookup.
- `crates/gateway-proxy/src/pingora_plane.rs`: LiteLLM upstream selection and
  credential header injection.
- `crates/gateway-api/src/app.rs`: Admin API routes.
- `crates/gateway-api/admin-ui/src/main.ts`: Admin UI source of truth.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.7`.

The change affects frozen provider proxy behavior, PostgreSQL schemas, Admin
API, Admin UI, and configuration semantics. The implementation must be additive:
existing deployments that only use `LITELLM_SERVICE_KEY` continue forwarding
with `Authorization: Bearer <credential>` unless an operator configures custom
header mode or mapping rows.

## Plan of Work

Add a migration that extends `provider_configs` with LiteLLM credential header
mode/name and creates a `litellm_credential_mappings` table. Model header mode,
scope, request/response types, and runtime credential lookup in
`gateway-core`. Implement the SQL store with write-only secrets and uniqueness
per scope/target.

Update `gateway-proxy` so LiteLLM route setup resolves the credential for the
authenticated key context, builds a LiteLLM upstream with the selected
credential, and injects it using the configured provider header mode. Preserve
all existing sensitive header stripping.

Add Admin API routes for mapping list/upsert/enable/disable/delete and extend
provider create/patch responses for header settings. Record audit events with
credential values redacted.

Update Admin UI Providers view to configure provider auth mode/header and
manage key/project LiteLLM mappings. Build the UI package to regenerate
checked-in static assets.

Add focused unit/API/UI/freeze tests and run the mandatory verification stack.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    node tests/freeze-v0.1.7-perimeter.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Admin UI changes require:

    cd crates/gateway-api/admin-ui
    npm run build
    cd /Users/jobz/Works/relayna-gateway
    node tests/admin-ui.test.mjs

## Validation and Acceptance

Acceptance criteria:

- Existing LiteLLM pass-through still uses `Authorization: Bearer
  LITELLM_SERVICE_KEY` when no DB provider config or mappings exist.
- A provider configured with `credential_header_mode = custom_header` and
  `credential_header_name = x-litellm-api-key` sends only that header upstream.
- Key mapping overrides project mapping, and project mapping overrides provider
  default credential.
- Disabled mappings fall back to the next available scope.
- Admin responses never include raw LiteLLM credential values.
- Freeze perimeter test is updated only for additive route/migration/UI surface
  changes.

## Idempotence and Recovery

Migrations use additive `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` and
`CREATE TABLE IF NOT EXISTS`. Re-running API upserts should update the existing
mapping for the same scope/target. Failed local verification can be rerun after
fixing compile or test issues. No Redis state changes are introduced.

## Artifacts and Notes

Initial freeze perimeter before edits:

    ok - v0.1.7 freeze baseline matches the current release version
    ...
    ok - release workflow publishes supply-chain artifacts

Final verification:

    npm run build:admin-ui
    node tests/admin-ui.test.mjs
    npm test
    node tests/freeze-v0.1.7-perimeter.test.mjs
    cargo check --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh

Real environment validation added a Docker Compose harness under
`internal/test-reports/litellm-real-passthrough/` with Gateway, PostgreSQL,
Redis, real `litellm/litellm:latest`, a LiteLLM front-door capture service,
and a mock OpenAI-compatible provider. The harness configured a LiteLLM
provider with `credential_header_mode = custom_header` and
`credential_header_name = x-litellm-api-key`, then exercised
`/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings`.

Observed precedence:

    sk-key -> sk-project -> sk-provider

The front-door capture confirmed Gateway sent `x-litellm-api-key`, did not send
`Authorization`, and stripped client Relayna/APIH/Apigee/JWT credentials before
LiteLLM. Browser screenshots captured the Provider UI header controls and
key/project mapping controls after fixing hidden-form control rendering.

## Interfaces and Dependencies

New admin-visible concepts:

- LiteLLM credential header mode: `authorization_bearer` or `custom_header`.
- LiteLLM credential mapping scope: `key` or `project`.
- Mapping target id: Relayna key UUID or project UUID.

No client request body or Relayna virtual key format changes.
