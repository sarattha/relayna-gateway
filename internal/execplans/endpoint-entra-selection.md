# Per-endpoint Entra verification

Maintain this living ExecPlan according to `PLANS.md`.

## Purpose / Big Picture

An operator can require Entra on any registered service or built-in proxy route,
including LiteLLM passthrough, while other endpoints in the same instance use
virtual keys without Entra. Audience, scopes, roles, groups and signed Apigee
acceptance are selected by endpoint. Tenant, issuer, JWKS and key header remain
shared trust configuration. Admin/operator login is outside this request-plane change.

## Progress

- [x] (2026-09-16) Inspect routing/authentication and released compatibility boundary.
- [x] Implement endpoint selection, persisted built-in route policies and scoped/audited admin API.
- [x] Add UI controls and regression/E2E tests. LLVM maps 64/64 changed executable lines for this extension (100%); full PR 675/679 (99.41%).
- [x] Verify using Computer Use at desktop and 390px, capture screenshots, pass mandatory stack (359 Nextest tests, zero skipped), and prepare update to draft PR119.

## Surprises & Discoveries

Direct canonical LiteLLM and trusted-ingress passthrough currently return before
endpoint verification. Verification must precede these returns. Canonical aliases
must resolve to the same identity policy, and service aliases must keep theirs.

## Decision Log

2026-09-16: Preserve v0.1.36 behavior for endpoints without an explicit override.
Add explicit skip_entra to the branch-local EndpointAccess configuration; it is
mutually exclusive with an Entra policy or Accessa binding. Existing deployments
retain their authentication until configured. User authorized this extension and
production freeze exception; no additional approval is required.

2026-09-16: Persist built-in route policies in an additive table keyed by canonical
route, expose a scoped/audited admin API and load policy before authentication.
Registered services retain their existing access field. Required Entra uses
Authorization plus the configured Relayna virtual-key header, including canonical
LiteLLM passthrough. Raw LiteLLM credentials are not accepted instead of an Entra
token on a protected route. Trusted ingress still uses its established credential
contract, but must first pass required endpoint identity verification.

## Context and Orientation

Work in /Users/jobz/Works/relayna-gateway. Core endpoint_access.rs owns claims and
configuration; route_settings.rs owns route store contracts. PostgresStore persists
policies. pingora_plane.rs selects matched route/service before authentication.
app.rs provides scoped, audited admin endpoints. Admin UI source main.ts remains
the source of truth; regenerate checked-in assets.

## Compatibility Boundary

Latest release v0.1.36. Additive table and API; absent override preserves released
behavior. skip_entra is explicit and does not disable key policy, rate or budget
checks. Accessa always requires its endpoint Entra policy. No credential/header or
public path changes for unchanged registrations.

## Plan of Work

Extend EndpointAccess validation, add a finite built-in route identity catalog,
store lookup/list/update methods, additive migration and admin handlers. Select
route identity before passthrough early returns, verify required policy once,
and choose key-only authentication when explicitly disabled. Reuse identity UI
fields for route and service controls. Add real proxy tests for mixed modes,
audience isolation, aliases and bypass denial, plus API persistence/auth tests.

## Validation and Acceptance

Use disposable PostgreSQL on15449 and Redis16449. npm run build:admin-ui and npm
test must pass; run .codex/skills/code-change-verification/scripts/run.sh with
DATABASE_URL/REDIS_URL and SSL_CERT_FILE=/etc/ssl/cert.pem. LLVM changed production
Rust coverage must exceed98%, without excluding error paths. Required routes deny
missing/wrong JWTs; disabled routes succeed with a valid virtual key and no JWT;
invalid keys remain denied. Saved UI controls survive reload; capture screenshots
through Computer Use. Push existing codex/accessa-channel-websockets branch and
update draft PR119.

## Idempotence and Recovery

Migration is additive; clear a route override to restore legacy behavior. Do not
change production services. Disposable tests can rotate the local operator token;
rebootstrap the demo token after testing if necessary. Preserve unrelated files.

## Outcomes & Retrospective

Runtime and UI implemented. Mixed Entra/key-only services, aliases, canonical direct passthrough, trusted ingress, corrupt storage and admin API authorization are verified. The full verification stack, strict docs build and UI build/tests passed. Screenshots cover mixed route/service policies and the persisted audience editor. Changes are ready for the existing draft PR119.
