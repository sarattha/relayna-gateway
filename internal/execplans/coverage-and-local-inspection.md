# Workspace Coverage and Local Gateway Inspection

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds. Maintain this document in accordance with `PLANS.md`.

## Purpose / Big Picture

After this work, the Rust workspace reports at least 95% line coverage under
`cargo llvm-cov --workspace --all-features`, using real PostgreSQL and Redis
where adapter behavior cannot be exercised faithfully in memory. A freshly
built Docker image runs in an isolated local stack so an operator can inspect
the gateway Admin UI and the OpenAPI endpoint-pricing feature through localhost.

## Progress

- [x] (2026-07-22 04:55Z) Measured the existing workspace baseline at 67.11%
  line coverage over 19,603 lines.
- [x] (2026-07-22 05:00Z) Identified PostgreSQL, Admin API, proxy, and process
  assembly code as the dominant uncovered areas.
- [x] (2026-07-22 05:20Z) Ran dependency-backed integration coverage against
  PostgreSQL 16 and Redis 7 and measured the remaining handler, proxy, process,
  and store gaps.
- [x] (2026-07-22 06:27Z) Added focused unit and integration contracts until a
  clean all-features workspace run reached 95.00% line coverage (21,632 lines,
  1,082 missed); `--fail-under-lines 95` exits successfully.
- [x] (2026-07-22 06:32Z) Ran the mandatory repository verification stack; all
  formatting, clippy, tests, audits, scans, and static checks passed.
- [x] (2026-07-22 06:36Z) Built image
  `relayna-gateway:openapi-coverage-local`, launched the isolated inspection
  stack, and verified Admin UI, health, readiness, and authenticated services.

## Surprises & Discoveries

- Observation: The no-dependency test run leaves 6,447 of 19,603 Rust lines
  uncovered; `gateway-store/src/postgres.rs` accounts for 2,723 missed lines
  because its integration tests intentionally return early without
  `DATABASE_URL` and `REDIS_URL`.
  Evidence: `cargo llvm-cov report --summary-only` and
  `crates/gateway-store/tests/control_state_integration.rs`.
- Observation: A child gateway process started by an instrumented test did not
  reliably merge its coverage profile into the parent run.
  Evidence: the process proxy test passed but did not change proxy line
  coverage; running Pingora in the instrumented test process did.
- Observation: Exercising filtered guardrail-event queries against PostgreSQL
  exposed malformed predicate assembly (`WHERE` fragments separated without a
  stable initial predicate).
  Evidence: the dependency-backed store contract failed with PostgreSQL syntax
  errors until the builder emitted `WHERE true` followed by `AND` predicates.
- Observation: The true all-features total includes more executable Rust than
  the dependency-free baseline because additional unit-test modules are built
  into crate test targets.
  Evidence: final clean summary reports 21,632 lines versus the 19,603-line
  baseline.

## Decision Log

- Decision: Interpret “95% overall” as line coverage for the complete Rust
  workspace with all features, and do not exclude production source files to
  inflate the result.
  Rationale: This is the broadest reproducible coverage metric already
  supported by the repository's toolchain and preserves the user's intent.
  Date/Author: 2026-07-22 / Codex.
- Decision: Use real disposable PostgreSQL and Redis containers for store and
  adapter coverage.
  Rationale: Mocking SQL text or Redis commands would add brittle tests without
  proving schema, transaction, or counter behavior.
  Date/Author: 2026-07-22 / Codex.
- Decision: Repair the guardrail-event filter SQL rather than weakening the
  integration assertion or bypassing filtered queries.
  Rationale: The test discovered a real released-path defect; the repair keeps
  the existing schema and API contract while making every supported filter
  combination executable.
  Date/Author: 2026-07-22 / Codex.
- Decision: Keep the inspection deployment isolated on loopback ports 18280
  (proxy) and 18281 (control/Admin UI), with its own network and dependencies.
  Rationale: The user can inspect the final image without changing existing
  development or coverage containers.
  Date/Author: 2026-07-22 / Codex.

## Outcomes & Retrospective

The objective is complete. Clean workspace line coverage increased from 67.11%
to an enforced 95.00%. New contracts exercise the real PostgreSQL admin/store
surface, Redis control-state lifecycle, the in-process Pingora request plane,
gateway startup, configuration, metrics, public error shapes, policy helpers,
provider selection, guardrail HTTP behavior, and the complete Admin API flow.

The mandatory verification script passed all commands, including 275 nextest
tests, Trivy, gitleaks, and Semgrep. Docker image
`sha256:61ba82075770e53e5c6482ed41494b8c64718d180622528517e43b7e2303d38b`
is running as `relayna-inspect-gateway` with healthy PostgreSQL and Redis.
`/admin-ui` returns 200, `/admin-ui/healthz` returns `ok`,
`/admin-ui/readyz` returns `ready`, and an authenticated services request
returns an empty initial registry as expected.

## Context and Orientation

`crates/gateway-store/src/postgres.rs` owns the durable implementation of most
admin, policy, usage, and registry interfaces. `crates/gateway-api/src/app.rs`
owns the authenticated control-plane routes and has an in-memory test store for
handler behavior. `crates/gateway-proxy/src/pingora_plane.rs` owns request-plane
behavior. Core modules contain plain policy and pricing logic. Coverage is
measured using `cargo-llvm-cov`; dependency-backed integration tests require a
migrated PostgreSQL database and Redis instance.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.20`. This work adds tests and
local test/deployment artifacts only. It must not change released gateway,
schema, Redis, API, or proxy behavior merely to improve the metric.

## Plan of Work

First run the existing workspace suite with isolated dependencies and inspect
file and line-level gaps. Add behavior-focused tests in the owning crate,
prioritizing high-volume adapters and public control/proxy paths. Recalculate
coverage after each milestone. Once the workspace is at or above 95%, run the
mandatory verification stack, build a uniquely tagged Docker image from the
final tree, and launch it with disposable PostgreSQL and Redis containers.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo llvm-cov --workspace --all-features --summary-only
    bash .codex/skills/code-change-verification/scripts/run.sh
    docker build -t relayna-gateway:openapi-coverage-local .

The final local gateway maps proxy port 8080 and control/Admin UI port 8081 to
unused loopback ports recorded in Outcomes & Retrospective.

## Validation and Acceptance

The final llvm-cov summary must report at least 95.00% total line coverage with
all workspace features. Formatting, clippy, workspace tests, dependency audit,
nextest, container scanning, secret scanning, and Semgrep must pass. The fresh
gateway container must reach healthy readiness, serve `/admin-ui`, and expose
the committed OpenAPI endpoint-pricing UI from the new image.

## Idempotence and Recovery

Coverage dependencies and the inspection stack use explicit unique container
names and loopback ports. Commands may be rerun after checking those exact
containers. Interrupted coverage runs can be restarted after `cargo llvm-cov
clean --workspace`; this only removes coverage artifacts. Existing unrelated
containers and user workloads are not modified.

## Artifacts and Notes

Baseline:

    TOTAL 19,603 lines; 6,447 missed; 67.11% covered

Final clean all-features result:

    TOTAL 21,632 lines; 1,082 missed; 95.00% covered

Inspection endpoints:

    Proxy:    http://127.0.0.1:18280
    Admin UI: http://127.0.0.1:18281/admin-ui

## Interfaces and Dependencies

No released interface changes are planned. Test dependencies are PostgreSQL 16,
Redis 7, Docker, and `cargo-llvm-cov`. The final runtime uses the repository
Dockerfile and existing gateway environment-variable contract.
