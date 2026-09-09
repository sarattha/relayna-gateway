# Preserve two-header authentication during Entra troubleshooting

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Issue #114 lets an operator require a nonempty Authorization bearer plus a valid
Relayna virtual key while temporarily suspending Entra verification. A virtual
key is the gateway-issued credential used for policy, budget and usage attribution.
The unverified bearer never establishes identity. Saved Entra configuration can
be restored without changing the client headers.

## Progress

- [x] (2026-09-09) Read issue, manifesto, skills and released implementation; create isolated worktree.
- [x] (2026-09-09) Add opt-in configuration, persistence, request authentication and UI controls.
- [x] (2026-09-09) Add regressions; real proxy pause/restore, key checks and provider mapping pass.
- [x] (2026-09-09) Build UI, run npm tests, verify desktop/mobile using Computer Use.
- [x] (2026-09-09) Full verification passes, including 337 nextest tests; open PR #115.
- [x] (2026-09-09) Address Codex finding: defer env conflict check until effective source selection; add regression.
- [x] (2026-09-09) Prepare 0.1.34 version, changelog and current documentation updates.
- [ ] Verify release metadata changes and finish Codex review.

## Surprises & Discoveries

Direct LiteLLM mode has an intentional early authentication branch which forwards
the client bearer as a LiteLLM credential. This mode is unchanged: the new option
applies to gateway-managed requests, and the UI/docs require managed routes.
The mandatory verification script also runs security/dependency checks and nextest.
See `internal/test-reports/issue-114/verification.md` for the pre-existing model-policy
timing gap, fixed test-listener race, and macOS certificate-store workaround.

## Decision Log

- 2026-09-09: Add `unverified_bearer_enabled` (false by default), with
  `GATEWAY_UNVERIFIED_BEARER_ENABLED` as the environment fallback. Persisted
  settings keep their existing precedence. Preserve Entra configuration while
  omitting its verifier from the runtime snapshot when paused. Reject coexistence
  with trusted Apigee headers at configuration, runtime and database boundaries.
- 2026-09-09: Use existing bearer error conventions and key authentication;
  never parse bearer claims. Preserve all existing downstream policy and upstream
  credential mapping code. User explicitly authorized this additive auth mode.

## Outcomes & Retrospective

Implementation, regression tests, migration compatibility and desktop/mobile UI QA
are complete. Full verification passes. PR #115 is open; Codex review and the
release metadata update remain.

## Context and Orientation

Worktree: `/Users/jobz/Works/relayna-gateway-issue-114`, branch
`codex/issue-114-two-header-auth`. Core `src/auth_settings.rs` owns effective
settings and shared runtime snapshots. API `src/config.rs` owns environment
loading. Proxy `src/pingora_plane.rs` chooses client authentication and strips
client credentials before injecting mapped provider credentials. Store
`src/postgres.rs` persists the singleton settings row. Admin UI source is
`crates/gateway-api/admin-ui/src/main.ts`; generated assets must be rebuilt.

## Compatibility Boundary

Latest release: v0.1.33. Preserve old defaults, PATCH omission semantics and
existing modes. Add a false-default database column through a forward migration;
no existing data is rewritten. API response and PATCH fields are additive.
The Entra enabled flag continues to indicate configured Entra authentication;
the new opt-in flag explicitly suspends verification without discarding settings.

## Plan of Work

Add the setting across environment, core, persistence and UI; gate only managed
request authentication, authenticating the normal Relayna header key. Test absent
and malformed headers, invalid/disabled/expired/revoked keys, policy and credential
handling, and pause/restore with a real local JWT issuer. Test persistence and UI
save behavior. Run required checks, open a PR, request Codex review and resolve
findings. Then update release metadata and documentation and verify the final head.

## Concrete Steps

Run from the worktree:

    npm ci
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features

Use disposable PostgreSQL/Redis services for integration tests and a local gateway
for Computer Use. Keep logs outside tracked files unless intentionally summarized.

## Validation and Acceptance

Both required headers with a valid key succeed under managed routing without
OIDC requests. Invalid headers/keys fail; policy remains enforced; upstream sees
only configured credentials. Restoring Entra rejects invalid JWTs again. Settings
survive persistence and reject Apigee ambiguity. Desktop/mobile UI can enable,
identify and disable the mode without losing saved Entra settings. All mandatory
checks and the final PR review must be assessed.

## Idempotence and Recovery

Use disposable service data only. SQLx applies the additive migration once.
Disable the new flag before rolling back a binary; the old binary ignores the
extra column. Reverting the migration requires dropping its check and column,
only after disabling the mode. Repeated builds and tests are safe. Preserve the
original checkout and unrelated user work.

## Artifacts and Notes

Issue: https://github.com/sarattha/relayna-gateway/issues/114
PR and test evidence will be recorded on completion.

## Interfaces and Dependencies

No new runtime dependencies. Extend the existing auth settings GET/PATCH API,
shared runtime, and singleton PostgreSQL table. Existing configured Relayna header
(including `x-litellm-key`) contains an `rk_live_...` key, never a LiteLLM key.
