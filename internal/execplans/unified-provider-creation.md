# One provider creation flow

This living ExecPlan follows `PLANS.md` at the repository root.

## Purpose / Big Picture

Providers has one Create provider action. Its provider selector includes LiteLLM,
Internal service and Azure Foundry. Selecting Foundry displays the project endpoint
and Azure identity fields; other providers display their existing credential
settings. No separate Add Azure Foundry action remains.

## Progress

- [x] 2026-09-20: Read UI guidance and current provider/editor implementation.
- [x] Share Foundry identity fields, build unified provider dialog and remove old form.
- [x] Verify provider payloads, switching, required fields, errors and pending saves.
- [x] Validate desktop creation/edit with Computer Use and refresh setup screenshot.
- [x] 2026-09-20: Complete all ten mandatory repository checks; nextest 388/388, zero skipped.

## Context and Orientation

Source is crates/gateway-api/admin-ui/src/main.ts and src/react/foundry-editor.tsx.
Reuse the existing React Dialog/Field/Input controls. Extract shared Azure fields
so creation and editing agree. Preserve all existing POST /admin-ui/admin/providers
payload contracts, enabled state, write-only secrets and custom-header settings.
Compatibility boundary v0.1.37: UI-only entry-point consolidation, no API/runtime,
configuration or schema change. The Rust embedded asset deployment stays intact.

## Decision Log

2026-09-20, Codex: Keep one creation entry point and select the adapter within
the dialog. Share Azure identity controls with editing to avoid divergent fields.
Keep existing request bodies and server validation; this is a UI consolidation,
not a new provider contract. Clear provider-specific credential inputs when
switching types so a secret cannot accidentally go to another adapter.

## Verification

Add React tests for all provider choices and Azure methods, switching without
submitting stale secrets, errors, pending saves and dialog focus/lifecycle. Run
npm test, npm run test:react:coverage, npm run typecheck:admin-ui,
npm run build:admin-ui, `bash .codex/skills/code-change-verification/scripts/run.sh`
and `.tmp-mkdocs-venv/bin/mkdocs build --strict`. The repository script needs
PostgreSQL (`DATABASE_URL`) and Redis (`REDIS_URL`); use the isolated local
`relayna_gateway` test database and Redis DB 0, not the demonstration database.
Expected results are exit status 0 and no skipped integration tests.
Computer Use must show one create action, dynamic fields and a successful local
Foundry creation. Remove only task-created fixtures afterwards.

## Surprises & Discoveries

The separate Foundry action was extracted from the legacy creation panel into
page actions. The same model can be selected in one shared dialog instead.
A static regression requires the custom-header raw/bearer explanation; it is now
a tooltip on Header value. Computer Use returned empty Chrome state after
attempting device emulation, preventing a new mobile check.

## Outcomes & Retrospective

The single Create provider dialog supports LiteLLM, Internal service and Azure
Foundry. Creation and editing share Azure identity controls. Existing APIs and
embedded asset URLs are unchanged. All 49 React tests pass; measured coverage is
100% statements/lines/functions and 99.71% branches. Static UI checks, typecheck,
asset build and strict MkDocs pass. Computer Use created and reopened a fictional
Foundry provider successfully; the task-created database row was then removed.
The guide screenshot now shows the unified selector. A fresh mobile check could
not be completed because Computer Use stopped returning Chrome accessibility
content/screenshots after opening device emulation; desktop validation completed
before that tool limitation. All ten repository checks passed: formatting, Clippy, workspace tests, audit
with existing exceptions, deny, machete, nextest (388/388), Trivy, Gitleaks and
Semgrep. Release metadata and staged secret checks also passed. No new runtime
API or database behavior was introduced.

## Recovery

No migrations or Azure mutations are needed. Rebuild embedded assets after source
edits. Retry the checks against the isolated test services. Revert only this
change to restore the previous UI; provider records use unchanged API contracts.
The fictional demo provider created for UI validation was removed by its exact
name and endpoint, leaving pre-existing keys, profiles and services untouched.
