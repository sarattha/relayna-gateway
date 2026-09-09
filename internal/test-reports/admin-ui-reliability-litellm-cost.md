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
  **95.01% line coverage**, 31,029 total lines, 1,548 missed. Region coverage 92.24%;
  function coverage 92.20%. `cargo llvm-cov report --summary-only --fail-under-lines 95`
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

Mandatory verification stack, PR review and version metadata update are tracked
in the living ExecPlan. This report will be finalized after those checks finish.
