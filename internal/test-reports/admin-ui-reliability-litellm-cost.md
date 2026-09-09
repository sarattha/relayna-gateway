# Admin UI reliability and LiteLLM spend verification

Date: 2026-09-09. Branch: `codex/admin-ui-reliability-litellm-cost`.

## Findings and behavior

Overview requested the full dashboard although it displayed only summary,
timeseries and project aggregates. The PostgreSQL dashboard computes additional
sections sequentially. The browser also imposed an eight-second timeout on all
finite requests. Overview now requests only the three required usage sources,
in parallel, with independent stale/error states. Analytics requests have a
bounded 30-second allowance, and navigation still cancels obsolete requests.
Rolling windows include both partial hourly/day buckets. This is source-level
diagnosis plus delayed-response validation; production query latency has not
been measured against a customer database.

Newly issued virtual keys remain visible after backdrop clicks or Escape.
Copy and explicit Close remain available. Other dialogs retain their previous
interaction behavior.

The new read-only spend endpoint reads LiteLLM's current `info.spend` counter
using the SHA-256 identifier of the mapped credential. It returns a sanitized
snapshot, never upstream key metadata. Key mappings take precedence over project
mappings. The UI distinguishes shared project spend, missing mapping, unavailable
spend and legitimate zero. This counter can reset and include non-Gateway traffic;
it is separate from Gateway estimates, date-filtered usage and budget enforcement.

## Automated evidence

- `npm test`: passed, including a real HTTP response delayed 8.1 seconds;
  cancellation, stale cache scope, rolling buckets, modal dismissal and spend states.
- `cargo llvm-cov --workspace --all-features --summary-only`: passed across the
  complete Rust workspace, with PostgreSQL and Redis integration services enabled.
  **95.02% line coverage**, 31,028 total lines, 1,545 missed. Region coverage 92.25%;
  function coverage 92.16%. `cargo llvm-cov report --summary-only --fail-under-lines 95`
  also passed. No path exclusions were added. This LLVM metric does
  not measure frontend TypeScript; frontend tests and UI checks are separate.
- Added authentication failure-path regressions cover malformed claims/key material,
  discovery/JWKS transport and response failures, portal storage outages, scoped
  project monitoring and project identity governance.
- Spend HTTP fixtures check authorization, unknown keys, no-store, key/project scope,
  configured authentication, SHA-256 query identifiers, secret redaction, zero spend,
  redirects, upstream failures, malformed JSON and the response-size bound.

## Computer Use evidence

Safari against a disposable synthetic local fixture at port 21481:

- Desktop Overview and virtual-key inventory rendered correctly.
- Desktop and 390px responsive spend dialogs showed amount, scope, retrieval time,
  caveats and usable actions.
- Escape left the show-once key visible on desktop and mobile.
- Mobile Copy displayed success; explicit Close returned to the inventory.
- Computer Use coordinate clicks returned `noWindowsAvailable`; keyboard and
  accessibility actions worked. Backdrop behavior is covered by automated tests.

The fixture created no persistent credentials and used no live LiteLLM account.
Screenshots were inspected locally in `/tmp/admin-cost-ui/`.

## Release and review

PR: https://github.com/sarattha/relayna-gateway/pull/116.

Codex identified two P2 issues. First, the spend handler enumerated mapping inventory.
Commit `a1142a1` fixes it by returning the effective credential and scope from one
scoped query. Database tests verify precedence and disabled mapping fallback;
the API fixture fails if inventory enumeration occurs. The thread was resolved,
and CI passed on the scoped-query correction. A second finding identified false
zero activity when project aggregation failed without cached data. Project activity
and request volume now explicitly show unavailable; successful empty responses and
cached data stay distinct. Frontend regressions, desktop/mobile Computer Use failure
and retry checks, and the full mandatory stack passed after the UI correction.
The final Codex re-review is tracked on the PR.

The full mandatory stack passed after the correction and with prepared 0.1.35
metadata: formatting, Clippy, Cargo tests, audit, deny, machete, all 345 Nextest
tests, Trivy, Gitleaks and Semgrep. Workspace build, frontend tests, asset build,
strict MkDocs and release metadata validation also passed. Verification used a
clean worktree because the original workspace contains an ignored, unrelated
design prototype with vulnerable dependencies. No exception was added and the
prototype was untouched.

A parallel coverage attempt hit an existing process-startup deadline under heavy
Docker VM load. The full isolated rerun passed with `--fail-under-lines 95`,
giving the final numbers above. No assertion or threshold was relaxed.

Version 0.1.35 updates Cargo packages, generated UI assets, deployment examples,
changelog and operator documentation. The final PR check status is visible on
GitHub; no merge or release tag is performed by this task.

Final re-review status: Codex failed twice to resolve `ac60b63`, although GitHub
confirms the commit exists. All CI checks on that code commit passed and both
review threads are resolved. Automated follow-up remains active; final review
clearance is pending rather than reported as successful.

Follow-up on 2026-09-10: Codex recovered and identified a third issue: a late
import/sync callback could generically dismiss a newer show-once token. Generic
closure now respects non-dismissible dialogs while the explicit Close handler
keeps its private closure. The regression recreates the deferred-request race.
Computer Use confirmed Copy, Escape protection and explicit Close in the mobile
fixture. The full mandatory stack (345 Nextest tests included) and workspace build
passed. Another review is tracked in the ExecPlan.
