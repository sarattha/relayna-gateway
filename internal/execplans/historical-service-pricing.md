# Optional historical service repricing

This living ExecPlan follows `PLANS.md` and extends draft PR #126.

## Purpose / Big Picture

When editing a registered service's pricing, an operator chooses new requests
only (the default) or recalculation of already recorded service costs for
reports. The user explicitly selected reports-only recalculation: budget
enforcement must continue using originally committed charges, including after
Redis recovery or a gateway restart.

## Progress

- [x] (2026-10-07) Read manifesto, engineering/implementation skills, UI guidance and existing pricing/usage/budget contracts.
- [x] (2026-10-07) Confirm reports-only historical updates with the user.
- [x] (2026-10-07) Add backward-compatible API option, historical attribution resolver, transactional store update and original budget-cost preservation.
- [x] (2026-10-07) Add shared service-edit choice/confirmation and result counts; document semantics and rollout.
- [x] (2026-10-07) Verify core/API/store/UI behavior, apply migration against disposable PostgreSQL/Redis, rebuild assets and run required checks; record the pre-existing Trivy failure below.
- [x] (2026-10-07) Push the verified feature and update existing draft PR #126 with the final title, scope, migration notes and verification limitations; prepare the handoff.
- [x] (2026-10-08) Prepare 0.1.42 workspace/lockfile metadata, changelog, current release references, operator/deployment docs and regenerated UI assets.
- [x] (2026-10-08) Verify 0.1.42 on pinned Rust 1.98.0, including workspace formatting/Clippy/tests/build, 407 Nextest tests with real dependencies, UI/coverage/type checks, strict docs, metadata and secret/static scans. Record the unchanged local Trivy limitation.
- [x] (2026-10-08) Prepare the final PR update and authorized landing workflow, guarded by the current head commit. GitHub PR #126 tracks the final automated-check and merge state; the user explicitly waived waiting for unavailable Codex review.

## Surprises & Discoveries

Historical usage retains cost source, rule name, method and rewritten endpoint
path; it does not retain request bodies. Unnamed or ambiguous body rules cannot
be replayed safely. Both named body rules and endpoint operation IDs appear in
pricing_rule_name. Upstream-reported costs cannot be recreated from a new fixed
estimate. Redis counters are live accounting; PostgreSQL budget reconstruction
currently sums the same estimated_cost column used by reports. Historical
updates therefore need a separate preserved budget charge.

The previous full verification run stopped at seven existing Trivy HIGH
findings in the unrelated prototype lockfile. Do not alter unrelated security
documents or dependencies as part of this feature.

The native collaborative preview supports interaction and DOM inspection, but
both text-only and screenshot snapshots fail in the preview client. Browser
verification used real UI interactions and DOM inspection against an isolated
gateway and a separate disposable `pricing_ui` database; no live data was used.

On release preparation, the Rust CI check was failing before tests: the moving
stable toolchain installed Rust 1.99.0 and Clippy flagged `double_must_use` in
external `async-trait` expansions throughout existing core traits. Local Rust
1.98.0 workspace Clippy passes with warnings denied. Pin local and CI/release
verification together rather than suppressing the lint or rewriting traits.

During the release pass, native preview metadata was available but browser
automation reported no connected host. HTTP checks against the disposable
gateway confirmed that its embedded HTML and JavaScript serve 0.1.42, and
automated UI tests verify both version labels. The prior feature's desktop and
mobile interaction verification remains documented above.

## Decision Log

- Decision: Add `reprice_existing_usage: bool` to service PATCH, default false.
  Rationale: Preserve released new-request-only behavior and make historical mutation explicit.
  Date/Author: 2026-10-07, Codex.
- Decision: Preserve the original budget charge in a nullable additive usage column on first repricing; baseline readers use that value when present.
  Rationale: User selected reports-only updates. Old writers remain compatible and no Redis format or counter changes are needed.
  Date/Author: 2026-10-07, user/Codex.
- Decision: Reprice by recorded attribution, retaining matching named body rules only when their selectors remain unchanged; do not replay newly introduced body selectors, ambiguous/unnamed rules or upstream passthrough charges.
  Rationale: Request payloads are not stored, so those matches cannot be reconstructed safely.
  Date/Author: 2026-10-07, Codex.
- Decision: Save service and usage changes in one repeatable-read transaction, processing usage in bounded batches.
  Rationale: Provide atomicity, serialize concurrent service edits and only touch requests already recorded in the transaction snapshot. In-flight requests retain their original resolved pricing.
  Date/Author: 2026-10-07, Codex.
- Decision: Index historical pagination by service and UUID.
  Rationale: Existing indexes sort service history by time; UUID batching otherwise repeatedly sorts the remaining history. The migration creates the matching index, with its write-lock rollout documented.
  Date/Author: 2026-10-07, Codex.
- Decision: Prepare patch release 0.1.42, update current image references and operator guidance, and pin verification to tested Rust 1.98.0.
  Rationale: Follow existing pre-1.0 release numbering and prevent compiler-channel drift from blocking this otherwise verified release. No additional public behavior changes are introduced by the release metadata.
  Date/Author: 2026-10-08, user/Codex.
- Decision: Merge after automated checks, bypassing only the unavailable review requirement if needed.
  Rationale: The user explicitly authorized landing and said not to wait for Codex review. Do not change repository protection rules or publish a release tag without a request.
  Date/Author: 2026-10-08, user/Codex.

## Outcomes & Retrospective

