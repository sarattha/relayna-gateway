# Issue 68 LiteLLM Passthrough Auth

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Relayna Gateway should support two explicit LiteLLM passthrough modes without
requiring downstream clients to use Relayna virtual-key authentication in those
opted-in paths.

First, trusted-ingress LiteLLM UI deployments should be able to proxy
allowlisted LiteLLM dashboard UI and dashboard API paths through Relayna while
leaving LiteLLM's session and UI authentication in control. Second,
OpenAI-compatible routes configured as `direct_litellm_passthrough`, such as
`/v1/responses`, should accept the normal LiteLLM bearer credential at the
Relayna boundary and forward it upstream using the configured LiteLLM provider
credential header mode.

The observable result is that explicitly configured LiteLLM passthrough traffic
reaches LiteLLM with credentials stripped or translated correctly, while default
Gateway-managed routes still require Relayna virtual keys.

## Progress

- [x] (2026-06-19T09:04Z) Fetched issue #68, confirmed branch
  `issue-68-litellm-passthrough-auth` from current `main`, and ran the
  v0.1.10 freeze perimeter test before edits.
- [x] (2026-06-19T09:04Z) Implement trusted-ingress credentialless bypass for explicitly exposed
  allowlisted LiteLLM admin dashboard paths.
- [x] (2026-06-19T09:04Z) Implement direct LiteLLM bearer capture before Relayna virtual-key
  authentication for eligible OpenAI-compatible routes.
- [x] (2026-06-19T09:04Z) Add focused regression tests for path classification, upstream credential
  translation, and default fail-closed behavior.
- [x] (2026-06-19T09:10Z) Run formatting, freeze perimeter, and gateway verification checks.

## Surprises & Discoveries

- Observation: `crates/gateway-proxy/src/pingora_plane.rs` only bypasses
  Relayna auth for `trusted_ingress_ui_path_allowed`, so explicitly exposed
  dashboard API paths fall through to virtual-key auth.
  Evidence: issue #68 observed `GET /global/spend/logs` and `GET /key/info`
  returning Relayna `401 missing_authorization` after LiteLLM UI login.
- Observation: direct OpenAI route mode is checked only after Relayna key
  authentication succeeds.
  Evidence: the `openai_route_mode` lookup for `Route::Responses`,
  `Route::ChatCompletions`, and `Route::LiteLlmEmbeddings` currently happens
  inside the `Ok(key)` branch in `request_filter`.

## Decision Log

- Decision: Treat this as an additive, explicit opt-in behavior change against
  freeze baseline `v0.1.10`.
  Rationale: The issue asks to alter released auth/proxy behavior, but only for
  routes already configured by operators as LiteLLM passthrough and constrained
  by allowlisted paths, methods, and exposure modes.
  Date/Author: 2026-06-19 / Codex.
- Decision: Keep the bypass fail-closed and route-specific rather than adding
  a general unauthenticated proxy mode.
  Rationale: `internal/design-manifesto.md` says Relayna Gateway owns identity
  by default. This issue is a narrow exception for LiteLLM-as-auth-authority
  passthrough surfaces.
  Date/Author: 2026-06-19 / Codex.
- Decision: Allow this issue to intentionally break the v0.1.10 freeze
  perimeter and the design manifesto identity rule where they conflict with the
  requested LiteLLM passthrough behavior.
  Rationale: The user explicitly authorized breaking freeze perimeters and the
  design manifesto for issue #68. The implementation should still make the
  exception explicit and scoped to configured LiteLLM passthrough.
  Date/Author: 2026-06-19 / Codex.

## Outcomes & Retrospective

Implemented and verified. Trusted-ingress LiteLLM passthrough now supports
explicitly exposed allowlisted admin dashboard API paths, and direct LiteLLM
passthrough routes can use the inbound OpenAI-compatible bearer credential as
the LiteLLM upstream credential. Focused tests and the full repository
verification stack passed.

## Context and Orientation

Issue #68 is about `crates/gateway-proxy/src/pingora_plane.rs` and
`crates/gateway-core/src/route_settings.rs`.

A Relayna virtual key is the normal Gateway credential carried in
`Authorization: Bearer rk_live_...`. LiteLLM passthrough is a configured proxy
mode where Relayna forwards selected paths to LiteLLM instead of applying full
Gateway policy and usage governance. A LiteLLM upstream credential is the key
Relayna sends to LiteLLM, either as `Authorization: Bearer ...` or as a custom
header such as `x-litellm-api-key`, based on provider settings edited in Admin
UI.

