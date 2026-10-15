# Accessa verified token forwarding and PUT provisioning

This living ExecPlan follows `PLANS.md`. Maintain Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective as work proceeds.

## Purpose / Big Picture

Resolve #133 and #134. Operators can opt an Accessa registration into forwarding
the Gateway-verified user JWT under an adjustable header (default x-access-token)
for the Accessa Broker's on-behalf-of exchange to Keystone. Accessa provisioning
includes PUT for feedback and settings, without silently widening existing
registrations. Verify create/edit/save/readback in Admin UI with Computer Use.

## Progress

- [x] (2026-10-15) Read repository instructions, design manifesto, skills and UI guidance; created feat/accessa-token-forward-put from origin/main.
- [x] (2026-10-15) Confirmed latest released boundary v0.1.44, current credential stripping, strict JSON access readers, existing PUT runtime support and missing provisioning methods.
- [x] (2026-10-15) Implemented configurable opt-in, reserved/dynamic-header validation, verified-JWT-only forwarding, sensitive HeaderValue and response-header stripping.
- [x] (2026-10-15) Added React controls, Accessa PUT/PATCH/DELETE defaults, fail-closed bootstrap method validation, six provisioning tests and HTTP/WS/auth/leakage regression coverage.
- [ ] Full verification in progress. Computer Use desktop/390px create/edit/save/readback, default-off/custom header/disable/reserved-name rejection, Traffic and Virtual keys checks passed. UI suite, typecheck and 54 React tests passed.
- [ ] Open PR, link it to this thread, monitor automatic Codex review and address findings without requesting another review.
- [ ] After review fixes, bump patch version, update CHANGELOG/release metadata/docs, rerun required checks and merge after required review/checks pass.

## Surprises & Discoveries

The actual FigJam board contains newer requirements than the original Accessa
transport. Its page 05 S9 explicitly requests verified-token forwarding and
page 08 uses PUT for feedback/settings. The current development registration
uses tara/cortex-frontend and lacks PUT; app migrations are outside this task.
Gateway diagnostics record selected metadata, not arbitrary header snapshots.
Existing authentication-profile revisions bind the complete saved access policy.
The first mandatory stack passed workspace tests but audit found nine advisories
in existing Rustls/Quinn dependencies. Update only Rustls 0.23.46 and Quinn Proto
0.11.18 (plus required lockfile edges), then rerun without new vulnerability
exceptions. Deny additionally reports archived unsafe-libyaml, with no patched
upstream version. A maintenance-only exception, dated 2026-11-15, matches the
existing Pingora dependency exception policy; Gateway does not parse caller YAML.
Computer Use also exposed a pre-existing hardcoded UI version 0.1.43; align it
with the authorized patch-version bump after review.

## Decision Log

- (2026-10-15, Codex) User authorizes implementation, UI testing, PR creation,
  automatic-review monitoring, fixes, release metadata updates and merge. No
  production rollout or repository Actions setting change is implied.
- (2026-10-15, Codex) Compatibility baseline v0.1.44. Add an optional strict
  forwarding configuration with a retained header name when disabled; omitted
  configuration retains credential stripping. Keep raw JWTs in the request
  lifetime only, never in Redis sessions, database rows, diagnostics or Debug.
  Old strict readers fail closed on new configuration; upgrade all replicas
  before configuring it and remove the configuration/drain before rollback.
- (2026-10-15, Codex) Reject reserved/trusted/framing/credential header names and
  dynamic Relayna-key-header collisions. Require verified JWT source, never
  infer verification from an arbitrary incoming header or Apigee assertion.
- (2026-10-15, Codex) Required audit/deny findings necessitate patched Rustls and
  Quinn lock versions plus a documented, dated maintenance-only unsafe-libyaml
  exception. Do not suppress the nine vulnerability advisories.
- (2026-10-15, Codex) Provision new Accessa app routes with GET/POST/PUT/PATCH/DELETE.
  Existing bootstrap registrations missing required methods fail clearly and
  require an explicit supported admin API update, preserving operator intent.

## Outcomes & Retrospective

Implementation and first Computer Use checks are complete. Full stack and automatic
PR review remain pending. Preserve the four pre-existing untracked user documents.
Desktop and mobile screenshots are in the local T3 browser artifacts directory;
no live Accessa deployment has been changed.

## Context and Orientation

Root: /Users/jobz/Works/relayna-gateway. Core endpoint_access.rs owns serialized
identity/Accessa configuration. Proxy pingora_plane.rs prepares upstream headers;
accessa_runtime.rs verifies endpoint identity and admits connections/turns. Store
persists access JSON and Redis session snapshots; neither should contain raw
JWTs. Admin UI main.ts owns service forms and React controls use components/ui.
deploy/kind/bootstrap-cortex.py provisions the development registration.
crates/gateway-api/tests/accessa_e2e.rs runs real Pingora with mock BFF, adapter,
Router, agent and OIDC services against disposable PostgreSQL/Redis.

## Plan of Work

Add the core configuration and validation, integrate fail-closed request checks
and upstream/response handling, then UI controls and provisioning changes. Test
default-off, custom names, reserved collisions, spoofed copies, JWT vs Apigee,
HTTP PUT, WS upgrades and no token persistence/diagnostics. Build assets from
source. Use isolated local services for browser tests rather than changing the
shared Cortex deployment. Submit the verified implementation for automatic
Codex review, fix findings, then update patch release metadata and documentation.

## Concrete Steps

Run from the repository root:

    npm run build:admin-ui
    npm test
    npm run test:react
    python3 -m unittest discover -s deploy/kind -p 'test_bootstrap_cortex.py'
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features

Run Accessa integration tests with DATABASE_URL and REDIS_URL pointing at
isolated services. Check release metadata and strict docs generation with the
repository's existing helpers. Inspect required PR status and reviews through gh.

## Validation and Acceptance

Enabled HTTP/WS Accessa requests deliver exactly the verified JWT to the chosen
upstream header while Authorization remains the service credential. Spoofed
copies never substitute a token. Invalid identity and forbidden names fail
closed. Disabled registrations strip configured copies. Response headers,
diagnostics, database and Redis snapshots contain no token. PUT feedback/settings
reach the adapter while denied methods/auth remain denied. UI exposes default,
custom header, validation and persisted save/readback at desktop and narrow
widths. All mandatory checks pass and review findings are resolved before merge.

## Idempotence and Recovery

Keep user files untouched and commits limited to this plan and task changes.
Use temporary disposable services and avoid shared database mutations. Observe
remote issue/PR state before retrying writes. Do not bypass failed required
checks/reviews or change intentionally disabled Actions. Remove the new
forwarding configuration with the new binary before an older-binary rollback.

## Artifacts and Notes

Figma source: https://www.figma.com/board/dpgtpunTKAkuIKyK3e7k0F/accessa-phase1
Issues: https://github.com/sarattha/relayna-gateway/issues/133 and /issues/134.

## Interfaces and Dependencies

Use existing service create/PATCH APIs, endpoint Entra verification, auth-profile
revision handling and Admin UI asset URLs. No new route, virtual key format,
provider credential, charging semantics, or production dependency is introduced.
