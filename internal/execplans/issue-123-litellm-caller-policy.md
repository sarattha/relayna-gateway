# Independent LiteLLM caller authentication and path denies

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Issue #123 lets applications use their existing LiteLLM bearer credential for allowlisted APIs while the operator disables the LiteLLM dashboard and blocks selected paths. LiteLLM validates the caller credential; Gateway never substitutes its own key.

## Progress

- [x] (2026-09-29) Inspect issue, released code and repository guidance; create isolated `issue-123` worktree.
- [x] (2026-09-29) Implement persisted settings, policy, proxy and UI.
- [x] (2026-09-29) Add regression tests; pass the complete mandatory verification stack, workspace build, UI checks, strict docs build and Computer Use checks.
- [x] (2026-09-29) Bump version to 0.1.40 and update CHANGELOG, manifesto and operational documentation.
- [ ] Open PR, monitor reviews/checks, merge and release.

## Surprises & Discoveries

The original worktree contains unrelated migration-command work. All changes for this issue use `/Users/jobz/Works/relayna-gateway-issue-123`. Local linking requires `DEVELOPER_DIR=/Library/Developer/CommandLineTools` because Xcode has an unaccepted license. Intermittent Pingora startup failures came from native macOS certificate loading (`InvalidCert`, Keychain I/O error); `SSL_CERT_FILE=/etc/ssl/cert.pem` uses the system CA bundle without disabling TLS verification. Existing trusted-ingress admin access depends on UI exposure and can use a Gateway service credential. Existing direct routes already support caller credentials and keyless Traffic records (no per-key usage row).

## Decision Log

- 2026-09-29: Preserve `gateway` authentication as the default and add explicit `litellm_bearer`; use an additive SQL migration against v0.1.39. User authorized this scoped exception in issue #123.
- 2026-09-29: Denies cover wildcard passthrough (including trusted ingress) and canonical routes when configured direct. Managed canonical routes, registered services and the separate authenticated operator UI proxy remain outside scope.
- 2026-09-29: Reject percent escapes, repeated slashes, backslashes and dot path segments within passthrough scope. Match only URI paths; do not decode or rewrite caller paths. Exact denies include an optional terminal slash to prevent router slash redirects bypassing an exact rule.
- 2026-09-29: Caller mode conflicts with route authentication profiles, endpoint Entra policy, and inherited gateway Entra/Apigee or unverified-bearer requirements. Fail closed with a stable configuration error; an explicit route skip of inherited identity may permit caller mode. Operator-only exposure remains unavailable without operator identity.

## Outcomes & Retrospective

Real Pingora upstream-mock test and UI form persistence tests pass. Computer Use verified login, desktop and 390px layouts, Providers save/reload, Overview, Audit Log, and keyboard navigation. Shortened option labels remove desktop truncation. All mandatory verification commands passed, including 392 Nextest tests with PostgreSQL/Redis available. The workspace build, UI build/tests/typecheck/React coverage, strict docs build and release metadata checks passed. Publishing remains.

## Context and Orientation

`crates/gateway-core/src/route_settings.rs` owns framework-independent settings and path eligibility. `crates/gateway-store/src/postgres.rs` persists the singleton settings row. `crates/gateway-proxy/src/pingora_plane.rs` resolves routes, authenticates and selects upstream credentials. `crates/gateway-api/admin-ui/src/main.ts` renders the operator form. Virtual keys are Gateway-issued identities; caller mode does not invent one and uses existing keyless Traffic recording. Registered services route to independent upstreams and are out of scope.

## Compatibility Boundary

Latest release is v0.1.39 (6746a88). Add columns with `gateway` and empty-deny defaults; existing callers retain authentication behavior. Denies are explicitly configured. Ambiguous wildcard paths fail closed. Canonical direct routes share denies only, retaining their own enable/method/limits configuration. Roll back only after clearing new denies and switching caller mode to gateway, since old binaries cannot enforce the new policy. Deploy all pods before enabling it.

## Plan of Work

Add authentication mode and blocked paths to core types and validation; migrate and read/write them in the store. Apply deny checks before passthrough shortcuts, then select caller credentials only in explicit mode. Add form labels/help and persistence tests. Add core path cases, database round trips and real upstream mock integration tests. Update manifesto, operational/API docs, changelog and version to 0.1.40. Build static assets, run checks and visually test desktop/mobile through Computer Use. Open a PR and monitor CI/review, fixing findings; request at most one additional Codex review if fixes are required. Merge after checks/review, tag the merged commit and monitor release publishing.

## Concrete Steps

From the isolated worktree:

    cargo fmt --all
    SSL_CERT_FILE=/etc/ssl/cert.pem DEVELOPER_DIR=/Library/Developer/CommandLineTools \
      DATABASE_URL=postgres://postgres:postgres@127.0.0.1:55432/relayna_test \
      REDIS_URL=redis://127.0.0.1:56379 \
      bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    npm ci
    npm run build:admin-ui
    npm test
    npm run typecheck:admin-ui
    npm run test:react:coverage
    python3 scripts/validate-release-metadata.py v0.1.40

Use local PostgreSQL/Redis test services for integration coverage; no production credentials or services are required.

## Validation and Acceptance

An upstream mock must receive the caller key, never the configured service key. Missing/malformed credentials stop locally; upstream 401/403 responses pass through without retry escalation. UI exposure toggles do not change API credential selection. Denies win over wildcard allows and trusted/direct branches, including query strings and ambiguous path attempts; local denial is 403 `policy_denied`. Verify custom credential header translation, no duplicate secrets, profile conflicts, persisted reload and next-request policy changes. Existing operator proxy and managed route tests remain passing. UI save/reload and desktop/mobile layout must be verified through Computer Use.

## Idempotence and Recovery

SQLx records the forward migration once. Tests use isolated databases/fixtures and restore shared settings when applicable. Rerun the full fail-fast verification after any failure. Do not modify the original dirty worktree. Do not delete remote branches/tags or undo unrelated work. Release workflow validates version metadata before publishing.

## Artifacts and Notes

Issue: https://github.com/sarattha/relayna-gateway/issues/123

## Interfaces and Dependencies

Add `authentication_mode: gateway | litellm_bearer` and `blocked_paths: string[]` to the existing GET/PATCH `/admin-ui/admin/providers/litellm-passthrough` contract. Empty denies preserve eligibility. Exact paths and one trailing `*` are supported; no regex or arbitrary glob. Preserve streaming, upstream authority, timeouts, correlation and header redaction through existing Pingora helpers.
