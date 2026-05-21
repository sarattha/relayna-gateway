# Move Control Paths Under /admin-ui

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Relayna Gateway runs in an AKS cluster where another gateway owns root ingress
paths such as `/`, `/healthz`, `/readyz`, and `/metrics`. After this change,
operators can route the Relayna control listener through one ingress prefix,
`/admin-ui/*`, without exposing or depending on root-level control paths.

Runtime/proxy traffic is not part of this change. Internal service routes such
as `/services/*`, legacy service aliases such as `/summary`, and OpenAI-style
proxy routes such as `/v1/chat/completions` keep their existing semantics.

## Progress

- [x] (2026-05-21) Read repository guidance, compatibility skills, and current
  route inventory.
- [x] (2026-05-21) Recorded the compatibility decision and implementation plan.
- [x] (2026-05-21) Updated Gateway API routes and route tests.
- [x] (2026-05-21) Updated Admin portal API calls and static contract tests.
- [x] (2026-05-21) Updated freeze perimeter tests, Kubernetes probe paths, and
  documentation.
- [x] (2026-05-21) Ran verification and recorded results.

## Surprises & Discoveries

- Observation: `/services/*` is runtime/proxy traffic, not a control route.
  Evidence: `gateway-core/src/routing.rs` resolves `/services/*` as
  `Route::ServiceWildcard`, and `gateway-proxy/src/pingora_plane.rs` uses it
  for persisted internal service routing.

## Decision Log

- Decision: Move control/admin routes to `/admin-ui/*` and remove old
  root-level control routes from the `gateway-api` Axum route table.
  Rationale: The target AKS ingress cannot dedicate root paths to Relayna
  Gateway because another gateway owns those paths.
  Date/Author: 2026-05-21 / Codex.

- Decision: Keep runtime/proxy routes unchanged.
  Rationale: The requested deployment only needs admin/control traffic through
  this ingress prefix, and moving runtime routes would require broader policy,
  service registration, usage, and proxy compatibility changes.
  Date/Author: 2026-05-21 / Codex.

## Outcomes & Retrospective

Implemented the `/admin-ui` control base path move. Root-level control routes
now return 404 from `gateway-api`, while runtime/proxy route semantics remain
unchanged. Kubernetes probes and ServiceMonitor paths now use the same
`/admin-ui` base path. `mkdocs build --strict` could not run locally because
`mkdocs` is not installed in this environment.

## Context and Orientation

`crates/gateway-api/src/app.rs` owns the Axum control-plane route table,
including health, readiness, metrics, admin APIs, guardrail catalog APIs, and
the embedded static Admin portal. The portal assets live under
`crates/gateway-api/src/static/admin-ui/` and currently call root-level
`/admin/*` and `/readyz` endpoints.

`tests/admin-ui.test.mjs` statically pins portal-critical views and API calls.
`tests/freeze-v0.0.9-perimeter.test.mjs` pins the production freeze perimeter,
including public control route inventory and proxy route semantics.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.10`; production freeze
baseline `v0.0.9`. This intentionally breaks released root-level control route
paths by replacing them with `/admin-ui/*` paths. Runtime/proxy routes remain
unchanged.

Touched freeze surfaces: public HTTP control routes, admin API endpoint
contracts, Admin portal endpoint calls, docs, and route inventory tests.

## Plan of Work

Update `crates/gateway-api/src/app.rs` so control routes are registered only
under `/admin-ui`. Register specific Admin portal API routes before the
`/admin-ui/{*path}` static catch-all.

Update `crates/gateway-api/src/static/admin-ui/app.js` so portal fetches use
`/admin-ui/admin/*` and `/admin-ui/readyz`.

Update Rust route tests in `app.rs` to exercise the new paths and prove old
root-level control paths return `404 Not Found`.

Update `tests/admin-ui.test.mjs` and
`tests/freeze-v0.0.9-perimeter.test.mjs` to pin the new control path contract
while preserving proxy route semantics for `/services/*` and other runtime
routes.

Update README and docs that mention `/admin/*`, `/healthz`, `/readyz`,
`/metrics`, or control-plane `/v1/guardrails*` URLs so operator examples use
the new `/admin-ui` base path.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    node tests/admin-ui.test.mjs
    node tests/freeze-v0.0.9-perimeter.test.mjs
    bash .codex/skills/code-change-verification/scripts/run.sh

If the helper script fails, fix the failure and rerun it so formatting,
clippy, and tests pass in sequence.

## Validation and Acceptance

Acceptance criteria:

- `GET /admin-ui/healthz` succeeds.
- `GET /admin-ui/readyz` succeeds.
- `GET /admin-ui/metrics` succeeds.
- `GET /admin-ui/admin/keys` requires an operator token and succeeds with a
  valid operator token.
- `GET /admin-ui/v1/guardrails` preserves the existing guardrail catalog auth
  behavior.
- Old root control paths `/healthz`, `/readyz`, `/metrics`, `/admin/keys`, and
  `/v1/guardrails` return `404`.
- `/admin-ui/app.js` and `/admin-ui/app.css` still serve static assets.
- `/services/*`, `/summary`, `/translation`, `/ocr`, `/embeddings`,
  `/v1/chat/completions`, `/v1/responses`, and `/providers/openai/*` retain
  current proxy route semantics.

## Idempotence and Recovery

All edits are source-only. If verification fails, inspect the failing route or
contract test, update the relevant route path or assertion, and rerun the
failed command. The change does not add migrations, Redis state, or external
side effects.

## Artifacts and Notes

Verification:

    node tests/admin-ui.test.mjs
    node tests/freeze-v0.0.9-perimeter.test.mjs
    bash .codex/skills/code-change-verification/scripts/run.sh

All commands passed. `mkdocs build --strict` was attempted but failed with
`command not found: mkdocs`.

## Interfaces and Dependencies

Final control route interface:

- Static portal: `/admin-ui`, `/admin-ui/app.js`, `/admin-ui/app.css`,
  `/admin-ui/{*path}`.
- Admin APIs: `/admin-ui/admin/*`.
- Health/readiness/metrics: `/admin-ui/healthz`, `/admin-ui/readyz`,
  `/admin-ui/metrics`.
- Guardrail control APIs: `/admin-ui/v1/guardrails` and
  `/admin-ui/v1/guardrails/test`.

No database, Redis, environment variable, or deployment manifest changes are
required for the route move itself.