Implemented the additive PATCH option and reporting summary, shared edit UI,
transactional historical updates, persisted Traffic snapshot synchronization,
and preserved budget-charge readers. Focused core, API and real PostgreSQL/Redis
tests pass, including transaction rollback, repeated repricing, free original
charges, cross-service isolation, deterministic late-record exclusion and a
1,002-record batch. Desktop verification proved Cancel preserves
the service and usage, Confirm changes report cost from 0.1 to 0.0002 while
retaining budget cost 0.1, and audit action is `services:reprice`. At 390px,
reopening defaults to new-only, saving 0.00025 leaves the report at 0.0002, and
the choice/help and confirmation fit the viewport. UI build, complete Node
tests, 50 React tests, TypeScript checks, workspace formatting/lint/tests and
workspace build pass. Final verification against a fresh disposable database
passed cargo audit with the repository's existing ignores, deny, machete and
all 407 Nextest tests (zero skipped). The full script exits at Trivy because
the unchanged prototype lockfile has seven HIGH findings; production Cargo
and root npm lockfiles have zero HIGH/CRITICAL findings. Gitleaks history and
staged-change scans and Semgrep passed separately. This leaves the repository's
full verification gate failing for the documented pre-existing dependency
findings, while this feature's checks pass.

Published the feature in draft PR
https://github.com/sarattha/relayna-gateway/pull/126 on
`fix/service-price-precision`. Unrelated local security documents were retained
and excluded from both commits and PR scope. Disposable gateway and dependency
fixtures are stopped after verification.

Prepared release 0.1.42 with synchronized workspace/lockfile versions, changelog,
current documentation and deployment image examples, and regenerated UI labels.
All local code, UI, documentation and metadata checks pass on Rust 1.98.0;
407 Nextest tests pass with zero skipped and real PostgreSQL/Redis dependencies.
The full script still exits at the same seven unrelated, unfixed prototype
Trivy HIGH findings. Production lockfiles have no HIGH/CRITICAL findings.
Gitleaks history/staged-change scans and Semgrep pass separately. Final GitHub
CI results and the explicitly authorized merge are recorded on linked PR #126.

## Context and Orientation

`crates/gateway-core/src/services.rs` owns service PATCH and pricing resolution.
`crates/gateway-store/src/postgres.rs` persists service registrations and usage,
and reconstructs budget spend. The additive migration is under
`crates/gateway-store/migrations/`. `crates/gateway-api/src/app.rs` authorizes
service updates and records their audit snapshots. Its service response gains
an optional repricing summary only for explicit historical saves.
`crates/gateway-api/admin-ui/src/main.ts` provides the shared service editor,
including Foundry and Studio-imported services. Static assets must be rebuilt.

## Compatibility Boundary

Latest released boundary: v0.1.41. Existing PATCH clients and service responses
keep their behavior unless the new flag is true. Schema change is additive,
with no eager backfill and original budget semantics preserved. Roll out the
migration and updated budget readers to all replicas before using historical
repricing. Older readers ignore the preserved-charge column, so do not roll
them back after repricing without restoring report costs to original charges.
The user explicitly authorized historical reporting changes and chose budget
preservation; no additional approval is needed to implement or test them.

## Plan of Work

Add the optional PATCH flag and response summary. Resolve past charges against
the saved attribution and old/new service configuration; skip unresolved and
upstream charges. Lock the service row, apply pricing, batch historical records,
preserve their original budget charge once, update reporting cost/metadata and
commit together. Update both budget seed readers to use preserved charges.
Expose the choice, persistent help, confirmation and changed/skipped counts in
the shared edit form. Add tests for rollback, isolation, repeated repricing,
report totals, original budget spend and safe rule attribution.

## Concrete Steps

Use isolated local PostgreSQL and Redis containers for dependency-backed tests.
From the repository root run focused tests while iterating, then:

    npm run build:admin-ui
    npm test
    npm run test:react
    npm run typecheck:admin-ui
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features

## Validation and Acceptance

Default saves leave old costs untouched. Opt-in saves update matching historical
report costs and return changed/skipped counts. Different services, upstream
passthrough, ambiguous rules and in-flight records remain untouched. Budget
spend matches original charges after first/repeated updates and Redis reseeding.
Cancelled confirmation issues no request. Shared editors default to new-only
and explicit history saves preserve fractional amounts. Migration applies and
can be rerun through SQLx without drift. Audit captures the selected action and
result summary without request bodies or secrets.

## Idempotence and Recovery

Failed transactions roll back service and usage updates together. Repeated
repricing does not overwrite the original budget charge. Test fixtures are
disposable; never mutate live usage for verification. Retain the added budget
column when rolling back code after historical changes, or restore usage cost
first. Do not change historical Redis counters.

## Artifacts and Notes

Record verification results and PR details in the living sections.

Local evidence: `/tmp/relayna-history-final-verification.log`,
`/tmp/relayna-history-final-build.log`, `/tmp/relayna-history-ui-tests.log`,
`/tmp/relayna-history-react.log`, `/tmp/relayna-history-typecheck.log`,
`/tmp/relayna-history-gitleaks.log`, `/tmp/relayna-history-staged-gitleaks.log`
and `/tmp/relayna-history-final-semgrep.log`.

## Interfaces and Dependencies

No new production dependencies. Public service PATCH gains an additive flag;
service responses gain an optional result. Existing usage/export shapes remain
compatible, with an audit summary marking recalculation. Original budget
charges remain internal to the store.
