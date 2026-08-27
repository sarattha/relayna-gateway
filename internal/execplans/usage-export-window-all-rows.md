# Add Bounded All-Rows Usage Exports

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Operators should be able to give a usage export its own exact start and end
timestamps and download every matching usage row. The Admin UI will make the
export window explicit and will retrieve an all-rows export through repeated,
ordered requests so the existing server-side maximum of 10,000 rows per request
continues to protect PostgreSQL, Gateway memory, and individual HTTP responses.
The completed change will be exercised in the real browser UI, reviewed through
the existing GitHub pull request, and included in the next release metadata.

## Progress

- [x] (2026-08-27 16:06Z) Read the repository design, Admin UI design system,
  current export implementation, and compatibility guidance.
- [x] (2026-08-27 16:06Z) Chose an additive UI implementation that preserves
  the released export API limit and route shapes.
- [x] (2026-08-27 16:14Z) Added export-only time-window controls and safe
  all-rows batching for JSON and CSV downloads.
- [x] (2026-08-27 16:16Z) Added Admin UI regression coverage and operator
  documentation.
- [x] (2026-08-27 23:01Z) Completed the real Safari flow through Computer Use.
  A Last 30d Entra-authenticated All rows download disabled offset, preview,
  URL, and curl controls, and produced a 169-line CSV containing one header and
  all 168 matching usage rows.
- [x] (2026-08-27 17:08Z) Regenerated checked-in Admin UI assets and ran the
  available verification stack. Formatting, clippy, workspace tests, a clean
  RustSec audit, cargo-deny, cargo-machete, 314 nextest cases, Trivy, and
  Gitleaks passed. Semgrep is environment-blocked because its runtime cannot
  initialize the sandboxed macOS trust store, and escalation was rejected to
  avoid enabling unapproved source or telemetry transmission.
- [ ] Commit, push, update the pull request, mark it ready, and request Codex
  review.
- [ ] Address the first Codex review with replies and resolved threads.
- [ ] Bump the release target, changelog, and release-facing documentation;
  re-verify, commit, and push.

## Surprises & Discoveries

- Observation: the Usage page already supports a custom time range, and the
  export currently inherits it indirectly through `usageQueryFromForm`.
  Evidence: `crates/gateway-api/admin-ui/src/main.ts` builds an export URL from
  the Usage form before applying export limit and offset.
- Observation: both PostgreSQL and the in-memory store clamp each export
  request to 10,000 rows, while results are ordered by creation time and request
  ID and accept an offset.
  Evidence: `crates/gateway-store/src/postgres.rs` and
  `crates/gateway-api/src/app.rs`.
- Observation: Computer Use visual QA showed that the existing `inline-form`
  class did not lay out the export controls; every control occupied a separate
  full-width row at desktop width.
  Evidence: the rebuilt local Admin UI at
  `http://127.0.0.1:18381/admin-ui#/usage` in Safari.
- Observation: the real Entra administrator session could load the Usage page
  but could not execute an export because the export fetch path sent only a
  break-glass bearer header and omitted the session CSRF header.
  Evidence: the export fetches in `crates/gateway-api/admin-ui/src/main.ts`
  bypassed the shared `api` header construction used by working dashboard
  requests.
- Observation: the repository verification script's cached RustSec checkout
  contained untracked duplicate advisory files, so `cargo audit` rejected the
  database before scanning the lockfile.
  Evidence: a fresh temporary checkout loaded 1,226 advisories and completed
  the lockfile audit with only the repository's allowed warnings.
- Observation: Semgrep cannot initialize its X509 authenticator in the
  filesystem sandbox because the macOS trust anchors are unavailable.
  Evidence: both normal and metrics-disabled scans fail before rule execution;
  an unsandboxed retry was rejected because it could permit unapproved network
  telemetry or source transmission.
- Observation: Safari initially appeared not to download the generated Blob,
  but the browser was waiting behind its one-time per-site download permission
  dialog.
  Evidence: accepting Safari's explicit 127.0.0.1 download prompt produced the
  expected 169-line CSV without another export request.
- Observation: the first Codex review found that export-range replacement ran
  after inherited Usage-range validation and that `created_at, request_id` was
  not a total database order because request IDs are client controlled.
  Evidence: review threads on PR #108 against `main.ts` and the PostgreSQL
  export query.

## Decision Log

- Decision: preserve the released 10,000-row server limit and implement “All
  rows” as browser-side pagination in 10,000-row batches.
  Rationale: this satisfies the operator workflow without turning one API call
  into an unbounded query or changing the public route contract.
  Date/Author: 2026-08-27 / Codex.
