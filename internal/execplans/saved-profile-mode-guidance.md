# Explain saved authentication profile restrictions

This living plan follows `PLANS.md` in `/Users/jobz/Works/relayna-gateway`.

## Purpose / Big Picture

An operator editing a route with saved authentication profiles must see why
legacy identity modes cannot be selected, before attempting to save. A profile
is an identity policy with explicitly assigned virtual keys. Removing the whole
profile configuration by changing modes is already prohibited by the backend.

## Progress

- [x] (2026-09-20) Confirmed saved-profile removal and stale revisions share a backend conflict error.
- [x] (2026-09-20) Added mode restrictions, explanatory text, preset preservation and serialization guard.
- [x] (2026-09-20) UI build, regression suite, typecheck, strict docs build and 28 React tests pass; React coverage is 100% statements (228/228), branches (120/120), functions (100/100), lines (212/212).
- [x] (2026-09-20) Computer Use confirms saved chat route disables all legacy modes with visible guidance; embeddings can switch to profiles and back before saving. Canceled both drafts without policy writes.
- [x] (2026-09-20) All ten mandatory verification steps passed; nextest 377/377 with zero skips.
- [x] (2026-09-20) Prepared verified changes and summary for existing draft PR #121.

- [x] (2026-09-20) Follow-up: split empty-profile, profile-count and assignment-count errors; add actionable empty-state guidance with regressions.
- [x] (2026-09-20) Computer Use: empty Save gives precise missing-profile error; Add clears error and prompt and focuses new profile. Canceled without saving. UI build, regressions, typecheck and 29 React tests pass; coverage is 100%, including 122/122 branches.
- [x] (2026-09-20) Empty-state follow-up: all ten verification steps passed; nextest 377/377, no skips (`/tmp/profile-empty-verification.log`).

## Surprises & Discoveries

An empty draft previously produced a combined 1–32 profiles / 1,024 bindings
error, obscuring the actual missing-profile problem.

Service type presets can also set the identity mode. They must preserve saved
profiles even when a preset normally chooses a single Entra policy.

## Decision Log

- 2026-09-20: Preserve backend restriction and existing conflict response. Disable
  unavailable options only for persisted explicit profiles (positive revision).
  Draft profiles remain reversible before their first save. Preserve genuine
  stale-edit reload guidance. No database, public API or authentication changes.
- Compatibility boundary: latest release v0.1.37; this UI correction changes no
  released wire format or persisted data. No migration or compatibility shim.

## Context and Orientation

`crates/gateway-api/admin-ui/src/react/identity-editor.tsx` owns the shared React
route/service editor. `src/main.ts` renders initial fields, applies service
presets and serializes forms. Tests live in `tests/admin-ui-react.test.tsx` and
`tests/admin-ui.test.mjs`. Generated assets are rebuilt from Vite sources.

## Plan of Work

Disable legacy choices for persisted profiles, connect concise visible guidance
to the selector, preserve profiles through service presets, and reject unsupported
form submissions before HTTP. Test retained revisions, bindings, unsaved mode
switches and genuine stale-error display. Validate in Chrome without saving
changes to existing route policies.

## Validation and Acceptance

From the repository root run `npm run build:admin-ui`, `npm test`,
`npm run test:react:coverage`, `npm run typecheck:admin-ui` and
`bash .codex/skills/code-change-verification/scripts/run.sh` with local test
PostgreSQL and Redis configured. React coverage must remain above 98%. Rebuild
the local gateway and use Computer Use to inspect a saved route and an unsaved
route, confirming unavailable legacy choices and reversible unsaved choices.

## Idempotence and Recovery

All checks can be repeated. No migration or policy mutation is needed. Cancel
browser drafts after validation. Regenerate assets instead of editing them.

## Outcomes & Retrospective

Saved profile restrictions are now explained before save. Service presets and
form serialization preserve the same restriction; real stale errors retain
reload guidance. Browser screenshots are local ignored evidence at
`internal/test-reports/admin4/saved-profile-mode-restriction.jpg` and
`internal/test-reports/admin4/unsaved-profile-mode-reversible.jpg`.
Full repository verification passed (`/tmp/profile-mode-verification.log`);
UI coverage exceeds 98%. No backend contracts or saved policies changed.

Follow-up evidence: `internal/test-reports/admin4/empty-profile-guidance.jpg`
and `internal/test-reports/admin4/empty-profile-validation.jpg` (ignored local
screenshots). Complete removal of profile mode is a separate product decision;
this follow-up changes only empty-state guidance and validation messages.
