# Clear per-key route assignments

## Purpose and scope

Redesign the Virtual keys authentication bindings dialog using the existing
Design 4.0 React components. Operators should identify the key, see profile
routes first, understand denied/assigned states, and save each route with clear
feedback. Existing route/service API and revision contracts remain unchanged.

## Progress

- [x] 2026-09-20: Inspected current UI, code and guidance on the existing PR branch.
- [x] Implement compact searchable route rows, collapsed existing-settings routes,
  contextual authentication details, per-route save/error states and draft guards.
- [x] Add regressions, retain over 98% measured React coverage, run verification.
- [x] Validate desktop/mobile via Computer Use and refresh guide screenshot.
- [x] All ten mandatory checks passed; prepared the verified update for draft PR #121.

## Decision Log

- Keep independent per-route saves: the API has no atomic multi-route operation.
  Label saves locally and preserve unsaved selections on errors; prevent dismiss
  while saving and confirm discard of unsaved edits.
- Put routes without profiles in one disclosure with a single explanation; do
  not imply these routes allow access. Profile assignment never grants permission.
- Use existing named key metadata, no new API or persistence boundary.

## Surprises & Discoveries

The old dialog places selectors/buttons in a two-column form grid, mixes read-only
routes into the same list and reports all saves in a distant shared status line.
Failed writes use a global handler, making recovery hard to connect to a route.

## Validation

Run npm test, npm run test:react:coverage, npm run typecheck:admin-ui,
npm run build:admin-ui and the code-change-verification script. Verify pending,
success, conflict and failure; search and empty states; draft dismissal and focus;
unchanged bindings for other keys and revision-preserving route/service requests.
Use Computer Use on local fixtures at desktop and 390px, then update the illustrated
guide. No production changes or release publication.

## Outcomes & Retrospective

Implemented a native React dialog with shared controls and per-route state.
All 36 React tests pass with 100% measured statements (321/321), branches
(206/206), functions (134/134), and lines (281/281). UI build, strict typecheck,
JavaScript regressions, and strict MkDocs pass. All ten gateway verification commands passed; nextest 377/377 with zero skips.
Two non-failing process-cleanup warnings passed cleanly on isolated rerun
(2/2; /tmp/bindings-nextest-recheck.log).

Computer Use verified the actual embedded UI at desktop and 390×844: path search,
Entra/key-only previews, a real Save with inline success, a second Save restoring
the fixture to unassigned using the new revision, disclosure, scroll, draft
confirmation, and focus restored to the originating key-editor button. No runtime
authentication rules changed. Local responses route revision advanced from 8 to
10 during the two verified fixture writes; its original bindings were restored.
Replaced guide screenshot 09 and documented independent saves and recovery.
Logs: /tmp/bindings-{react,types,build,js,docs,verification}.log.
