# Release 0.1.39 readiness

## Purpose and context

Prepare branch codex/traffic-filters-auth-profiles and PR #121 for review and merge. The latest released compatibility boundary is v0.1.37; 0.1.38 was an unreleased target. This task changes metadata and documentation, not runtime contracts. Existing branch work adds Admin UI 4.0, route authentication profiles (key-to-policy assignments), and Azure Foundry provider/agent forwarding. Content Safety is follow-up issue #122 only.

## Progress

- [x] Create Content Safety feature request and clarify deferred scope.
- [x] Audit current documentation and update version to 0.1.39.
- [x] Run verification and inspect final changes.
- [ ] Push release metadata, update PR, and mark ready.
- [ ] Monitor checks/reviews and merge when GitHub requirements are met.

## Plan and acceptance

Update Cargo workspace/lock versions, UI release labels, deployment examples and current docs together; regenerate embedded assets with npm run build:admin-ui. Preserve historical internal reports. Add missing Foundry migration/rollout guidance and clarify cancelled provider drafts. Run release metadata validation, strict MkDocs, frontend checks and the mandatory code-change-verification script with local PostgreSQL 15432 and Redis 16379. Build the workspace for release packaging. All must pass before pushing. Review latest PR checks and approvals before merging; never bypass branch protection.

## Surprises & Discoveries

Deployment notes omitted the Foundry migration. Existing draft-retention text incorrectly generalized provider dialogs. GitHub requires one approving review; no reviews were present at audit start.

## Decision Log

User clarified Content Safety belongs in the feature request only. Advance the unreleased patch target to 0.1.39 without inventing a published 0.1.38 release. Existing mock tests validate Foundry behavior; no live Azure validation is claimed.

## Outcomes & Retrospective

All ten mandatory verification commands passed, including 388/388 Nextest tests without skips. Workspace build, 49 React tests (100% statements/functions/lines; 99.71% branches), operational UI tests, strict typecheck, release metadata and strict MkDocs passed. Checked 1,793 local links/assets/anchors across 25 rendered pages with no failures. PR readiness is pending. If approval is unavailable after checks, retain the merge requirement and monitor for it rather than bypassing it.
