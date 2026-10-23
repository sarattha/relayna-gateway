# Reusable body-admission benchmark CLI

## Purpose / Big Picture

Operators can rerun the six-size request-body experiment from source on native
amd64 or arm64, including Apple Silicon through Docker Desktop. A single CLI
owns setup, measurement, cleanup, and a self-contained HTML report. Completed,
failed, and gracefully interrupted runs leave interpretable reports and raw
observations. Existing historical measurements remain immutable.

## Progress

- [x] Read the design manifesto, repository skills, existing runner, adaptive
  search, report prototypes, and current branch/main state. Main remains at
  v0.1.49; all earlier benchmark work is on this branch and will be included.
- [x] Define the command surface and compatibility boundary.
- [x] Implement orchestration, architecture checks, checkpoint/failure handling,
  generic HTML/JSON/CSV reporting, and workflow integration.
- [ ] Verify real native arm64 smoke workloads and failure reports; run focused
  regression tests and the full required repository verification stack.
- [ ] Open and register the PR, monitor checks/reviews, address findings, merge.

## Surprises & Discoveries

A real native ARM smoke attempt showed that the old v0.1.49 digest is an
amd64-only index, not a multi-platform release. Retain that expected failure
report and default the new workflow to a native source build; do not change
release packaging for this benchmark task.

The existing measurement runner already works through Docker and Kubernetes;
the amd64 restriction lives mainly in the manual workflow and report prose.
The adaptive controller currently writes checkpoints only after analysis, so
setup errors and failed analysis can leave no summary for a trial. The final
report prototype assumes all six sizes completed and cannot represent a failed
or interrupted run. Promote the useful behavior into maintained source rather
than importing code from historical test reports.

## Decision Log

- Compatibility boundary: released v0.1.49 runtime/configuration/data contracts
  remain intact. Changes are benchmark tooling and ephemeral fixture setup.
  Rewrite unreleased branch-local workflow orchestration directly; preserve
  historical raw data and source snapshots.
- Use Python 3 standard-library argparse and the existing runner/mock, with
  `doctor`, `run`, and offline `report` commands. An executable repository
  wrapper resolves its source location, so callers can invoke it from any
  directory. Do not add a new Rust crate or third-party Python dependencies.
- Native means client host, Docker engine, runtime image and kind node have
  matching normalized amd64/arm64 architectures. Apple Silicon uses a Linux
  arm64 Docker VM; record both host and engine provenance. Reject silent
  emulation. Explicitly pull/build for the detected platform.
- Each run owns a randomly named kind cluster and private temporary kubeconfig;
  never change the user's Kubernetes context or put kubeconfig credentials in
  report artifacts. Cleanup applies only to the owned cluster.
- Keep the requested protocol: two fresh Gateway pods per trial, each capped at
  2 CPUs / 1 GiB and 512 MiB admitted body bytes, 10-second upstream read pause,
  two-second response delay, and 60-second client watchdog. Provide the full
  adaptive search and an explicit fixed-slot sweep for quick repeatable runs.
- Always create a run checkpoint before setup. Save trial intent before child
  execution and retain invalid/incomplete analysis explicitly. A `finally`
  path generates HTML/JSON/CSV and cleans up after normal errors and SIGINT/
  SIGTERM. Abrupt SIGKILL/host loss can be recovered with offline `report`.
- Report observed edges only after at least three stable same-cohort trials
  and an adjacent observed OOM. Finite search ceilings are lower bounds; fixed
  one-trial smoke tests and incomplete runs never become confirmed limits.
  Record expected admission 503s separately from transport errors.

## Outcomes & Retrospective

Implemented the repo-local CLI, generic interactive report, fixed/adaptive modes,
checkpoint recovery and native Actions workflow. Twenty-one behavioral tests pass
on Python 3.10 and 3.15; the historical search regression/replay script and
actionlint pass. The first production-Dockerfile native arm64 build and 5/100 MiB
one-slot smoke completed with verified successes and expected admission 503s,
no resource failures, and automatic cluster cleanup. Final source-image rerun,
full repository verification and PR/merge remain pending.


## Context and Orientation

`benches/body-admission/run.py` owns real PostgreSQL/Redis fixtures, a streaming
mock, the Node request driver, fresh Gateway pods, and cgroup/RSS sampling.
`capacity_sweep.py` owns adaptive slot exploration and resource classification.
Historical report scripts under `internal/test-reports/` are frozen evidence;
new maintained report code belongs in `benches/body-admission/`.

## Plan of Work

Add a repo-local executable wrapper and Python command parser. Provision and
record a native isolated kind environment, then invoke the existing controller
for requested JSON sizes. Support custom/prebuilt images and native builds of
the source tree. Refine checkpoint and exception handling in the controller so
the report can retain failed trials. Add a generic, offline report generator
with clear status, limits/occupancy/outcomes, memory and latency charts, resource
provenance, missing-data labels and links to JSON/CSV evidence. Update the
manual workflow to call the CLI on either native architecture, always upload
the report, and run the CLI tests in CI. Document commands and interpretation.

## Validation and Acceptance

Focused tests must prove architecture normalization/mismatch rejection,
reporting before setup succeeds, partial/corrupt data preservation, signal
handling/cleanup, exact fixed sweeps, adaptive confirmation behavior, resource
vs HTTP delivery classification, same-cohort grouping, incomplete/lower-bound
labels, and HTML escaping. Run real native arm64 fixed-slot smoke tests at
5 and 100 MiB and an actual failure case. Preview the resulting HTML on mobile
and desktop and exercise all controls. Run the mandatory repository stack with
isolated PostgreSQL/Redis fixtures before pushing. Open a PR with concrete
commands/results, register it with T3, and use the app-owned PR watcher.

## Idempotence and Recovery

Refuse to overwrite an existing run directory. Reports can be regenerated from
checkpoints without Docker or Kubernetes and without editing raw observations.
Run namespaces are confined to the owned kind cluster. On a process/host crash,
use the recorded cluster name for cleanup and `report` for a partial view.
Historical raw evidence and old source snapshots must remain byte-identical.

## Artifacts and Notes

Current branch: `test/body-admission-size-capacity-amd64`, based on main
`48c93c07d42b113b8073a403604b111871bf0fda`. User explicitly authorized opening,
babysitting and merging the PR. Prior permission to use the repository admin
merge override after checks pass persists if code-owner approval is the only
remaining branch-protection requirement.

## Interfaces and Dependencies

Python 3, Docker, kind, kubectl, the native Gateway image/source Dockerfile,
Node 22.23.4 fixture image, PostgreSQL 16 and Redis 7. The CLI's JSON envelope
and local run/report schema are versioned tooling surfaces. Gateway public
APIs, production environment variables, schemas and runtime code are untouched.
