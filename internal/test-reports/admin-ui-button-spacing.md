# Admin UI button spacing follow-up

Date: 2026-09-05
Branch: `codex/admin-ui-3-followup-fixes`

## Findings and fixes

The Task drilldown input and Load task usage button had a measured 0 px gap.
The shared `inline-form` class had no CSS layout. The same problem occurred in
Health debug lookup, provider authentication and service/project membership
assignment forms. A shared single-column grid now supplies a 12 px gap and
keeps submit buttons at their natural width. Route forms retain their existing
responsive columns and use the 8 px spacing token.

Task results and inline debug investigations have 16 px separation from their
lookup form. Action bars with following content no longer apply the negative
bottom margin intended for final footers. Overview's Explore requests action
has 12 px above it, and direct notice buttons have a separate row with 12 px
above them. The prior Traffic failure-group 16 px gap is preserved.

## Verification

- `npm run build:admin-ui` and `npm test` passed. Generated assets were rebuilt
  from the Vite source; this follow-up changes CSS only.
- Reviewed button markup in `main.ts`, `traffic.ts`, `investigation.ts`, shared
  design-system components and `index.html`, including login, selection
  dialogs, import/pricing controls, owner dashboards and conditional notices.
- Chrome DOM geometry checks covered all 13 Admin navigation pages at desktop
  (2560 px) and mobile (390 px) widths, with expandable sections open. Waited
  for populated data in Overview, Traffic, Usage and Virtual keys before
  accepting their measurements. No page-level horizontal overflow was found.
- Shared inline forms measured 12 px gaps; route forms measured 8 px. Task
  drilldown measured 12 px input-to-button spacing at both widths and 16 px
  button-to-result spacing after a no-match lookup on mobile.
- Additional browser checks covered project, service, provider and virtual-key
  creation drawers; workload identities and registration; guardrail creation;
  and the request investigation drawer. The expanded service drawer was also
  inspected in a screenshot. Sticky footer backgrounds intentionally overlay
  scrolling form sections; their action buttons retain padded separation.
- Source review covered conditional controls unavailable in the current admin
  session, including owner-only views and sign-in states. These were not
  exercised under another account. No configuration writes were submitted.

The local demo remains at `http://127.0.0.1:20381/admin-ui/`. The viewport override
was reset after responsive checks.

## Sidebar viewport follow-up

The sidebar scrolled off-screen because `overflow-x: hidden` made the body an
unintended scroll container for sticky positioning. Replacing it with `clip`
preserves horizontal clipping while letting the sidebar stick to the viewport.
The sidebar uses dynamic viewport height, its header cannot shrink, and only
the navigation list scrolls when vertical space is limited.

Chrome verification on the long Traffic page reproduced a sidebar top of
−2003.5 px before the fix. After rebuilding, its top remained 0 and its bottom
1352 px at both the page top and a scroll position of 1869.5 px. Sign out and
the profile remained at the bottom, with the profile ending 14 px above the
viewport edge. At 1280 × 600 and with the mobile menu open at 390 × 600, the
sidebar filled all 600 px; the profile ended at 586 px and the navigation list
scrolled independently. Mobile navigation closed successfully and the viewport
override was reset. The rebuilt frontend and `npm test` passed again.

## Export field explanations

All five export controls now have visible labels and per-field descriptions
associated through `aria-describedby`. Rows to skip explains 0 and gives a
100-row example. Format, maximum rows, local dates, inclusive start/exclusive
end, inherited filters and All rows restrictions are explained using the
existing query behavior. Task drilldown also has a visible Task ID label and
an explanation of the summary and applied filters.

Chrome verified aligned desktop controls, five nonempty label/description
associations, and stacked fields with visible help at 390 px without horizontal
overflow. Selecting All rows disabled Rows to skip, Preview, Copy URL and Copy
curl while leaving Download enabled; returning to 1,000 restored the controls.
No export behavior or API contract changed. Build and frontend tests passed.
