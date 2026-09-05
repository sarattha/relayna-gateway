# Review and release preparation for PR 112

Maintain this plan under `PLANS.md`. Worktree: `/Users/jobz/.codex/worktrees/9b27/relayna-gateway`;
branch: `codex/admin-ui-3-followup-fixes`; PR: https://github.com/sarattha/relayna-gateway/pull/112.

## Purpose / Big Picture

Address Codex reviews and CI failures, prepare the next patch version with matching
changelog and operator documentation, and merge the exact verified head only after
all applicable CI jobs pass. The user authorized review replies, resolving addressed
threads and merging, but this task does not publish a release or deploy.

## Progress

- [x] 2026-09-06: Mark PR ready and request Codex review.
- [x] 2026-09-06: Identify Admin portal CI missing npm dependencies.
- [ ] Fix and verify CI, address review findings and resolve handled threads.
- [ ] Bump version, update changelog/docs and validate release metadata.
- [ ] Verify final-head CI, merge and pause review heartbeat.

## Surprises & Discoveries

CI runs the frontend tests directly without installing dependencies. New tests
import TypeScript and fail with ERR_MODULE_NOT_FOUND on a clean runner. Local tests
passed because dependencies were installed. CI must use npm ci, rebuild and compare
the checked-in assets, and run the complete npm test suite.

## Decision Log

- 2026-09-06: Preserve existing Node version for this focused CI fix. Install from
  the lockfile and verify generated asset consistency rather than skipping tests.
- 2026-09-06: Use next patch version after 0.1.32 once reviewed changes are ready;
  no schema migration is required for this additive diagnostic/UI follow-up.
- 2026-09-06: A heartbeat continues the authorized review cycle. Do not request
  further review rounds solely for P2 findings. Prior approval bypass was scoped
  to PR 111; do not silently infer new bypass permission for PR 112.

## Validation

Run npm ci, npm run build:admin-ui, npm test, generated asset diff checks, the full
code-change-verification script, workspace build, release metadata validation and
strict documentation build. Inspect all CI runs and review threads for the final
commit. Reply to each addressed finding with fix and validation evidence before
resolving it. Read back merged state and commit after merging.

## Outcomes & Retrospective

Review and CI remediation in progress.
