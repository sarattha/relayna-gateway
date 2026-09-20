# Illustrated authentication profile guide and 0.1.38 release metadata

This living ExecPlan follows PLANS.md in /Users/jobz/Works/relayna-gateway.

## Purpose / Big Picture

Operators need an illustrated guide that distinguishes route authentication mode,
profile authentication and forwarding mode. Document both profile types, key
names and selection, maintenance, recovery, credential headers and multi-replica
behavior. Update related entry points and release notes for gateway 0.1.38;
Design 4.0 remains the separate UI design version.

## Progress

- [x] (2026-09-20) Reviewed current docs, UI, release workflow and latest tag v0.1.37.
- [x] (2026-09-20) Bumped all five workspace crates/lockfile, UI labels/assets, image examples/manifests and release notes to 0.1.38. Corrected current feature, deployment, database and credential guidance.
- [x] (2026-09-20) Captured ten current UI screenshots with synthetic local fixtures and draft-only edits; restored zoom, canceled all drafts and preserved route revisions.
- [x] (2026-09-20) Added separate Entra/key-only procedures, mode comparison, field reference, key aliases/search/assignment, forwarding semantics, maintenance and actionable troubleshooting, each with relevant captures.
- [x] (2026-09-20) Strict MkDocs and version validation pass; 884 local links/anchors/images checked. Computer Use verified desktop/390px guide and full-size image links. All ten checks pass, nextest 377/377; 32 React tests remain at 100% coverage. One non-failing nextest output-cleanup warning passed cleanly on isolated rerun.
- [x] (2026-09-20) Prepared verified documentation, release metadata and updated description for delivery through existing draft PR #121; final secret scan found no leaks.

## Surprises & Discoveries

The deployment introduction incorrectly attributes the embeddings alias to 0.1.37
and the documentation homepage still describes Admin UI 2.0. LiteLLM guidance
omits the profile-mode restriction on native credential passthrough. The existing
profile reference is accurate but lacks an illustrated task-oriented setup guide.

## Decision Log

- 2026-09-20: Apply karpathy, implementation-strategy, code-change-verification,
  pr-draft-summary and Computer Use. Latest release boundary is v0.1.37. This work
  changes documentation and release metadata only, preserving runtime contracts.
- Follow the repository's existing 0.1.x release sequence: 0.1.38. Prepare metadata
  in draft PR #121; do not create a release tag or publish a release.
- Capture actual current UI with synthetic local data. Edit only drafts for setup
  illustrations and cancel afterward. Do not expose credentials or alter saved
  authentication policies merely to obtain screenshots.

## Context and Orientation

MkDocs Material builds docs/ using mkdocs.yml. The dedicated guide already lives
at docs/operations/authentication-profiles.md; keep its reference anchors and
expand it with clear setup sections. Commit publication images under
 docs/assets/screenshots/authentication-profiles/ (test-report screenshots are
ignored). Cargo.toml workspace.package sets all five gateway crate versions;
Cargo.lock must agree. CHANGELOG.md and docs/releases.md describe the target.

## Plan of Work

Update workspace version and regenerate lockfile without dependency upgrades.
Build the embedded gateway so screenshots show the correct runtime release.
Capture route mode, Entra profile fields, key-only profile fields, key search and
selection, key names, and maintenance/error recovery. Add captions and numbered
steps next to each relevant image; explain multiple keys per profile and one
profile per key per route. Verify all credential and rollout claims against
source. Correct current docs and preserve historical release descriptions.

## Validation and Acceptance

Run python3 scripts/validate-release-metadata.py v0.1.38, cargo metadata, npm test,
npm run test:react:coverage, npm run typecheck:admin-ui, strict MkDocs build and
.codex/skills/code-change-verification/scripts/run.sh.
All ten gateway checks must pass. React coverage must remain above 98%. Confirm
all committed screenshots are linked, readable and contain no secrets. Use
Computer Use to inspect the rendered guide and verify both setup flows without
saving policy changes. Keep the Rust embedded asset deployment unchanged.

## Idempotence and Recovery

Cancel all browser drafts. Rebuild after version edits and restart only the local
18460/18461 demo process. Documentation and metadata changes are reversible via
normal Git edits. Do not publish tags or mutate unrelated user work.

## Outcomes & Retrospective

Published-guide source now documents gateway 0.1.38 and Design 4.0 with ten
real screenshots. Corrected the native LiteLLM credential exception throughout
current docs and separated per-request profile reads from five-second persisted
Entra synchronization. Historical 0.1.37 compatibility notes remain intact.

Validation: `/tmp/profile-guide-verification.log` (all ten commands pass,
377/377 nextest), `/tmp/profile-guide-nextest-recheck.log` (clean targeted rerun),
`/tmp/profile-guide-react.log` (251/251 statements, 150/150 branches, 107/107
functions, 230/230 lines), `/tmp/profile-guide-js.log`,
`/tmp/profile-guide-types.log` and `/tmp/profile-guide-docs.log`. Computer Use
checked both profile flows and rendered docs without saving authentication
changes. Ten JPEGs total about 1.2 MiB and show synthetic identifiers/prefixes,
not raw credentials. The local documentation preview runs on port 18462.

This prepares a release version in the existing draft PR; no release tag or
published release was created.
