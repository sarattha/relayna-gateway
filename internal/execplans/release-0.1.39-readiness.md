# Release 0.1.39 readiness

## Purpose and context

Prepare branch codex/traffic-filters-auth-profiles and PR #121 for review and merge. The latest released compatibility boundary is v0.1.37; 0.1.38 was an unreleased target. This task changes metadata and documentation, not runtime contracts. Existing branch work adds Admin UI 4.0, route authentication profiles (key-to-policy assignments), and Azure Foundry provider/agent forwarding. Content Safety is follow-up issue #122 only.

## Progress

- [x] Create Content Safety feature request and clarify deferred scope.
- [x] Audit current documentation and update version to 0.1.39.
- [x] Run verification and inspect final changes.
- [x] Push release metadata, update PR, and mark ready.
- [ ] Monitor checks/reviews and merge when GitHub requirements are met.

## Plan and acceptance

Update Cargo workspace/lock versions, UI release labels, deployment examples and current docs together; regenerate embedded assets with npm run build:admin-ui. Preserve historical internal reports. Add missing Foundry migration/rollout guidance and clarify cancelled provider drafts. Run release metadata validation, strict MkDocs, frontend checks and the mandatory code-change-verification script with local PostgreSQL 15432 and Redis 16379. Build the workspace for release packaging. All must pass before pushing. Review latest PR checks and approvals before merging; never bypass branch protection.

## Surprises & Discoveries

Deployment notes omitted the Foundry migration. Existing draft-retention text incorrectly generalized provider dialogs. GitHub requires one approving review; no reviews were present at audit start.

## Decision Log

User clarified Content Safety belongs in the feature request only. Advance the unreleased patch target to 0.1.39 without inventing a published 0.1.38 release. Existing mock tests validate Foundry behavior; no live Azure validation is claimed.

## Outcomes & Retrospective

All ten mandatory verification commands passed, including 388/388 Nextest tests without skips. Workspace build, 49 React tests (100% statements/functions/lines; 99.71% branches), operational UI tests, strict typecheck, release metadata and strict MkDocs passed. Checked 1,793 local links/assets/anchors across 25 rendered pages with no failures. PR readiness is pending. If approval is unavailable after checks, retain the merge requirement and monitor for it rather than bypassing it.


## Review follow-up (2026-09-20)

Automated review found project reassignment could bypass service profile ownership, identity changes retained stale secrets, and network token refresh held a global cache lock. All three are in the authorized merge-readiness scope. Latest released boundary remains v0.1.37. Preserve already applied migration checksums: add a migration replacing the profile guard with a key row update lock and adding the reverse key-project guard. Reject incompatible ownership changes atomically with an actionable error, rather than silently deleting assignments. Clear irrelevant secrets in the backend as well as the UI. Release the token map lock before I/O and recheck before caching to preserve newer revisions. Verify database ownership and identity transitions through real E2E, cached access during a deliberately stalled refresh, and mounted React submission; then rerun mandatory checks. No Content Safety implementation.

Focused ownership E2E (including a concurrent binding-save/key-move), Foundry identity-transition E2E, and stalled token-cache regression pass. Frontend checks pass with 50 mounted tests, 436/436 statements, 348/349 branches, 159/159 functions and 358/358 lines. Strict docs and workspace build pass. Iteration caught and fixed a test method typo and socket read-count lint; a first proxy startup attempt failed before the tested changes and passed on rerun. The project-clearing store test uses an explicit nullable patch because existing JSON null decoding omits that field; this review does not alter the released PATCH contract. All ten mandatory verification commands passed after the fixes, including 388/388 Nextest tests, zero skipped, and security scans with existing advisory exceptions. The three review findings are ready to close with this follow-up commit; merge still awaits required GitHub approval and checks for its new head.


## Second review follow-up

Review of 23897fb found stale read/merge/write behavior in provider PATCH. Serialize the existing provider read and update using one transaction and SELECT FOR UPDATE. This preserves released request/response shapes and migration state while preventing lost updates and invalid identity/credential combinations. Test a rename waiting behind an uncommitted identity/secret change, then assert the new identity is retained and rejected edits release the transaction for a valid retry. Run focused Foundry E2E and the complete mandatory stack before pushing. No additional feature scope.

Second-review verification: focused Foundry E2E, workspace build, release metadata and all ten mandatory verification commands pass, including 388/388 Nextest tests with no skips. No new exceptions or frontend changes. Ready to push and request review of the transaction fix.