Current code classifies arbitrary allowlisted LiteLLM paths as
`Route::LiteLlmPassthrough` when the built-in route resolver does not match
them. Credentialless trusted-ingress forwarding is limited to UI/support paths
via `LiteLlmPassthroughSettings::trusted_ingress_ui_path_allowed`. Direct
OpenAI route modes for built-in routes are checked after Relayna virtual-key
authentication, so an inbound LiteLLM bearer key currently fails as malformed
Relayna auth.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.10`; this change touches
released authentication and LiteLLM proxy behavior. The strategy is additive and
explicitly opt-in. Existing Gateway-managed routes and unmanaged LiteLLM paths
continue requiring Relayna virtual keys or return existing errors. No database,
Redis, migration, or public response-shape changes are planned.

Freeze surfaces touched: authentication, route policy, LiteLLM proxy behavior,
upstream credential handling, and tests.

## Plan of Work

Update `crates/gateway-core/src/route_settings.rs` with a helper that recognizes
trusted-ingress credentialless passthrough for UI/support paths and admin paths
only when settings are enabled, method/path allowlists match, UI exposure is
`trusted_ingress`, and admin API exposure is `explicitly_exposed` for admin
paths.

Update `crates/gateway-proxy/src/pingora_plane.rs` so `request_filter` can
classify direct LiteLLM passthrough before Relayna virtual-key authentication
for built-in OpenAI-compatible routes. For eligible direct passthrough, extract
the bearer value from the downstream `Authorization` header as a LiteLLM
credential, configure the LiteLLM upstream with active provider base URL and
header mode, skip Gateway key-dependent governance, and let upstream request
preparation strip downstream auth before inserting the configured upstream
credential.

Keep trusted-ingress UI/admin credentialless mode separate from direct
bearer passthrough in context state so response redirect rewriting still only
applies to trusted-ingress browser flows.

Add focused tests in crate-local test modules. Cover route settings helper
behavior, custom-header translation for client-supplied LiteLLM bearer
credentials, and the default behavior that non-passthrough OpenAI routes still
use Gateway governance.

## Concrete Steps

Run these from `/Users/jobz/Works/relayna-gateway`:

    node tests/freeze-v0.1.10-perimeter.test.mjs
    cargo fmt --all --check
    cargo test -p gateway-core route_settings
    cargo test -p gateway-proxy litellm
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Acceptance criteria:

- Trusted-ingress credentialless passthrough allows allowlisted LiteLLM
  UI/support paths and allowlisted LiteLLM admin dashboard paths only when
  `ui_exposure == trusted_ingress` and
  `admin_api_exposure == explicitly_exposed` for admin paths.
- Direct LiteLLM passthrough for configured built-in OpenAI-compatible routes
  accepts `Authorization: Bearer <LiteLLM key>` without trying to parse it as a
  Relayna virtual key.
- Direct passthrough strips the downstream `Authorization` header and forwards
  the client LiteLLM key using provider `authorization_bearer` or
  `custom_header` settings.
- Default managed Gateway routes and disabled passthrough routes remain
  protected by Relayna virtual-key auth.
- `node tests/freeze-v0.1.10-perimeter.test.mjs` passes unless intentionally
  updated with compatibility notes.
- `$code-change-verification` passes before the goal is complete.

## Idempotence and Recovery

The work is local to Rust source and tests. If an edit fails, rerun `git diff`
to inspect partial changes and continue with surgical patches. The branch was
created from clean `main`; no local services, migrations, PostgreSQL state, or
Redis counters are required for the planned unit checks. Verification commands
are safe to rerun.

## Artifacts and Notes

Pre-edit freeze perimeter:

    node tests/freeze-v0.1.10-perimeter.test.mjs
    ok - current release metadata is valid and v0.1.10 is the freeze baseline
    ok - control-plane public route inventory is pinned
    ok - proxy route resolver keeps v0.1.10 public route semantics
    ok - public gateway error codes are pinned
    ok - release configuration environment variables are pinned
    ok - PostgreSQL migration inventory is pinned
    ok - Redis key formats and TTLs are pinned
    ok - LiteLLM passthrough exposure modes are pinned
    ok - admin portal static test covers all control endpoints it depends on
    ok - kubernetes control probes use the admin-ui base path
    ok - kubernetes production hardening remains enabled
    ok - release image latest tag is gated only by explicit metadata tag
    ok - release workflow publishes supply-chain artifacts

Focused tests after implementation:

    cargo test -p gateway-core route_settings
    test result: ok. 5 passed; 0 failed

    cargo test -p gateway-proxy litellm
    test result: ok. 12 passed; 0 failed

Final verification:

    node tests/freeze-v0.1.10-perimeter.test.mjs
    all checks passed

    bash .codex/skills/code-change-verification/scripts/run.sh
    code-change-verification: all commands passed.

## Interfaces and Dependencies

No new environment variables, migrations, Redis keys, or Admin UI controls are
planned. Existing provider config fields `credential_header_mode` and
`credential_header_name` define how direct client LiteLLM bearer credentials are
forwarded upstream.
