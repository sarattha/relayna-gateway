# Traffic monitor verification

Date: 2026-09-03. Release: 0.1.31.

Computer Use (`@oai/sky` through the Computer plugin) exercised the real Admin
UI in Chrome against an isolated local gateway, PostgreSQL, Redis and a synthetic
upstream. Only generated test credentials and traffic were used.

Confirmed:

- Live incoming requests and an upstream 503 appear with client/upstream statuses,
  attempt count, request ID, gateway instance and failure explanation.
- Inspect opens the timeline. The final build identifies the upstream HTTP error
  at `upstream_response`, before body delivery.
- Pause/Resume works; stopping the QA gateway produces a disconnected/retrying
  notice. Restarted gateways have a different instance identity.
- Saved history includes requests from the prior gateway process. Filtering by
  `qa-upstream-503` returns exactly the expected request and its saved timeline.
- Desktop details remain readable, and a 390-pixel mobile viewport stacks the
  filters without page-level horizontal overflow.
- Discover → Services and Govern → Keys still render through the shared shell.

Local screenshots (ignored by repository policy): `live-detail.jpg`,
`history-detail.jpg`, and `mobile.png`. Computer Use occasionally returned stale
window captures; a fresh QA browser window allowed the remaining checks to finish.

Automated regressions supplement the UI checks with missing authentication,
unresolved routes, Redis failures before forwarding, body admission 503s,
upstream 503 passthrough, stream interruption after HTTP 200, independent failures
of usage/debug/traffic persistence, access scopes and CSRF, bounded retention,
cursor gaps, duplicate correlation IDs, and split SSE frames.

The mandatory verification script passed on the committed runtime in an isolated
worktree using the committed dependency lock. Nextest ran 325 tests with zero
failures or skips; one existing catalog test had a non-failing process-exit
warning. Audit used a clean advisory database path after an untracked duplicate
advisory made the default local cache unreadable. No exclusions were added.
Workspace build, Admin UI build/tests and release metadata validation passed.
