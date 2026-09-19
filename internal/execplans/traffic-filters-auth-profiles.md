# Traffic filters and route authentication profiles

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Repair applied Traffic filters using simulated requests through a real local gateway, then implement issue #120: route-local named authentication profiles selected by an authenticated Relayna virtual key's explicit route binding. Entra profiles must never downgrade to key-only. Operators manage profiles and bindings and inspect historical authentication outcomes in Traffic and Usage.

## Progress

- [x] (2026-09-20) Aligned Assigned keys and popup typography with form labels, softened key summaries and narrowed the popup. UI tests/build and gateway build passed; Computer Use verified desktop and 390px layouts.

- [x] (2026-09-20) Replace inline key search with a modal popup, isolated selection drafts, Apply/Cancel, search reset and available-only results; validate nested-dialog focus and desktop/mobile layouts. Clarify many keys per profile / one profile per key per route. UI regressions and all ten checks pass (368/368).

- [x] (2026-09-20) Replace manual profile UUID entry with searchable key inventory, selected summaries and explicit add/remove; test loading failures, duplicate prevention and project restrictions; validate desktop/mobile with Computer Use. All ten checks pass (368/368), UI regressions pass and changed Rust coverage remains 137/137.

- [x] (2026-09-20) Remove duplicated inline tooltip explanations, retain concise profile status, verify desktop/narrow layouts and all ten checks (368/368 tests); deliver through PR #121.

- [x] (2026-09-19) Read repository skills, UI rules, manifesto and issue #120; created `codex/traffic-filters-auth-profiles` from clean main at 872614b.
- [x] Reproduced whitespace filter failures with real mock-upstream traffic; fixed trimming, simultaneous project/key selection, and stale live-reader updates with mounted regressions.
- [x] Implemented core profiles, atomic PostgreSQL revisions/references, request-plane enforcement before passthrough shortcuts, and Accessa per-turn snapshots/revalidation.
- [x] Implemented route/service profile editing, key assignment inspection, bounded historical diagnostics and rollout/operator documentation.
- [x] Complete Computer Use desktop/narrow validation and >98% changed production Rust line coverage (137/137, 100%).
- [x] Finished the final verification sequence: 368 nextest tests passed, zero skipped; all ten mandatory checks passed.
- [x] Prepared the verified branch and draft PR description for publication.

- [x] (2026-09-19 follow-up) Add contextual guidance/tooltips and dynamic endpoint/profile fields, preserving drafts while excluding inactive inputs.
- [x] Audit all Traffic filters across live/history, combinations, reason chips, mode transitions and pagination; extend behavioral regressions.
- [x] Rebuild, validate with Computer Use and rerun all ten mandatory checks (368/368, zero skipped); publish follow-up through draft PR #121.

## Surprises & Discoveries

The Traffic project callback clears a newly submitted key whenever project changes. The live reader lacks a generation check after awaiting a read, potentially overwriting history after a mode switch. Mounted regressions now prove both. Real traffic also reproduced surrounding-whitespace filters returning no rows. Computer Use found the overflowing bottom Add button, invalid HTML pattern under Unicode-v rules, and errors outside the dialog; all were corrected. Generic LiteLLM passthrough had a second governance bypass that also needed to exclude explicitly profiled routes.

Follow-up: uppercase UUIDs and zero-padded status values disagreed with live predicates; an old history cursor remained usable while filters reloaded. Normalization and cursor invalidation now have regression coverage. Previous tests verified the reproduced failures but did not exhaust every filter. The profile editor also exposed Entra-only controls in key-only mode and lacked field-level guidance.

## Decision Log

- 2026-09-20 picker: preserve existing profile binding payloads and released key/project APIs. Search existing inventory locally; retain saved selections if details fail to load. Registered-service project ownership and server validation remain authoritative.

- 2026-09-19: Preserve released v0.1.37 contracts; new profiles are explicit opt-in, route-local, and use exactly one key binding per canonical route. Legacy inheritance and native LiteLLM behavior remain unchanged outside opted-in routes. User explicitly requested issue #120 implementation including this compatibility boundary.
- 2026-09-19: Use the existing strict >98% changed executable production Rust line coverage gate, documenting denominator and exclusions; separately report whole-workspace coverage rather than conflate the metrics.

