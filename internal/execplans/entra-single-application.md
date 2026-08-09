# Consolidate Entra on One Application Registration

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

Maintain this document in accordance with `PLANS.md` at the repository root.

## Purpose / Big Picture

Relayna Gateway operators should provision one Microsoft Entra application for
human portal sign-in and Gateway API access. The application acts as both the
confidential Web client and the API resource, following Arcweft's established
pattern. It exposes `gateway.invoke` for request-plane callers and
`gateway.monitor.read` for service-monitoring callers. Separate managed
identities receive only the roles required by their workloads, while Relayna
virtual-key policy and exact service bindings remain authoritative for
fine-grained access.

The result is observable in the development OIDC fixture, Kubernetes contract,
deployment verifier, configuration tests, browser sign-in journey, and owner
API token journey. Production guidance should contain one application ID, its
derived `api://<application-id>` resource identifier, two application roles,
and role-to-managed-identity assignments.

## Progress

- [x] (2026-08-09 00:00Z) Read the repository manifesto, contributor rules,
  implementation skills, existing Entra implementation, and Arcweft's
  single-application provisioning reference.
- [x] (2026-08-09 00:00Z) Established `v0.1.25` as the released compatibility
  boundary and created `codex/entra-single-application` from a clean `main`.
- [x] (2026-08-09 15:20Z) Implemented the canonical shared application-ID configuration and directly
  replace the duplicated mode-specific environment variables.
- [x] (2026-08-09 15:30Z) Updated the development issuer and integration tests to model one Web/API
  application, the `gateway.invoke` and `gateway.monitor.read` roles, and
  separate least-privilege workload clients.
- [x] (2026-08-09 15:35Z) Bumped the release to `0.1.26` and updated the Kubernetes contract,
  operational handoff, release documentation, and Entra examples.
- [x] (2026-08-09 16:05Z) Ran focused tests, the mandatory verification stack, release metadata
  validation, and Computer Use browser journeys against development OIDC.
- [x] (2026-08-09 16:15Z) Committed and published PR #104, waited for the first
  Codex review, addressed all actionable feedback, replied to and resolved the
  review thread, and reran affected checks.

## Surprises & Discoveries

- Observation: The runtime already permits the portal client ID and owner API
  audience to identify the same Entra application; the two-registration
  requirement exists in deployment documentation and fixtures rather than an
  OAuth or Rust verifier constraint.
  Evidence: `crates/gateway-api/src/config.rs` loads the values independently,
  and `crates/gateway-api/src/portal.rs` verifies portal ID tokens against the
  configured client ID.

- Observation: Arcweft requests `api://<application-id>/.default` but verifies
  v2 access tokens whose `aud` claim is the application ID GUID. The identifier
  URI and token audience are related values, not two application registrations.
  Evidence: Arcweft's `arcweft-entra-config.json` and
  `frontend/lib/bff/service-token.test.ts`.

- Observation: The GitHub CLI installation is present but its saved token is
  invalid. Local implementation can proceed; PR publication and thread-aware
  review handling require authentication to be refreshed.
  Evidence: `gh auth status` on 2026-08-09.

- Observation: Chrome rendered new loopback pages as blank in the current user
  profile, while Safari rendered the same responses normally. Computer Use
  completed the requested journey in Safari without changing the server or
  test data.
  Evidence: the development account chooser, authenticated Admin overview,
  v0.1.26 indicator, and expanded single-application Entra settings were all
  inspected through Safari accessibility state and screenshots.

- Observation: The default Cargo audit cache contained untracked duplicate
  advisory files after its fetch, while a fresh temporary RustSec checkout
  loaded cleanly and passed. Trivy also initially inspected an ignored local
  `design-prototypes` directory that is absent from Git; the tracked repository
  surface passed when that local-only directory was excluded.
  Evidence: fresh-database `cargo audit` reported zero unignored
  vulnerabilities, and tracked-surface Trivy reported zero HIGH or CRITICAL
  findings for `Cargo.lock`.

- Observation: The first Codex review found that three checked-in environment
  harnesses still supplied the removed `ENTRA_AUDIENCE` variable. Migrating
  their compose files alone was insufficient because their mock tokens also
  needed the newly defaulted `gateway.invoke` role.
  Evidence: commit `8ba768e` migrates all three harnesses, adds the role claim,
  and adds a development OIDC regression test; the review thread was replied
  to and resolved.

## Decision Log

- Decision: Use one canonical `ENTRA_APPLICATION_ID` for portal client identity
  and v2 access-token audiences. Remove `PORTAL_OIDC_CLIENT_ID`,
  `OWNER_ENTRA_AUDIENCE`, and `ENTRA_AUDIENCE` from the runtime deployment
  contract instead of retaining compatibility aliases.
  Rationale: The user explicitly authorized breaking the production freeze, and
  a direct replacement makes contradictory multi-application configuration
  impossible while keeping exact audience checks.
  Date/Author: 2026-08-09 / Codex.

- Decision: Define `gateway.invoke` and `gateway.monitor.read` on the shared
  application and assign them to separate managed-identity security boundaries.
  Rationale: Entra should express coarse service-to-service capability, while
  Relayna retains virtual-key governance and exact service ownership.
  Date/Author: 2026-08-09 / Codex.

- Decision: Do not add a database migration or new authorization path.
  Rationale: Existing token-role checks, managed-identity bindings, and
  front-door role configuration already implement the required authorization;
  the missing behavior is the shared application contract and representative
  development coverage.
  Date/Author: 2026-08-09 / Codex.

## Outcomes & Retrospective

