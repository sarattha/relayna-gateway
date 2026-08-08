# Entra Admin and Service Owner Monitoring

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

After this work, Relayna Gateway has one browser portal at `/admin-ui` for both
administrators and registered service owners. Humans sign in through a
confidential OpenID Connect BFF flow backed by Microsoft Entra ID; the browser
receives only an opaque, HttpOnly session cookie. Relayna membership and
service-role records remain authoritative after Entra proves identity.

Administrators can approve, block, restore, and assign members; bind Owner or
Viewer access to registered services; and register workload identities for
service-to-service monitoring. Service owners can see only the traffic,
failures, logs, endpoints, and usage for services to which they are assigned.
The same boundary is available programmatically under
`/owner/v1/services/{service_id}/*`. Existing Admin UI pages and existing
`/admin-ui/admin/*` APIs remain available, and the operator token remains a
break-glass authentication path.

## Progress

- [x] (2026-08-08) Read the design manifesto, repository Admin UI guidance,
  mandatory implementation skills, and Arcweft OIDC reference architecture.
- [x] (2026-08-08) Established `v0.1.23` as the released compatibility
  boundary and created branch `codex/entra-owner-monitoring`.
- [x] (2026-08-08) Chose one branch and one PR so schema, API, UI, development
  identity provider, regression coverage, and documentation are reviewed as
  one coherent security boundary.
- [x] (2026-08-08) Added access-domain types and durable migrations for members, memberships,
  workload identity bindings, OIDC login transactions, and browser sessions.
- [x] (2026-08-08) Added confidential OIDC BFF login, callback, session, CSRF, and logout
  endpoints with operator-token break-glass compatibility.
- [x] (2026-08-08) Added admin member, service membership, and managed identity APIs.
- [x] (2026-08-08) Added service-owner monitoring and log/error APIs with mandatory
  service-scoped authorization.
- [x] (2026-08-08) Added managed-identity bearer-token authorization with audience, app-role,
  tenant, client/object ID, and exact Relayna service binding checks.
- [x] (2026-08-08) Expanded Admin UI navigation, Entra login/pending-access states, workspace
  switcher, member management, managed identities, and owner dashboards.
- [x] (2026-08-08) Added the local development OIDC fixture and documented test identities.
- [x] (2026-08-08) Added regression tests and retained 95.25% all-features Rust workspace
  line coverage without production-source exclusions.
- [x] (2026-08-08) Regenerated checked-in Admin UI assets and passed all Node UI contract tests.
- [x] (2026-08-08) Passed the mandatory repository verification stack:
  formatting, Clippy, workspace tests, audit, deny, machete, 299 nextest tests,
  Trivy, Gitleaks, and Semgrep.
- [x] (2026-08-08) Ran Computer-driven desktop end-to-end journeys against the
  local development OIDC flow for every new view and recorded the evidence.
- [x] (2026-08-08) Ran an additional real-browser 390 x 844 service-owner
  journey covering OIDC, responsive navigation, services, metrics, and errors;
  the final dashboard had no console errors. Computer could not repeat this
  viewport after macOS auto-locked, so the fallback is identified explicitly.
- [x] (2026-08-08) Commit, push, open the PR for review, and monitor through the first Codex
  review; address actionable findings before handoff.
- [x] (2026-08-08) Opened PR #99 and received the first Codex review.
- [x] (2026-08-08) Addressed all five first-review findings and reran the full
  verification and coverage gates on a fresh database.
- [x] (2026-08-08) Pushed the review-fix commit, replied to and resolved all five
  review threads.
- [ ] Confirm the final CI run after hardening the development OIDC readiness
  ceiling for parallel GitHub test load.

## Surprises & Discoveries

- Observation: Relayna already contains a production-grade Entra JWT verifier
  for front-door and workload authorization, including OIDC discovery, JWKS
  caching, audience/issuer/tenant checks, roles, scopes, and groups.
  Evidence: `crates/gateway-core/src/entra.rs`.
- Observation: The Admin UI already uses hash routing under `/admin-ui`, so all
  browser views can stay under the stable asset route without server-side deep
  route fallback changes.
  Evidence: `crates/gateway-api/admin-ui/src/main.ts` and
  `crates/gateway-api/src/app.rs`.
