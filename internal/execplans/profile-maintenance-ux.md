# Make authentication profile maintenance understandable

This living plan follows PLANS.md in /Users/jobz/Works/relayna-gateway.

## Purpose / Big Picture

Operators should understand how to change profiles and assignments, why an
operation is unavailable, and how to correct invalid input without losing their
draft. Profile mode still requires at least one saved profile and exactly one
profile assignment per key per route.

## Progress

- [x] (2026-09-20) Reviewed shared route/service editor, serializer, key picker and error delivery.
- [x] (2026-09-20) Implemented removal guidance, Undo/focus recovery, named validation errors and persistent key-conflict details.
- [x] (2026-09-20) 32 React tests pass: 100% statements (251/251), branches (150/150), functions (107/107), lines (230/230). UI build, JavaScript regressions, typecheck and strict docs build pass.
- [x] (2026-09-20) Computer Use: last-profile removal disabled, assigned-key removal explained, unbound draft removal/Undo restores values and focus, duplicate-name error identifies profile 3, post-render scrolling exposes the message at desktop and 390px. Canceled drafts; revisions remain unchanged.
- [x] (2026-09-20) All ten mandatory verification steps passed; nextest 377/377 with zero skips. One guardrail test had a non-failing process-output cleanup warning; isolated rerun passed cleanly (1/1).
- [x] (2026-09-20) Prepared verified implementation and summary for existing draft PR #121.

## Surprises & Discoveries

Computer Use found that the legacy presenter scrolled before React committed
error text, so the error could remain below the visible scroll area. React now
focuses and scrolls the rendered message after commit; a regression asserts
that the message text exists when scrolling occurs.

The profile serializer groups missing names, duplicate IDs and malformed IDs
into one error. It measures UTF-16 characters where the backend measures UTF-8
bytes, so some names/audiences pass local validation then fail at the API.
Removal currently allows the last saved profile to disappear into an invalid
draft. Key-picker conflicts do not name the problematic key.

## Decision Log

- 2026-09-20: Apply karpathy, implementation-strategy and code-change-verification.
  Compatibility boundary is v0.1.37. This change improves UI validation and
  draft maintenance while preserving backend authentication, API and database
  contracts. It does not introduce a reset to legacy authentication.
- Keep normal controls compact. Show contextual explanations only for blocked
  actions and errors. A removed profile is only a draft change; provide Undo and
  retain Cancel as the way to discard all edits.

## Context and Orientation

Vite source lives in crates/gateway-api/admin-ui/src. auth-profiles.ts serializes
profile drafts and routes errors into active dialogs. react/identity-editor.tsx
owns profile editing; react/key-picker.tsx owns isolated key-selection drafts.
main.ts owns API writes and passes errors to the shared presenter. Generated
app.js and app.css are rebuilt, never edited directly.

## Plan of Work

Split validator errors by actual cause and identify the affected profile and
field. Match backend length validation. Give saved-profile removal clear,
accessible restrictions and reversible draft removal. Give picker conflicts
specific key names and corrective actions. Keep genuine server errors visible
with appropriate recovery guidance and preserve draft values.

## Validation and Acceptance

Run npm run build:admin-ui, npm test, npm run test:react:coverage and npm run
typecheck:admin-ui. Test removal restrictions, Undo, draft preservation,
invalid/duplicate input, Unicode boundaries and key conflicts. Coverage remains
above 98%. Use Computer Use on the local demo without saving policy changes.
Run .codex/skills/code-change-verification/scripts/run.sh with local PostgreSQL
and Redis; require all ten steps to pass before pushing.

## Idempotence and Recovery

No migration or backend setting change. Cancel browser drafts after validation.
Regenerate static assets after edits. All tests are repeatable against local
fixtures. Keep screenshots under ignored internal/test-reports/admin4.

## Outcomes & Retrospective

Implemented the maintenance flow without changing backend authentication rules.
Computer Use caught a real timing bug that unit text-presence assertions alone
missed: errors existed but were scrolled out of view. A new regression checks
that the error text is present when focus/scroll runs after React commits it.

Local ignored screenshots: `internal/test-reports/admin4/profile-last-removal-guidance.jpg`,
`profile-removal-undo.jpg`, `profile-duplicate-name-error-visible.jpg`, and
`profile-error-mobile.jpg`. Full verification passed (`/tmp/profile-maintenance-verification.log`), as did
the targeted guardrail rerun (`/tmp/profile-maintenance-nextest-recheck.log`).
The UI build, regression suite, typecheck and coverage were rerun after the
Computer Use timing fix. No saved policies or backend contracts changed.
