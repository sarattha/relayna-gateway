# Embeddings route alias

This living ExecPlan follows `PLANS.md` at the repository root.

## Purpose / Big Picture

Clients can POST to `/embeddings` using the same LiteLLM route as
`/v1/embeddings`, including managed and direct passthrough modes, like rerank.
An absent internal embeddings service must no longer produce `missing_service`.

## Progress

- [x] (2026-09-10) Read repository guidance and skills; created `fix/embeddings-route-alias`.
- [x] (2026-09-10) Compared routing against latest release `v0.1.35`.
- [x] (2026-09-10) Added alias, regression coverage, and route documentation.
- [x] (2026-09-10) Nine focused routing tests and the live proxy integration test passed, including both embeddings paths in managed and direct modes.
- [x] (2026-09-10) Formatting, Clippy, all workspace tests, audit, deny, machete, all 345 Nextest tests (zero skipped), and the workspace build passed.
- [x] (2026-09-10) Final diff reviewed. Full verifier stopped at unrelated Trivy findings; Gitleaks and Semgrep passed separately with no findings. Disposable test containers removed.

## Surprises & Discoveries

The released resolver contains an internal embeddings fallback even though the
user has no internal embeddings service. Explicit registered service routes
are checked before unversioned aliases, including rerank. The resolved Route
controls settings and policy; the incoming path is preserved upstream.

The first live test hit a macOS native certificate-store I/O error. Setting
`SSL_CERT_FILE=/etc/ssl/cert.pem` allowed Pingora to start and the test passed.
Dedicated disposable PostgreSQL and Redis containers avoid touching the existing
local gateway stack.

## Decision Log

- (2026-09-10, Codex) Replace the fallback with `LiteLlmEmbeddings` as explicitly
  requested by the user. Preserve registered service precedence like rerank.
- (2026-09-10, Codex) Keep legacy Route enum and persisted policy decoding intact
  to avoid changing historical records or silently broadening old policies.
  Clients of this alias need canonical `/v1/embeddings` policy permission.

## Outcomes & Retrospective

Implementation and documentation are updated. Focused tests and live proxy
forwarding passed in both modes. All Rust checks passed. The full verification
stack remains blocked by 14 high-severity Trivy findings in the unchanged
`design-prototypes/service-owner-monitoring/package-lock.json`; no route-fix
dependency changes were made.

## Context and Orientation

`crates/gateway-core/src/routing.rs` resolves HTTP methods and paths into Route
identities. `crates/gateway-proxy/src/pingora_plane.rs` uses those identities for
LiteLLM settings, authentication, policy, and usage. The process integration test
in `crates/gateway-api/tests/proxy_process_integration.rs` exercises real Pingora
against a mock upstream with PostgreSQL and Redis available.

## Compatibility Boundary

Latest release: `v0.1.35`. This intentionally replaces the released implicit
internal-service fallback for `/embeddings`, authorized by the user's request.
Explicit registered services retain precedence. Canonical `/v1/embeddings`
settings, policy strings, credentials, schemas, and response formats stay intact.
No migration is needed, and old serialized route identities remain readable.

## Plan of Work

Extend the LiteLLM embeddings match arm, remove its internal-service fallback,
and test both paths, canonical identity, absent service selection, and rejected
methods. Extend the existing proxy integration table and upstream path assertions.
Exercise both paths in direct mode with a LiteLLM test bearer, verify upstream
credentials, and restore managed mode. Update current features, LiteLLM
passthrough, and auth route documentation.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-core routing
    cargo test -p gateway-api --test proxy_process_integration -- --nocapture
    bash .codex/skills/code-change-verification/scripts/run.sh

## Validation and Acceptance

Both POST paths resolve to LiteLLM with canonical `/v1/embeddings` identity and
no service name. Unsupported methods remain rejected. The process integration
exercise must forward both paths successfully and preserve the requested path.
The mandatory verifier must pass; report any environment-gated tests honestly.

## Idempotence and Recovery

Edits and verification can be repeated. No deployment, database migration, or
commit is requested. Keep work on the new branch; preserve unrelated files.

## Artifacts and Notes

The final diff includes core routing, process regression coverage, documentation,
and this plan. Verification logs: `/tmp/relayna-embeddings-verification.log`,
`/tmp/relayna-embeddings-proxy-test.log`, and `/tmp/relayna-embeddings-build.log`.
The service-backed tests used dedicated `postgres:16-alpine` and `redis:7-alpine`
containers, with the database initialized by the store. To reproduce, start
fresh containers and set `DATABASE_URL` and `REDIS_URL` to their published local
ports. Set `SSL_CERT_FILE=/etc/ssl/cert.pem`, `RUST_TEST_THREADS=1`, and
`NEXTEST_TEST_THREADS=1` for the verifier on this Mac. Disposable test services
are removed after verification.

## Interfaces and Dependencies

No new dependency or route setting is introduced. Both paths use existing
`Route::LiteLlmEmbeddings` and the `embeddings` route setting.
