# Admin UI 4.0 verification

Date: 2026-09-20. Branch: `codex/traffic-filters-auth-profiles`, draft PR #121.
Design 4.0 is separate from gateway release v0.1.37.

## Implementation boundary

React/TypeScript owns the shell, endpoint/profile editing, key-selection popup
and route configuration launchers. Tailwind and shared Radix-backed components
provide consistent controls, contextual help and dialog focus behavior. Every
Admin/Owner view shares the 4.0 visual rules. Existing operational controllers
still own their page content, API calls, tables, charts and request lifecycle;
this change does not claim to convert all controller code into JSX.

The Rust backend and the embedded `/admin-ui`, `/admin-ui/app.js` and
`/admin-ui/app.css` contract are unchanged by this upgrade. A Node server is not
needed at runtime. Build dependencies require Node 24.15 or newer; CI and release
jobs use Node 24. The production JavaScript is about 369 KB gzipped.

## Mounted regressions and coverage

`npm run test:react:coverage`: 26 tests pass. All new `src/react` and
`src/components` modules are included, with no per-file exclusions. Coverage:
223/223 statements, 104/104 branches, 99/99 functions, 209/209 executable lines
(**100% each**). CI enforces a 98% floor on all four metrics. Static JSX markup
is not itself evidence that every user interaction has been exercised.

Coverage includes empty/cleared searches, multi-term matching, project scope,
keys assigned elsewhere, draft Apply/Cancel, unknown saved bindings, failed
loads/retry, late resolution/rejection, result limits, changing assignments,
keyboard Enter/Escape, focus restoration, help controls, mode-dependent FormData,
retained hidden drafts, profile removal guards, inline server errors, component
cleanup and route drawers preserving the original form and submit handlers.
Service presets explicitly reset React identity state; Add profile focuses its
new stable-ID field. Both have regressions.

`npm test` also retains the existing operational, API-contract, escaping,
show-once credential, navigation, lifecycle, usage, owner and Traffic tests.
Traffic covers every filter independently plus 192 outcome/intersection
combinations, history dates, normalized IDs/statuses, pagination and stale reads.

No production Rust source changed in this upgrade. The preceding task's unchanged
changed-production-line gate remains 137/137 (100%); see
`traffic-profiles.md` and `traffic-profiles-coverage.json`. This is not a claim
of >98% whole-workspace coverage; the prior full LCOV report measured 95.48%.

## Computer Use matrix

Used the Computer plugin (`@oai/sky`) in Chrome. The real embedded local gateway
used PostgreSQL, Redis and a mock upstream on ports 18460/18461. No production
services or credentials were used.

| Views | Browser checks |
| --- | --- |
| Overview, Health | Real aggregate data, cards/charts, readiness and failure states; aligned responsive metrics |
| Traffic | Live delivery of synthetic traffic; trimmed request-ID matches in Live and Saved history |
| Usage & cost | Populated usage and diagnostic/filter controls |
| Projects, Services, Providers | Inventories, empty states and contextual forms; service preset switching resets audience drafts |
| Routes | Compact inventory; original form moves into Configure drawer; real unchanged configuration saves successfully |
| Virtual keys, Policies & guardrails | Inventory and available governance controls |
| People & identities, Workload identities | Both destinations and empty binding states |
| Audit log, Settings | Populated audit rows and grouped settings; shared fields/checkboxes |
| My services, Service dashboard, My projects, Project dashboard | Read-only Owner fixture; scoped navigation, empty charts/tables and corrected metric grid |
| Login, pending and blocked membership | Existing operator login plus fixture membership states; restricted states do not display console navigation |

Identity interaction checks: real profile save succeeds; switching Entra/key-only
hides claims and restores unsaved values on return. Key search by project shows
only eligible unassigned keys. Add clears the search and result list; manual
clearing also hides results. Escape cancels popup edits, keeps the parent editor
open and restores its Select keys trigger. A profile retains multiple keys while
each key belongs to at most one profile on the route.

Responsive checks used Chrome's 390 × 844 device viewport through Computer Use:
Owner dashboard/navigation, stacked identity fields, scrollable editor with
visible Save/Cancel and the nested key popup with wrapping UUIDs and visible
Apply/Cancel. Desktop checks covered all 18 destinations.

The traffic exercise sent 12 policy-denied requests plus 12 permitted requests
(8 successful, 4 upstream 503) through the real proxy. This verified live
updates after rebuilding and the persisted history read path.

Owner visual validation uses `tests/fixtures/admin-ui-owner-preview.mjs`, a
read-only localhost fixture. It is not a live Microsoft sign-in test or a
replacement for backend authorization E2E. Real OIDC/profile/Owner backend
coverage remains in the workspace integration suite. Not every destructive
button or possible production configuration was manually exercised.

Screenshots are retained locally under ignored `internal/test-reports/admin4/`;
the documentation includes the sanitized 4.0 Overview capture. During checks,
we fixed desktop navigation geometry, oversized native checkboxes, Owner metric
stacking and React service-preset state synchronization.

## Final verification

- Clean `npm ci`, generated asset build and operational UI regressions pass.
- Strict React typecheck and mounted coverage pass.
- Strict MkDocs build and release metadata validation pass.
- Full mandatory verification rerun: all ten commands passed (format, Clippy,
  workspace tests, audit with existing exceptions, deny, machete, nextest, Trivy,
  Gitleaks and Semgrep). Nextest: 368/368 passed, zero skipped.
- `npm audit`: zero vulnerabilities.
- All seven generated assets reproduce byte-for-byte on repeat build; Git
  whitespace checks pass.

These checks bound known behavior; neither coverage nor visual checks establish
that all possible inputs, timing, deployments and external services are defect-free.

## Status alignment refinement

Usage and Owner request rows now use a fixed-width outcome badge column and a
consistent gap before the HTTP code. Removed the badge's inherited bottom/right
margin so badge and code centers align. Computer Use verified success/failure
rows on desktop and retained horizontal table scrolling at 390px. UI build,
existing UI tests, gateway build and whitespace checks pass. This presentation-only
change does not alter runtime behavior or coverage; the full Rust stack was not
rerun after the preceding complete verification.
