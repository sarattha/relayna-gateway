# Generate omitted profile IDs

This living plan follows `PLANS.md` in `/Users/jobz/Works/relayna-gateway`.

## Purpose and compatibility

The Admin UI lets operators leave a new authentication profile's ID blank. On
save, form serialization derives an ASCII slug from its name and adds a random
suffix within the existing 1–64 character ID format. Explicit IDs remain valid.
Saved IDs survive profile renames or a cleared ID field. Backend schemas, API
requirements, revision checks and persisted bindings remain unchanged relative
to v0.1.37; this is client-side convenience, not a new API default.

## Progress

- [x] (2026-09-20) Inspect React editor and shared form serialization.
- [x] (2026-09-20) Implement generation, preserved saved IDs and field guidance.
- [x] (2026-09-20) Cover names, bounds, collisions, bindings and mounted optional fields.
- [x] (2026-09-20) Verify UI/build/coverage and repository stack; Computer Use save/reopen.
- [x] (2026-09-20) Prepare verified change and update for existing draft PR #121.

## Decision Log

- Generate only on submission, using the full profile name and Web Crypto's
  random hexadecimal suffix. Non-ASCII-only names use `profile` as the slug. Reserve all
  explicit IDs before generation and retry collisions. Do not derive IDs again
  from a renamed saved profile. `crypto.getRandomValues` also works for Admin UI
  deployments over HTTP, without requiring `crypto.randomUUID` secure-context support.
- Preserve each saved ID in a hidden form field, so leaving the editable ID
  blank does not accidentally replace an established identity.

## Plan and validation

Update `admin-ui/src/auth-profiles.ts` under gateway-api for the shared serializer
and initial fields, `src/react/identity-editor.tsx` for the mounted field, and
shared guidance. Test generated IDs and bindings, manual and saved IDs,
normalization, long names and deterministic collision retries. Run UI build,
regressions, React typecheck/coverage (>98%), and the repository verification
script. Rebuild the embedded gateway and use Computer Use on the localhost demo
to save a disabled, unbound test profile, reopen and verify its generated ID;
remove only that test profile afterward without disturbing existing bindings.

## Surprises & Discoveries

The legacy form serializer is still the shared save path for React route and
service editors. Generating there covers both without changing their wire format.

## Recovery

Source changes are reversible; regenerate embedded assets after changes. Test
profiles have no keys and remain disabled. Cleanup uses the current revision
and preserves all pre-existing profiles and bindings. No migration is required.

## Outcomes & Retrospective

UI build, regression suite, React typecheck and strict MkDocs build pass. All 27
React tests pass with 100% statements (224/224), branches (106/106), functions
(100/100) and lines (210/210) in the measured component scope.

Computer Use saved a disabled, unbound `Profile ID QA` with a blank ID on the
local demo, then reopened `profile-id-qa-fd66dd87`. Renaming it and clearing its
ID retained the same saved ID. Cleanup removed only this test profile; an API
comparison confirmed all original profiles and bindings were preserved. Local
screenshots are `internal/test-reports/admin4/profile-id-blank.jpg` and
`profile-id-persisted.jpg` (ignored by repository policy).

All ten repository verification commands pass, including 373/373 nextest tests
with zero skips. Log: `/tmp/profile-id-verification.log`. No Rust production
source changed; the previous complete-PR changed-line LLVM result remains
162/162 (100%), distinct from the React component coverage above.

Prepared the existing draft PR #121 update on `codex/traffic-filters-auth-profiles`.
