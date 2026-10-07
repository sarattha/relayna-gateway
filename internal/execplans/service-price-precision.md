# Allow small service route prices throughout the Admin UI

This ExecPlan follows `PLANS.md`. Keep its living sections updated.

## Purpose / Big Picture

Operators must be able to enter and save service default, request-rule and
OpenAPI endpoint prices below USD 0.001 without browser validation blocking
submission. Related policy limits and Usage filters must accept those amounts,
and positive tiny costs must remain visible in inventories and investigations.

## Progress

- [x] (2026-10-07) Read the manifesto, UI guidance and mandatory skills; trace all price entry, serialization, validation and storage paths.
- [x] (2026-10-07) Identify native number-input step restrictions as the cause.
- [x] (2026-10-07) Update all eight monetary input locations and small-cost presentation; add DOM regression coverage for price submission, reload, blank/zero and negative amounts.
- [x] (2026-10-07) Rebuild assets; npm test, all 50 React tests, TypeScript checking and git diff --check pass.
- [x] (2026-10-07) Rust formatting, Clippy, workspace tests, audit, deny, machete and all 399 nextest tests pass. Mandatory script stops at unrelated Trivy findings in the unchanged prototype lockfile.
- [x] (2026-10-07) Real-browser desktop/mobile create and PATCH preserve prices as small as 0.00000025. Check Monitor, Discover and Govern DOM layouts; endpoint table scrolls inside a 390px viewport.
- [x] (2026-10-07) Separate gitleaks check scans 388 commits with no leaks; Semgrep completes with zero findings. Prepare final handoff on fix/service-price-precision.

## Surprises & Discoveries

Service create/edit defaults use step 0.01; request rules and OpenAPI endpoint
rules use step 0.001. These reject valid prices with native stepMismatch
validation. `gateway-core/src/services.rs::validate_cost` only rejects negative
or nonfinite values (and a missing fixed estimate). PostgreSQL service prices
are double precision; JSON pricing rules preserve numeric values. UI serializers
use Number without rounding. Foundry pricing uses the shared service edit form;
Studio selection has no separate price input. The shared money formatter rounds
prices below 0.00005 to zero, and investigation formatting rounds below 0.0000005.

The verification script's Trivy step reports seven HIGH vulnerabilities in
`design-prototypes/service-owner-monitoring/package-lock.json`, which this patch
does not modify. Cargo.lock and the root package-lock.json report zero HIGH or
CRITICAL vulnerabilities. The script therefore did not reach gitleaks or
Semgrep; run those separately without claiming the complete script passed.
Browser snapshot capture fails in the available T3 preview. Browser interaction
and DOM/layout evaluation work, so checks cover those; screenshot-based visual
review is unavailable. The disposable Usage fixture initially lacked the API's
breakdowns field; correct the fixture before checking Usage.

## Decision Log

- Decision: Use step="any" with existing min="0" on monetary controls.
  Rationale: Match the existing API numeric contract without inventing another minimum increment. Retain browser rejection of negative amounts.
  Date/Author: 2026-10-07, Codex.
- Decision: Leave existing prototype dependency vulnerabilities outside this pricing fix.
  Rationale: The prototype lockfile is unchanged and unrelated to gateway price entry; report the failed mandatory security gate explicitly.
  Date/Author: 2026-10-07, Codex.
- Decision: Preserve small positive costs with up to 20 fractional digits in currency displays; retain ordinary summary currency formatting.
  Rationale: Prices accepted by the editor must not appear free after saving.
  Date/Author: 2026-10-07, Codex.

## Outcomes & Retrospective

All service price editors now accept fractional amounts without step mismatch,
including dynamically added request rules and Foundry edits. Related policy
limits and Usage filtering use the same numeric acceptance. Regression tests
preserve POST/PATCH numbers and existing zero/blank semantics, reject negative
amounts, and verify small prices remain visible. Generated app.js matches the
source changes. Backend and persisted contracts are unchanged from v0.1.41.
The full verification gate remains unsuccessful because of existing prototype
dependency findings; screenshot review is unavailable due to preview tooling.
The new plan and regression test are untracked additions; pre-existing untracked
security documents remain untouched.

Ready for review on `fix/service-price-precision`. Suggested PR title:
`fix: Allow sub-mill service route prices in every Admin UI editor`.
Draft description: This pull request fixes browser step validation blocking
service prices below USD 0.001. Service defaults, request rules, OpenAPI endpoint
prices, Foundry service edits, policy monetary limits and Usage filters accept
fractional values; tiny costs remain visible. It adds DOM and serialization
regression coverage and regenerates deployed assets without changing API or
database contracts. UI and Rust verification pass; the full security gate stops
on seven existing HIGH findings in the untouched prototype lockfile.

## Context and Orientation

`crates/gateway-api/admin-ui/src/main.ts` renders service create/edit, request
pricing rules, OpenAPI endpoint pricing, policy monetary limits and Usage
filters. It serializes prices through `serviceBody`, `pricingRuleFromRow` and
`syncEndpointPricingEditor`. Foundry shares serviceEditForm. Investigation costs
are rendered by `src/investigation.ts`. Vite writes checked-in deployment assets
to `crates/gateway-api/src/static/admin-ui/`. Tests live in `tests/` and run via
the root package.json.

## Compatibility Boundary

Latest release tag: v0.1.41. This is a UI correction within the released API
contract. All existing price values, blank/zero semantics, routes, persisted
formats and backend validation remain compatible. No migration or shim is
needed; the user's requested bug fix authorizes relaxing the accidental UI
restriction.

## Plan of Work

Replace fixed decimal steps on all monetary inputs. Adjust tiny-cost formatting.
Add DOM regression coverage for rendered create/edit forms, dynamically added
rules, endpoint rules, Foundry edits, policy limits and Usage filters. Exercise
serialization and POST/PATCH payloads without rounding. Build generated assets
from source. Use a disposable local fixture for real-browser checks.

## Concrete Steps

Run from the repository root:

    npm run build:admin-ui
    npm test
    npm run test:react
    npm run typecheck:admin-ui
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Values such as 0.0002, 0.000123456 and 0.00000025 pass native validity and
survive JSON serialization in all pricing editors. Zero and blank retain their
existing meanings; negative values remain invalid. Tiny positive cost displays
do not read as zero. Desktop and mobile smoke checks include login, service
drawers/rules/endpoint tables and Monitor, Discover and Govern navigation.

## Idempotence and Recovery

Tests and builds can be rerun. Only disposable browser fixture writes are
allowed during UI checks. Regenerate static assets rather than editing them.
Preserve the pre-existing untracked security documents.

## Artifacts and Notes

Record final test results and browser observations in the living sections.

## Interfaces and Dependencies

No new production dependencies, API fields, schemas or numeric formats.
Regression tests use the existing jsdom and TypeScript development dependencies.
