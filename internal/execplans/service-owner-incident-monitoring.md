# Service-owner Incident Monitoring Dashboard

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Service members need one scoped monitoring view that shows whether their service
is healthy, which releases correlate with incidents, every request outcome, and
sanitized request details. After this change, the service dashboard displays a
responsive error-rate and P95-latency chart, supports time/outcome/status filters
and pagination, and opens an accessible details drawer without using admin-only
routes. Gateway records a bounded optional upstream service version on usage
events so the chart can mark the first time each version transition was observed.

## Progress

- [x] (2026-08-09 06:45Z) Fetched `origin/main`, rebased the merged Entra branch,
  and created `codex/service-owner-incident-dashboard` from `66e6597`.
- [x] (2026-08-09 06:50Z) Confirmed issue #101, repository rules, UI design
  guidance, current source ownership, and compatibility boundary.
- [x] (2026-08-09 08:20Z) Added the usage-event migration, version capture, P95/status/transition data,
  and owner-scoped request-details API.
- [x] (2026-08-09 08:55Z) Added the incident chart, filters, pagination, correct badges, and accessible
  request-details drawer in the Admin UI source package.
- [x] (2026-08-09 09:15Z) Added focused store, API, proxy, and Admin UI tests.
- [x] (2026-08-09 09:35Z) Bumped the release target to 0.1.25 and updated CHANGELOG and behavior docs.
- [x] (2026-08-09 15:45Z) Regenerated checked-in Admin UI assets and passed formatting,
  Clippy, workspace tests, audit, dependency-policy, nextest, scoped Trivy, UI,
  build, and release-metadata verification.
- [x] (2026-08-09 20:30Z) Validated desktop and 390px mobile behavior with Computer,
  including chart markers/tooltips, keyboard filters, pagination, missing-debug
  details, focus containment, Escape close, and explicit focus restoration.
- [ ] Publish the PR and
  address the first Codex review.

## Surprises & Discoveries

- Observation: `origin/main` already contains the former
  `agent/entra-production-ready` branch through PR #102, so rebasing was a
  fast-forward and the dashboard work can stay in a new focused branch.
  Evidence: `git rev-list --left-right --count origin/main...HEAD` returned
  `1 0` before the rebase and HEAD became merge commit `66e6597` afterward.
- Observation: the only pre-existing uncommitted file is `.gitignore`, adding
  `design-prototypes`; it is unrelated user work and must not be staged.
  Evidence: `git status -sb` after the rebase reports only `M .gitignore`.
- Observation: local `gh` authentication is expired. SSH and the GitHub app may
  cover fetch, push, and PR creation, but thread-resolution automation requires
  refreshed `gh` authentication if the app cannot expose review-thread state.
  Evidence: `gh auth status` reports an invalid token for `sarattha`.
- Observation: `gh` authentication became valid again before publication, with
  repository and workflow scopes available.
  Evidence: the final `gh auth status` check reports the `sarattha` account as
  active.
- Observation: the mandatory verification script passes formatting, Clippy,
  workspace tests, audit, dependency policy, and nextest, but its repository-wide
  Trivy step also traverses the unrelated untracked `design-prototypes/` user
  directory and reports that prototype's stale npm dependencies.
  Evidence: rerunning the identical Trivy policy with
  `--skip-dirs design-prototypes` reports zero high or critical findings in the
  tracked Gateway dependency set.
- Observation: capturing the drawer trigger after its async details request is
  not robust in Chrome accessibility flows because focus may move while the
  request resolves.
  Evidence: Computer initially reported the page root after Escape; explicitly
  preserving `event.currentTarget` before the request restored focus to the
  exact `View details` button on the repeat test.

## Decision Log

- Decision: evolve the owner dashboard routes directly, without aliases or a
  compatibility shim.
  Rationale: the latest release tag is v0.1.23 and these owner routes are
  unreleased branch-local interfaces in the 0.1.24 target.
  Date/Author: 2026-08-09 / Codex.
- Decision: add nullable `usage_events.service_version` through a forward-only,
  idempotent migration and keep existing rows readable.
  Rationale: `usage_events` is released durable state, so the new field must be
  additive and optional.
  Date/Author: 2026-08-09 / Codex.
- Decision: record only an upstream response header matching
  `[A-Za-z0-9][A-Za-z0-9._+-]{0,63}` and leave the response header untouched.
  Rationale: this prevents unbounded or unsafe metadata while preserving proxy
  and streaming semantics; invalid values are ignored rather than rejected.
  Date/Author: 2026-08-09 / Codex.
- Decision: bump the unreleased release target from 0.1.24 to 0.1.25 after the
  behavior is verified.
  Rationale: the user explicitly requested a version bump and 0.1.24 is already
  the repository-wide release target on main.
  Date/Author: 2026-08-09 / Codex.

## Outcomes & Retrospective

