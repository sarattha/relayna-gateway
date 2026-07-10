# Redesign the Admin UI Operator Experience

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Relayna Gateway operators should be able to understand gateway posture, find
traffic anomalies, and begin a governed change without scanning a collection of
unrelated engineering forms. The redesigned `/admin-ui` keeps every existing
admin API workflow and secret boundary, while introducing a compact operational
overview, responsive navigation, accessible dialogs, progressive disclosure for
dense forms, and consistent tables and action feedback.

The result is observable by running the real gateway against local PostgreSQL
and Redis, signing into `/admin-ui`, visiting every Monitor, Discover, and Govern
view, exercising the non-destructive controls plus confirmation cancel paths,
and comparing desktop and mobile captures with the approved visual target.

## Progress

- [x] (2026-07-10 01:10Z) Audited all Admin UI views and captured desktop,
  dialog, and mobile evidence.
- [x] (2026-07-10 01:54Z) Created branch `codex/admin-ui-redesign` from release
  tag `v0.1.18` / `origin/main`.
- [x] (2026-07-10 02:17Z) User approved the combined progressive-governance and
  investigation visual target.
- [x] (2026-07-10 02:02Z) Implement the redesigned shell, overview, shared interaction patterns,
  responsive behavior, and progressive form treatments.
- [x] (2026-07-10 02:03Z) Update generated static assets and focused Admin UI tests.
- [x] (2026-07-10 02:06Z) Run the real gateway with PostgreSQL and Redis and verify every view and
  core workflow at desktop and mobile widths.
- [x] (2026-07-10 02:07Z) Complete design QA against the approved 1440 x 1024 source mock and leave
  `design-qa.md` with `final result: passed`.
- [x] (2026-07-10 02:20Z) Run the full repository verification stack and capture
  the final desktop, dialog, menu, and mobile screenshots.
- [ ] Publish the reviewed branch as a draft pull request.

## Surprises & Discoveries

- Observation: The current CSS-only mobile test passes even though a live
  390-pixel viewport has a document width of 828 pixels and places the workspace
  below a 721-pixel navigation block.
  Evidence: `.tmp-admin-ui-audit/14-mobile-overview.png` and the browser DOM
  measurement captured during the audit.

- Observation: Existing confirmation dialogs are visually clear but have no
  dialog role, `aria-modal`, focus transfer, focus trap, Escape handling, or
  deterministic focus restoration.
  Evidence: live DOM inspection of the rotate-token confirmation and the modal
  builders in `crates/gateway-api/admin-ui/src/main.ts`.

- Observation: The latest release tag and current branch base are both
  `v0.1.18` at commit `3b626d0`, so compatibility is judged against the exact
  current Admin UI release.
  Evidence: `git log -1 --oneline --decorate`.

- Observation: Vite correctly emitted the Tabler font assets, but the released
  Rust static handler only served `index.html`, `app.js`, and `app.css`, so live
  browser captures initially displayed missing-glyph boxes.
  Evidence: the first live screenshot and an HTTP 404 for
  `/admin-ui/admin-ui-tabler-icons.woff2`.

- Observation: importing Chart.js changes Rollup's internal symbol naming even
  with minification disabled, making tests that match compiled function names
  unstable.
  Evidence: the initial focused test failure for
  `guardrailOverrideControls`; source-level assertions now read `main.ts` while
  deployed endpoint-contract assertions continue to read `app.js`.

## Decision Log

- Decision: Preserve `/admin-ui`, `/admin-ui/app.js`, `/admin-ui/app.css`, and
  every `/admin-ui/admin/*` request/response assumption.
  Rationale: The task is an operator-experience redesign, not a control-plane API
  migration. Existing clients, deployments, and tests must remain compatible.
  Date/Author: 2026-07-10 / Codex.

- Decision: Use Chart.js for responsive canvas charts and Tabler Icons Webfont
  for the approved outline icon language.
  Rationale: Both are maintained, freely available libraries suited to the
  selected visual target; they avoid handcrafted SVG/CSS artwork and integrate
  with the existing Vite package.
  Date/Author: 2026-07-10 / Codex.

