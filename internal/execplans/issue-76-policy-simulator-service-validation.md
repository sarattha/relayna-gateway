# Issue #76 Policy Simulator Service Validation

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This plan follows `/Users/jobz/Works/relayna-gateway/PLANS.md`.

## Purpose / Big Picture

Operators using the Admin UI policy simulator should not get stale or
misleading LiteLLM denial results after selecting an internal service. When a
service simulation has an incomplete or mismatched path, the UI blocks the dry
run before it reaches `/admin-ui/admin/policy/simulate`, clears any old result,
and tells the operator how to enter a concrete service path.

## Progress

- [x] (2026-06-20 16:10Z) Read issue #76, confirmed there are no issue comments, and selected the validation-only approach.
- [x] (2026-06-20 16:10Z) Created branch `codex/issue-76-policy-simulator-service-validation`.
- [x] (2026-06-20 16:15Z) Update Admin UI source validation.
- [x] (2026-06-20 16:16Z) Regenerate checked-in Admin UI assets.
- [x] (2026-06-20 16:15Z) Update static Admin UI tests.
- [x] (2026-06-20 16:17Z) Run required verification: `npm run build:admin-ui`, `npm test`, and `bash .codex/skills/code-change-verification/scripts/run.sh`.

## Surprises & Discoveries

- Observation: Admin UI service records expose `route_pattern` and allowed
  methods, but not concrete service operation paths.
  Evidence: `ServiceResponse` contains `route_pattern`, not endpoint catalog
  data, so auto-filling paths such as `/hi` would be invented UI behavior.

## Decision Log

- Decision: Use validation-only client behavior and leave backend simulator API
  unchanged.
  Rationale: The issue is misleading UI state. Blocking incomplete service
  simulations is sufficient and avoids invented service endpoint paths.
  Date/Author: 2026-06-20 / Codex.

- Decision: Compatibility boundary is latest release tag `v0.1.13`; no shim or
  migration is required.
  Rationale: This change does not alter public routes, response shapes,
  persisted data, provider routing, or runtime policy decisions.
  Date/Author: 2026-06-20 / Codex.

## Outcomes & Retrospective

Implemented validation-only service simulator handling. The Admin UI now clears
stale simulator output before validation and before a new request, blocks
wildcard or incomplete service paths, and reports selected-service/path
mismatches before calling the backend simulator. Generated assets and static
tests were updated, and the full gateway verification script passed.

## Context and Orientation

The Admin UI source of truth is
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/src/main.ts`.
Generated assets are checked in under
`/Users/jobz/Works/relayna-gateway/crates/gateway-api/src/static/admin-ui/` and
must be regenerated with `npm run build:admin-ui`.

The policy simulator form submits to `/admin-ui/admin/policy/simulate`.
`simulatePolicy` currently treats `provider === "internal-service"` or paths
beginning with `/services/` as service mode. It only blocks `/services/*`,
which allows `/services/` to resolve as a default LiteLLM route if other fields
remain defaulted.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.13`; Admin UI validation-only
change. No public API, response shape, persisted schema, Redis state, provider
routing, or Relayna runtime contract changes.

## Plan of Work

Update `simulatePolicy` to clear the displayed simulation result before
validation and before request submission. Add service path validation that
rejects wildcard paths, incomplete `/services` paths, selected-service requests
that do not use `/services/<service>`, and mismatches between the selected
service and the path service segment.

Keep `bindPolicySimulatorControls` behavior compact and unchanged except where
needed to support the new validation. Update `tests/admin-ui.test.mjs` to pin
the generated JavaScript contract. Regenerate static assets from the Vite
source package.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance:

- Selecting a service with `/services/` blocks before the API call.
- Blocked validation clears the result panel to `No simulation run.`.
- Concrete paths such as `/services/mock-hi-service/hi` still submit with
  `service_name: "mock-hi-service"`.
- The backend simulator route remains unchanged.

## Idempotence and Recovery

The UI source edit is deterministic. If generated assets become stale, rerun
`npm run build:admin-ui`. Tests and verification commands are safe to rerun
after fixing any reported failures.

## Artifacts and Notes

Issue: `https://github.com/sarattha/relayna-gateway/issues/76`.

## Interfaces and Dependencies

No new runtime dependencies, environment variables, backend routes, request
fields, or response fields are introduced.
