# Entra Production Readiness for v0.1.24

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

Relayna Gateway 0.1.24 must authenticate its confidential browser client with
Microsoft Entra by using a certificate-signed `private_key_jwt`, expose the
service-owner monitoring API through the production Kubernetes control-plane
path, and give operators a read-only way to verify the checked-in ConfigMap,
Secrets, mounted certificate material, routing, and live workload. Browser
tokens remain server-side, existing `/admin-ui` routes remain stable, and exact
Relayna service bindings continue to constrain every managed-identity token.

The DevOps team owns Helm generation, Entra tenant application provisioning,
managed-identity creation, AKS workload-identity federation, app-role
assignment, and production secret synchronization. This plan documents the
inputs and contracts that work must satisfy but does not automate those
DevOps-owned operations.

## Progress

- [x] (2026-08-09 06:07Z) Created branch `agent/entra-production-ready`, read
  repository guidance and mandatory skills, and inspected the local Arcweft
  confidential-BFF, certificate, development OIDC, and verification patterns.
- [x] (2026-08-09 06:07Z) Established `v0.1.23` as the released compatibility
  boundary and received an explicit v0.1.24 production-freeze exception.
- [x] (2026-08-09 06:25Z) Replaced portal client-secret authentication with PS256
  `private_key_jwt`, mounted private-key/public-certificate paths, and focused
  tests, including the local development issuer.
- [x] (2026-08-09 06:25Z) Added development certificate generation plus live
  expiry and key/certificate matching checks; rotation and rollback guidance
  remains in the post-review documentation milestone.
- [x] (2026-08-09 06:25Z) Routed `/owner/v1` through the private control Ingress
  and made the NetworkPolicy admit the selected internal ingress controller on
  port 8081.
- [x] (2026-08-09 06:25Z) Added read-only verification for the raw Kubernetes
  ConfigMap, Secrets, Deployment, Ingress, NetworkPolicy, certificate pair, and
  running workload.
- [x] (2026-08-09 07:06Z) Passed focused Rust and Node checks and the mandatory
  Rust, dependency, secret, and static-analysis stack for tracked PR scope.
- [x] (2026-08-09 07:02Z) Built the image, exercised the raw manifest on an
  isolated Docker Desktop kind cluster, and used Computer to test signed-out,
  administrator, pending, blocked, owner, managed-identity, logout, and scoped
  `/owner/v1` paths.
- [ ] Commit and push the scoped patch, open a draft PR, and wait for the first
  Codex review.
- [ ] Fix, reply to, and resolve every actionable first-review thread, then
  finalize 0.1.24 changelog/release documentation and the DevOps requirements
  report.

## Surprises & Discoveries

- Observation: PR #99 already added `PORTAL_OIDC_*` and `OWNER_ENTRA_*` to the
  raw Kubernetes manifest, so issue #100's deployment checklist is partly
  stale.
  Evidence: `deploy/kubernetes/relayna-gateway.yaml` on `main`.
- Observation: the control Ingress exposes only `/admin-ui`, although owner
  APIs are served from the same Axum control listener under `/owner/v1`.
  Evidence: `deploy/kubernetes/relayna-gateway.yaml` and
  `crates/gateway-api/src/app.rs`.
- Observation: Arcweft signs five-minute client assertions with PS256, exact
  token-endpoint audience, issuer and subject equal to client ID, unique JTI,
  and an `x5t#S256` certificate thumbprint.
  Evidence: local Arcweft `frontend/lib/bff/crypto.ts` and
  `frontend/lib/bff/oidc.ts`.
- Observation: a user-owned `.gitignore` change was present before this branch
  and is excluded from this work.
  Evidence: initial `git status -sb` and `.gitignore` diff.
- Observation: the GitHub CLI token is currently invalid. The connected GitHub
  app can create and inspect the PR, but thread-aware review resolution will
  require refreshed CLI authentication if the token remains invalid.
  Evidence: initial `gh auth status`.
- Observation: the mandatory verifier's Trivy command scans the user-owned,
  git-ignored `design-prototypes/` directory and found four unrelated frontend
  advisories there; the tracked PR scope reports zero HIGH/CRITICAL findings
  when that directory is excluded.
  Evidence: verification transcript and `git check-ignore design-prototypes`.
- Observation: Docker Desktop already exposed local ports 18090 and 18381 for
  unrelated Relayna/Arcweft work. An isolated kind-only OIDC sidecar and ports
  28090/28381 prevented stale UI state from contaminating the browser test.
  Evidence: `lsof`, kind port-forward output, and Computer UI state showing
  v0.1.24 with an empty PostgreSQL database.
- Observation: first-administrator bootstrap is durable. After the configured
  email signed in, emptying `PORTAL_ADMIN_EMAILS` and restarting both replicas
  still returned the persisted active Admin member.
  Evidence: Computer UI sign-in before and after the kind rollout.

## Decision Log

- Decision: use direct replacement rather than support both client secrets and
  certificates.
  Rationale: `v0.1.23` is the latest release tag; portal OIDC configuration is
  unreleased branch-local behavior intended for v0.1.24. One fail-closed
  certificate path is simpler and avoids shipping a production secret mode.
  Date/Author: 2026-08-09 / Codex.
