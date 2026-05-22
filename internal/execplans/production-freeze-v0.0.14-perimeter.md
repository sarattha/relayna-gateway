# Production Freeze v0.0.14 Perimeter

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md`.

## Purpose / Big Picture

Retarget the Relayna Gateway production freeze perimeter from `v0.0.9` to the
current `v0.0.14` release. After this change, contributors and CI will treat
the `v0.0.14` public route, error-code, configuration, migration, Redis, admin
UI, and release metadata surfaces as the compatibility baseline.

## Progress

- [x] (2026-05-22 00:00Z) Confirmed the latest release tag and current `HEAD`
  are both `v0.0.14`.
- [x] (2026-05-22 00:00Z) Read freeze, implementation strategy, verification,
  and design manifesto guidance.
- [x] (2026-05-22 00:00Z) Updated active contributor guidance, freeze skill
  guidance, CI, and release
  workflows to reference the `v0.0.14` perimeter test.
- [x] (2026-05-22 00:00Z) Renamed and updated the deterministic freeze
  perimeter test to
  `tests/freeze-v0.0.14-perimeter.test.mjs`.
- [x] (2026-05-22 00:00Z) Ran the updated perimeter test, release metadata
  validation, admin portal test, code-change verification stack, and workspace
  build successfully.

## Surprises & Discoveries

- Observation: `v0.0.14` points to the current `HEAD`, so this is a baseline
  retargeting change rather than a behavior diff against the release tag.
  Evidence: `git rev-parse --short v0.0.14` and `git rev-parse --short HEAD`
  both returned `0574d61`.

## Decision Log

- Decision: Use `v0.0.14` as the production freeze baseline.
  Rationale: The user asked to make the current version the freeze perimeter
  version, and the latest fetched release tag is `v0.0.14`.
  Date/Author: 2026-05-22 / Codex.

- Decision: Rename the perimeter test file instead of keeping the old
  `v0.0.9` filename.
  Rationale: The filename is used in CI and contributor instructions as the
  human-visible freeze baseline marker.
  Date/Author: 2026-05-22 / Codex.

## Outcomes & Retrospective

Relayna Gateway now treats `v0.0.14` as the active production freeze perimeter.
The deterministic perimeter test was renamed to
`tests/freeze-v0.0.14-perimeter.test.mjs`, active contributor guidance and CI
workflows point to it, and verification passed locally.

No runtime gateway behavior changed. The main risk was leaving stale active
guidance or workflow references to the previous perimeter script; a targeted
search found none in active guidance, workflows, tests, docs, or scripts.

## Context and Orientation

`AGENTS.md` and `.codex/skills/production-freeze-guard/SKILL.md` define the
production freeze workflow for contributors. `.github/workflows/ci.yml` and
`.github/workflows/release.yml` run the deterministic perimeter script. The
perimeter script reads repository source files directly and asserts that frozen
surfaces remain pinned without needing PostgreSQL, Redis, or provider services.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. This change does not
alter gateway runtime behavior, public HTTP semantics, persisted schemas, Redis
formats, or provider proxy behavior. It changes the compatibility baseline used
by future contributors and CI.

## Plan of Work

Update active guidance to describe `v0.0.14` as the freeze baseline. Rename the
perimeter test and change its release assertions and test labels to reference
`v0.0.14`. Update CI and release workflows to execute the renamed script.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.0.14-perimeter.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.14
    git status --short

## Validation and Acceptance

The updated perimeter test passes locally. Release metadata validation for
`v0.0.14` passes. A repository search over active guidance and workflows no
longer points contributors to `tests/freeze-v0.0.9-perimeter.test.mjs`.

## Idempotence and Recovery

The changes are text-only and can be safely reapplied. If a renamed test path is
missed, CI will fail with a missing file. If a perimeter assertion fails, inspect
the named source file and either correct the assertion to the `v0.0.14` baseline
or revert the unintended source change.

## Artifacts and Notes

Verification completed on 2026-05-22:

    node tests/freeze-v0.0.14-perimeter.test.mjs
    python3 scripts/validate-release-metadata.py v0.0.14
    bash .codex/skills/code-change-verification/scripts/run.sh
    node tests/admin-ui.test.mjs
    cargo build --workspace --all-features

No external services are required.

## Interfaces and Dependencies

The final test command is `node tests/freeze-v0.0.14-perimeter.test.mjs`. The
release metadata validation command remains
`python3 scripts/validate-release-metadata.py v0.0.14`.