- Observation: The workspace already enforces and has previously achieved
  95.00% all-features Rust line coverage with real PostgreSQL and Redis.
  Evidence: `internal/execplans/coverage-and-local-inspection.md`.
- Observation: Current audit rows require an operator-token foreign key.
  Browser administrators therefore need a first-class member audit identity;
  impersonating them as operator tokens would weaken traceability.
  Evidence:
  `crates/gateway-store/migrations/20260522000100_operator_scopes_audit_events.sql`.
- Observation: An existing healthy PostgreSQL and Redis coverage pair is
  available on loopback ports 25432 and 26380. The new durable access contract
  test passes against the migrated PostgreSQL schema and validates activation,
  exact service membership, managed identities, one-time OIDC transactions,
  opaque sessions, and revocation.
  Evidence: `portal_access_state_is_durable_scoped_and_revocable` in
  `crates/gateway-store/tests/control_state_integration.rs`.
- Observation: The first complete coverage measurement was 92.45%. Exercising
  the public portal routes, security failure paths, real development OIDC
  authorization-code flow, and managed-identity client-credentials flow raised
  full-workspace line coverage to 95.19% without exclusions or a threshold
  change.
  Evidence: `cargo llvm-cov --workspace --all-features --fail-under-lines 95
  --summary-only` passed on 2026-08-08 with 24,580 instrumented lines and 1,183
  missed lines.
- Observation: Computer caught a startup-time temporal-dead-zone error that
  static Admin UI contracts did not expose: `viewFromHash()` read
  `state.workspace` while `state` was still being initialized. Initializing the
  view first and resolving the hash after state construction fixed the blank
  portal; a source contract now guards the initialization order.
  Evidence: `tests/admin-ui.test.mjs` and the development OIDC desktop journey.
- Observation: The two PostgreSQL-backed gateway integration binaries mutate
  shared control-plane fixtures and can race when `cargo nextest` starts them
  concurrently. Holding one PostgreSQL advisory lock for each complete workflow
  makes the existing global-fixture boundary explicit without weakening test
  assertions.
  Evidence: `postgres_admin_integration.rs` and
  `proxy_process_integration.rs` pass together under nextest.
- Observation: Computer completed the desktop journeys in Safari, including
  pending sign-in, administrator approval, Owner assignment, managed identity
  registration, service-owner sign-in, service metrics/error rendering, and
  logout. A real client-credentials token returned the scoped Orders dashboard
  and received 403 for a different service. A separate Chromium journey at
  390 x 844 verified the responsive navigation and scoped owner dashboard with
  a clean final console; macOS auto-lock prevented repeating that viewport with
  Computer itself.
  Evidence: local development OIDC issuer on port 18090 and gateway control
  listener on port 18381 using the isolated `relayna_owner_ui` database.
- Observation: The mandatory historical Gitleaks scan exposed three exact
  test-token fixtures already committed before `v0.1.23` and repeated across
  two historical commits. Adding only those exact strings to the existing
  test-fixture allowlist restored a clean scan without relaxing the general
  live-token rule.
  Evidence: `.gitleaks.toml`; Gitleaks scanned 202 commits with no leaks.
- Observation: The first Codex review found five valid gaps: unpruned abandoned
  OIDC transactions, unbound browser login state, local-only logout, incorrect
  managed-identity conflict mapping, and swallowed owner service lookup errors.
  Evidence: PR #99 review `4888905195`.
- Observation: The post-review GitHub run passed the Rust job but the security
  job's parallel nextest run allowed only one second for the development OIDC
  child process to become ready. The same test passed outside that load. A
  ten-second bounded readiness ceiling still returns immediately on success
  and makes the failure threshold appropriate for shared CI runners.
  Evidence: PR #99 security job `59048666615`; the mandatory local stack then
  passed all 299 nextest cases under parallel load.

## Decision Log

- Decision: Keep the portal at `/admin-ui` and retain hash-based browser routes.
  Keep owner APIs outside the Admin API namespace at
  `/owner/v1/services/{service_id}/*`.
  Rationale: Browser navigation and machine authorization are different trust
  surfaces. This preserves existing assets and Admin API contracts while
  making owner API intent explicit.
  Date/Author: 2026-08-08 / Codex.
