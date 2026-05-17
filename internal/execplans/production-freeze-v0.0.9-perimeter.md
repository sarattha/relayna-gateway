# Production Freeze v0.0.9 Perimeter

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Freeze Relayna Gateway v0.0.9 as the production compatibility baseline and add
tests, repository guidance, and an agent skill that make public surface changes
visible before future feature work merges.

After this work, contributors can add features only by preserving the v0.0.9
perimeter tests or intentionally updating them with compatibility notes. The
first perimeter is deterministic and local: it pins public control routes,
generation route resolution, config environment variables, migration inventory,
Redis key formats, error codes, release metadata, and admin portal endpoint
usage without requiring PostgreSQL, Redis, LiteLLM, or provider services.

## Progress

- [x] (2026-05-17 00:00Z) Fetched tags and confirmed `v0.0.9` exists at
  `2413fb1`.
- [x] (2026-05-17 00:00Z) Confirmed current branch tree matches `v0.0.9`
  content.
- [x] (2026-05-17 00:00Z) Added production freeze guard skill.
- [x] (2026-05-17 00:00Z) Updated `AGENTS.md` with v0.0.9 freeze rules.
- [x] (2026-05-17 00:00Z) Added deterministic freeze perimeter tests.
- [x] (2026-05-17 00:00Z) Wired freeze perimeter tests into CI and release
  checks.
- [x] (2026-05-17 00:00Z) Ran required verification.

## Surprises & Discoveries

- Observation: The working branch describes as `v0.0.8-8-gfffa0e4`, but
  `origin/main` has tag `v0.0.9` at merge commit `2413fb1`.
  Evidence: `git diff --name-status v0.0.9...HEAD` returned no file changes.
- Observation: Existing tests are mostly crate-local unit tests plus
  `tests/admin-ui.test.mjs`; there is no black-box service test harness yet.
  Evidence: `find tests crates -maxdepth 4 -type f ...` surfaced only the admin
  UI script outside crate-local tests.

## Decision Log

- Decision: Use `v0.0.9` as the production freeze baseline.
  Rationale: The user asked to pull the 0.0.9 tag before creating the plan, and
  the tag exists upstream.
  Date/Author: 2026-05-17 / Codex.
- Decision: Start with static contract tests and leave service-backed contract
  tests as a follow-up expansion point.
  Rationale: Static tests catch accidental route/config/schema/key/error drift
  in ordinary CI without external services, which makes the perimeter
  continuously enforceable immediately.
  Date/Author: 2026-05-17 / Codex.
- Decision: Future feature work may update the perimeter tests only when the
  change is intentional and compatibility notes are recorded.
  Rationale: The selected gate was "Tests only"; this blocks accidental breaking
  changes while still allowing deliberate additive features.
  Date/Author: 2026-05-17 / Codex.

## Outcomes & Retrospective

The repository now has a v0.0.9 freeze baseline, mandatory future-agent skill
guidance, CI coverage for the perimeter test, and a concrete test script that
fails on unreviewed public surface drift. Remaining gaps are runtime
environment contract tests for live PostgreSQL, Redis, upstream proxy, and
streaming behavior; those should be added when a stable local service harness
is introduced.

## Context and Orientation

Relayna Gateway v0.0.9 is a Rust workspace with these public and
compatibility-sensitive surfaces:

- `crates/gateway-api/src/app.rs`: Axum control-plane routes, admin APIs,
  health, readiness, metrics, and embedded admin UI.
- `crates/gateway-core/src/routing.rs`: OpenAI-compatible, direct provider, and
  internal service route resolution.
- `crates/gateway-core/src/errors.rs`: stable public error codes, status codes,
  and messages.
- `crates/gateway-core/src/budgets.rs` and `rate_limits.rs`: Redis key format
  constructors.
- `crates/gateway-store/migrations/`: PostgreSQL schema history.
- `crates/gateway-api/src/config.rs`: environment variables and default config.
- `tests/admin-ui.test.mjs`: existing static admin portal contract checks.

## Compatibility Boundary

Compatibility boundary: production freeze tag `v0.0.9` at `2413fb1`. Future
changes to public routes, response shapes, status codes, virtual key behavior,
environment variables, PostgreSQL migrations, Redis keys, provider routing,
streaming semantics, usage event fields, telemetry, admin UI endpoints, or
Relayna runtime contracts must preserve the freeze perimeter tests or update
them with explicit compatibility notes.

## Plan of Work

Add `.codex/skills/production-freeze-guard/SKILL.md` so future agents have a
named workflow for feature work after the freeze.

Update `AGENTS.md` to make `$production-freeze-guard` mandatory for new
features and compatibility-sensitive changes after v0.0.9.

Add `tests/freeze-v0.0.9-perimeter.test.mjs` to pin the current public route
inventory, route resolver contracts, error code inventory, migration list, env
var names, Redis key formats, release metadata, and admin portal endpoint
coverage.

Update `.github/workflows/ci.yml` and `.github/workflows/release.yml` to run the
freeze perimeter test with the existing Node setup.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    git fetch --tags --prune-tags
    node tests/freeze-v0.0.9-perimeter.test.mjs
    node tests/admin-ui.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.9
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance requires:

- Freeze perimeter test passes.
- Existing admin UI test passes.
- Release metadata validates for `v0.0.9`.
- `cargo fmt --all --check` passes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  passes.
- `cargo test --workspace --all-features` passes.

## Idempotence and Recovery

All added tests are read-only and may be rerun safely. If a future feature
intentionally adds a public route, config variable, migration, Redis key, or
error code, update the perimeter test in the same PR and record the
compatibility decision in the PR description or ExecPlan.

No database or Redis cleanup is required for the current perimeter test because
it does not start external services.

## Artifacts and Notes

Current baseline:

    git tag -l 'v*' --sort=-v:refname | head -n1
    v0.0.9

    git rev-parse --short v0.0.9
    2413fb1

## Interfaces and Dependencies

The freeze perimeter depends only on Node.js built-ins and repository files.
It does not add Rust crates, npm packages, services, fixtures, or generated
artifacts.
