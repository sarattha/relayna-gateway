# Patch Node.js tooling for CVE-2026-111497

This ExecPlan is a living document maintained according to [PLANS.md](../../PLANS.md).

## Purpose / Big Picture

Keep Relayna Gateway's build, test, local helper, and benchmark tooling off
Node.js versions affected by CVE-2026-111497. The production gateway is a Rust
binary that embeds its Admin UI files; the affected Node.js decoder belongs to
the development toolchain and Node-based helper images. A successful change
rejects affected Node versions during dependency installation, selects patched
runtimes in automation and containers, and preserves normal builds and tests.

## Progress

- [x] (2026-10-22) Read the repository guidance and official Node.js advisory;
  identify package requirements, CI/release jobs, local helper Dockerfiles,
  benchmark image references, and developer setup instructions.
- [x] (2026-10-22) Confirm the existing package accepts local Node.js 26.12.0;
  prepare an isolated Node.js 24.22.1 runtime and disposable PostgreSQL/Redis.
- [x] (2026-10-22) Reconcile the independent pre-patch investigation and update
  npm requirements, CI/release setup ordering, helper/benchmark/test-harness
  images, current setup docs, and the Unreleased changelog.
- [x] (2026-10-22) Verify npm rejects local 26.12.0 with EBADENGINE; test the
  manifest/lockfile version matrix and successfully install/build/test on
  isolated 24.22.1. Both rebuilt helper containers report 22.23.4 and pass health
  checks; a Brotli compression/decompression round trip also succeeds.
- [x] (2026-10-22) Complete one independent candidate review with no concrete
  bypass or regression found.
- [x] (2026-10-22) Complete the full repository verification helper with
  disposable PostgreSQL/Redis and patched Node on PATH; all steps pass.
  The additional workspace build passes. Remove disposable database/cache
  containers and helper smoke-test images.
- [x] (2026-10-22) Opened review-ready PR #140 at c5ec74a and requested
  @codex review. Codex returned a clean review receipt for this exact head with
  no review threads or findings; proceed to the authorized release follow-up.
- [x] (2026-10-22) Prepare and locally verify v0.1.49 after the clean initial
  review: synchronize workspace/lockfile versions, UI labels/generated assets,
  deployment examples, changelog, and release/upgrade documentation. The full
  verification helper and additional workspace build pass again.
- [ ] Request Codex review of the release follow-up and merge only after
  final-head CI and review requirements are satisfied. Track the external
  review and merge outcome on PR #140.

## Surprises & Discoveries

The local Node.js runtime is 26.12.0, which is affected. The historical amd64
benchmark records Node.js 22.23.3. Do not change historical measurements to
represent a new runtime. An unbounded minimum such as >=24.22.1 would still
allow affected 26.x releases and the unsupported 25.x line.

The independent investigation found Cargo tests spawn the development OIDC
helper with Node. CI's Rust and security jobs therefore need explicit patched
Node setup, and release setup must precede Cargo tests. Runnable Compose
harnesses under internal/test-reports also need patched images; their recorded
results and image provenance are historical data and remain unchanged.

The repository's verification helper includes dependency audits, Nextest,
Trivy, Gitleaks, and Semgrep in addition to formatting, Clippy, and Cargo tests.
Run the helper as the documented superset of the mandatory Rust checks.

npm installation reports six existing dependency advisories (three moderate,
three high). The dependency graph is byte-for-byte equivalent after normalizing
the root engines metadata; this patch does not update unrelated npm packages.

## Decision Log

Compatibility boundary: latest release tag v0.1.48. This security maintenance
changes supported development runtimes and helper image versions. Gateway
routes, configuration, schemas, credentials, and wire protocols retain their
released behavior. No compatibility shim or data migration is needed. The
user's request to fix the finding authorizes raising insecure tooling floors.
Date/author: 2026-10-22, Codex.

After the clean initial Codex review, prepare v0.1.49: the latest fetched tag
and main remain v0.1.48. Synchronize workspace/crate lockfile versions, UI
version labels and regenerated assets, the Kubernetes image example, README,
release target/checklist, changelog, current-feature index, and upgrade guidance.
This is release metadata maintenance under the user's explicit authorization;
the tooling compatibility decision and gateway contracts remain unchanged.
Date/author: 2026-10-22, Codex.

The user authorizes the complete PR lifecycle: open for review, babysit and
resolve Codex review, then bump the version and finalize changelog/docs before
merging. Keep release preparation after the initial review is resolved, select
the next available patch version from current main/tags, and verify the final
head. Do not publish a Git tag or create a release unless separately requested.
Date/author: 2026-10-22, Codex.

Use ^24.22.1 || ^26.12.1 for the frontend toolchain, enforce npm engines with
the root .npmrc, and select the latest matching ^24.22.1 in every CI job that
executes repository JavaScript. Keep standalone helper containers on the 22.x
line with an exact 22.23.4-alpine tag. This excludes known vulnerable versions
without adding a runtime version checker or changing application code.
Date/author: 2026-10-22, Codex.

## Outcomes & Retrospective