- Decision: Implement OIDC as a confidential BFF. Store login transactions and
  sessions server-side, set an opaque `HttpOnly; Secure; SameSite=Lax` cookie,
  validate state, nonce, PKCE, issuer, audience, tenant, and token lifetime, and
  require CSRF for cookie-authenticated mutations. Never expose Entra tokens to
  Admin UI JavaScript.
  Rationale: This follows the Arcweft security pattern and removes bearer tokens
  from browser storage.
  Date/Author: 2026-08-08 / Codex.
- Decision: Entra proves identity; Relayna records determine pending, active,
  or blocked state, administrator capability, and service Owner/Viewer access.
  Rationale: Entra group/app-role configuration is not a substitute for exact
  gateway service ownership and reviewable local authorization.
  Date/Author: 2026-08-08 / Codex.
- Decision: Preserve operator-token authentication as an explicit break-glass
  path and preserve all released routes and response shapes. New schema and
  routes are additive; audit identity becomes a tagged principal that can
  reference either an operator token or a member.
  Rationale: `v0.1.23` operators must retain recovery access during Entra
  outages. The user authorized the production-freeze exception, but no existing
  recovery or request-plane contract needs to break.
  Date/Author: 2026-08-08 / Codex.
- Decision: Reuse the current Entra verifier for workload identities and extend
  its identity context only with optional human OIDC claims. Require the
  `gateway.monitor.read` application role plus an enabled exact service binding.
  Rationale: One verifier avoids divergent JWT validation rules, while exact
  binding prevents a tenant-wide workload token from reading every service.
  Date/Author: 2026-08-08 / Codex.
- Decision: Interpret the requested 95% as the repository's established
  `cargo llvm-cov --workspace --all-features --fail-under-lines 95` gate, not a
  narrower feature-only calculation.
  Rationale: The repository already supports this broader, reproducible metric.
  Date/Author: 2026-08-08 / Codex.
- Decision: Serialize only the two shared-database integration workflows with a
  PostgreSQL advisory lock; keep every unit and independent integration test
  parallel.
  Rationale: The race is in shared test fixtures, not production behavior, and
  the narrow lock preserves nextest speed and deterministic coverage.
  Date/Author: 2026-08-08 / Codex.
- Decision: Bind each OIDC transaction to a hashed, short-lived HttpOnly login
  cookie, create the transaction only after discovery succeeds, and prune
  expired rows transactionally before inserting a new login.
  Rationale: State, nonce, and PKCE protect the protocol, while the browser
  binding prevents login CSRF/session swapping and pruning bounds abandoned
  durable state.
  Date/Author: 2026-08-08 / Codex.
- Decision: Return the provider's discovered end-session URL from the
  CSRF-protected local logout and have the browser navigate to it.
  Rationale: The local Relayna session must be revoked first, but shared
  machines also need Entra SSO termination and reliable account switching.
  Date/Author: 2026-08-08 / Codex.

## Outcomes & Retrospective

The implementation now provides one human portal under `/admin-ui`, a
confidential Entra OIDC BFF under `/admin-ui/auth/*`, administrator member and
managed-identity controls under `/admin-ui/admin/*`, and exact service-scoped
monitoring under `/owner/v1/services/{service_name}/*`. The additive migration
creates member, membership, managed-identity, OIDC transaction, and session
state while preserving operator-token recovery and existing browser/API
contracts.

The post-review tree passes the 95% gate at 95.25% line coverage (24,733 lines,
1,174 missed), all Node Admin UI contracts, the production Vite build, and the
full mandatory repository verification stack, including 299 nextest tests on a
fresh database. Computer verified every new
desktop view and found the initialization-order defect that was fixed; a second
real-browser journey verified the service-owner experience at 390 x 844.

PR #99 is available at https://github.com/sarattha/relayna-gateway/pull/99. Its
first Codex review identified five actionable findings; all five are fixed,
covered, replied to, and resolved. The final updated CI result remains to be
recorded before this plan is closed.

## Context and Orientation

`crates/gateway-core` owns plain access-domain types and authorization
decisions. `crates/gateway-store` owns the PostgreSQL migrations and all member,
membership, workload binding, OIDC transaction, session, and service-scoped
usage queries. `crates/gateway-api` owns BFF protocol handling, cookies, CSRF,
Admin/member routes, owner APIs, and the Axum middleware boundary.

