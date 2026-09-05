# Admin UI toggle audit

This plan follows `PLANS.md`. Source: `crates/gateway-api/admin-ui/src/`.

## Purpose / Big Picture

An active display filter must be removable by activating the same button again.
Audit all button families; preserve page navigation and explicit server actions.
Only frontend presentation changes are needed; no API or storage boundary changes.

## Progress

- [x] 2026-09-05: Inspect every button creation and click-handler family in the source.
- [x] 2026-09-05: Make Usage breakdown selection reversible and persist the all-breakdowns state.
- [x] 2026-09-05: Mark People/Workload identities as current-page navigation.
- [x] 2026-09-05: Add regression coverage and run UI build/tests plus full verification.
- [x] 2026-09-05: Verify button families in Chrome and update operator documentation.
- [x] 2026-09-05: Prepare verified changes and updated summary for draft PR #112.

## Surprises & Discoveries

Usage breakdown selectors used pressed-button semantics but always required one
selection. People/Workload identities also used pressed semantics for navigation.
Traffic failure buttons already toggle every failure code and retain zero-match
selected filters. Native checkboxes and details summaries already reverse on repeat
activation. Help buttons pin on first click and dismiss on the second. Pause/Resume
already alternates its action. The governed menu already toggles expanded state.
Mobile navigation uses a modal panel with a separate close button because its opener
is inert while the panel is open.

## Decision Log

- 2026-09-05: A second click on a selected Usage breakdown shows every breakdown;
  null records that choice across data refreshes. Keep the initial Recent requests view.
- 2026-09-05: Page navigation uses aria-current. Enable/Disable and role-management
  controls keep their explicit action labels and existing API/confirmation behavior.
  They already expose the inverse action after refreshing authoritative state.

## Validation

Run `npm run build:admin-ui`, `npm test`, and
`bash .codex/skills/code-change-verification/scripts/run.sh`. Exercise Usage selection,
repeat activation, switching, and refresh; Traffic failure filters; help click/keyboard
toggle; details and checkboxes; Pause/Resume; and mobile navigation. Test server-action
direction with mocked APIs instead of changing real provider or authorization settings.

## Outcomes & Retrospective

All 12 Usage breakdown buttons passed browser click-to-select and Enter/Space-to-clear
checks; all 12 sections remained visible after refreshing with no selection. At
390 × 844, repeated selection cleared correctly and document width remained 390px.
Help buttons passed click/click and Enter/Enter; Task drilldown passed click/Enter.
Traffic failure buttons passed on/off and Pause/Resume returned to live updates.
People and Workload identities switched the aria-current page in both directions.
The provider passthrough checkbox passed click/Space without saving; test edits
were discarded. Mobile navigation opened and closed, resetting aria-expanded.

API-mocked tests covered Enable/Disable for keys, providers, mappings, both route
families and services, including Cancel; workload identity and admin-role actions
covered both boolean states and authoritative refreshes. Existing native controls
and server actions needed no behavior changes. Browser selector interaction with
Resume timed out; a screenshot-grounded click worked and live updates resumed.

UI build/tests and the complete verification script passed. The implementation
changes are confined to Usage selection and identity navigation semantics; no
runtime API, authorization, or persisted resource settings were changed.