- Decision: Keep charts grounded in the existing usage dashboard API and derive
  attention rows from actual provider-health data rather than inventing new
  backend concepts.
  Rationale: The redesign must remain functional on the real environment and
  should not imply unsupported incident-management behavior.
  Date/Author: 2026-07-10 / Codex.

- Decision: Implement navigation state with the URL hash while retaining the
  deployed `/admin-ui` route contract.
  Rationale: Hash-backed view state provides deep links and browser navigation
  without requiring new server routes or asset fallback behavior.
  Date/Author: 2026-07-10 / Codex.

- Decision: Add additive, immutable-cache responses for the generated WOFF2,
  WOFF, and TTF font fallbacks while
  preserving the three released Admin UI asset routes unchanged.
  Rationale: the icon library and its declared fallbacks must work through the
  real gateway; explicit font responses are the smallest compatible extension
  and have focused route coverage.
  Date/Author: 2026-07-10 / Codex.

## Outcomes & Retrospective

The redesigned portal now exposes real operational posture, chart-backed usage
signals, investigation links, governed-change shortcuts, hash-deep-linked
navigation, progressive forms, accessible dialogs, object-specific action
labels, pending feedback, and a mobile drawer without changing admin API
payloads or secret semantics.

Real-environment browser verification loaded all 11 authenticated views without
page errors at 1440 × 1024. At 390 × 844 the document width remained exactly
390 pixels and wide tables scrolled only inside their wrappers. Command search,
governed shortcuts, refresh/navigation state, mobile drawer behavior, and the
rotation confirmation cancel path all passed; the final console error/warning
readout was empty.

Design QA required two density corrections before passing. The final deliberate
differences from the mock are live API values and omission of mock-only owner
avatars/sparklines that the current backend does not supply. The residual P3 is
documented in `design-qa.md`.

The mandatory verification stack passed in full after isolating Trivy from a
locally hung Docker credential helper: formatting, Clippy with warnings denied,
workspace tests, RustSec, cargo-deny, cargo-machete, nextest, Trivy, gitleaks,
and Semgrep. The focused Admin UI build/tests and production-dependency audit
also pass.

## Context and Orientation

The Admin UI source lives at `crates/gateway-api/admin-ui/`. Its Vite/TypeScript
entry point is `src/main.ts`, the visual system is in `src/app.css` and
`src/design-system/`, and the static deployment assets are generated into
`crates/gateway-api/src/static/admin-ui/`. The Axum control API serves those
assets and all existing admin endpoints; this plan does not change Rust route
handlers, PostgreSQL schemas, Redis formats, gateway proxy behavior, virtual key
formats, usage event shapes, or secret storage.

A virtual key is the Relayna credential presented by external clients. Key
policy limits its routes, models, providers, budgets, rate limits, and
guardrails. Usage events are durable records of gateway requests. LiteLLM and
internal services are upstream targets whose credentials must remain write-only
in the Admin UI.

The selected source of visual truth is:

    /Users/jobz/.codex/generated_images/019f4930-71ff-7461-b08f-9ce6ca737d1d/exec-1c98d33f-60cf-4562-a1b6-a48c10aa2be3.png