The implementation and first review cycle are complete. Relayna now has one canonical Entra
application ID for browser and API token audiences, with separate invoke and
monitoring workload identities represented by distinct application roles.
Focused and full Rust tests, development OIDC integration, Admin UI tests,
strict documentation build, release metadata, dependency checks, secret
scanning, static analysis, and Computer Use browser validation all pass. PR
104 is open, its initial CI run passed, and its one actionable Codex thread was
fixed, replied to, and resolved.

## Context and Orientation

`crates/gateway-api/src/config.rs` loads the request-plane Entra verifier,
confidential portal OIDC client, and owner API verifier. `crates/gateway-api/src/portal.rs`
implements certificate-backed authorization-code exchange. The owner API checks
managed-identity tenant, client/object ID, token roles, and exact service binding
in `crates/gateway-api/src/app.rs` and `crates/gateway-store/src/postgres.rs`.

`scripts/entra/development-oidc.mjs` is a production-refusing Entra-shaped local
issuer. `scripts/entra/entra-integration.test.mjs` checks its certificate flow and
the raw Kubernetes contract. `scripts/entra/verify-deployment.sh` validates a
deployed ConfigMap, certificate Secret, private Ingress, NetworkPolicy, and
availability. `docs/operations/entra-integration-requirements.md` is the main
production handoff and currently asks DevOps to create two application
registrations.

An Entra application registration can be both a confidential Web client and an
API resource. The application ID identifies the client and is the expected
audience of v2 access tokens. Its identifier URI, normally
`api://<application-id>`, is the resource used when managed identities request
`/.default`. Application roles declared on that same application are emitted in
authorized workload tokens.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.25`; environment variables are
a released deployment surface. The user authorized a direct production-contract
replacement. Version `0.1.26` removes the three duplicated mode-specific
application/audience variables and requires `ENTRA_APPLICATION_ID` whenever an
Entra mode is enabled. The changelog and deployment guide must give an explicit
rollout mapping. Public routes, response shapes, PostgreSQL state, Redis state,
and token-role semantics remain unchanged.

## Plan of Work

First, load `ENTRA_APPLICATION_ID` once in `crates/gateway-api/src/config.rs`
and supply it to each enabled Entra mode. Extend `config_contract.rs` with
canonical shared-ID, missing-ID, role-default, and tenant/issuer alignment
cases. Remove the superseded mode-specific variables directly.

Next, reshape `scripts/entra/development-oidc.mjs` around one application ID and
identifier URI. Give an invoke workload and a monitoring workload different
client/object IDs, secrets, and application roles. Extend the Node integration
test to prove each token has the shared audience and only its assigned role, and
that role/resource mismatches fail.

Then update `deploy/kubernetes/relayna-gateway.yaml`, the read-only deployment
verifier, and Entra documentation. Bump workspace, image, Admin UI, README,
release, and changelog metadata to `0.1.26`. Follow Arcweft's vocabulary and
examples without copying its product-specific roles or paths.

Finally, run focused Node and Rust tests, generate any checked-in Admin UI assets
required by the version bump, run the repository verification stack, and use
Computer Use against the development issuer and Gateway. Publish a draft PR,
wait for the first Codex review, implement all actionable feedback, reply and
resolve every addressed thread, and rerun affected validation.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    node --test scripts/entra/entra-integration.test.mjs
    cargo test -p gateway-api --test config_contract
    npm run build:admin-ui --prefix crates/gateway-api/admin-ui
    bash .codex/skills/code-change-verification/scripts/run.sh
    python3 scripts/validate-release-metadata.py v0.1.26

Start isolated development OIDC, PostgreSQL, Redis, and Gateway services using
the repository's existing local test procedure. Use Computer Use to complete
administrator sign-in and inspect the shared-application Entra settings. Use
the development token endpoint and Rust end-to-end test to prove invoke and
monitoring role separation, including denial of an invoke token on owner APIs.

## Validation and Acceptance

Configuration tests must prove one canonical application ID configures all
enabled Entra audiences and that every enabled Entra mode fails startup when
the canonical ID is absent.

Development OIDC tests must prove browser ID tokens and both workload access
tokens belong to one application, each workload receives only its assigned app
role, wrong resource/client credentials fail, and certificate-backed portal
exchange still rejects replay, wrong assertion audience, and wrong certificate
thumbprints.

The full Rust formatting, linting, and all-features workspace tests must pass.
Release metadata validation for `v0.1.26` must pass. Computer Use must complete
the development browser journey without exposing Entra tokens to JavaScript.
The draft PR must have no unresolved actionable thread after the first Codex
review response is processed.

## Idempotence and Recovery

All code and documentation edits are ordinary Git changes. Focused and full
tests are safe to rerun. The development issuer refuses production markers and
uses temporary/generated development-only credentials. Any interrupted local
services can be stopped and restarted on isolated loopback ports. No production
tenant, role assignment, managed identity, database row, or Git tag is created
by local verification. Review fixes are committed separately when useful and
pushed to the same branch.

## Artifacts and Notes

Arcweft references:

- `/Users/jobz/Works/arcweft/scripts/entra/arcweft-entra-config.json`
- `/Users/jobz/Works/arcweft/scripts/entra/01-configure-application.sh`
- `/Users/jobz/Works/arcweft/scripts/entra/04-assign-application-roles.sh`
- `/Users/jobz/Works/arcweft/docs/operations/entra-confidential-bff.md`

## Interfaces and Dependencies

The canonical external configuration is `ENTRA_APPLICATION_ID`.
`PORTAL_OIDC_CLIENT_ID`, `OWNER_ENTRA_AUDIENCE`, and `ENTRA_AUDIENCE` are removed.
Entra v2 token verification stays exact and RSA/JWKS based. Portal client
assertions remain PS256 with `x5t#S256`; access tokens remain RS256. Application
roles are `gateway.invoke` and
`gateway.monitor.read`. Relayna virtual-key and managed-identity binding models
do not change.
