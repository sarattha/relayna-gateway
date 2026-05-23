# Admin UI Design System 2.0

The design system is the reusable foundation for the Relayna Gateway operator
portal. It keeps the UI dense, scannable, and governance-oriented without
copying any external brand.

## Files

- `tokens.css`: color, spacing, radius, shadow, focus, density, and status
  tokens. Import this through `src/app.css`; do not duplicate token values in
  feature views.
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
