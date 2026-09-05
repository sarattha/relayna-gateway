# Admin UI component guidance verification

Date: 2026-09-05. Branch: `codex/admin-ui-3-followup-fixes`.
Local demo: `http://127.0.0.1:20381/admin-ui/` with representative sample data.

## Result

Visible labels, essential field explanations and accessible secondary tooltips
were reviewed across Admin UI component families. Frontend and full repository
verification passed. No runtime/API/submission behavior changed in this follow-up.

## Source coverage

The audit inventoried inputs, selects, textareas, grouped checkboxes, row actions,
metric cards, table headings and request investigation facts. Contextual guidance
covers key/policy limits; Usage, Traffic and audit filters; route and service
configuration; provider authentication and LiteLLM passthrough; pricing and
OpenAPI editors; Studio and Entra settings; health state; guardrail configuration;
portal memberships; workload identities; and owner dashboard filters.

Essential guidance remains visible and is linked to controls with
`aria-describedby`. Existing export and task guidance is preserved. Grouped
checkboxes have shared explanatory text. Picker rows and icon-only actions have
contextual accessible names. Inventory search, command search and Overview time
range now retain visible labels. Endpoint pricing has shared visible USD/zero
and stale-rule explanations.

Semantics were checked against policies, budgets, Redis limits, PostgreSQL usage
limits/queries and owner authorization source. Examples: zero RPM blocks requests;
zero budget blocks immediately; blank policy limits retain inherited restrictions;
zero minimum cost includes free records; zero UTC hour means midnight; zero
endpoint estimate means no estimated charge. No generic zero-means-unlimited rule
is used.

## Browser coverage

Chrome automation inspected 30 states, including these component families:

| Area | Checked |
| --- | --- |
| Main views | Overview, Traffic, Usage & cost, Health, Projects, Services, Providers, Routes, Virtual keys, Policies & guardrails, People & identities, Audit log, Settings |
| Expanded editors | Create project, service, provider and virtual key; edit service; guardrail editor; managed identity registration |
| Dynamic controls | Add/remove an unsaved pricing rule, endpoint pricing inputs, nested guardrail picker, moved/reopened forms |
| Mobile, 390 × 844 | Request investigation, expanded key editor, guardrail picker, Providers, expanded provider editor, visible inventory search |
| Interactions | Inventory filtering, tooltip hover/focus/click dismissal, hover into tooltip, keyboard navigation, Escape inside a drawer |

Across the inspected states, the DOM audit found no unnamed controls/buttons,
broken `aria-describedby` references or document horizontal overflow. Tables
retain their own horizontal scrolling. Key drawer checks found no duplicate
helper IDs or repeated hints after rendering/moving forms. Inventory filtering
still returns the matching row.

Screenshots confirmed readable desktop/mobile field help and tooltips. An initial
mobile investigation screenshot exposed a CSS specificity problem: metric styles
made tooltip text the same color as its background. After fixing it, rendered
text is white (`rgb(255,255,255)`) on dark green (`rgb(25,49,47)`), at 13 px/19.5 px.
The 320 px tooltip fits inside the 390 px viewport with 8 px edge clearance.

Focus shows help; a second click dismisses pinned help. First Escape removes a
drawer's tooltip and keeps the drawer open; second Escape closes the drawer.
Moving the pointer from a help trigger into its tooltip retains the tooltip.
Browser error logs were empty. The temporary viewport override was reset.

## Automated checks

- `npm run build:admin-ui`: passed; regenerated checked-in assets.
- `npm test`: passed, including contextual zero/blank guidance and viewport
  placement regression cases.
- `bash .codex/skills/code-change-verification/scripts/run.sh`: passed after the
  final source changes. Includes fmt, clippy, workspace tests, audit, deny,
  machete, nextest, Trivy, Gitleaks and Semgrep.
- `git diff --cached --check`: passed.

## Limits

Computer Use was attempted but the Mac was locked; browser automation provided
visual and interaction coverage instead. Click activation was checked at mobile
width, not on physical touch hardware. Owner-only dashboards and the signed-out
login state were reviewed in source, not retested under separate sessions.
The DOM audit is not a formal screen-reader/accessibility certification. No
configuration saves, external provider requests or credential changes were
needed for this presentation follow-up.