The owner workspace now provides incident signals, every request outcome,
exact-code filtering, offset pagination, correctly labelled status badges, and
sanitized details without granting access to admin debug routes. Gateway stores
only validated bounded service versions, preserves streaming and response
headers, and exposes additive P95/status/transition data through the scoped
owner API. Computer QA passed on desktop and a 390px responsive viewport. The
only verification exception is the repository-wide Trivy wrapper traversing an
unrelated untracked `design-prototypes/` directory; the same high/critical scan
  passes when that user directory is excluded. The implementation is published
  for review at https://github.com/sarattha/relayna-gateway/pull/103; record the
  first-review outcome after Codex responds.

## Context and Orientation

`crates/gateway-proxy/src/pingora_plane.rs` owns upstream response processing and
usage-event construction. `crates/gateway-core/src/usage.rs` owns the framework-
independent usage event, while `crates/gateway-core/src/observability.rs` owns
query response types. `crates/gateway-store/src/postgres.rs` inserts and queries
usage data; migrations live under `crates/gateway-store/migrations/`.

`crates/gateway-api/src/app.rs` exposes owner routes and enforces exact service
membership. The Vite/TypeScript source of truth is
`crates/gateway-api/admin-ui/`; generated `/admin-ui/app.js` and
`/admin-ui/app.css` assets are rebuilt into
`crates/gateway-api/src/static/admin-ui/`.

A usage event is the sanitized metering record produced for each request. A
debug bundle is a separately stored redacted diagnostic record. P95 latency is
the 95th percentile request latency in a summary bucket. A version-transition
marker records the first event at which Gateway observed a version different
from the previous valid observed version, including rollbacks.

## Compatibility Boundary

Compatibility boundary: latest release tag v0.1.23. Owner dashboard APIs are
unreleased and can be replaced directly. The released PostgreSQL `usage_events`
schema receives only a nullable column, preserving old rows and readers. Public
proxy bodies, status codes, streaming, credentials, and upstream response
headers remain unchanged.

## Plan of Work

Extend the usage model and PostgreSQL queries with `service_version` and
`p95_latency_ms`, plus scoped status-code and ordered version-transition data.
Add an exact-service request lookup that returns sanitized usage metadata and an
optional debug bundle, using the same 404 for absent and cross-service requests.

Capture and validate `X-Relayna-Service-Version` in the Pingora upstream response
hook and carry it only into the eventual usage event. Do not buffer response
bodies, remove the header, or change error handling.

Replace the service dashboard's fixed `/errors` request with `/events`. Add range,
outcome, status-code, and pagination state. Render the responsive Chart.js chart
with dual axes and a small marker plugin, keep the visible time-series table, and
add a screen-reader summary. Replace owner Debug actions with View details and use
the existing modal focus lifecycle to implement a drawer with an explicit
missing-debug state.

Add focused tests at each ownership boundary, update release metadata and docs,
build the Admin UI assets, run repository verification, then exercise desktop
and mobile UI paths in the real Chrome app through Computer.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    npm run build:admin-ui
    npm test
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    python3 scripts/validate-release-metadata.py v0.1.25

Use focused `cargo test -p ... <name>` and `node --test ...` commands while
iterating. Rerun the full verification script after any review-driven runtime
change.

## Validation and Acceptance

Tests must prove correct P95 calculation; valid, invalid, repeated, and rollback
version observations; status and exact-code filters; offset pagination; optional
debug bundles; and identical missing/cross-service 404 behavior. Proxy tests must
show version capture neither leaks credentials nor changes streaming or malformed
response handling. Admin UI tests must prove chart/filter/action wiring and
correct badge labels.

Computer QA must verify desktop and mobile layouts, chart markers/tooltips,
keyboard-operable filters, Escape/focus behavior, missing-debug feedback, and
that every visible owner request action responds.

## Idempotence and Recovery

The migration uses `ADD COLUMN IF NOT EXISTS`, so repeated application is safe.
Build and test commands are repeatable. The pre-existing `.gitignore` edit stays
unstaged. If verification fails, fix the narrow failure and rerun the complete
stack. If the local server is interrupted during UI QA, restart it with the
repository's documented demo command; do not delete user data or reset the
worktree.

## Artifacts and Notes

GitHub issue: https://github.com/sarattha/relayna-gateway/issues/101

The issue contains the confirmed current defects and the scoped acceptance
checklist. Current and expected screenshots remain sanitized; raw headers,
request bodies, prompts, credentials, and unredacted provider errors are outside
all owner responses and published evidence.

## Interfaces and Dependencies

The owner dashboard response adds P95 values, distinct scoped status codes, and
version-transition markers. The owner request-details route is:

    GET /owner/v1/services/{service_name}/requests/{request_id}

It returns sanitized usage metadata and `debug_bundle: null` when no matching
bundle exists. Chart.js remains the existing frontend chart dependency. No new
runtime dependency or external service is introduced.