- Decision: mount the private key and public certificate from a dedicated
  Kubernetes Secret rather than inject PEM material through `envFrom`.
  Rationale: the existing application Secret is loaded into environment
  variables. A dedicated read-only volume follows Arcweft's production pattern
  and keeps key bytes out of environment variables and ConfigMaps.
  Date/Author: 2026-08-09 / Codex.
- Decision: keep Helm, Entra provisioning, and AKS workload-identity automation
  out of this patch.
  Rationale: the user assigned those responsibilities to DevOps and requested
  only the raw ConfigMap/Secret contract plus a requirements report.
  Date/Author: 2026-08-09 / Codex.
- Decision: accept the user-authorized production-freeze exception for the
  scoped v0.1.24 readiness patch.
  Rationale: the requested authentication and network fixes must land before
  production-ready Entra support can be claimed for v0.1.24.
  Date/Author: 2026-08-09 / Codex.

## Outcomes & Retrospective

Work is in progress. Completion requires green runtime, security, kind, UI,
and first-review evidence with no unresolved actionable Codex comments.

## Context and Orientation

`crates/gateway-api/src/portal.rs` owns the confidential OIDC token exchange.
`crates/gateway-api/src/config.rs` maps environment variables into the runtime.
`scripts/entra/development-oidc.mjs` is the production-refusing local identity
provider used for browser and workload journeys. The raw production deployment
contract is `deploy/kubernetes/relayna-gateway.yaml`; its control Service serves
both `/admin-ui` and `/owner/v1` on port 8081.

`private_key_jwt` is OAuth client authentication in which Relayna signs a
short-lived JWT with the private half of a certificate registered on the Entra
Web application. The assertion is sent only to the discovered token endpoint.
The public certificate is not secret; the private key is mounted read-only from
a DevOps-managed Kubernetes Secret.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.23`. Portal OIDC certificate
configuration and `/owner/v1` were introduced after that tag, so the secret
configuration can be replaced directly and the missing Kubernetes route can be
added without a compatibility shim. Existing released `/admin-ui`, request
plane, virtual-key, database, Redis, streaming, and usage-event contracts remain
unchanged.

## Plan of Work

Update the portal runtime to load a PKCS#8 or PKCS#1 RSA private key and matching
X.509 certificate paths at startup, create a five-minute PS256 assertion bound
to the discovered token endpoint, and send the standard client assertion form
fields. Update the local development issuer to validate the same protected
header, claims, signature, lifetime, and single-use JTI behavior.

Add a development certificate generator and a read-only deployment verifier.
Update the Kubernetes ConfigMap with file paths, add a dedicated certificate
Secret and read-only volume, add `/owner/v1` to the control Ingress, and align
the NetworkPolicy with the internal ingress namespace. Update operator docs for
initial install, overlap rotation, rollback, diagnosis, and the exact DevOps
application/role/managed-identity requirements.

## Concrete Steps

Run focused tests while iterating, then from the repository root run:

    cargo test -p gateway-api portal --all-features
    node --test scripts/entra/entra-integration.test.mjs
    bash .codex/skills/code-change-verification/scripts/run.sh
    python3 scripts/validate-release-metadata.py v0.1.24

Build the image, create or reuse a local kind cluster backed by Docker Desktop,
apply the raw manifest and supporting local PostgreSQL/Redis/LiteLLM fixtures,
run the deployment verifier, and exercise portal login, pending, administrator,
owner, denied, logout, and scoped `/owner/v1` journeys through Computer.

## Validation and Acceptance

The portal token request must contain `client_assertion_type` and a PS256
`client_assertion`, contain no client secret, use the exact discovered token
endpoint as audience, and carry the public certificate's SHA-256 thumbprint.
Missing/unreadable/invalid certificate material must fail startup. The local
issuer must reject wrong algorithms, thumbprints, audiences, expired/replayed
assertions, and invalid signatures.

The raw manifest must mount certificate material read-only, expose both
`/admin-ui` and `/owner/v1` through the control Ingress, and admit the internal
ingress controller to port 8081. The verifier and kind journey must prove the
ConfigMap, Secret, volume, route, NetworkPolicy, rollout, certificate pair,
health, browser roles, and exact owner-service denial behavior.

## Idempotence and Recovery

Certificate generation writes only to an explicit or ignored development
directory and refuses to overwrite existing material. Verification is
read-only. Kubernetes resources are declarative and safe to reapply. Rotation
keeps both old and new public certificates registered during rollout; rollback
restores the previous Secret version while the old certificate remains valid.
No tenant object, managed identity, role assignment, database row, or Git tag is
created by the operational verifier.

## Artifacts and Notes

The final DevOps report will list the confidential Web application, owner API
resource/audience, exact application role, required managed identities and
Relayna bindings, redirect/logout URIs, issuer/discovery values, certificate
Secret keys and mount paths, verification commands, and rotation/rollback
ownership.

## Interfaces and Dependencies

The final runtime interface uses `PORTAL_OIDC_PRIVATE_KEY_PATH` and
`PORTAL_OIDC_CERTIFICATE_PATH` instead of `PORTAL_OIDC_CLIENT_SECRET`.
`OWNER_ENTRA_*` remains unchanged. The standard assertion type is
`urn:ietf:params:oauth:client-assertion-type:jwt-bearer`; PS256 and
`x5t#S256` match Arcweft and Entra's confidential-client posture.
