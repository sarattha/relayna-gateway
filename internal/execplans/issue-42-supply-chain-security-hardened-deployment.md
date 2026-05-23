# Issue 42 Supply-Chain Security and Hardened Deployment

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document follows `PLANS.md`.

## Purpose / Big Picture

Harden Relayna Gateway's build, release, container, and Kubernetes production
defaults so operators can deploy the gateway with supply-chain evidence and
restricted runtime permissions. After this change, CI checks dependencies,
secrets, static analysis, and filesystem/image vulnerabilities; releases
publish signed GHCR images with SBOM and provenance artifacts; and the
Kubernetes example separates public proxy traffic from the private control
plane.

## Progress

- [x] (2026-05-23 09:10Z) Read issue #42, `internal/codex-roadmap-goals.md`,
  `internal/design-manifesto.md`, `PLANS.md`, and the freeze/verification
  skill guidance.
- [x] (2026-05-23 09:12Z) Confirmed latest release tag is `v0.0.14` and the
  pre-change freeze perimeter test passes.
- [x] (2026-05-23 09:25Z) Added scanner configuration and local/CI
  verification wiring.
- [x] (2026-05-23 09:30Z) Added release SBOM, image scan, signing, and
  provenance steps.
- [x] (2026-05-23 09:35Z) Hardened Docker/Kubernetes production defaults and
  split proxy/control services.
- [x] (2026-05-23 09:42Z) Updated deployment, release, operations, and
  exception documentation.
- [x] (2026-05-23 09:55Z) Ran Rust, Node, docs, audit, deny, machete, nextest,
  Gitleaks, and Semgrep verification successfully.
- [ ] Complete local Trivy filesystem/image scan after vulnerability DB access
  is available.

## Surprises & Discoveries

- Observation: The repository already runs Rust format, Clippy, workspace tests,
  admin UI tests, docs build, release metadata validation, and the v0.0.14
  freeze perimeter, but has no scanner policy files yet.
  Evidence: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, and a
  targeted search for `deny.toml`, `.gitleaks.toml`, `.semgrep.yml`, and
  `.trivyignore`.
- Observation: The existing Kubernetes `NetworkPolicy` allows all ingress and
  all egress.
  Evidence: `deploy/kubernetes/relayna-gateway.yaml` has `ingress: - {}` and
  `egress: - {}`.
- Observation: `cargo audit` found two transitive vulnerabilities and two
  unmaintained transitive crates already present through SQLx/Pingora.
  Evidence: local `cargo audit` and `cargo deny check` output for
  `RUSTSEC-2023-0071`, `RUSTSEC-2024-0437`, `RUSTSEC-2024-0388`, and
  `RUSTSEC-2025-0069`.
- Observation: Local Trivy could not complete its first vulnerability database
  download from either `ghcr.io/aquasecurity/trivy-db` or
  `public.ecr.aws/aquasecurity/trivy-db`; Docker build also stalled while
  resolving Docker Hub image config.
  Evidence: local Trivy and Docker build commands made no progress until
  terminated.

## Decision Log

- Decision: Treat `v0.0.14` as the compatibility and production freeze
  baseline.
  Rationale: It is the latest release tag and the active freeze baseline in
  repository guidance.
  Date/Author: 2026-05-23 / Codex.

- Decision: Keep runtime routes, environment variable names, database schemas,
  Redis key formats, and proxy behavior unchanged.
  Rationale: Issue #42 is scoped to supply-chain, release, container, and
  deployment hardening.
  Date/Author: 2026-05-23 / Codex.

- Decision: Make CI security checks strict, while documenting local setup and
  exception handling.
  Rationale: Production supply-chain checks should fail on actionable findings;
  local developer machines may need tool installation before running the full
  stack.
  Date/Author: 2026-05-23 / Codex.

- Decision: Add temporary, documented RustSec exceptions for existing
  non-high/critical transitive findings and keep CI failing for new advisories.
  Rationale: The issue acceptance emphasizes high/critical blocking where
  practical; the current findings are transitive through SQLx/Pingora and need
  upstream upgrades or replacement tracking.
  Date/Author: 2026-05-23 / Codex.

