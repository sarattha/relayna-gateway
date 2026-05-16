# Issue 24 Admin Studio Connection

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Operators should be able to configure the Relayna Studio backend connection from
the protected Gateway Admin portal after Gateway is already running. The
connection status should be visible without exposing bearer token values, and
the existing Studio service import flow should use the saved connection without
a process restart.

## Progress

- [x] (2026-05-16 00:00Z) Read issue #24, `internal/design-manifesto.md`, and
  implementation-strategy guidance.
- [x] (2026-05-16 00:00Z) Add core Studio connection settings types,
  precedence resolution, validation, and token-redaction tests.
- [x] (2026-05-16 00:00Z) Add PostgreSQL singleton settings migration and
  `PostgresStore` read/patch support.
- [x] (2026-05-16 00:00Z) Add protected admin connection read, patch, and test
  endpoints; make Studio import resolve settings dynamically.
- [x] (2026-05-16 00:00Z) Add Admin portal Settings view for Studio connection
  status, save, clear, and test actions.
- [x] (2026-05-16 00:00Z) Update docs and admin UI static tests.
- [x] (2026-05-16 00:00Z) Run full `$code-change-verification`; all commands
  passed.

## Surprises & Discoveries

- Observation: `Option<Option<T>>` in serde is easy to misuse for PATCH
  semantics because omitted and null fields need distinct handling.
  Evidence: Implemented a small tri-state `PatchValue<T>` for Studio settings.

## Decision Log

- Decision: Persisted settings override environment settings; clearing the
  persisted base URL clears the persisted token and reveals env fallback.
  Rationale: Preserves existing deployment behavior while allowing runtime
  operator changes and safe token cleanup.
  Date/Author: 2026-05-16 / Codex.
- Decision: Store the bearer token as write-only Gateway-owned secret material
  at the same protection level as existing provider and service credentials.
  Rationale: The repository already stores these operator-managed secrets and
  redacts them in API responses.
  Date/Author: 2026-05-16 / Codex.

## Outcomes & Retrospective

Operators can configure, test, and clear the Studio connection from Admin
Settings, while existing `RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN`
continue to work as fallback configuration. Rust formatting, clippy, workspace
tests, and the Admin UI static test passed.

## Context and Orientation

Current Studio import lives in `crates/gateway-api/src/app.rs` as
`GET /admin/studio/services`. Before this work, `AppState` held an optional
startup-created `StudioCatalogClient` derived from `RELAYNA_STUDIO_BASE_URL`
and `RELAYNA_STUDIO_TOKEN`, so operators could not update the connection after
startup.

Gateway API owns the protected admin routes and Admin portal assets. Gateway
core owns framework-agnostic request, response, validation, and precedence
types. Gateway store owns PostgreSQL migrations and persisted settings access.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.7`. Existing env var behavior
is preserved as fallback. New admin routes and the new PostgreSQL settings table
are additive. No public proxy routes, virtual key format, Redis state, usage
event shape, or streaming behavior changes.

## Plan of Work

Add `gateway-core` Studio settings types with redacted responses, a tri-state
PATCH field, validation, and effective source resolution. Add a PostgreSQL
singleton table and store trait implementation. Replace startup-only Studio
client usage in the admin API with per-request effective settings lookup. Add
Admin portal Settings UI and docs that explain env fallback precedence and
token rotation.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

Use `.codex/skills/code-change-verification/scripts/run.sh` for the final
fail-fast verification sequence.

## Validation and Acceptance

Acceptance:

- `GET /admin/studio/connection` requires an operator token and returns
  `base_url`, `token_configured`, `source`, and `updated_at` without a token.
- `PATCH /admin/studio/connection` can set, replace, and clear persisted
  settings; clearing base URL also clears the persisted token.
- `POST /admin/studio/connection/test` reports service count on successful
  Studio catalog fetch and `studio_unavailable` on unreachable Studio.
- `GET /admin/studio/services` uses persisted settings immediately after save
  and falls back to env settings when persisted base URL is cleared.
- Admin UI static tests prove Settings navigation and token-redaction affordance
  exist.

## Idempotence and Recovery

The migration uses `CREATE TABLE IF NOT EXISTS` and is safe to rerun under sqlx
migration tracking. PATCH requests are idempotent for the same payload. If a
token is cleared accidentally, operators can set a replacement through the
Settings form or restore the env fallback token.

## Artifacts and Notes

API response example:

    {
      "base_url": "http://127.0.0.1:8000",
      "token_configured": true,
      "source": "persisted",
      "updated_at": "2026-05-16T00:00:00Z"
    }

Clear persisted settings:

    PATCH /admin/studio/connection
    { "base_url": null }

## Interfaces and Dependencies

New protected admin routes:

- `GET /admin/studio/connection`
- `PATCH /admin/studio/connection`
- `POST /admin/studio/connection/test`

Existing env vars remain supported:

- `RELAYNA_STUDIO_BASE_URL`
- `RELAYNA_STUDIO_TOKEN`
