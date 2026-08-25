# Entra Authorization Debug Diagnostics

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Relayna Gateway operators need one explicitly enabled diagnostic mode that
explains every Entra authorization decision across portal OIDC sign-in, portal
session and cookie establishment, owner monitoring, request-plane managed
identity, and trusted Apigee paths. With `ENTRA_AUTH_DEBUG=true`, Docker and
production logs must show the precise phase, outcome, safe reason, expected
authorization requirements, decoded JWT protected header and claims, and final
Relayna membership or binding decision. Compact credentials, OAuth codes,
cookies, state, nonce, PKCE values, CSRF values, private keys, and certificate
contents must never be logged.

The mode defaults off. Existing HTTP status codes, error bodies, authorization
decisions, cookies, database state, and proxy behavior remain unchanged except
that cookie-header construction can no longer fail silently.

## Progress

- [x] (2026-08-25 16:37Z) Read repository guidance and the mandatory Karpathy,
  implementation-strategy, verification, PR-summary, Computer Use, and GitHub
  review skills.
- [x] (2026-08-25 16:37Z) Established `v0.1.28` as the released compatibility
  boundary and recorded the user's production-freeze exception.
- [x] (2026-08-25 18:05Z) Implemented the guarded telemetry contract, configuration, token evidence,
  and precise Entra verification reasons.
- [x] (2026-08-25 18:05Z) Instrumented portal OIDC, callback, session persistence, cookie emission,
  cookie observation, owner authorization, request-plane, and Apigee paths.
- [x] (2026-08-25 18:05Z) Added focused regression coverage and the detailed operator runbook.
- [x] (2026-08-25 22:42Z) Ran the real local Docker environment through Computer
  Use and validated successful portal sign-in/session/logout, a missing login
  cookie callback, accepted/rejected direct Entra tokens, Relayna-key failure,
  and accepted/rejected trusted Apigee proofs against emitted diagnostic logs.
- [x] (2026-08-26 00:19Z) Ran the complete mandatory Rust, dependency, secret,
  static-analysis, Admin UI, prototype, documentation, and release-metadata
  verification stack. All code findings are clear; the local RustSec cache
  duplication was isolated by rerunning cargo-audit against a clean official
  advisory database.
- [x] (2026-08-26 00:19Z) Bumped the release target to `0.1.29` and updated the
  changelog, deployment examples, Admin UI indicators, release documentation,
  and detailed operator runbook.
- [x] (2026-08-26 00:18Z) Committed and pushed `7f05456`, and updated PR #107's
  title and description with the feature, security, compatibility, local
  runtime, and verification evidence.
- [x] (2026-08-26 00:29Z) Requested exactly one additional Codex review. Its one
  P2 finding identified the string-encoded event envelope; changed the stable
  envelope to native tracing fields, updated the runbook, and reran formatting,
  Clippy, the full workspace test suite, telemetry tests, and strict docs build.
- [ ] Push the review fix, reply with commit/test evidence, resolve the thread,
  and confirm the final CI run.

## Surprises & Discoveries

- Observation: `append_portal_cookies` and related helpers silently discard
  invalid `Set-Cookie` header construction, which can leave a durable server
  session without a browser credential and no diagnostic evidence.
  Evidence: `crates/gateway-api/src/app.rs` cookie helpers use `if let Ok` around
  `HeaderValue::from_str`.
- Observation: the current JWT verifier deliberately maps many discovery,
  JWKS, key, signature, and claim failures to the same public
  `invalid_entra_token` response.
  Evidence: `crates/gateway-core/src/entra.rs`.
- Observation: PR #107 currently contains documentation-only Entra/DevOps
  checklist work plus two uncommitted user edits. Both edits are in scope and
  must be preserved.
  Evidence: initial `git status` and `gh pr view`.
- Observation: an emitted `Set-Cookie` header proves only Gateway emission;
  the server cannot directly observe browser acceptance. The later callback or
  session request is the first server-side proof that a browser returned it.
  Evidence: the login callback and portal session handlers receive independent
  request header maps.
- Observation: the first real Docker login exposed a nonce value inside the
  otherwise safe decoded-claims diagnostic, even though the normalized identity
  projection hid it.
  Evidence: the first local `jwt_claims` event contained `nonce`; after adding
  transaction-claim redaction and rebuilding, the repeated login showed
  `nonce: "[redacted]"` and only `nonce_present: true` in identity metadata.