- Decision: Allow local workspace path dependencies in `cargo deny` while
  keeping advisory, source, license, and registry policy active.
  Rationale: Internal crates use path dependencies without versions by design;
  treating those as denied wildcard registry dependencies blocks the workspace
  without improving supply-chain posture.
  Date/Author: 2026-05-23 / Codex.

## Outcomes & Retrospective

Implemented supply-chain and deployment hardening for issue #42. CI now has a
strict security job, release publishes SBOM/signing/provenance artifacts, local
verification includes the new scanner stack, and the Kubernetes example is
restricted by default with split proxy/control Services and non-open
NetworkPolicy rules.

Local verification passed for Rust format, Clippy, workspace tests, nextest,
admin UI tests, freeze perimeter tests, docs build, `cargo audit` with
documented exceptions, `cargo deny check`, `cargo machete`, Gitleaks, and
Semgrep. Local Trivy filesystem/image scanning and Docker image build remain to
be rerun in an environment with working external OCI registry access.

## Context and Orientation

Relayna Gateway is a Rust workspace with one published Docker image. CI is
configured in `.github/workflows/ci.yml`; tag releases are configured in
`.github/workflows/release.yml`; local verification is routed through the root
`Makefile` and `.codex/skills/code-change-verification/scripts/run.sh`.

The production Kubernetes example is `deploy/kubernetes/relayna-gateway.yaml`.
It currently deploys one container that serves proxy traffic on `8080` and the
control plane, admin UI, readiness, and metrics on `8081`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.0.14`. This change affects
freeze surfaces for CI, release workflow, Docker packaging, and Kubernetes
deployment defaults. It must not alter released gateway runtime API behavior,
public route response shapes, auth semantics, PostgreSQL schemas, Redis key
formats, usage event shapes, or provider proxy semantics.

## Plan of Work

Add security scanner policy files at the repository root and document exception
requirements in `docs/security-exceptions.md`.

Extend local verification through the `Makefile` and code-change verification
script with Rust dependency checks, unused dependency checks, nextest, Trivy,
Gitleaks, and Semgrep.

Extend CI with a dedicated strict security job. Keep existing jobs intact.

Extend release workflow permissions and steps so GHCR images are scanned,
signed by Cosign keyless signing, accompanied by SBOM artifacts, and attested
with GitHub provenance.

Harden the Kubernetes example with restricted pod/container security contexts,
read-only root filesystem, split proxy/control services, internal control-plane
ingress, and explicit NetworkPolicy ingress/egress rules.

Update deployment, release, operations, and MkDocs navigation for production
defaults, local-dev exceptions, and artifact verification.

## Concrete Steps

    cd /Users/jobz/Works/relayna-gateway
    node tests/freeze-v0.0.14-perimeter.test.mjs
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh
    mkdocs build --strict
    kubectl apply --dry-run=client -f deploy/kubernetes/relayna-gateway.yaml

Run security tools locally when installed:

    cargo audit
    cargo deny check
    cargo machete
    cargo nextest run --workspace --all-features
    trivy fs --severity HIGH,CRITICAL --exit-code 1 .
    gitleaks detect --source . --no-git
    semgrep scan --config .semgrep.yml

## Validation and Acceptance

CI includes Rust dependency, image, secret, filesystem, and static-analysis
checks. Release images publish SBOM, signature, and provenance artifacts.
Kubernetes manifests follow restricted pod security defaults and no longer use
open ingress/egress NetworkPolicy rules. Documentation explains production
defaults, local development exceptions, and how to manage scanner exceptions.

## Idempotence and Recovery

Security scanner commands are read-only and safe to rerun. Release signing and
attestation steps run only for pushed tags. Kubernetes examples are declarative;
operators can dry-run them before apply. If CI security checks fail, either fix
the finding or add a documented exception with owner, reason, issue/link, and
expiration date.

## Artifacts and Notes

Issue: https://github.com/sarattha/relayna-gateway/issues/42

Pre-change freeze test on 2026-05-23 passed:

    node tests/freeze-v0.0.14-perimeter.test.mjs

## Interfaces and Dependencies

Required CI tools and actions: `cargo audit`, `cargo deny`, `cargo machete`,
`cargo nextest`, Trivy, Gitleaks, Semgrep, Syft, Cosign, and GitHub artifact
attestations. Kubernetes examples retain ports `8080` and `8081` and the image
name `ghcr.io/sarattha/relayna-gateway`.
