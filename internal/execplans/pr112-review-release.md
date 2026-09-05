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
- [x] 2026-09-06: Fix CI and verify the review finding against released/dependency code; reply pending push.
- [x] 2026-09-06: Prepare version 0.1.33, changelog/docs, metadata, strict docs, complete verification and workspace build.
- [ ] Verify final-head CI, merge and pause review heartbeat.

## Surprises & Discoveries

Codex flagged first-address DNS selection as a P1 regression. Comparison against
v0.1.31 and Pingora 0.8.0 `upstreams/peer.rs:612-615` shows the old hostname
constructor also used `to_socket_addrs().next()` and stored one SocketAddr.
No address-failover capability was removed. A focused test compares native
multi-address construction with the explicit resolved peer, including SNI and
verification settings; operator docs record the existing limitation.

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

CI remediation passed both remote CI runs. Version 0.1.33 passed frontend tests, strict docs, release metadata validation, the full verification script (333 nextest tests) and workspace build. Final-head CI and merge remain pending.