- 2026-09-19: Store bounded profiles and bindings in existing EndpointAccess JSON. PostgreSQL triggers enforce optimistic revisions and key ownership; old readers reject new configuration and old writers cannot erase it. Opted-in routes cannot silently return to legacy mode. Accessa requires reconnect after any profile configuration edit.

- 2026-09-19 follow-up: this UI change preserves v0.1.37 API/persistence contracts. Hide and disable inactive controls without clearing drafts; require only the Entra audience, with optional scope/role/group restrictions. Use the shared accessible tooltip installer.

## Outcomes & Retrospective

Follow-up completed: contextual field tooltips and dynamic identity controls preserve drafts and omit inactive fields. Every Traffic filter now has independent and combined regressions; normalization and pagination defects found during the audit are fixed. Computer Use desktop/narrow checks and the full mandatory verification sequence pass. Changed production Rust coverage stays 137/137 (100%).

Implemented the Traffic fixes and issue #120. Full LLVM suite plus the final focused policy/ownership regressions produce 137/137 changed executable production Rust lines covered. Desktop and 390px Computer Use checks verify profile creation, rename, disable, save/reopen, key assignments, inline validation, Traffic live/history filtering and Traffic/Usage snapshots. All mandatory checks passed on the final code. The verified branch is ready for draft PR publication. Detailed reproduction, test commands, coverage denominator/exclusions and Computer Use evidence are recorded in `internal/test-reports/traffic-profiles.md`.

## Context and Orientation

Repository root is `/Users/jobz/Works/relayna-gateway`. `crates/gateway-api/admin-ui/src/traffic.ts` mounts the live/history UI and `main.ts` synchronizes scope. `gateway-core/src/endpoint_access.rs` owns endpoint identity policy, `gateway-proxy/src/pingora_plane.rs` enforces requests, and gateway-store persists configuration/usage. A virtual key is a gateway-authenticated caller credential; Entra is the additional JWT identity policy. LiteLLM is the upstream provider proxy. Accessa binds HTTP/WebSocket channels and admits each turn separately.

## Compatibility Boundary

Latest release is v0.1.37. Preserve existing routes, key headers, aliases, inherited policy, native direct credentials and history JSON. New profile configuration must fail closed on old readers, support atomic revision checks, and never silently reassign a key to weaker authentication. No multi-issuer or native LiteLLM profile is included.

## Plan of Work

First reproduce and fix mounted Traffic filtering and stale transport updates. Then settle profile storage, revisions and binding validation; integrate selection before credential shortcuts, enforce Accessa per-turn revalidation, add scoped audited APIs and UI editors, and snapshot sanitized outcomes in diagnostics. Extend real-service E2E and unit tests, document rollout/rollback and cache semantics, and regenerate static assets.

## Concrete Steps

Run from repository root: `npm run build:admin-ui`, `npm test`, `bash .codex/skills/code-change-verification/scripts/run.sh`, instrumented `cargo llvm-cov --workspace --all-features --lcov --output-path /tmp/traffic-profiles.lcov`, and `python3 scripts/check-changed-rust-coverage.py /tmp/traffic-profiles.lcov 872614b`. Use isolated PostgreSQL/Redis and mock upstream/OIDC for request tests. Run strict docs and release metadata checks. Create a draft PR only after evidence is recorded.

## Validation and Acceptance

Filters must apply to live updates and history, retain submitted compatible project/key filters, and resist stale reads. Profiles must independently enforce full policy, reject missing/ambiguous/disabled bindings and credentials, preserve upstream isolation and policy/budget/rate checks, and prohibit Accessa key-only access. CRUD must honor RBAC, ownership, revisions and audit. Computer Use verifies create/edit/save/reopen and Traffic inspection on desktop/narrow layouts. Changed production Rust executable line coverage must exceed 98% without excluding failure paths.

## Idempotence and Recovery

Use named disposable containers and database fixtures; do not alter existing deployments. Retry fail-fast verification from the start after repairs. Do not overwrite unrelated work. Keep additive schema state during application rollback; document old-reader fail-closed behavior before enabling profiles.

## Artifacts and Notes

Issue: https://github.com/sarattha/relayna-gateway/issues/120. Screenshots and concise verification results will be recorded with the final implementation.

## Interfaces and Dependencies

Existing Rust workspace, Vite/TypeScript UI, PostgreSQL, Redis, mock OIDC/JWKS and mock LiteLLM. Profile selection will use authenticated key identity and canonical route ownership, never unsigned JWT claims or client selector headers. Historical snapshots exclude credentials and payloads.