The Vite/TypeScript source in `crates/gateway-api/admin-ui` remains the UI source
of truth. `npm run build:admin-ui` regenerates the checked-in files under
`crates/gateway-api/src/static/admin-ui`. Existing `/admin-ui`,
`/admin-ui/app.js`, and `/admin-ui/app.css` contracts must remain stable.

The Arcweft references used for the OIDC shape are
`/Users/jobz/Works/arcweft/docs/operations/entra-confidential-bff.md`,
`/Users/jobz/Works/arcweft/frontend/lib/bff/auth-session.ts`, and
`/Users/jobz/Works/arcweft/frontend/lib/bff/oidc.ts`. Relayna must adapt the
pattern to Rust/Axum rather than copy the Next.js implementation.

## Plan of Work

First add the access domain and additive migration. Build store contracts before
exposing routes so authorization cannot be implemented as client-side filters.
Then add the BFF OIDC protocol and a development issuer compatible with the
production verifier. Route existing Admin handlers through a principal resolver
that accepts either an active Admin member session or the existing operator
token.

Next add admin management APIs and owner monitoring APIs. Every owner query
injects the authorized service identifier in the store layer; client-supplied
filters may narrow that set but cannot broaden it. Workload tokens pass the same
service binding decision.

Finally expand the existing Admin UI. The login page prefers Entra and keeps a
clearly labelled break-glass token option. The signed-in shell derives visible
navigation and API roots from the server session, supports admin/service-owner
workspace switching, and never renders or stores Entra tokens. Build assets,
exercise all journeys with automated contracts and Computer, measure coverage,
run the full verification skill, and publish one PR.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    npm run build:admin-ui
    npm test
    cargo test --workspace --all-features
    cargo llvm-cov --workspace --all-features --fail-under-lines 95
    bash .codex/skills/code-change-verification/scripts/run.sh

Local end-to-end testing will use isolated loopback ports recorded in Outcomes
& Retrospective and a development OIDC issuer adapted from Arcweft. Computer
will test Entra sign-in, pending access, admin approval and assignment, workspace
switching, service dashboard scoping, owner API errors, managed identities,
logout, break-glass login, and responsive/mobile navigation.

## Validation and Acceptance

An anonymous browser is redirected or invited to Entra login. First sign-in
creates a pending member without granting data access. An active Admin can
approve the member and assign Owner or Viewer access to one or more services.
The owner dashboard and API return only those services; attempts to substitute
another service ID return a stable forbidden or not-found response without
leaking its existence. Blocked members and disabled bindings lose access on the
next request.

Cookie-authenticated mutations fail without a valid CSRF header. Login callback
fails for invalid state, nonce, issuer, audience, tenant, signature, expiry, or
PKCE exchange. Managed identities fail without the required audience and app
role or without an exact enabled service binding. Existing operator tokens still
authorize the released Admin API, and existing UI pages still render.

All Rust tests, Admin UI tests, formatting, linting, security/static checks, and
the 95% coverage gate pass. Computer verifies the newly added UI at desktop and
mobile widths. The PR remains monitored until the first Codex review arrives
and every actionable comment is answered or fixed.

## Idempotence and Recovery

The migration uses additive tables, nullable audit-principal columns, unique
constraints, and `IF NOT EXISTS` where supported. OIDC login transactions and
sessions expire and are safe to prune or recreate. Local OIDC, PostgreSQL, and
Redis instances use explicit names and loopback ports and do not modify unrelated
containers. Operator-token break-glass access remains available if Entra or the
development issuer is unavailable.

## Artifacts and Notes

The static design prototype remains under
`design-prototypes/service-owner-monitoring/` as implementation reference. It
will be included in this PR only if it remains aligned with the final production
UI; otherwise it will remain an untracked local artifact and will not be staged.

## Interfaces and Dependencies

New browser/BFF endpoints are under `/admin-ui/auth/*`. New administrator access
management endpoints are under `/admin-ui/admin/members` and related nested
resources. Owner monitoring endpoints are under
`/owner/v1/services/{service_id}` for overview, usage, events/errors, logs,
endpoints, and export. Exact response types will be added in `gateway-core` and
kept framework-independent.

The feature should reuse existing workspace dependencies where possible. Any
new cryptographic or cookie dependency must have a narrow purpose, Rustls-based
networking, and direct tests for invalid and boundary cases.
