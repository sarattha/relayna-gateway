# Admin UI Design System 4.0

The design system is the reusable foundation for the Relayna Gateway operator
portal. It keeps the UI dense, scannable, and governance-oriented without
copying any external brand.

## Files

- `tokens.css`: color, spacing, radius, shadow, focus, density, and status
  tokens. Import this through `src/app.css`; do not duplicate token values in
  feature views.
- `insights.ts`: escaped, read-only metric strips, measured comparison bars and
  configuration-flow diagrams shared across admin and owner pages.
- `view-meta.ts`: Monitor, Discover, and Govern navigation metadata.
- `components.ts`: small HTML helpers for panels, badges, notices, metrics,
  empty states, modal shells, action groups, and table wrappers.
- `templates.ts`: reusable page templates for dashboards, lists, create/edit
  forms, audit logs, analytics views, filters, and Studio import diffs.
- `index.ts`: the public import surface for Admin UI source code.

## Usage Rules

- Import helpers from `./design-system`, not from individual files.
- Keep workflow code in `src/main.ts`; add helpers here only when two or more
  views need the same structure or semantics.
- Use `badge()` for health, lifecycle, circuit, denial, fallback, and policy
  status so operator risk signals stay consistent.
- Use `panel()`, `metricTile()`, `tableWrap()`, and `emptyState()` for standard
  view structure instead of ad hoc container markup.
- Preserve security invariants: raw virtual keys and operator tokens are shown
  once, provider credentials stay write-only, and secrets are never rendered
  back into tables, exports, audit snapshots, or notices.

## Visual and interaction contract

The light sidebar uses an elevated active item and subtle lift/shadow feedback
on actionable controls. Read-only plots retain their position. Keyboard focus
remains visible, and `prefers-reduced-motion` removes movement.

Inventories lead with configuration coverage, governance pages with lifecycle
and access summaries, and scoped dashboards with recorded demand and outcomes.
Native progress bars represent known values; missing values remain unknown.
Longer metric definitions use authored `data-help-tooltip` controls, enhanced
by the same hover/focus/tap help system as forms and tables. Bind each trigger
only once as content is inserted dynamically.
