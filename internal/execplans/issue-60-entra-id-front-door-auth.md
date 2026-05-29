# Issue 60 Entra ID Front-Door Authorization

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Relayna Gateway provider traffic can be protected by Microsoft Entra ID before
existing Relayna virtual-key authentication. When enabled, callers present an
Entra access token in `Authorization: Bearer <jwt>` and the Relayna virtual key
in the configured Relayna key header. The default is `X-Relayna-Key`, and
deployments can override it with `ENTRA_RELAYNA_KEY_HEADER`. Entra proves coarse
enterprise identity and entitlement; the Relayna key remains the project,
policy, budget, rate-limit, guardrail, and usage anchor.

## Progress

- [x] (2026-05-29 19:42 +07) Read issue #60, the Entra design artifact, current
      auth/proxy code, latest tag, and v0.1.0 freeze perimeter.
- [x] (2026-05-29 19:44 +07) Confirmed latest release tag is `v0.1.4` and
      `node tests/freeze-v0.1.0-perimeter.test.mjs` passes before changes.
- [x] (2026-05-29 20:18 +07) Added opt-in Entra config, verifier, proxy
      enforcement, trusted Apigee proof handling, and tests.
- [x] (2026-05-29 20:23 +07) Ran freeze perimeter, formatting, clippy, and
      workspace tests successfully.
- [x] (2026-05-29 20:41 +07) Changed the Entra-mode Relayna key header to
      default to `X-Relayna-Key` and made it deployment-configurable through
      `ENTRA_RELAYNA_KEY_HEADER`.
- [x] (2026-05-29 23:46 +07) Added and ran a Docker-backed real-environment
      review harness with Postgres, Redis, Gateway, mock OIDC/JWKS authority,
      mock Apigee paths, and mock provider upstream.
- [x] (2026-05-29 23:48 +07) Captured browser screenshots for the live review
      dashboard and raw result evidence under
      `internal/test-reports/entra-front-door-real-env/screenshots/`.
- [x] (2026-05-30 00:00 +07) Expanded the Docker review harness to cover
      `/v1/responses`, `/providers/openai/*`, built-in internal service routes,
      and `/services/*` in addition to `/v1/chat/completions`.

## Surprises & Discoveries

- Observation: Current provider traffic authenticates only
  `Authorization: Bearer rk_live_...` in `gateway-proxy`.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs`.
- Observation: The v0.1.0 perimeter test pins public error codes and config
  environment variables, so Entra additions must update the perimeter test with
  an explicit additive compatibility decision.
  Evidence: `tests/freeze-v0.1.0-perimeter.test.mjs`.
- Observation: A live Microsoft tenant is not required for repeatable
  verification of the resource-server behavior.
  Evidence: `gateway-core` tests start a local OIDC discovery/JWKS authority,
  sign an RSA JWT, and validate the real discovery, JWKS fetch, and signature
  path.
- Observation: The Docker real-environment test exposed a Postgres-backed admin
  key creation bug: `key_policies` inserted 26 columns with only 25 SQL
  placeholders.
  Evidence: `crates/gateway-store/src/postgres.rs` and the first Docker harness
  run returning `store_unavailable` from `POST /admin-ui/admin/keys`.

## Decision Log

- Decision: Entra support is opt-in through `ENTRA_AUTH_ENABLED=false` by
  default.
  Rationale: Preserves released virtual-key behavior for existing clients.
  Date/Author: 2026-05-29 / Codex.
- Decision: Replace the fixed `X-AIH-API-Key` Entra-mode Relayna key header
  with configurable `ENTRA_RELAYNA_KEY_HEADER`, defaulting to `X-Relayna-Key`.
  Rationale: Operators need to bootstrap the header name per deployment while
  keeping a Relayna-branded default.
  Date/Author: 2026-05-29 / Codex.
- Decision: Implement trusted Apigee header mode only behind explicit config
  and HMAC proof.
  Rationale: Prevents forged public-listener identity headers.
  Date/Author: 2026-05-29 / Codex.

## Outcomes & Retrospective

Implemented opt-in Entra front-door auth for provider traffic. Existing
`Authorization: Bearer rk_live_...` behavior remains the default. When Entra is
enabled, Pingora validates either the Entra JWT or trusted Apigee identity
proof before authenticating the Relayna key from the configured Relayna key
header.

Verification passed:

- `node tests/freeze-v0.1.0-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `internal/test-reports/entra-front-door-real-env/run.sh`

The Docker harness report is
`internal/test-reports/entra-front-door-real-env/report.md`. It verifies direct
Entra JWT validation through mock OIDC metadata/JWKS, invalid token rejection,
configured `X-Relayna-Key` handling, LiteLLM routes, direct OpenAI-compatible
routes, built-in internal service routes, `/services/*` routes, Apigee JWT
revalidation, trusted Apigee HMAC identity proof rejection on tamper, and
upstream credential stripping.

## Context and Orientation

Current Relayna Gateway provider traffic is owned by Pingora in
`crates/gateway-proxy/src/pingora_plane.rs`. It reads `Authorization`, validates
a Relayna virtual key through `gateway-core::Authenticator`, applies route,
policy, rate-limit, budget, guardrail, and provider forwarding behavior, then
strips client credentials before injecting internal provider credentials.

The Entra feature adds a coarse enterprise identity gate before virtual-key auth
without changing the existing disabled-by-default behavior. The shared verifier
belongs in `gateway-core` so Axum/Tower and Pingora can reuse it later.

## Compatibility

Latest release tag: `v0.1.4`. Production freeze baseline: `v0.1.0`.

Touched freeze surfaces: authentication behavior, public request headers,
public error codes, environment variables, proxy credential stripping, and
telemetry/auth-denial reasons. The intended impact is additive because Entra
auth is disabled by default and the existing `Authorization: Bearer rk_live_...`
contract remains unchanged outside Entra mode.

## Implementation Plan

Add `gateway-core` Entra primitives for configuration, OIDC/JWKS fetching,
JWT validation, sanitized identity context, and trusted Apigee header proof.
Expose them from `gateway-core`.

Extend gateway config parsing with Entra and Apigee environment variables.
Validate required Entra fields only when `ENTRA_AUTH_ENABLED=true`; validate
trusted-header secret only when trusted-header mode is enabled. Default
`ENTRA_RELAYNA_KEY_HEADER` to `X-Relayna-Key`.

Extend `PingoraLiteLlmConfig` with optional Entra and Apigee trusted-header
settings. In Entra mode, verify either a trusted Apigee identity header or the
Entra JWT from `Authorization` before authenticating the Relayna virtual key
from the configured Relayna key header.

Strip the configured Relayna key header before forwarding to upstream
providers.

Update the v0.1.0 perimeter test for additive error codes and environment
variables.

## Validation

Run:

- `node tests/freeze-v0.1.0-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`

Add focused unit tests for JWT validation, auth ordering, trusted Apigee proof,
and credential stripping. Add a mock OIDC/JWKS test that exercises discovery,
JWKS fetch, token verification, key rotation refresh, and failure behavior
without requiring a live Microsoft tenant.
