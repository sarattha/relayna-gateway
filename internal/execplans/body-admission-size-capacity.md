# Native amd64 body-size resource boundaries

This living ExecPlan follows `PLANS.md`. Maintain Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective throughout execution.

## Purpose / Big Picture

Measure the useful retained-body slot range and resource-failure boundary for
5, 10, 20, 40, 80, and 100 MiB JSON under identical Gateway resources: two
native amd64 pods, each limited to two CPUs and 1 GiB memory, with a 512 MiB
serialized-body budget per process. The user explicitly selected the resource
boundary as the criterion; report latency without inventing a latency SLO.

The configured slot maximum is an upper bound, not an allocated array, so no
universal finite configuration maximum follows from these resources alone.
Distinguish arithmetic complete-body capacity, observed partial-body occupancy,
the largest stable tested count, and where increasing the count becomes
byte-limited or causes OOM/restarts/errors under the specified fixture.

## Progress

- [x] (2026-10-22 15:11Z) Inspect the existing benchmark, configuration,
  service size constraints, skills, and design manifesto. The user selected
  resource stability with latency reported separately.
- [x] (2026-10-22 15:17Z) Create `test/body-admission-size-capacity-amd64`
  from released v0.1.49/current main. Preserve the earlier 128-slot experiment
  on its existing branch. Confirm HTTP admission code is unchanged since
  v0.1.47; use the immutable v0.1.49 image.
- [x] (2026-10-22 15:31Z) Extend and locally verify the harness for variable body size,
  simultaneous bounded cohorts, upstream read backpressure, failure evidence,
  adaptive slot discovery, and repeated boundary confirmation.
- [x] (2026-10-22 15:36Z) Finish mandatory full repository verification before
  pushing: formatting, Clippy, workspace tests, audit, deny, machete, 437/437
  Nextest tests, Trivy, full-history Gitleaks, and Semgrep passed. Focused
  driver/analyzer checks, syntax checks, and actionlint also passed.
- [ ] Run native amd64 discovery and confirmation sequentially for each size.
- [ ] Independently validate and preserve every observation, including failures.
- [ ] Publish a source-backed per-size resource/slot/memory/timing report and
  the final visual comparison.

## Surprises & Discoveries

- The original driver and summarizers assume exactly 5 MiB and the fixture
  permits only 8 MiB. The released service API supports up to exactly 100 MiB;
  only benchmark fixture registrations/policies need larger limits.
- Existing fixed-rate tests can exhaust client queues without isolating held
  buffers. A finite simultaneous cohort and explicitly paused upstream reader
  exercise retained-body resource pressure directly and avoid extrapolating a
  universal maximum from an offered-RPS test.
- Mathematical complete-body bounds from 512 MiB are 102, 51, 25, 12, 6, and
  5 per pod. Partial bodies can occupy more slots. Parsing, allocations,
  sockets, and other runtime state remain outside the serialized-byte budget.
- The first full 5 MiB sweep found OOMs in all three 512-slot trials at
  1024 offered requests per pod. Its 256-slot refinement lost connections but
  the immediate pod snapshot still claimed Ready/restartCount=0. Both previous
  container logs were retrieved seconds later, proving a restart reporting race.
  Preserve this evidence, add post-condition observation and previous-container
  restart detection, and repeat the full native comparison rather than claim a
  usable boundary from the prematurely stopped refinement.

## Decision Log

- Runtime boundary: latest release v0.1.49 at
  `48c93c0`; use image index
  `sha256:1b40a38f8c5f211d9c5b650a450c4645ee53149d1af7235599afdc133ea69f75`.
  No Rust runtime, external configuration, public contract, schema, Redis, or
  production policy changes. Existing benchmark defaults remain reproducible.
  Date/Author: 2026-10-22, Codex.
- Use exactly two fresh Gateway pods per trial with 2 CPU / 1 GiB limits and
  512 MiB serialized-body budgets. Keep the single-string JSON shape and exact
  body bytes. Use a 10-second upstream read pause and two-second post-upload
  delay, labeled as a backpressure resource test rather than the earlier
  fast-reading workload. Validate actual occupancy in the native pilot.
  Date/Author: 2026-10-22, Codex.
- Use a fixed simultaneous cohort per body size across initial slot settings:
  twice the largest slot count in the discovery grid per pod. Discovery samples
  below/at/above the byte-cap boundary, then repeats the boundary settings
  until there are three fresh-pod trials. If a memory cliff appears, refine
  the neighboring integer counts. If bytes dominate without a cliff, report
  that no finite configuration ceiling was established rather than fabricating
  one. Preserve non-monotonic failures and avoid treating a single run as a
  universal safe maximum.
  Date/Author: 2026-10-22, Codex.
- The pilot survived all six conditions: 5 MiB at 64/128/256 slots and 100 MiB
  at 1/4/5 slots, with verified bodies and no OOM/restarts. At 256 configured
  slots its 204-request-per-pod cohort could exercise only 204 slots; the byte
  budget plateaued near 512 MiB. Correct that offered-load ceiling before the
  full search: start at twice the largest discovery slot setting; if all
  settings are clean, double the ceiling/cohort up to 4096 configured slots.
  Refine and confirm only within the same final offered cohort. Report a
  tested lower bound if that finite search still finds no resource cliff.
  Native pilot retained buffers for at least eleven seconds at 100 MiB, so the
  existing upstream pause is effective without changing its implementation.
  Date/Author: 2026-10-22, Codex.
- Use a single native GitHub runner for all six sizes to keep CPU hardware
  consistent. Upload each size's results immediately before proceeding to the
  next size; retain final combined artifacts and always clean up the cluster.
  Date/Author: 2026-10-22, Codex.