The selected layout keeps the dark Relayna sidebar, adds a compact global
header, combines operational posture with a governed-change center, visualizes
traffic/failure/latency data, prioritizes health attention, and uses a unified
operations table. Those patterns should propagate to the rest of the portal
without removing any current functionality.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.18`. The redesign changes
released Admin UI presentation and client-side navigation behavior, but
preserves the released HTTP asset paths, all admin endpoint paths and payloads,
session-token storage semantics, show-once secret behavior, and destructive
confirmation requirements. No shim or data migration is needed.

## Plan of Work

First, update the source package dependencies for the approved chart and icon
libraries. Rebuild the shell in `index.html`, keeping the login and app roots but
adding a responsive drawer trigger, compact global bar, environment indicator,
last-refresh status, command navigation, and governed-change menu.

Next, update `src/main.ts` with hash-backed navigation, consistent active state,
mobile drawer behavior, request-pending feedback, accessible dialog lifecycle,
and an overview composed from real usage, provider-health, route, service, key,
and audit data. Use Chart.js for the main timeseries and small table trends.

Then refine `src/app.css` and the design-system helpers so all views inherit the
new density, table, status, modal, form-section, loading, and responsive rules.
Use progressive disclosure on the longest forms while keeping every existing
field available. Update repeated row actions with object-specific accessible
labels.

Finally, update focused tests, regenerate static assets, run the real gateway,
exercise every view with the in-app browser, capture desktop/mobile/dialog
evidence, complete iterative design QA, run the mandatory verification stack,
and publish the reviewed diff as a draft PR.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    npm install chart.js@4.5.1 @tabler/icons-webfont@3.44.0
    npm run build:admin-ui
    npm test
    scripts/run-gateway.sh

With the real gateway running, open `http://127.0.0.1:18081/admin-ui` in the
Codex in-app browser, sign in with the existing local operator token, and verify
all views at 1440 x 1024 and 390 x 844.

After browser and design QA pass:

    bash .codex/skills/code-change-verification/scripts/run.sh

Then inspect the diff, stage only intended files, commit, push the branch, and
open a draft PR against `main`.

## Validation and Acceptance

Acceptance requires all of the following observable evidence:

1. `/admin-ui`, `/admin-ui/app.js`, and `/admin-ui/app.css` load from the real
   gateway; no existing admin endpoint path or payload assumption changes.
2. Login, sign-out, token-rotation confirmation cancel, view navigation,
   refresh, command navigation, mobile drawer, and governed-change shortcuts
   operate from the browser.
3. Overview data and charts render from real API responses and link to relevant
   Health, Usage, Keys, Providers, Services, and Audit views.
4. Projects, Keys, Guardrails, Providers, Services, Routes, Usage, Health, Audit,
   and Settings still load and expose every pre-existing form, table, modal,
   filter, lifecycle action, export, simulation, and drilldown control.
5. Dialogs expose dialog semantics, move focus inside, trap Tab, close on Escape
   where safe, and restore focus. Pending mutations cannot be double-submitted.
6. At 390 x 844, the workspace is immediately reachable through a drawer,
   `document.documentElement.scrollWidth` equals `clientWidth`, persistent
   controls remain visible, and wide tables scroll only inside their wrappers.
7. `npm run build:admin-ui`, `npm test`, and the full repository verification
   script pass.
8. `design-qa.md` cites the source mock and final browser screenshot and ends
   with exactly `final result: passed`.
9. Final desktop and mobile screenshots cover login, Overview, every navigation
   view, dialogs, dense tables, and representative empty/loading/success states.
10. A draft PR exists on GitHub with the branch, commit, validation, screenshots,
    compatibility note, and review guidance.

## Idempotence and Recovery

Dependency installation, Vite generation, tests, and verification scripts are
safe to rerun. The source package remains authoritative; never hand-edit the
generated static files. Browser verification should avoid confirming destructive
actions against persistent local data. If a mutation is needed for coverage,
create uniquely prefixed audit fixtures and remove only those fixtures through
the UI after capture. If the gateway is interrupted, restart
`scripts/run-gateway.sh`; migrations and budget rehydration are designed to be
idempotent. Do not clear unrelated PostgreSQL rows or Redis keys.

## Artifacts and Notes

The pre-redesign audit captures are local-only in `.tmp-admin-ui-audit/` and
must not be staged. Final review captures will be placed in a dedicated tracked
or PR-linked artifact directory chosen during verification. The approved source
mock remains outside the repository and is referenced by absolute path in
`design-qa.md`.

## Interfaces and Dependencies

The final frontend uses the existing Vite/TypeScript source package plus
`chart.js@4.5.1` and `@tabler/icons-webfont@3.44.0`. No new Rust crate,
environment variable, database table, Redis key, admin API route, provider
credential representation, or usage event field is introduced. Existing admin
API types and write-only/show-once secret semantics remain authoritative. The
only additive HTTP surface is the generated static icon-font assets under the
existing `/admin-ui/{*path}` handler.
