# Routing mode and passthrough metering verification

Date: 2026-09-05. Branch: `codex/admin-ui-3-followup-fixes`.

## Outcome

Request diagnostics retain optional `routing_mode`. Usage and Traffic tables show
**Gateway managed**, **LiteLLM passthrough**, or **Not recorded**. The shared
investigation presents the same mode and shows **Not metered by gateway** for
passthrough input/output/total tokens and estimated cost, including keyless
Traffic. Usage per-request token and cost cells use the same formatter.

No database migration, forwarding change, billing change, raw path collection or
metric label was introduced. Existing API numeric fields remain numeric/null.
Service passthrough cost pricing does not imply LiteLLM passthrough routing.

## Automated checks

- Frontend build and `npm test`: passed. Cases cover known/unknown modes,
  legacy `/litellm/*` rows, unknown string values, missing measurements, measured
  zero, service passthrough pricing, and Traffic with no Usage/billing identity.
- Runtime build: `cargo build --workspace --all-features` passed.
- Focused proxy mode-classification test passed; terminal usage test verifies
  mode is recorded on early failures. Legacy Traffic JSON reads without the new
  field. New Traffic JSON round-trips the mode.
- Full `code-change-verification` script passed: fmt, clippy, workspace tests,
  audit, deny, machete, 332 nextest tests, Trivy, Gitleaks and Semgrep.
- Diff whitespace check passed.

## Real local request checks

Rebuilt and restarted the native demo, preserving PostgreSQL/Redis and existing
data. All requests targeted the configured localhost mock upstream. Temporarily
changed only local canonical/wildcard routing settings and restored their original
values in a finally block. Read back each saved Traffic record through the admin
history API using its exact request ID.

| Case | Response | Recorded route | Recorded mode | Usage snapshot |
| --- | --- | --- | --- | --- |
| Managed with Relayna key | 200 | `/v1/chat/completions` | `managed_by_gateway` | Present, 22 tokens and recorded cost |
| Canonical passthrough with Relayna key | 200 | `/v1/chat/completions` | `litellm_passthrough` | Present, null tokens/cost |
| Canonical passthrough with mock LiteLLM credential | 200 | `/v1/chat/completions` | `litellm_passthrough` | Absent, as expected without Relayna key |
| Wildcard passthrough with Relayna key | 404 from mock | `/litellm/*` | `litellm_passthrough` | Present, null tokens/cost |

The wildcard mock intentionally has no handler for the test path, proving labels
also survive an upstream error. Completed and failed terminal JSON logs contain
`relayna.routing_mode` with the expected values. These records remain available
under request IDs beginning `demo-routing-` for local inspection.

## Browser checks

Chrome displayed all four requests in live Traffic with the expected badges.
Usage displayed the three identified requests, with two unmetered cells per
passthrough row and numeric tokens/cost for the managed row. Opening keyless
Traffic investigation showed the passthrough mode and four unmetered fields.
Opening canonical passthrough through Usage Debug preserved the mode after the
exact saved Traffic record loaded. Older Usage rows displayed **Not recorded**
for mode rather than being classified using current settings.

At 390 × 844, the investigation's unmetered text wrapped clearly, with no document
horizontal overflow. The Routing mode help tooltip remained within viewport edges,
used white text on the dark background, and dismissed on Escape. Browser error
logs were empty. The temporary viewport was reset after inspection.

Computer Use was attempted but the Mac was locked; Chrome automation supplied
screenshots and interaction evidence. This was viewport testing, not physical
mobile hardware. No production credentials or external providers were used.
