# Admin UI Entra and Apigee Auth Settings

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This plan follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators can currently configure the Entra ID and Apigee front-door auth
entrances only through deployment environment variables. After this change, an
operator using the Admin UI Settings page can view and update the effective
Entra ID and Apigee trusted-header settings, including enablement, issuer,
tenant, audience, discovery URL, scope, role, groups, algorithms, Relayna key
header, cache and clock settings, and Apigee trusted-header secret. The sidebar
also remains usable on short displays so the Sign out control can be reached.

## Progress

- [x] (2026-05-30 00:00Z) Read repository instructions, design manifesto, Admin
  UI guidance, and mandatory skills.
- [x] (2026-05-30 00:00Z) Established compatibility boundary as latest release
  tag `v0.1.4`; production freeze baseline remains `v0.1.0`.
- [x] (2026-05-30 00:00Z) Inspect current Entra/Apigee config loading, runtime usage, admin APIs,
  persistence patterns, and Admin UI Settings implementation.
- [x] (2026-05-30 00:00Z) Add persisted auth-settings model, storage, admin API, audit coverage,
  and runtime config resolution.
- [x] (2026-05-30 00:00Z) Add Settings page controls and fix short-display sidebar scrolling.
- [x] (2026-05-30 00:00Z) Add focused tests, rebuild Admin UI static assets, and run required
  verification.
- [x] (2026-05-30 00:00Z) Run local real-environment/browser verification, generate HTML report, and
  post it to the PR comments when a PR is available.

## Surprises & Discoveries

- Existing Studio settings already support persisted-over-environment behavior
  and provide the closest local pattern for write-only secrets and Settings
  page UX.
- The first gateway auth migration timestamp collided with the existing
  LiteLLM embeddings migration. The Docker real-environment harness exposed
  this before handoff; the migration was renamed to
  `20260530000200_gateway_auth_settings.sql`.

## Decision Log

- Decision: Treat the Admin UI controls as an additive post-freeze surface and
  preserve existing environment-variable fallback.
  Rationale: Operators who deploy with env vars must not lose current behavior;
  portal settings should override env values only when persisted.
  Date/Author: 2026-05-30 / Codex.
- Decision: A persisted auth settings row overrides environment config even
  when the entrance is disabled.
  Rationale: Enable/disable must be controllable from the portal; otherwise an
  env-enabled entrance could not be disabled without redeployment.
  Date/Author: 2026-05-30 / Codex.
- Decision: Use a shared runtime auth config updated by the admin API after
  persistence.
  Rationale: Startup-only config would not make portal changes affect live
  proxy traffic. The shared snapshot keeps verifier construction cached while
  allowing Settings saves to update runtime behavior.
  Date/Author: 2026-05-30 / Codex.

## Outcomes & Retrospective

Implemented. Operators can configure Entra ID and Apigee trusted-header auth
from the Admin UI Settings page, secrets remain write-only, and saved settings
update the running proxy auth runtime. The short-display sidebar now keeps
Sign out reachable while the navigation area scrolls independently. HTML
reports were written under `internal/test-reports/admin-auth-settings/` and
`internal/test-reports/litellm-real-passthrough/`, and a PR comment was posted
to PR #62.

## Context and Orientation

Entra ID front-door auth validates Microsoft Entra access tokens before Relayna
virtual-key authentication. Apigee trusted-header mode accepts signed identity
headers from an Apigee edge when the trusted-header secret validates the proof.
Today `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/config.rs`
builds these settings from environment variables only. The Admin UI source of
truth is `/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/`, and
generated assets are checked in under
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/`.

The existing Studio connection settings flow spans:

- `/Users/jobz/Works/relayna-gateway/crates/gateway-core/src/studio_settings.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-store/src/postgres.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/app.rs`
- `/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/src/main.ts`

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.4`; production freeze
baseline `v0.1.0`. This work touches external config, auth behavior, Admin API,
Admin UI, migrations, and tests. The intended strategy is additive:
environment-variable configuration remains valid, persisted settings are
write-only for secrets, and public proxy response shapes remain unchanged.

## Plan of Work

First inspect where proxy and API runtime auth decisions consume
`EntraAuthConfig` and `ApigeeTrustedHeaderConfig`. Then add a persisted settings
type and migration following the Studio connection settings pattern. Expose a
protected admin route under `/admin-ui/admin/auth/front-door` for reading and
patching effective settings, with `settings:update` scope required for writes
and audit events recorded.

Update runtime config resolution so persisted settings override environment
values without exposing secrets in responses. Update the Settings view to add
compact Entra and Apigee controls, preserve write-only secret handling, and add
sidebar overflow behavior so short displays can scroll to Sign out. Add tests
for config precedence, API read/patch behavior, UI endpoint coverage, and
freeze perimeter updates if needed.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.1.0-perimeter.test.mjs
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

Use focused Rust tests while iterating, then run the mandatory stack before
handoff.

## Validation and Acceptance

Acceptance requires:

- Settings page shows Entra and Apigee controls and writes to admin APIs.
- Persisted Entra/Apigee settings override env settings without exposing
  secrets.
- Invalid header names, invalid URLs, missing required enabled fields, and
  missing Apigee secrets fail closed.
- Sidebar is scrollable on short viewports and Sign out remains reachable.
- Admin UI static asset tests, freeze perimeter test, and Rust verification
  stack pass or documented blockers are reported.
- An HTML real-environment report is generated under `internal/test-reports/`
  and posted to the PR comments when a PR exists.

## Idempotence and Recovery

Migrations use `IF NOT EXISTS` and singleton rows where possible. Re-running UI
builds overwrites generated static assets from source. Failed focused tests can
be rerun after fixes; failed full verification must be rerun from the start.

## Artifacts and Notes

Pending.

## Interfaces and Dependencies

The new persisted auth settings interface must not return stored Apigee secrets
or raw tokens. It should expose `secret_configured` booleans, source metadata,
and effective non-secret values. Runtime callers should continue to receive
`Option<EntraAuthConfig>` and `Option<ApigeeTrustedHeaderConfig>` so verifier
construction remains localized.
