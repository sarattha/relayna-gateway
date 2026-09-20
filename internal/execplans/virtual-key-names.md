# Name virtual keys for profile selection

This living ExecPlan follows `PLANS.md` in `/Users/jobz/Works/relayna-gateway`.

## Purpose / Big Picture

Administrators can give a virtual key an optional human-readable name at creation
or later from Edit. The name is an alias for discovery, not an authentication
credential. Profile assignment search matches that name, prefix, UUID, project
and service; selected keys display the name with the stable UUID.

## Progress

- [x] (2026-09-20) Inspect key API, persistence, picker and release boundary.
- [x] (2026-09-20) Add compatible name persistence, validation and API coverage.
- [x] (2026-09-20) Add create/edit fields and consistent key labels with regression tests.
- [x] (2026-09-20) Run verification and coverage; validate with Computer Use.
- [x] (2026-09-20) Prepare verified changes and draft PR #121 update.

## Surprises & Discoveries

The profile catalog already searches `key.name`, but neither the key API nor
PostgreSQL stores that field. The existing key mutation permissions must also
cover name changes combined with disable/enable updates.

## Decision Log

- 2026-09-20: One optional `name` field acts as the alias. Trim surrounding
  whitespace; blank or null clears it; omission in PATCH preserves it. Permit
  duplicate names and retain UUIDs to distinguish keys. Limit names to 120
  characters and reject control characters. No credential or profile-ID changes.
- 2026-09-20: Compatibility boundary is release v0.1.37. Add a nullable PostgreSQL
  column through a new migration and additive optional API fields. Existing
  requests, unnamed rows, key prefixes, hashes and profile bindings stay valid.
  User authorization covers this feature's additive persistence change.

## Context and Orientation

Core request/response models live in `crates/gateway-core/src/admin.rs`.
`crates/gateway-store/src/postgres.rs` owns key SQL and validation on writes;
`crates/gateway-api/src/app.rs` owns authorization and audit events.
Admin UI create/edit forms and inventory are in `admin-ui/src/main.ts` under
gateway-api; `auth-profiles.ts` supplies the React picker's searchable catalog.
Generated embedded assets must be rebuilt with `npm run build:admin-ui`.

## Plan of Work

Add optional name models, normalization, null-aware patch deserialization, an
additive migration, and SQL reads/writes. Include name in mutation scope checks.
Update key forms and labels; reuse existing profile search behavior. Add core,
API, real PostgreSQL and UI regressions for create/rename/clear/omission,
validation, authorization, escaping and name search without altering bindings.

## Concrete Steps / Validation and Acceptance

From repository root run UI build, `npm test`, React coverage and typecheck.
Run `.codex/skills/code-change-verification/scripts/run.sh` with local PostgreSQL
15432 and Redis 16379 test databases. Run LLVM coverage and
`scripts/check-changed-rust-coverage.py` against the pre-feature commit 252c762;
changed production Rust coverage must exceed 98%. Computer Use must demonstrate
renaming an existing demo key, reloading it and locating it by name in the profile
picker at desktop and narrow widths. Existing unnamed keys must remain usable.

## Idempotence and Recovery

SQLx records successful migrations and retries unapplied migrations atomically.
The nullable column does not need a data rewrite. For application rollback, keep
the additive column; older code ignores it. Do not drop names or user data.
Integration fixtures use unique key IDs. UI verification uses the local demo.

## Outcomes & Retrospective

Administrators can create, rename and clear key names through existing admin
APIs and forms. Inventory, selectors and profile search display aliases while
preserving stable UUIDs, credentials and bindings. Names have on-demand help.

All ten mandatory verification commands passed; nextest ran 373 tests with zero
skips. UI build, operational regressions, React typecheck and 26 mounted tests
passed; measured React component coverage is 100% on all four metrics. LLVM
coverage is 25/25 changed executable production Rust lines for this feature and
162/162 for the complete PR relative to 872614b. No coverage exclusions changed.

Computer Use on localhost verified rename/save/reload, persistence across gateway
restart, clear-to-prefix fallback, case-insensitive multi-word alias search, Add
clearing the query, selected aliases, and desktop/390px layouts. Profile drafts
were cancelled without changing route access. The named demo key remains named
QA Production Automation. Regression coverage includes PostgreSQL reconnects,
legacy unnamed rows, invalid names, unchanged credential hashes, null/omitted
PATCH fields, RBAC, audit records and escaped UI labels.

Evidence: `/tmp/key-names-verification.log`, `/tmp/key-names-coverage.log`,
`/tmp/key-names.lcov`, and ignored screenshots in `internal/test-reports/admin4/`.
`internal/test-reports/virtual-key-names-coverage.json` records PR-wide changed
line coverage. These scoped metrics do not claim whole-workspace 100% coverage.
