# Admin UI 3 design and UI 2 audit

Maintain per /Users/jobz/Works/relayna-gateway/PLANS.md.

## Purpose / Big Picture

Review existing Admin UI 2 source and rendered workflows; deliver an additive, responsive HTML design prototype and a prioritized implementation backlog. Production behavior is not changed.

## Progress

- [x] 2026-09-05: Read manifesto, UI design guidance, engineering and Product Design skills; inspected source and clean working tree.
- [x] 2026-09-05: Attempted Computer Use twice; Chrome state capture times out. Connected in-app browser.
- [x] 2026-09-05: Captured representative existing UI workflows with a synthetic read-only fixture in Chrome; user confirmed no running target.
- [x] 2026-09-05: Saved 30 prioritized findings, all 18 surfaces, overlap mapping, implementation sequence and evidence limits.
- [x] 2026-09-05: Built standalone HTML with 13 destinations and core demo flows; verified desktop and 390px phone layouts, keyboard search, drawers, sample creation and filtering.

## Surprises & Discoveries

No local gateway server was found. UI 2 can be reviewed using unchanged compiled assets with synthetic API responses; this cannot validate real authentication, persistence or upstream behavior. Source reveals readiness failure can erase monitoring views and most views lack stale-response guards.

## Decision Log

2026-09-05: Keep artifacts in internal/design/admin-ui-3; do not edit shipped UI or regenerate assets. Use existing teal/dark-sidebar identity and improve operational hierarchy. Fixture data is explicitly synthetic. Audit findings distinguish reproduced defects, static evidence and proposals.

## Outcomes & Retrospective

Delivered internal/design/admin-ui-3/index.html, audit.md, qa.md, README.md, serve.py and screenshot evidence. Production UI remains untouched. Reproduced readiness dashboard erasure and expired-key miscount; navigation race remains a source finding after an inconclusive timing experiment. Baseline npm test passes. Full Rust checks and production rebuild are inapplicable to additive design artifacts. Computer Use capture failed; user-approved Chrome integration completed visual work. Prototype direct HTML was used to satisfy the requested artifact format and existing source design target; no image generation or deployment was needed.

## Context and Orientation

UI source: /Users/jobz/Works/relayna-gateway/crates/gateway-api/admin-ui/src/main.ts, traffic.ts and app.css. Shared view metadata identifies 18 admin/owner surfaces. UI is governance and metering for AI requests, services, projects and virtual access keys.

## Compatibility Boundary

Additive design artifacts only; no released API, database or UI asset changes. No compatibility shims or migrations required.

## Plan of Work

Serve existing static assets with synthetic data; capture representative Monitor, Discover and Govern workflows. Inspect remaining functions statically. Build standalone HTML with functional navigation, filters, detail drawer and contextual creation flow. Save audit and QA evidence.

## Concrete Steps

Run python3 internal/design/admin-ui-3/serve.py on loopback port 18430. Existing UI is /admin-ui/; prototype is /. Inspect both using browser tools. Run npm test for baseline only if useful; no production test or build files change.

## Validation and Acceptance

Report evidence for all navigation surfaces, identifying uncaptured areas. Reproduce readiness and navigation-race defects with fixture toggles. Verify desktop/mobile layout, keyboard dialog dismissal, search, filtering and prototype creation. All data/actions must be marked demo; no real credential issuance.

## Idempotence and Recovery

Restart server safely; remove temporary fixture flags after experiments. Artifacts can be removed without affecting production. Preserve all unrelated work.

## Artifacts and Notes

internal/design/admin-ui-3/audit.md, index.html, evidence/, serve.py.

## Interfaces and Dependencies

Standalone HTML/CSS/JS, local Tabler font assets and Python standard-library preview server. No backend mutations.
