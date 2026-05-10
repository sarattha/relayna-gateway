# Release 0.0.2 Readiness

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Prepare Relayna Gateway for the `v0.0.2` release by aligning crate versions,
container packaging, admin portal testing, CI, release notes, public
documentation, and README content. After this change, operators can build one
Docker image that runs both the Pingora gateway proxy and the embedded admin UI,
read setup/deployment documentation through Material for MkDocs, and rely on CI
to verify Rust, admin portal, and documentation changes.

## Progress

- [x] (2026-05-10) Read `internal/design-manifesto.md`, repository workflows,
  and mandatory skills for release, compatibility, and verification.
- [x] (2026-05-10) Set workspace crate versions to `0.0.2` and update
  `Cargo.lock`.
- [x] (2026-05-10) Add `CHANGELOG.md`, root `Dockerfile`, `.dockerignore`,
  admin UI static tests, and documentation files under `docs/`.
- [x] (2026-05-10) Update GitHub CI, release, and docs workflows to include
  admin portal tests, MkDocs builds, GitHub Pages deployment, and changelog
  release notes.
- [x] (2026-05-10) Refresh `README.md` to describe the implemented gateway and
  remove the MVP target section.
- [x] (2026-05-10) Run admin UI tests, MkDocs strict build, mandatory Rust
  verification, release build, and Docker image build.

## Surprises & Discoveries

- Observation: The repository had `mkdocs.yml` but no `docs/` tree.
  Evidence: `find docs -maxdepth 3 -type f` returned no files before edits.
- Observation: No local `v*` release tags existed.
  Evidence: `git tag -l 'v*' --sort=-v:refname | head -n5` returned no tags.
- Observation: Docker release build required `cmake` for `libz-ng-sys`.
  Evidence: First `docker build -t relayna-gateway:0.0.2 .` failed with
  `is cmake not installed?`; adding `cmake` to the builder image fixed it.

## Decision Log

- Decision: Treat this as additive release packaging/docs/CI work rather than a
  runtime behavior change.
  Rationale: Public routes, response bodies, PostgreSQL schemas, Redis key
  formats, auth decisions, policy decisions, usage event fields, and streaming
  behavior were not changed.
  Date/Author: 2026-05-10 / Codex.
- Decision: Test the admin portal as static embedded assets with Node's built-in
  assertions.
  Rationale: The admin portal is currently plain HTML/CSS/JS embedded in
  `gateway-api`, so a dependency-free test keeps CI lightweight and release
  relevant.
  Date/Author: 2026-05-10 / Codex.
- Decision: Do not create the `v0.0.2` Git tag before the release changes are
  committed.
  Rationale: A Git tag points at a commit; tagging before commit would not mark
  this release content.
  Date/Author: 2026-05-10 / Codex.

## Outcomes & Retrospective

Release readiness now includes versioned crate metadata, changelog-backed
GitHub releases, one image for the gateway and admin portal, Material for
MkDocs documentation, admin portal CI checks, docs deployment, and refreshed
README guidance. The remaining release step is to commit these changes and then
create the annotated `v0.0.2` tag on that commit.

## Context and Orientation

Relayna Gateway is a Rust workspace. `gateway-api` starts the process, serves
Axum control-plane routes and the embedded admin UI, and starts the Pingora
proxy service. `gateway-core` contains framework-independent authentication,
policy, routing, usage, service, and operator-token types. `gateway-proxy`
contains Pingora provider proxy behavior. `gateway-store` owns PostgreSQL and
Redis access. `gateway-telemetry` owns logs and metrics.

The admin portal lives in
`crates/gateway-api/src/static/admin-ui/{index.html,app.css,app.js}` and is
served from `/admin-ui` by `gateway-api`.

## Compatibility Boundary

Compatibility boundary: no existing local release tag was present. This work
does not change released HTTP routes, response shapes, virtual key behavior,
PostgreSQL schemas, Redis formats, provider routing, streaming semantics,
usage events, or Relayna runtime contracts. Deployment metadata and docs are
additive, and the Kubernetes image tag advances to `0.0.2`.

## Plan of Work

Update workspace version metadata and lockfile. Add release notes in
`CHANGELOG.md`. Add a multi-stage root `Dockerfile` that builds `gateway-api`
and runs it as `relayna-gateway`, exposing proxy port `8080` and control/admin
port `8081`. Add admin UI test coverage under `tests/admin-ui.test.mjs`.
Replace the placeholder MkDocs nav with gateway docs, and update CI workflows
for Rust, admin UI, docs, release notes, and GitHub Pages deployment. Refresh
the README around the implemented gateway instead of the MVP roadmap.

## Concrete Steps

Commands run from `/Users/jobz/Works/relayna-gateway`:

    node tests/admin-ui.test.mjs
    ruby -ryaml -e 'Dir[".github/workflows/*.yml"].each { |path| YAML.load_file(path) }; YAML.load_file("mkdocs.yml")'
    cargo metadata --no-deps --format-version 1
    /tmp/relayna-gateway-docs-venv/bin/mkdocs build --strict
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    docker build -t relayna-gateway:0.0.2 .

## Validation and Acceptance

Acceptance requires the Rust formatting, Clippy, and workspace tests to pass;
admin portal tests to pass; MkDocs to build with `--strict`; and the Docker
image to build as `relayna-gateway:0.0.2`. All listed commands passed after the
Dockerfile builder dependency fix.

## Idempotence and Recovery

All validation commands are safe to rerun. `mkdocs build --strict` regenerates
`site/`, which is ignored. Docker builds can be retried with the same tag or
with `--no-cache` if a cached layer becomes stale. If release tagging is
interrupted, delete only the local incorrect tag with `git tag -d v0.0.2` before
recreating it on the committed release commit.

## Artifacts and Notes

The successful Docker build produced local image
`docker.io/library/relayna-gateway:0.0.2`.

## Interfaces and Dependencies

Required runtime environment variables remain `DATABASE_URL`, `REDIS_URL`,
`LITELLM_BASE_URL`, `LITELLM_SERVICE_KEY`, `GATEWAY_BIND_ADDR`,
`GATEWAY_CONTROL_BIND_ADDR`, and `LOG_LEVEL`. Optional variables remain
`DIRECT_OPENAI_BASE_URL`, `DIRECT_OPENAI_SERVICE_KEY`, and
`RELAYNA_WORKER_TOKEN`.
