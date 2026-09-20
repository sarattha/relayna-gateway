# Admin UI 4.0 React console overhaul

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Give every Admin and Owner page a consistent, compact interface using React,
TypeScript, Vite, Tailwind and shared accessible components. Keep all existing
operator workflows and the Rust-served `/admin-ui`, `app.js` and `app.css` contract.
A virtual key is the gateway credential; profiles assign each key to one identity
policy per route. The UI must never expose stored credentials.

## Progress

- [x] (2026-09-20) Inventory: 18 views, login/session shell, live Traffic, operational dialogs, owner dashboards and authentication profiles.
- [x] (2026-09-20) Read repository design/implementation/verification guidance; confirm released boundary v0.1.37.
- [x] (2026-09-20) Establish React/Tailwind component foundation and typed shell.
- [x] (2026-09-20) Apply shared 4.0 layouts, tokens and control styling to all 18 views.
- [x] (2026-09-20) Replace profile/key selection flow with React state and accessible dialogs; blank queries show no results.
- [x] (2026-09-20) Add 26 mounted React tests and all-destination shell coverage; preserve existing operational and Traffic regressions.
- [x] (2026-09-20) Validate all destinations, desktop and 390px dialogs/navigation with Computer Use; record actual write and fixture limits in the report.
- [x] (2026-09-20) Run all ten mandatory checks, document coverage and Computer Use limits, and update draft PR #121.

## Surprises & Discoveries

The existing console has approximately 6,400 lines of domain workflow code and
18 views, including Owner pages. A wholesale controller rewrite would also replace
working session, proxy diagnostics and operational state logic; the chosen
boundary retains that logic and rebuilds the shell and interactive identity flows.
Existing tests include imperative controller
regressions and textual assertions, so new mounted React coverage is required.

Computer Use found legacy checkbox sizing and navigation rules conflicting with
new primitives, and a missing display rule on Owner metric grids. These were
corrected in shared styling. Service presets needed an explicit event bridge to
reset React identity state instead of only changing DOM input values.

## Decision Log

- 2026-09-20: User names the upgrade Admin UI 4.0; gateway version remains v0.1.37.
- 2026-09-20: Preserve existing API, credential, policy, traffic and owner logic.
  Separate React-owned shell/components from controller-owned DOM boundaries
  during migration; never let two renderers reconcile the same DOM subtree.
- 2026-09-20: Use one calm visual language: dark navigation, neutral workspace,
  compact 13px content, 12px labels, restrained headings, teal primary actions,
  consistent semantic status colors and progressive disclosure. No marketing UI.
- 2026-09-20: Keep statically embedded deployment. No Next.js runtime or Rust API
  changes. Introduce shared React components where interaction state belongs.

## Outcomes & Retrospective

Shared design and native React flows are implemented. All 18 destinations have
Computer Use evidence, with read-only fixtures used for Owner and membership
states. Real route/profile saves and synthetic proxy traffic passed. New React
coverage is 100% in each metric; operational regressions and strict typechecks
pass. See `internal/test-reports/admin-ui-4.md` for denominators and limitations.
All ten final mandatory checks pass; nextest reports 368/368 with zero skips.
The draft PR includes the final scope and evidence. All page content
has shared styling; controller-owned tables/charts are deliberately retained,
so this is not a claim that every controller is rewritten into JSX.

## Context and Orientation

Repository: `/Users/jobz/Works/relayna-gateway`. UI source is
`crates/gateway-api/admin-ui/src`; `main.ts` contains API workflow controllers,
`traffic.ts` manages streaming and history, `auth-profiles.ts` handles assignments.
`design-system` supplies tokens and guidance. Built files go to
`crates/gateway-api/src/static/admin-ui`. Rust embeds those files at compile time.

All-view inventory: Overview, Traffic, Usage & cost, Health, Projects, Services,
Providers, Routes, Virtual keys, Policies & guardrails, People & identities,
Managed identities, Audit log, Settings, My services, Service dashboard,
My projects, Project dashboard. Login, pending/denied access, command navigation,
credential reveal and confirmation flows are also part of acceptance.

## Compatibility Boundary

Latest release v0.1.37: preserve URLs, API bodies, session/CSRF and operator-token
behavior, hashes, deployment and saved data. UI rendering is internal. No schema,
proxy or policy migration is needed; user explicitly authorizes the UI overhaul.

## Plan of Work

Introduce typed React entry and shared shadcn-style primitives backed by Radix,
Tailwind tokens and common form/table/status styling. Move shell rendering to
React and keep explicit lifecycle boundaries around operational controllers.
Replace identity editing and key search with stateful React components. Inventory
all actions before changes; validate dynamic fields, draft cancellation, stale
loads and errors, filters, exports and owner role boundaries. Consolidate visual
rules rather than adding per-page patches.

## Concrete Steps

From repository root run `npm ci`, `npm run build:admin-ui`, `npm test`, the new
React typecheck/mounted tests and
`bash .codex/skills/code-change-verification/scripts/run.sh` with test PostgreSQL
and Redis configured. Rebuild `gateway-api` for local Computer Use validation.

## Validation and Acceptance

Every listed view must remain reachable for its authorized role and display real
API results, loading, empty and error states. Verify writes and draft cancellation,
focus restoration, keyboard dialog navigation, search clearing, mobile navigation,
wide table scrolling and secret reveal. Test role-scoped requests and rejection
without rendering unauthorized pages. Existing Traffic regressions must pass.
Preserve >98% changed production Rust coverage from the preceding task; report
React coverage independently and target >98% for new testable UI behavior.
Computer Use evidence must identify pages checked and remaining gaps explicitly.

## Idempotence and Recovery

Preserve the current branch's completed work. Use disposable local demo data for
writes; never change production. Rebuild generated assets from source. Stop only
the known local demo process when restarting. Keep credentials out of screenshots
and test reports. Dependency lockfiles make installation reproducible.

## Artifacts and Notes

Existing draft: https://github.com/sarattha/relayna-gateway/pull/121.
Record screenshots and the functional matrix in a dedicated test report.

## Interfaces and Dependencies

React/ReactDOM, Tailwind's Vite integration, Radix accessible primitives,
class-variance-authority, clsx and tailwind-merge. Vite retains fixed asset names.
No UI dependency may require an external server at runtime.
