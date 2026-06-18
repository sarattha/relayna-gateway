# Issue 66 Browser-Safe LiteLLM UI Access

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This plan follows `PLANS.md`.

## Purpose / Big Picture

Operators need browser-safe access to LiteLLM's `/ui` through Relayna Gateway
without exposing LiteLLM credentials or making unauthenticated `/ui` passthrough
public. After this change, operators can open a Gateway-owned
`/admin-ui/litellm-ui/*` route with an existing operator token, and deployments
that use trusted Entra or Apigee ingress keep `/ui` passthrough gated by trusted
operator context.

## Progress

- [x] (2026-06-18 21:50+07:00) Issue #66 and comments inspected; there are no
  comments beyond the issue body.
- [x] (2026-06-18 21:50+07:00) Baseline freeze perimeter test passed before
  edits with `node tests/freeze-v0.1.9-perimeter.test.mjs`.
- [x] (2026-06-18 21:51+07:00) Branch checked out:
  `codex/issue-66-litellm-ui-access`.
- [x] (2026-06-18 22:05+07:00) Added the operator-authenticated Axum LiteLLM
  UI proxy route under `/admin-ui/litellm-ui`.
- [x] (2026-06-18 22:08+07:00) Added Axum proxy tests and tightened
  trusted-ingress passthrough gate tests.
- [x] (2026-06-18 22:09+07:00) Updated the v0.1.9 freeze perimeter route
  inventory for the additive route.
- [x] (2026-06-18 22:11+07:00) Focused tests passed:
  `cargo test -p gateway-api litellm_ui -- --nocapture`,
  `cargo test -p gateway-proxy litellm -- --nocapture`, and
  `node tests/freeze-v0.1.9-perimeter.test.mjs`.
- [x] (2026-06-18 23:58+07:00) Full code-change verification passed with
  `bash .codex/skills/code-change-verification/scripts/run.sh`.
- [x] (2026-06-19 00:27+07:00) Real Docker environment with real LiteLLM and
  Relayna Gateway passed the issue #66 UI checks and browser screenshots were
  captured at `internal/test-reports/litellm-real-passthrough/screenshots/08-litellm-ui-proxy-real-env.png`
  and `internal/test-reports/litellm-real-passthrough/screenshots/09-real-env-issue-66-report.png`.
- [x] (2026-06-19 00:31+07:00) Final code-change verification passed after
  real-environment hardening with
  `bash .codex/skills/code-change-verification/scripts/run.sh`.
- [x] (2026-06-19 00:32+07:00) Final freeze perimeter test passed with
  `node tests/freeze-v0.1.9-perimeter.test.mjs`.

## Surprises & Discoveries

- Current Pingora wildcard passthrough already classifies `/ui` paths and allows
  `operator_only` only when an Entra or Apigee identity is present, but it still
  requires Relayna virtual-key authentication. That does not satisfy plain
  browser address-bar access by itself.
- Real LiteLLM serves its UI as a Next.js app with root
  `/litellm-asset-prefix/*` assets, a `/litellm/.well-known/litellm-ui-config`
  bootstrap request, and absolute `/ui` redirects based on the upstream host.
  The Gateway proxy needs bounded text rewrites for HTML and JavaScript plus
  `Location` rewriting by URL path so browser traffic stays under
  `/admin-ui/litellm-ui`.

## Decision Log

- Decision: Implement both issue-supported paths, with the Axum proxy as the
  primary browser path and Pingora trusted-ingress passthrough retained and
  tested.
  Rationale: The user selected both paths, and only the Axum route solves
  browser access without manual Relayna key headers.
  Date/Author: 2026-06-18 / Codex.

- Decision: Require `SCOPE_PROVIDERS_UPDATE` for the Axum UI proxy.
  Rationale: LiteLLM UI can expose provider/operator controls, so read-only
  usage scope is too weak.
  Date/Author: 2026-06-18 / Codex.

- Decision: Treat the public route addition as an allowed additive freeze
  perimeter update.
  Rationale: The user explicitly allowed freeze perimeter breaks, and the
  existing public route inventory test is the right place to document the new
  route.
  Date/Author: 2026-06-18 / Codex.

## Outcomes & Retrospective

Implemented the primary browser-safe Axum proxy path and retained the
trusted-ingress Pingora gate. The additive freeze perimeter route update and
full workspace/security verification passed after real-environment hardening.
The real LiteLLM UI loads at the Gateway prefix and redirects to the proxied
login route without exposing the Docker-internal LiteLLM host.

## Context and Orientation

Relayna Gateway uses Axum for control-plane routes under `/admin-ui` and
Pingora for the proxy plane. Operator tokens protect `/admin-ui/admin/*` routes
through scoped auth. LiteLLM provider config already stores the upstream base
URL, credential secret, and credential header mode. Pingora wildcard passthrough
already strips client credentials and injects server-side LiteLLM credentials
for API-client traffic.

The new Axum route must preserve Gateway identity boundaries: browser clients
authenticate only to Gateway with an operator token; Gateway strips browser
credentials before forwarding and injects the configured server-side LiteLLM
credential upstream.

## Compatibility Boundary

Freeze baseline: v0.1.9. Latest local release tag is v0.1.8, but workspace
metadata and perimeter tests pin v0.1.9. This change touches public
control-plane routes, auth behavior, LiteLLM proxy behavior, and credential
handling. Compatibility strategy is additive: keep existing API-client
passthrough behavior unchanged, add `/admin-ui/litellm-ui/{*path}`, and update
the perimeter route inventory.

## Plan of Work

Add `ProviderConfigLookup` to the Gateway API data trait so Axum can resolve the
active LiteLLM runtime config. Add a route under `/admin-ui/litellm-ui/{*path}`
that requires provider-update operator scope, maps the route to LiteLLM
`/ui/{path}`, forwards safe browser headers and body with `reqwest`, strips
sensitive credentials, injects the configured LiteLLM credential, and rewrites
redirect and HTML `/ui/` references back to the Gateway prefix.

Add focused Rust tests in the Gateway API and Gateway proxy test modules. Update
the JavaScript freeze perimeter test with the new route.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.1.9-perimeter.test.mjs
    cargo test -p gateway-api litellm_ui
    cargo test -p gateway-proxy litellm
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Success means unauthenticated `/admin-ui/litellm-ui/*` calls fail, authenticated
operator calls proxy to LiteLLM `/ui/*`, server-side LiteLLM credentials are
injected, client-supplied credentials are stripped, redirects and basic HTML
asset paths work behind the Gateway prefix, and existing LiteLLM passthrough
tests continue to pass.

## Idempotence and Recovery

All changes are code and tests only. Failed cargo or node commands can be rerun
after fixing the reported issue. No migrations or external service state are
required for the unit tests.

## Artifacts and Notes

Issue: https://github.com/sarattha/relayna-gateway/issues/66

## Interfaces and Dependencies

Public route added: `/admin-ui/litellm-ui/{*path}`.
No new environment variables, database tables, Redis keys, or credential formats
are introduced.