- Observe pod status after diagnostics and another ten seconds in failure-
  retaining mode; keep the immediate snapshot too. Classify any terminated
  Gateway container or available previous-container logs as a resource failure;
  keep OOMKilled distinct from other terminations or inferred restarts.
  This affects evidence capture after measurement, not the offered workload,
  memory sampler or client timings. Repeat all six sizes on one new native host
  with corrected boundary detection. Date/Author: 2026-10-22, Codex.

## Outcomes & Retrospective

Pending native measurements. Never silently omit OOMs, connection errors,
timeouts, incomplete measurements, or invalid warmups to report a clean limit.

## Context and Orientation

`benches/body-admission/run.py` creates isolated Kubernetes resources in the
`gateway-body-bench` namespace with an explicit kubeconfig. It owns real
PostgreSQL/Redis fixtures, a registered mock service, fresh Gateway deployments,
and cgroup/RSS samplers. `workload.mjs` sends governed service requests and
verifies successful upstream byte counts, hashes, and credentials.
`mock.mjs` streams input into a hash without retaining the whole body.

Extend those benchmark-only surfaces, and add a capacity orchestrator and
failure-tolerant analysis for variable payloads. Record image/source/workload
hashes, native host/node architectures, all resource settings, cohort sizes,
read delays, client deadlines, retained-body gauges, rejection reason counters,
process/cgroup memory, CPU throttling, pod restarts/OOMs, and diagnostics.

## Compatibility Boundary

v0.1.49 is the runtime under test. Production behavior and released interfaces
are unchanged. Only ephemeral, isolated benchmark fixture service/policy limits
are raised to allow the requested JSON sizes. Preserve old benchmark defaults;
new controls are explicit opt-ins. No migration or compatibility shim needed.

## Plan of Work

Add explicit body-size and cohort/backpressure controls to the benchmark
runner/driver. Ensure errors and timeouts still leave pod snapshots, samples,
and safe process diagnostics. Implement discovery/confirmation over a finite
cohort with progress checkpoints. Reuse the registered manual GitHub workflow
on this branch to pull the exact published amd64 image and run six sequential
body-size steps with per-size uploads. Before pushing, run focused driver and
analysis checks plus the required repository verification skill.

## Validation and Acceptance

Every valid trial must have exactly two fresh Ready Gateway pods with matching
configured limits, exact-sized JSON bodies and balanced offered cohorts. Verify
all successful responses' sizes/hashes/credentials. Keep response failures,
transport errors and unsuccessful warmups explicitly classified. Resource
stability requires no OOM/restarts or missing/invalid evidence; report other
runtime failures separately rather than relabeling them as admission 503s.
Verify count/byte bounds and 503 counter agreement when metrics remain
continuous; flag counter resets and censored timing after crashes. Confirm
candidate boundaries with three independent fresh-pod runs and report tested
ranges/lower bounds when the fixture does not establish a finite ceiling.

Run syntax checks, meaningful local mock/driver fixtures at multiple sizes,
failure capture checks, artifact-analysis checks, actionlint, and
`bash .codex/skills/code-change-verification/scripts/run.sh`. No heavyweight
work may overlap measurements on their native host.

## Idempotence and Recovery

Never overwrite completed trials; unique output folders identify stages,
sizes, limits, repetitions, and runs. Recover from a crashed condition with
fresh pods and retain all earlier files. Use finite client/condition deadlines.
Upload partial results after each size, including failed measurements, and
always delete only the workflow's own kind cluster. Keep local fixture ports
and containers task-specific and remove them after verification.

## Artifacts and Notes

Native run IDs, source commits, verification logs, body-size outputs and final
report links will be recorded as they become available.

Local preflight evidence: `/tmp/relayna-capacity-driver-check.log` validates
exact 5/100 MiB streaming uploads through the real Node mock, preserved 5 MiB
SHA-256, policy/service resizing, credentials, upstream read pause, early 503s,
absolute deadlines, and structured unsuccessful warmups. Analyzer checks use
a real earlier native artifact and inject an OOM/restart and unsuccessful
warmup to verify failure classification. Syntax checks and actionlint passed.
Full required verification log: `/tmp/relayna-capacity-verification.log`.

Native pilot: Actions run
<https://github.com/sarattha/relayna-gateway/actions/runs/39806500087>,
harness commit `5ea737f`; dispatched after the complete verification passed.
The pilot is evidence for fixture validation, separate from the subsequent
full six-size comparison.

Expanded-cohort verification passed again at 15:53Z, including all 437 Nextest
tests and every required security check. Log:
`/tmp/relayna-capacity-expanded-verification.log`. The additional driver check
completed 300 requests per distinct host (600 total), proving the configured
client socket ceiling can exceed 256. Adaptive-search checks proved integer
refinement and three-run confirmations stay within the same offered cohort.

Full native six-size run:
<https://github.com/sarattha/relayna-gateway/actions/runs/39809024251>,
harness commit `86b120e4b6eb201cb79c4e48a082d97b12baf9ce`. Per-size artifacts
are uploaded immediately; final raw evidence, boundaries, and outcomes pending.

## Interfaces and Dependencies

Native GitHub-hosted Ubuntu 24.04 amd64, Docker, kind v0.33.0/Kubernetes
v1.36.1, Python standard library, patched Node 22.23.4, PostgreSQL 16, Redis 7,
and the immutable published Debian Gateway v0.1.49 image. Benchmark credentials
stay in Kubernetes Secrets or the temporary driver filesystem and are excluded
from artifacts and logs.
