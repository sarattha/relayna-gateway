# Cortex gateway slice in shared Kind

The requested Cortex E2E stack must use the real Relayna Gateway between the
Cortex BFF and development Accessa mocks. The gateway validates an RS256 user
token from Keystone's existing development OIDC issuer and a BFF-owned virtual
key, forwards trusted caller metadata, and admits every router turn separately.
The gateway's existing released Accessa HTTP/WebSocket contract already handles
these requirements; this change supplies reproducible deployment and bootstrap.

## Progress

- [x] 2026-10-10: Read gateway instructions, design manifesto and repository skills.
- [x] 2026-10-10: Created isolated worktree from current origin/main e1bf6d2, preserving primary work.
- [x] 2026-10-10: Confirmed existing full-path forwarding, trusted headers, per-turn admission and WS limits with Cortex mock owner.
- [x] 2026-10-10: Added dedicated development namespace gateway/database/cache manifests.
- [x] 2026-10-11: Built latest runtime, loaded both Kind nodes, deployed ready gateway/database/cache and bootstrapped service/key/profile.
- [x] 2026-10-11: Full repository verification helper passed (fmt, Clippy, cargo tests, audit, deny, machete, nextest417, Trivy, Gitleaks, Semgrep).
- [x] 2026-10-11: Supplemental Accessa E2E passed against isolated Kind PostgreSQL database and Redis DB 15.
- [x] 2026-10-11: Real analyst RS256 JWT and BFF key upgraded WebSocket with HTTP 101; malformed JWT, missing key and wrong key each returned HTTP 401.
- [x] 2026-10-11: Repeated bootstrap preserved the key and authentication profile revision 1.
- [x] 2026-10-11: Recorded runtime commit/image identity; final verified delivery commit is recorded in the draft PR.

## Surprises & Discoveries

Latest release v0.1.43 already supports Accessa WS GET run paths. Shared Keystone
development issuer is localhost:5556 but discovery/JWKS must resolve inside the
cluster. An ExternalName service preserves compatibility with an earlier short
JWKS hostname; Keystone now advertises its FQDN. The dedicated PostgreSQL storage is
ephemeral; pod recreation loses its test keys/configuration and requires bootstrap. Shared Docker image imports/builds caused intermittent
Kind API TLS/etcd timeouts and dropped port forwards. A supplemental DB-backed
Accessa test first raced creation of its isolated database, then timed out after
a port-forward drop. Its default workspace invocation skipped DB-backed work
without DATABASE_URL; this baseline is not claimed as database-backed evidence.

Keystone's default-deny namespace policy initially blocked discovery, producing
invalid_entra_token. Keystone's owner added narrow OIDC ingress for the gateway.
With no inherited policy rows, the released gateway uses an LLM-only default,
which intersected the Cortex service key policy and denied the request. Bootstrap
now sets the dedicated project's service-only policy without modifying global
policy. An unchanged authentication-profile PATCH advances its revision and
invalidates live admissions; bootstrap omits that PATCH section when unchanged.

## Decision Log

2026-10-10: Compatibility boundary is released v0.1.43; preserve every runtime
route, frame format, authentication rule and database schema. Add only local
development deployment and bootstrap tooling. No authentication bypass exists:
JWT signature, exact issuer/tenant/audience, expiry, profile assignment, key and
service policy remain mandatory. Local OIDC and Accessa components are explicit
development fixtures. No hosted Actions are added or triggered.

2026-10-10: Use cortex-e2e namespace and independent gateway PostgreSQL/Redis.
Keystone's authoritative document store remains unchanged. Raw channel key is
returned once by the admin API and saved only in a mode0600 excluded local file
for BFF secret provisioning; it is never printed or committed.

2026-10-11: Preserve the released runtime and correct development configuration
through supported admin APIs. Keep per-user Keystone delegation in the explicit
private-header fixture; the mock broker must verify it and bind it to the
gateway's trusted caller identity. No static owner token establishes user access.

## Validation

Run `.codex/skills/code-change-verification/scripts/run.sh` for formatting,
workspace Clippy and tests. Validate the manifest with kubectl dry-run, bootstrap
through real admin APIs, inspect saved profile/key metadata, and exercise valid
and invalid user-token/key requests through the real Kind proxy. Root Cortex
E2E verification will cover full browser-to-agent and browser-to-Keystone paths.

The full repository helper passed with Rust/Cargo 1.98.0: format, Clippy with
warnings denied, workspace tests, audit (repository's existing advisory ignores),
deny, machete, nextest 417/417, Trivy, Gitleaks and Semgrep. The unchanged manifest
passed client dry-run and was applied successfully. Its later dry-run recheck
could not download Kubernetes OpenAPI because of a transient TLS handshake
timeout. After final bootstrap edits, focused checks passed Python 3.14.8 syntax,
local document links, diff whitespace, and two actual bootstrap reruns. No Rust
code changed after its passing checks.

Supplemental `cargo test -p gateway-api --test accessa_e2e -- --nocapture` passed
1/1 in 9.04s with DATABASE_URL pointing to isolated relayna_verify on the deployed
Kind PostgreSQL and REDIS_URL using isolated DB 15. It covers HTTP/WS trusted
identity isolation, per-turn admission, heartbeat, limits, revocation, budget,
replay and authentication profile changes.

## Outcomes & Retrospective

Latest runtime source is e1bf6d25b1507ef0d2fd60de3d3236f964353649 (v0.1.43).
The built linux/arm64 image relayna-gateway:cortex-e2e-e1bf6d2 has Docker identity
sha256:616dac230e0d79e9b46cb136f4edcb31a4169c181bed694f19359cce0f57e298.
Gateway, PostgreSQL and Redis are 1/1 ready in kind-keystone/cortex-e2e. Project
f62963a6-70a9-466a-a34e-46d45879803c owns key
41963b4d-0052-4c83-8f67-d852ede78996 and service cortex-web, routed to
http://accessa-adapter:8080 with profile cortex-development-users revision 1.

Signed user authentication and a real upstream WS handshake passed. The valid
documents HTTP request passed gateway checks and reached the mock stack, whose
HTTP 502 downstream response remains a root-owned Keystone integration task at
this handoff. Full browser normal-chat and Keystone retrieval evidence belongs
to the coordinating Cortex task. This is development evidence and does not
establish production Entra connectivity or production Accessa behavior.
