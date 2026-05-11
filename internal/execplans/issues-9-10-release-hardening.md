# Issues 9 and 10 Route Controls and Release Hardening

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md`. Product intent and phase
scope come from `internal/design-manifesto.md`.

## Purpose / Big Picture

Fix two gateway route-control issues and make tag releases fail before
publishing when release metadata is inconsistent.

After this change, service wildcard routes such as
`/services/translation-service/health` can use `GET` when the registered service
allows `GET`. Operators can also use the admin API and embedded admin portal to
disable or re-enable `/v1/chat/completions` and `/v1/responses` globally while
keeping both routes enabled by default for compatibility.

## Progress

- [x] (2026-05-11 23:00 +07) Confirm latest release tag is `v0.0.3` and
      runtime/admin/persistence changes affect released behavior.
- [x] (2026-05-11 23:05 +07) Draft ExecPlan and compatibility strategy.
- [x] (2026-05-11 23:15 +07) Add PostgreSQL-backed OpenAI route settings.
- [x] (2026-05-11 23:25 +07) Add admin APIs and admin portal controls.
- [x] (2026-05-11 23:35 +07) Update service wildcard route resolution and proxy route-setting
      enforcement.
- [x] (2026-05-11 23:45 +07) Harden release metadata validation in CI and release workflow.
- [x] (2026-05-11 23:55 +07) Run `$code-change-verification`, admin UI tests, and release metadata
      validation.

## Surprises & Discoveries

- Observation: The route resolver rejects every non-POST request before
  checking whether the path is a service wildcard route.
  Evidence: `crates/gateway-core/src/routing.rs` returns
  `GatewayError::UnsupportedRoute` when `method != Method::POST`.
- Observation: The embedded admin portal already has service enable/disable
  patterns that can be reused for OpenAI route controls.
  Evidence: `crates/gateway-api/src/static/admin-ui/app.js` uses
  `/admin/services/{service_name}/enable` and `/disable` actions.
- Observation: Existing Argon2 salt generation imported `OsRng`, but the
  workspace did not enable `rand_core`'s `getrandom` feature, so gateway-core
  tests could not compile.
  Evidence: `cargo test -p gateway-core routing` failed with
  `no OsRng in the root` before adding the feature-unifying dependency.

## Decision Log

- Decision: Persist OpenAI route settings in PostgreSQL and seed both released
  LiteLLM routes enabled by default.
  Rationale: Admin changes must survive restarts and apply consistently across
  gateway pods while preserving `v0.0.3` default behavior.
  Date/Author: 2026-05-11 / Codex.

- Decision: Use `403 disabled_route` for globally disabled OpenAI routes.
  Rationale: The route exists and the key may be valid, but an operator has
  intentionally disabled the gateway route.
  Date/Author: 2026-05-11 / Codex.

- Decision: Share one release metadata validation script between CI and release.
  Rationale: The tag, workspace version, and changelog checks should be
  testable before a release tag is pushed.
  Date/Author: 2026-05-11 / Codex.

## Outcomes & Retrospective

Implemented. Operators can toggle the two LiteLLM OpenAI-compatible routes
through new admin APIs and the embedded portal. Service wildcard `GET` requests
now reach service-registration method enforcement instead of being rejected by
the initial route resolver. Release workflows now validate metadata before
publishing.

Validation passed:

    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    node tests/admin-ui.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.3
    /tmp/relayna-mkdocs-venv/bin/mkdocs build --strict

## Context and Orientation

`gateway-core` owns route resolution and framework-agnostic policy types.
`gateway-proxy` owns Pingora proxy request handling for LiteLLM, direct
providers, and internal service routes. `gateway-api` owns Axum admin routes and
the embedded admin portal. `gateway-store` owns PostgreSQL migrations and query
implementations. `.github/workflows/release.yml` publishes tag-based releases.

A virtual key is the client-facing Relayna bearer token. A service wildcard
route is `/services/{service-name}/*`, which resolves to a registered internal
service before upstream forwarding. LiteLLM routes are the OpenAI-compatible
routes `/v1/chat/completions` and `/v1/responses`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.3`.

This change affects released public proxy routes, admin APIs, PostgreSQL schema,
and release workflow behavior. Compatibility is preserved by keeping both
OpenAI routes enabled by default, making admin APIs additive, and allowing
service wildcard `GET` only when a service registration explicitly allows that
method.

## Plan of Work

Add route setting types and traits in `gateway-core`, implement them in
`gateway-store`, and expose admin routes through `gateway-api`. Add a migration
for `openai_route_settings` with two seeded rows. Update `gateway-proxy` so
LiteLLM route settings are checked after authentication and before policy,
rate-limit, or budget decisions.

Update route resolution so service wildcard paths can resolve for standard
service methods, while generation, direct provider, and legacy named service
routes remain POST-only.

Add admin portal controls and static tests for the new endpoints. Add release
metadata validation as a repository script and call it from both CI and release.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    node tests/admin-ui.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.3

## Validation and Acceptance

- `GET /services/{service-name}/health` resolves and forwards only when the
  service registration allows `GET`.
- `GET /v1/chat/completions` and `GET /v1/responses` still return
  `unsupported_route`.
- Admin can list, disable, and enable `chat-completions` and `responses`.
- Disabled OpenAI routes return `403 disabled_route` for authenticated proxy
  calls and do not contact the upstream.
- Release validation fails before publishing when the tag version,
  `Cargo.toml` workspace version, or `CHANGELOG.md` section disagree.

## Idempotence and Recovery

The migration uses `CREATE TABLE IF NOT EXISTS` and idempotent seeded upserts.
Admin enable/disable operations can be safely repeated. Release validation is
read-only. Failed tests or verification commands can be rerun after fixing the
reported issue.

## Artifacts and Notes

No external services are required for unit tests. PostgreSQL migrations are
compiled by SQLx and applied by `PostgresStore::connect`.

## Interfaces and Dependencies

Expected new interfaces:

- `OpenAiRouteSetting` response shape: `route_id`, `route`, `enabled`,
  `updated_at`.
- `AdminOpenAiRouteStore`: list and set route enabled state.
- `OpenAiRouteSettingsLookup`: proxy-time enabled-state lookup by `Route`.
- Admin routes:
  - `GET /admin/openai-routes`
  - `POST /admin/openai-routes/{route_id}/enable`
  - `POST /admin/openai-routes/{route_id}/disable`