- Observation: the workstation RustSec checkout contains a duplicate tracked
  and stale untracked advisory, so the default `cargo audit` invocation fails
  before auditing the dependency graph.
  Evidence: `RUSTSEC-2026-0244` exists twice in the shared cache; the same audit
  against a clean clone of the official advisory database passed with only the
  repository's three documented ignores and two informational warnings.
- Observation: the repository-wide Trivy gate also scans the ignored local
  service-owner design prototype, whose stale lockfile initially contained
  three high-severity frontend build dependency advisories.
  Evidence: refreshing that ignored prototype to nanoid 3.3.18, postcss 8.5.26,
  and Vite 6.4.3 made its tests/build and the exact Trivy command pass. No
  prototype file is part of the tracked PR diff.

## Decision Log

- Decision: add one process-wide, environment-only `ENTRA_AUTH_DEBUG` flag that
  defaults to false and is independent of `LOG_LEVEL` and persisted Admin auth
  settings.
  Rationale: this mirrors Arcweft's explicit operator-owned disclosure boundary
  while preventing a database or browser caller from enabling sensitive logs.
  Date/Author: 2026-08-25 / Codex.
- Decision: keep public errors and authorization outcomes stable while using
  internal diagnostic reasons and structured log fields.
  Rationale: the production-freeze exception permits released changes, but
  richer server evidence does not require leaking verification detail to
  callers or weakening fail-closed behavior.
  Date/Author: 2026-08-25 / Codex.
- Decision: log decoded JWT protected headers and claims only, tagged as
  unverified or signature-verified; never log compact credentials.
  Rationale: operators need Arcweft-equivalent claim evidence without exposing
  a replayable bearer token or client assertion.
  Date/Author: 2026-08-25 / Codex.
- Decision: do not add authorization diagnostics to the browser session API.
  Rationale: server logs satisfy the request without expanding a released
  response contract or exposing sensitive claims to frontend JavaScript.
  Date/Author: 2026-08-25 / Codex.
- Decision: make portal session/CSRF cookie construction atomic and fail closed
  before appending either header.
  Rationale: this prevents a partial browser credential and turns the former
  silent error into an explicit public configuration failure plus a precise
  diagnostic event.
  Date/Author: 2026-08-25 / Codex.

## Outcomes & Retrospective

Implementation, local runtime validation, documentation, release metadata, and
the pre-push verification stack are complete. The only remaining work is the
GitHub handoff: commit and push the branch, update PR #107, request the one
authorized Codex re-review, address and resolve any actionable review threads,
and confirm final PR checks.

## Context and Orientation

`crates/gateway-core/src/entra.rs` verifies direct Entra JWTs and trusted Apigee
identity proofs. `crates/gateway-api/src/portal.rs` owns confidential-client
discovery, authorization URL construction, certificate-backed client
assertions, token exchange, and ID-token verification.
`crates/gateway-api/src/app.rs` owns browser login/callback/session/cookie
handlers and owner service/project authorization. `crates/gateway-proxy/src/
pingora_plane.rs` owns request-plane Entra admission before Relayna virtual-key
policy. `crates/gateway-store/src/postgres.rs` persists one-time login
transactions, portal sessions, members, and exact managed-identity bindings.
`crates/gateway-telemetry/src/lib.rs` owns JSON tracing setup and the new guarded
diagnostic recorder.

