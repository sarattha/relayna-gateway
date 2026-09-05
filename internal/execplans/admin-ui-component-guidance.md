# Admin UI component labels and guidance

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Operators should understand the purpose, units, empty values and zero values of
controls throughout Admin UI 3.0. Essential guidance stays visible. Secondary
definitions on metrics, table headings and icon actions use tooltips accessible
by mouse, keyboard and touch, including inside drawers.

## Progress

- [x] (2026-09-05) Inventory source forms, table controls, shared renderers and shell actions.
- [x] (2026-09-05) Verify field semantics against runtime source and record guidance coverage.
- [x] (2026-09-05) Implement visible labels/help and reusable accessible tooltips.
- [x] (2026-09-05) Verify dynamic forms, dialog focus, touch/keyboard dismissal, desktop/mobile layout and frontend tests.
- [x] (2026-09-05) Run frontend and complete repository verification; document results.
- [x] (2026-09-05) Prepare verified follow-up commit and update draft PR #112; publish on the existing branch.

## Context and Orientation

Worktree: `/Users/jobz/.codex/worktrees/9b27/relayna-gateway`, branch
`codex/admin-ui-3-followup-fixes`. Vite source is in
`crates/gateway-api/admin-ui/src/`, principally `main.ts`, `traffic.ts`,
`investigation.ts`, `app.css` and `design-system/`. The local demo runs at
`http://127.0.0.1:20381/admin-ui/`. Existing export guidance covers only a small
part of the UI. Resource editors are moved into drawers and some controls are
inserted after initial rendering.

## Compatibility Boundary

Presentation and accessible interactions only: no API, policy, authentication,
stored schema or submission semantics change. Explain current behavior rather
than altering it. Preserve existing form names, handlers and stored drafts.

## Plan of Work

Audit every component family in source and all available views in Chrome.
Check policy, rate, budget, routing and auth source before describing blank/zero
semantics. Keep reusable guidance in the design system with explicit contextual
definitions; never infer policy behavior from a numeric control's minimum.
Supply missing labels in templates and contextual visible help on forms.
Use one tooltip interaction with Escape dismissal, hover/focus persistence and
touch activation, preserving modal focus and avoiding clipping at viewport edges.
Record source-only coverage where a role or conditional state is unavailable.

## Validation and Acceptance

Every audited control has a visible label or a contextual table/checkbox label,
plus an accessible name. Technical units, optional/zero semantics and risky
credential/exposure settings are explained. Re-rendering and drawer moves do
not duplicate guidance or alter field values. Tooltips do not submit forms;
Escape dismisses the tooltip before closing its containing dialog. At 390 px,
help wraps and tooltips remain within the viewport. Build with
`npm run build:admin-ui`, run `npm test`, and run the repository verification
script when tests or build behavior change before pushing the draft update.

## Surprises & Discoveries

Provider authentication controls, picker checkboxes, endpoint pricing selects
and the Health debug lookup have gaps in labeling. Many policy limits are plain
numbers whose blank/zero semantics need runtime verification. Runtime inspection
confirmed that zero daily/monthly budgets block immediately, unlike a zero
minimum-cost filter. Owner/viewer membership does not itself grant global admin
access. Mobile screenshots exposed inherited metric styles making tooltip text
match its background; a more specific shared tooltip rule fixes the contrast.
Computer Use could not access the locked Mac; Chrome browser automation remained
available and supplied DOM, screenshots and interaction checks.

## Decision Log

- 2026-09-05: Audit and fix all component families requested by the user; leave
  existing backend behavior intact. Use visible guidance for required decisions
  and tooltips only for secondary explanations.

## Outcomes & Retrospective

Implemented visible labels and reviewed contextual guidance across shared forms,
filters, policy/guardrail controls, provider authentication, pricing, identity
bindings and shell controls. Secondary definitions use shared popover tooltips
with hover, focus, click, Escape and viewport handling. Source guidance includes
owner dashboards; browser coverage includes all 13 admin views and 30 inspected
states, with no unnamed controls, broken description references or document
horizontal overflow found. Dynamic pricing rows and drawer moves retain their
help without duplication. Both frontend checks and the full verification script
passed. See `internal/test-reports/admin-ui-component-guidance.md` for scope and
limitations.

## Idempotence and Recovery

Build regenerates checked-in assets. Keep changes on the current follow-up
branch and preserve prior commits. Browser checks should avoid configuration
writes and restore viewport and session state. No migration or release bump.
