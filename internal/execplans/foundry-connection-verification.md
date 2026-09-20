# Verify Azure Foundry connections from the gateway

## Purpose and acceptance

Operators can check a saved Azure Foundry provider from Providers. The gateway
obtains a fresh Azure token using the same identity implementation as forwarding,
then issues a read-only GET of the project's agents. Results distinguish identity
setup, network failures and Foundry permissions, include actionable remedies,
and identify the gateway instance and checked configuration revision. No agent
runs, credentials, agent listings or raw Azure errors appear in diagnostics.
Disabled saved providers can be checked without enabling traffic.

## Progress

- [x] 2026-09-20: Inspected implementation, skills and Microsoft REST/RBAC docs.
- [x] 2026-09-20: Implemented fresh identity diagnostics, protected API and minimal check dialog.
- [x] 2026-09-20: Focused mock E2E and unit tests pass; 44 React tests pass with 100% coverage.
- [x] 2026-09-20: Computer Use verified success, 403, removed workload file and recovery; captured two guide screenshots; strict docs build passes.
- [x] 2026-09-20: All ten verification steps pass; 388 nextest tests, UI/static/type/build checks, strict docs and 98.824% changed Rust coverage.

## Context and implementation

Work in /Users/jobz/Works/relayna-gateway. gateway-proxy/src/foundry.rs owns
Azure token acquisition; gateway-store reads saved secrets; gateway-api exposes
admin provider routes and the embedded React UI. Add an admin-only saved-config
lookup including disabled providers, leaving forwarding's enabled check intact.
POST /admin-ui/admin/providers/{id}/verify-connection requires providers:update.
The response is ephemeral, with safe fixed diagnostic codes/messages and HTTP
status where available. Audit the check using existing operator audit conventions.

Use GET PROJECT/agents?api-version=v1&limit=1 (no pagination or redirects), fresh
uncached tokens, bounded token/probe body reads and existing 5s connect/10s total
HTTP timeouts. Successful read validates identity and project read access only;
it does not prove invocation, tools, model quota or every pod. The UI explicitly
states these boundaries and allows retry after repairs. Save edits before checking.

## Compatibility and Decision Log

2026-09-20: Latest release v0.1.37; Foundry is unreleased in v0.1.38. This is an
additive admin API; no schema, proxy route or caller-authentication change. Reuse
and directly refine the unreleased token helper, retaining generic runtime errors.

2026-09-20: GET agents is documented in Microsoft's current REST reference
https://learn.microsoft.com/en-us/rest/api/microsoft-foundry/aiproject . A 403
read probe is partial verification, not proof that identity or invocation is
broken. RBAC source: https://learn.microsoft.com/en-us/azure/foundry/concepts/rbac-foundry .
Do not recommend broadening production roles merely to pass this diagnostic.

## Surprises & Discoveries

Computer Use caught a wrong API namespace in the new button wiring; fixed and covered by a regression assertion.

Foundry Agent Consumer has endpoint invocation rights without project read rights.
The existing token cache could hide a broken federated token mount, so checks
must bypass it. Disabled providers are intentionally omitted by runtime lookup.

## Validation and recovery

Extend isolated mock Foundry end-to-end tests (PostgreSQL 15432, Redis 16379) to
assert fresh token exchange, only read probes, all staged outcomes, disabled
provider checks and credential redaction. Add React tests for pending, success,
partial/failure, retry and HTTP errors. Run npm test, npm run test:react:coverage,
npm run typecheck:admin-ui, npm run build:admin-ui; run the mandatory
.codex/skills/code-change-verification/scripts/run.sh with test DATABASE_URL and
REDIS_URL. Build docs with .tmp-mkdocs-venv/bin/mkdocs build --strict. Capture a
real local UI screenshot via Computer Use using fictional mock configuration.
Remove only fixtures created for this task. Changes require no migration or
configuration rollback; checks neither save settings nor enable providers.

## Outcomes & Retrospective

Delivered a protected, audited saved-connection diagnostic and minimal React
results dialog. All three Azure identity methods share the production acquisition
path. Real mock E2E plus Computer Use cover permission failure, missing workload
file and recovery. Documentation includes two screenshots. No live Azure tenant
was available; agent invocation and other instances are explicitly outside this
read-only check. No schema or released forwarding behavior changed.
