# Release 0.1.21 and OpenAPI Pricing Documentation

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Relayna Gateway publishes the completed OpenAPI endpoint-discovery and pricing
work as patch release `0.1.21`. Operators gain one durable documentation page
that explains how to preview and sync a registered service's relative
`/openapi.json`, assign endpoint cost modes, retain OCR multipart selector
pricing, account for budget ceilings, and operate the feature within its
security boundaries.

## Progress

- [x] (2026-07-22 06:50Z) Inspected release metadata, documentation structure,
  Admin UI version sources, and the implemented OpenAPI Admin API contracts.
- [x] (2026-07-22 07:10Z) Bumped workspace, Admin UI, deployment, and public
  release references to `0.1.21`.
- [x] (2026-07-22 07:35Z) Added and linked the dedicated OpenAPI import and
  endpoint cost reference across the documentation map, Admin Portal, feature,
  deployment, database, architecture, README, and release pages.
- [x] (2026-07-22 09:27Z) Built Admin UI assets, passed release metadata and
  strict documentation validation, and completed the mandatory repository
  verification stack.

## Surprises & Discoveries

- Observation: The Admin Portal and Current Features pages already contain a
  concise OpenAPI workflow, but there is no focused reference suitable for API
  automation, security review, or budget troubleshooting.
  Evidence: `docs/admin-portal.md` and `docs/current-features.md`.
- Observation: The Admin UI release-critical test intentionally pins the
  visible version and failed after the source bump until its assertion was
  updated to `v0.1.21`.
  Evidence: `npm test` first failed at `tests/admin-ui.test.mjs:50`, then all
  Admin UI checks passed after the release assertion changed.
- Observation: MkDocs is not installed globally in this workspace shell.
  Evidence: `mkdocs build --strict` was unavailable; an isolated
  `uvx --from mkdocs-material mkdocs build --strict` completed successfully.

## Decision Log

- Decision: Use patch version `0.1.21`.
  Rationale: Latest release is `v0.1.20`; the feature is additive and preserves
  existing service registrations, request paths, and pricing fallback behavior.
  Date/Author: 2026-07-22 / Codex.
- Decision: Add `docs/openapi-service-pricing.md` and keep shorter workflow text
  in the Admin Portal page.
  Rationale: One canonical reference prevents large duplicated explanations
  while keeping the setup flow discoverable.
  Date/Author: 2026-07-22 / Codex.

## Outcomes & Retrospective

Release metadata now consistently targets `0.1.21` across Cargo, the embedded
Admin UI, Kubernetes, README, changelog, and public release/deployment pages.
The new OpenAPI pricing reference documents the supported UI and Admin API
flows, full-list PATCH semantics, endpoint and body-rule precedence, Relayna
default classification, OCR multipart selection, preflight budget ceilings,
usage attribution, drift safety, fetch constraints, and troubleshooting.

`python3 scripts/validate-release-metadata.py v0.1.21`, the Admin UI production
build and test suites, Cargo workspace checking, and strict MkDocs validation
all passed. The mandatory fail-fast verification stack also passed formatting,
clippy, workspace tests, audits, 275 nextest tests, Trivy, gitleaks, and Semgrep.

## Context and Orientation

`Cargo.toml` owns the shared workspace package version and `Cargo.lock` records
the five workspace crate versions. Admin UI source version labels live under
`crates/gateway-api/admin-ui/`; checked-in static assets are generated from that
source. Public release and deployment references live in `README.md`, `docs/`,
and `deploy/kubernetes/relayna-gateway.yaml`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.20`. This release metadata and
documentation update describes additive branch behavior. It does not change a
public route, response shape, PostgreSQL or Redis format, authentication rule,
or proxy billing implementation.

## Plan of Work

Bump the shared version and all current-release references, add the changelog
entry, update Admin UI source and regenerate static assets, then add a focused
operator/API reference and link it from the documentation map, navigation,
Admin Portal, Current Features, README, and release notes. Finish with release
metadata validation, strict docs build, Admin UI tests, and the mandatory
verification script.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo check --workspace --all-features
    npm run build:admin-ui
    npm test
    python3 scripts/validate-release-metadata.py v0.1.21
    mkdocs build --strict
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Every current-release surface reports `0.1.21`; historical ExecPlans retain
their original version evidence. The new page documents UI and Admin API
preview/sync, `none`/`fixed`/`passthrough`, OCR `engine=docint`, preflight
budget behavior, drift, and secure fetch constraints. Release validation,
strict MkDocs build, Admin UI checks, and repository verification all pass.

## Idempotence and Recovery

Version and documentation edits are deterministic. Cargo can regenerate the
workspace entries in `Cargo.lock`, and the Admin UI build can safely regenerate
the checked-in static assets after any interrupted attempt.

## Artifacts and Notes

Latest release tag before this work: `v0.1.20`.

## Interfaces and Dependencies

No runtime interface changes. Documentation must match the existing
`POST /admin-ui/admin/services/{service_name}/openapi/preview` and
`POST /admin-ui/admin/services/{service_name}/openapi/sync` contracts.