The portal session is server-side PostgreSQL state addressed by an opaque
HttpOnly cookie. A separate readable cookie supplies the CSRF token. Successful
sign-in therefore has three distinct stages: database persistence, `Set-Cookie`
construction/emission, and later browser return plus server resolution. Logs
must distinguish all three; the server can infer, but cannot directly observe,
a browser or intermediary rejecting an emitted cookie.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.28`. The Entra routes,
environment variables, PostgreSQL session schema, public error shapes, and
authorization semantics are released. `ENTRA_AUTH_DEBUG` is additive and false
by default. Existing public responses and decisions remain stable. No database
migration or cookie format change is planned. The user explicitly permits the
production-freeze exception needed for this release work.

## Plan of Work

Add the flag to gateway-api configuration and initialize a process-wide guarded
recorder in gateway-telemetry. The recorder will use one event namespace with
stable `surface`, `phase`, `outcome`, and `reason` fields, plus bounded JSON
details. Enabling the flag will admit its dedicated tracing target even when
the ordinary crate filter remains at info.

Extend gateway-core JWT verification with request/surface context, safe
unverified decoding, signature trust labels, exact discovery/JWKS/header/claim
reasons, expected configuration, normalized identity, and accepted/rejected
events. Add equivalent proof stages for trusted Apigee identity without logging
the signed header or HMAC.

Instrument portal discovery and token exchange, including safe Entra OAuth
error fields, decoded client-assertion metadata, and ID-token evidence. In
gateway-api handlers, log login initialization, callback validation, login
transaction consumption, member status/bootstrap outcome, server session
persistence, cookie construction/emission, and subsequent cookie/session/CSRF
observation. Cookie helpers will return errors instead of silently dropping
headers.

Instrument owner service/project binding results and request-plane admission
with the request ID and exact surface. Keep all diagnostic fields out of
Prometheus labels. Add tests that prove the flag defaults off, token evidence
contains claims but not compact credentials, cookie construction is checked,
and representative accepted and rejected paths preserve their public errors.

Document every field, event phase, example, sensitive-data warning, enablement
and disablement procedure, retention guidance, and the boundary between server
proof and browser inference. Update raw Kubernetes and local Compose examples
without enabling the mode by default.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    cargo fmt --all --check
    cargo test -p gateway-core entra --all-features
    cargo test -p gateway-api portal --all-features
    cargo test -p gateway-proxy --all-features
    docker compose -f deploy/local/docker-compose.yml up --build
    bash .codex/skills/code-change-verification/scripts/run.sh
    python3 scripts/validate-release-metadata.py v0.1.29

Use Computer Use with Chrome to perform local portal sign-in, session
resolution, logout, and at least one deliberately failing cookie or callback
journey. Inspect Docker logs to prove the expected safe diagnostic events.

## Validation and Acceptance

With the flag absent or false, no `relayna.authorization_debug` event may be
emitted. With it true, an accepted ID or access token logs every decoded header
and claim, expected tenant/issuer/audience/role, signature-verified trust, and
normalized identity without the compact token. A rejected token logs its exact
stage and reason and labels decoded evidence unverified until signature
validation succeeds.

Portal login logs discovery, transaction, callback, exchange, identity, member,
session, and cookie stages. Cookie logs show only presence, lengths, attributes,
emission counts, safe host/scheme comparisons, session resolution outcome, and
CSRF result. They never show values or hashes. A server-created session with a
cookie not returned by the browser is distinguishable from a database failure,
header-construction failure, expired/missing session, or stale/mixed cookie
pair.

All existing public response/status tests pass. The full formatting, Clippy,
workspace test, dependency, secret, and static-analysis stack passes. The local
Docker browser journey and logs demonstrate real behavior. Release metadata
targets the bumped version consistently.

## Idempotence and Recovery

The mode requires only an environment change and restart; disabling it and
rolling back stops new sensitive diagnostics. Compose builds and tests are safe
to rerun. Failed local containers may be restarted without deleting named
volumes. No migration is introduced. Existing portal sessions remain valid
across rollout because cookie and database formats do not change.

If GitHub review finds an issue, apply only the reviewed fix, rerun focused and
mandatory checks, reply with commit/test evidence, and resolve the thread. Do
not request a second additional review; the user authorized exactly one.

## Artifacts and Notes

Expected event envelope:

    {
      "event": "relayna.authorization_debug",
      "surface": "portal_oidc",
      "phase": "id_token_validation",
      "outcome": "rejected",
      "reason": "audience_mismatch",
      "request_id": "...",
      "details": "{\"public_error_code\":\"invalid_entra_audience\",\"token_trust\":\"signature_verified\",\"token\":{...}}"
    }

## Interfaces and Dependencies

`Config` gains `entra_auth_debug: bool`. `gateway_telemetry::init` accepts the
flag and exposes guarded authorization-debug event helpers. Core verifier entry
points gain a diagnostic context while retaining simple wrappers where useful
for existing tests and internal consumers. The environment contract gains only
`ENTRA_AUTH_DEBUG`; there is no Admin API or persisted setting for it.
