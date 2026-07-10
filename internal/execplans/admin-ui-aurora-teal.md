# Apply the Aurora Teal Admin UI Palette

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md` at the repository root.

## Purpose / Big Picture

Operators should see the existing Relayna Gateway Admin UI with the selected
Aurora Teal color identity: a midnight-teal sidebar, emerald controls, aqua data
accents, sunflower warnings, coral failures, and a pale mint workspace. Layout,
copy, navigation, API behavior, and operational density remain unchanged.

## Progress

- [x] (2026-07-10 08:30Z) Resolve the selected Aurora Teal mock and inspect the existing design tokens and CSS overrides.
- [x] (2026-07-10 03:43Z) Update the source palette and chart colors without changing layout or behavior.
- [x] (2026-07-10 03:43Z) Rebuild checked-in Admin UI assets and run Admin UI tests.
- [x] (2026-07-10 03:43Z) Capture desktop and mobile browser evidence and complete design QA.
- [x] (2026-07-10 03:47Z) Commit and push the verified change to PR #92 and mark it ready for review.

## Surprises & Discoveries

- Observation: The modern operator-console layer contains a few hard-coded
  legacy palette values after the shared tokens import.
  Evidence: `crates/gateway-api/admin-ui/src/app.css` overrides the sidebar,
  selected navigation, sidebar text, and state borders below the main responsive
  rules.

- Observation: Both integrated browser surfaces blocked local HTTP navigation,
  while the user-approved local Playwright CLI reached the isolated QA gateway.
  Evidence: desktop and mobile captures under `output/playwright/aurora-teal/`
  show the real authenticated Admin UI backed by a disposable database clone.

## Decision Log

- Decision: Keep this as a token-led recolor and update only hard-coded values
  that would visibly preserve the old gold/navy identity.
  Rationale: The selected design changes color identity, not information
  architecture or interaction behavior.
  Date/Author: 2026-07-10 / Codex.

- Decision: No compatibility strategy or migration is required.
  Rationale: Public routes, response shapes, configuration, persisted state,
  authentication, and runtime integration contracts are unchanged.
  Date/Author: 2026-07-10 / Codex.

- Decision: Use a darker accessible emerald than the brightest generated mock
  swatch for controls and selected navigation.
  Rationale: `#087b60` preserves the selected teal identity while providing a
  5.23:1 contrast ratio with white text.
  Date/Author: 2026-07-10 / Codex.

## Outcomes & Retrospective

The Aurora Teal palette is implemented in source tokens, shell overrides,
Chart.js series, and semantic Overview metrics. Checked-in static assets were
rebuilt, all Admin UI tests passed, and desktop/mobile visual QA passed after
one P2 metric-color correction. The user approved the screenshots, and the
verified change was published to PR #92 for review.

## Context and Orientation

The Admin UI source of truth is `crates/gateway-api/admin-ui/`. Shared visual
tokens live in `src/design-system/tokens.css`, the operator-console CSS lives in
`src/app.css`, and Chart.js series colors are configured in `src/main.ts`.
`npm run build:admin-ui` from the repository root regenerates the deployed
assets in `crates/gateway-api/src/static/admin-ui/` while preserving the
`/admin-ui`, `/admin-ui/app.js`, and `/admin-ui/app.css` contract.

## Compatibility Boundary

This visual-only change does not affect a released compatibility boundary. The
existing static asset URLs, HTML structure, admin routes, and API contracts are
preserved.

## Plan of Work

Replace the shared navy/gold/green palette with Aurora Teal tokens, convert the
modern shell's remaining hard-coded legacy colors to the new palette, and align
chart series colors with teal, coral, and aqua semantics. Rebuild assets, run
the Admin UI test suite, then compare browser captures at desktop and mobile
widths against the selected 1440 by 1024 mock.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    npm run build:admin-ui
    npm test

Use the local gateway or an existing Admin UI test environment for browser
captures. Record the final comparison in `design-qa.md`.

## Validation and Acceptance

Acceptance requires regenerated assets, a passing Admin UI test suite, no
browser console errors, a desktop view matching the selected color hierarchy,
and a mobile view with no new overflow or clipped controls. `design-qa.md` must
end with `final result: passed`.

## Idempotence and Recovery

The build and test commands are safe to rerun. Generated assets must be
regenerated from the Vite/TypeScript source rather than edited directly. If a
visual check exposes a contrast or fidelity issue, adjust the source token or
selector, rebuild, and recapture.

## Artifacts and Notes

The selected visual target is the generated Aurora Teal mock at
`/Users/jobz/.codex/generated_images/019f49ed-7044-7e32-98ba-b5cdfc4a90bf/exec-b6301782-0ede-4a67-bc54-31b75d8efd8a.png`.

## Interfaces and Dependencies

No new package, font, icon library, API, route, or runtime dependency is added.
The existing CSS custom-property interface and Chart.js configuration remain in
place.