The tooling patch is implemented and independently reviewed. Focused checks
pass: affected 24.22.0, 25.x, and 26.12.0 fail the package requirement, patched
24.22.1/26.12.1 pass, and npm rejects an affected runtime in practice. On patched
24.22.1, npm ci, the Admin UI build/static regressions, TypeScript checks, 54
React tests, and six standalone OIDC tests pass. Rebuilt generated UI files
match the checked-in assets. Both helper container health checks pass on
22.23.4. Workflow ordering assertions, metadata checks, Compose YAML parsing,
benchmark Python parsing, and the strict MkDocs build pass.

With disposable PostgreSQL/Redis and patched Node on PATH, all 437 Cargo tests
and all 437 Nextest tests pass with zero Nextest skips. The full verification
helper exits successfully, including formatting, Clippy, dependency
audits/policy checks, unused-dependency checks, Trivy, Gitleaks, and Semgrep.
The additional cargo build --workspace --all-features passes. All five affected
Compose configurations validate with required fixture variables supplied.
Disposable database/cache containers and helper smoke-test images are removed.

The authorized v0.1.49 follow-up synchronizes release metadata, current image
examples, changelog/docs, and generated UI version labels. Frontend build/static
regressions, 54 React tests, type checks, metadata checks, and strict MkDocs pass.
The version badge was visually checked at desktop and mobile widths using the
read-only Owner fixture in Chrome after the T3 snapshot tool timed out twice.
The full verification helper and additional workspace build pass for the
release follow-up, with all 437 Cargo tests and 437 Nextest tests passing and
zero Nextest skips. Disposable release-check PostgreSQL/Redis containers and the
read-only UI fixture server are removed. Final-head GitHub checks and Codex
review remain the merge gate; no tag or published release is created here.

Deployed application stacks and the machine-wide Node.js installation remain
outside this repository patch; validation uses isolated patched runtimes and
setup docs explain how to refresh existing helper containers. The full
Kubernetes benchmark and a code-execution exploit PoC were not run: the focused
substitute is rejection of affected runtime installations plus verification of
actual patched binary versions and legitimate build/helper/test behavior.

## Context and Orientation

The root package.json and package-lock.json define the React/Vite build and
test toolchain. .github/workflows/ci.yml and release.yml select Node for those
commands. deploy/local/Dockerfile.oidc and Dockerfile.mock-upstream run local
development servers. benches/body-admission/run.py and its workflow and README
select images for the benchmark mock server and workload driver.

The official advisory at
https://nodejs.org/en/blog/vulnerability/october-2026-security-releases lists
fixed Node.js releases 22.23.4, 24.22.1, and 26.12.1. Review all active copies of
the version requirements, including lockfile root metadata and setup docs.

## Plan of Work

First reconcile a read-only security-boundary investigation. Select patched
supported versions with a requirement that excludes affected higher major
versions, enforce the requirement during npm installation, and select patched
runtimes in CI and helper images. Keep benchmark pull/load/provenance references
consistent. Update current setup documentation and the Unreleased changelog.

Then use focused checks to reject affected versions and accept patched versions,
build the Admin UI, run its existing tests and type checks, and smoke-test both
helper images. Obtain one independent read-only candidate review, address only
confirmed issues, and finish the full repository verification helper.

## Concrete Steps

Run commands from the repository root. Use an isolated Node.js 24.22.1 executable
at the front of PATH for npm verification. Keep dependency package versions
unchanged and update only root lockfile engine metadata.

    npm ci
    npm run build:admin-ui
    npm test
    npm run typecheck:admin-ui
    npm run test:react:coverage
    make metadata-check
    uv run --with mkdocs-material mkdocs build --strict
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features

Supply DATABASE_URL and REDIS_URL for disposable test containers when running
the verification helper. Build the local OIDC and mock-upstream images with
docker build --pull, check their Node versions, and request their health routes
without touching any existing development stack.

## Validation and Acceptance

Affected Node.js 24.22.0 and 26.12.0 and unsupported 25.x fail the package
requirement; patched 24.22.1 and 26.12.1 pass. npm installation fails with
EBADENGINE on the affected local runtime and succeeds on isolated 24.22.1.
Every active Node helper image reference uses a patched 22.x release. Both local
helper images start and return healthy responses. Existing UI tests, generated
asset consistency, metadata/docs validation, and the repository verification
helper pass. The final diff contains no runtime changes or modified historical
benchmark results.

## Idempotence and Recovery

Package metadata and image-reference edits are repeatable and require no
migrations. Use uniquely named disposable validation containers and remove only
those containers when finished. Leave existing local services and data alone.
Rebuild helper images and recreate their containers to apply patched runtimes;
changing a Dockerfile cannot update an already-running container.

## Artifacts and Notes

Pull request: https://github.com/sarattha/relayna-gateway/pull/140.
Initial review request:
https://github.com/sarattha/relayna-gateway/pull/140#issuecomment-6342521198.
Clean initial review receipt:
https://github.com/sarattha/relayna-gateway/pull/140#issuecomment-6342524945.
The release follow-up requires fresh final-head checks and Codex review before
merge. The user has not requested a Git tag or published release.

Record focused results, the independent review outcome, and full verification
status in the living sections before the final handoff. No exploit or production
deployment is required for this tooling version fix.

## Interfaces and Dependencies

No new runtime dependencies or exported interfaces are needed. Preserve the
checked-in Admin UI asset contract and use the existing npm, Docker, GitHub
Actions, and Python tooling conventions.