- Decision: require a bounded start and end time for an all-rows download and
  support it only for Download, not Preview or single-URL/curl actions.
  Rationale: large previews can freeze the browser, and the existing endpoint
  intentionally cannot represent an unlimited request in one URL.
  Date/Author: 2026-08-27 / Codex.
- Decision: export-specific timestamps override the Usage page timestamps only
  when either export timestamp is entered; otherwise the export continues to
  inherit the active Usage time filter.
  Rationale: existing operator behavior stays compatible while the new controls
  make an exact export window possible.
  Date/Author: 2026-08-27 / Codex.
- Decision: integrate the export workflow into the current
  `feat/litellm-rerank-routes` branch and its existing pull request, as the user
  explicitly requested renaming and updating that PR rather than opening a new
  one.
  Rationale: preserve the branch and unrelated existing work while extending
  its production-freeze scope.
  Date/Author: 2026-08-27 / Codex.
- Decision: add a dedicated responsive `usage-export-form` grid and reuse the
  existing `actions` class for the buttons.
  Rationale: Computer Use exposed a material scanning and density problem that
  could not be expressed by the repository's existing form classes.
  Date/Author: 2026-08-27 / Codex.
- Decision: centralize export authentication headers so break-glass sessions
  send a bearer token and Entra browser sessions send their CSRF token.
  Rationale: both supported administrator authentication modes must be able to
  use the export workflow.
  Date/Author: 2026-08-27 / Codex.
- Decision: determine whether an export-specific range exists before building
  the inherited Usage query, and omit inherited date validation when the
  export range replaces it.
  Rationale: an explicit export override must be independent of stale or
  invalid custom Usage timestamps, as promised by the panel.
  Date/Author: 2026-08-27 / Codex review follow-up.
- Decision: add the unique `usage_events.id` as the final PostgreSQL export
  ordering key without exposing it in the export schema.
  Rationale: repeated OFFSET batches need a total order, while the released
  JSON and CSV response shapes remain unchanged.
  Date/Author: 2026-08-27 / Codex review follow-up.

## Outcomes & Retrospective

Implementation and verification are pending.

## Context and Orientation

The operator Usage view is rendered by
`crates/gateway-api/admin-ui/src/main.ts`. Its export panel calls the existing
`/admin-ui/admin/usage/export.json` and `.csv` routes. The store orders rows by
`created_at` and `request_id`, applies `limit`, and applies `offset`; each call
is capped at 10,000 rows. Generated browser assets live under
`crates/gateway-api/src/static/admin-ui/` and must be rebuilt from the source
package. Static contract tests live in `tests/admin-ui.test.mjs`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.29`; the usage-export HTTP
routes, query fields, output shapes, and 10,000-row cap are released behavior.
The implementation is additive in the Admin UI and composes the released
`from`, `to`, `limit`, and `offset` query fields. No backend shim, migration, or
wire-format change is needed.

## Plan of Work

Update the export panel in `crates/gateway-api/admin-ui/src/main.ts` with exact
datetime inputs and an “All rows” choice. Add helpers that validate the final
bounded time window, fetch ordered 10,000-row pages, merge CSV pages without
duplicating headers, assemble JSON pages into one valid document, and download
the result. Keep Preview, Copy URL, and Copy curl limited to one API request and
explain that all-rows mode is download-only.

Extend `tests/admin-ui.test.mjs` with source-level assertions for the controls,
validation, pagination, and CSV header handling. Update `docs/admin-portal.md`
to document the export override, bounded all-rows behavior, and the unchanged
API cap. Rebuild checked-in assets from the Vite package. Exercise the rendered
form and its all-rows state with Computer Use. After verification, update the
existing pull request and request Codex review, address its first actionable
review, then advance release metadata and documentation to the next patch
version and re-run verification.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

The source and generated Admin UI must expose `export_from`, `export_to`, and
an `all` row-count option. Entered export timestamps must be converted to ISO
8601, replace inherited Usage timestamps, and reject an inverted range. An
all-rows download must reject an unbounded window, request pages of 10,000 rows
with increasing offsets until a short page is returned, include one CSV header
only, and construct valid paged JSON. Existing 100, 1,000, 5,000, and 10,000
single-request exports must continue unchanged.

The Admin UI build, Node test suite, and mandatory Rust verification stack must
all pass.

## Idempotence and Recovery

The source edits and tests are safe to rerun. `npm run build:admin-ui`
deterministically replaces generated assets from source. Failed verification
does not change runtime state; fix the reported issue and rerun the complete
relevant command.

## Artifacts and Notes

No database migration, new API route, or dependency is planned.

## Interfaces and Dependencies

The existing `UsageQuery` fields `from`, `to`, `limit`, and `offset` remain the
only server interface. The browser uses standard `fetch`, `Blob`, and object URL
APIs already used by the current export download.
