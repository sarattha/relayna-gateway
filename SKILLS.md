# Relayna Gateway UI Skills

This file defines repository-local UI implementation guidance for agents and
contributors. Use it with `AGENTS.md` when changing the Admin UI or adding
operator-facing frontend screens.

## Admin UI 3.0 Design System

Use this skill whenever work touches:

- `crates/gateway-api/admin-ui/`
- `crates/gateway-api/src/static/admin-ui/`
- Admin portal navigation, layout, forms, tables, modals, notices, badges, or
  other frontend components
- Admin UI tests under `tests/admin-ui*.mjs`
- Operator-facing Admin UI documentation

### Design Intent

Admin UI 3.0 is an operator console for governing AI traffic, not a marketing
site. The interface should be dense, calm, scannable, and optimized for repeat
operations.

The system borrows product concepts from Boomi-style API governance without
copying Boomi branding:

- **Monitor**: Overview, Traffic, Usage & cost, Health and request investigation.
- **Discover**: Projects, Services, Providers and Routes.
- **Govern**: Virtual keys, Policies & guardrails, People & identities, Audit log
  and Settings. Workload identities remain a tab under People & identities.

### Source and Build Contract

The source of truth is the Vite/TypeScript package at:

```text
crates/gateway-api/admin-ui/
```

The generated static assets are checked in at:

```text
crates/gateway-api/src/static/admin-ui/
```

Keep the deployed asset contract stable:

- `/admin-ui`
- `/admin-ui/app.js`
- `/admin-ui/app.css`

Build after UI source changes:

```bash
npm ci
npm run build:admin-ui
```

Do not edit generated static assets by hand when a matching source change
belongs in `crates/gateway-api/admin-ui/`.

### Component Rules

Use existing Admin UI 3.0 tokens and component classes before adding new ones:

- Tokens in `src/design-system/tokens.css`: `--rg-color-*`, `--rg-status-*`,
  `--rg-space-*`, `--rg-radius-*`, `--rg-shadow-*`, and `--rg-focus-ring`.
- View metadata in `src/design-system/view-meta.ts`.
- Reusable helpers exported from `src/design-system/index.ts`, imported by
  feature code through `./design-system`.
- Shell classes: `app-shell`, `sidebar`, `nav-groups`, `nav-group`,
  `workspace`, `toolbar`, `eyebrow`, `view-summary`.
- Component classes: `panel`, `panel-heading`, `stat`, `metric-strip`,
  `form-grid`, `form-actions`, `actions`, `table-wrap`, `badge`, `notice`,
  `modal`, `modal-backdrop`, `empty-state`.

Add new component classes only when existing classes cannot express the needed
state or layout. Keep components compact and stable across desktop and mobile.
Add new helper functions under `src/design-system/` only when at least two
views need the same structure or status semantics.

### Component Labels and Guidance

- Give every editable control a persistent visible label. Use a contextual
  accessible name for row-selection checkboxes and icon actions; placeholders
  are examples, not labels.
- Keep essential units, blank/zero semantics, inheritance and credential handling
  visible beside fields and connected with `aria-describedby`. Verify meanings
  against runtime behavior; zero does not universally mean unlimited.
- Maintain reviewed contextual field and metric definitions in
  `src/design-system/guidance-content.ts`. The shared guidance installer handles
  dynamically rendered forms without replacing controls or duplicating help.
- Use shared help triggers for secondary metric/diagnostic definitions. Tooltips
  must work on hover, focus and tap, remain within the viewport, and dismiss on
  Escape before their containing dialog. Never put essential instructions only
  in a tooltip.

### Interaction and Security Rules

- Put inventories/results before creation forms; use contextual drawers without
  cloning bound forms or losing input on close/reopen.
- Commit workspace and scope changes only after navigation is accepted. Preserve
  drafts and filters on Cancel; guard navigation while writes are pending.
- Keep global project scope, Usage project/key filters, and Traffic filters
  consistent. Clear incompatible key filters when the project changes.
- Treat readiness and current availability separately from historical reliability.
  Use shared lifecycle/reliability helpers, sampled rates and explicit stale or
  unavailable states. Never label failed fetches as zero activity or healthy.
- Keep request investigation in accessible drawers, trap focus and restore it to
  the originating control. Wide tables retain horizontal scrolling.

- Preserve all existing `/admin-ui/admin/*` API routes and response assumptions
  unless the task explicitly changes backend behavior and passes compatibility
  review.
- Keep provider credentials, Studio tokens, operator tokens, virtual keys, and
  bearer tokens write-only or show-once.
- Never render stored secret values back into the UI.
- Keep destructive actions behind confirmation UI.
- Escape user-controlled values before rendering HTML.
- Keep wide tables horizontally scrollable instead of compressing content into
  unreadable cells.

### Visual Rules

- Use the dark-sidebar and light-workspace shell.
- Group navigation by Monitor, Discover, and Govern.
- Use compact panels and tables for operational workflows.
- Use status badges and status colors consistently:
  - `good` for healthy, enabled, configured, active, success.
  - `bad` for missing, failed, disabled, revoked, expired, timeout, degraded.
  - `warn` for elevated recorded rates, pending or opt-in states.
  - `neutral` for insufficient samples and informational states.
- Do not add decorative hero sections, gradient-only pages, nested cards, or
  oversized marketing layout patterns.
- Keep text inside controls from wrapping awkwardly or overflowing.
- Maintain responsive behavior for narrow screens with single-column layouts
  and horizontal table scrolling.

### Required Verification

For Admin UI source or generated asset changes, run:

```bash
npm run build:admin-ui
npm test
```

Run the full `$code-change-verification` stack when the change also touches
Rust runtime code, tests, packaging, build/test behavior, or anything required
by `AGENTS.md`.

When browser tooling is available, visually verify `/admin-ui/` at desktop and
mobile widths, including login, navigation, modals, wide tables, and at least
one Monitor, Discover, and Govern view.
