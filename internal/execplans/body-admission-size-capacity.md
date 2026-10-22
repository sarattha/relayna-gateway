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
- [x] (2026-10-22 17:02Z) Complete and independently validate 5 MiB discovery
  and confirmation on native amd64.
- [ ] Complete 10/20/40/80/100 MiB discovery and confirmation on independent
  native hosts, preserving the requested per-pod limits and per-size hardware.
- [x] (2026-10-22 17:39Z) Complete and independently validate 40 MiB:
  177 slots passed three trials; 178 had an OOM. At 1024 offered requests per
  pod, clean-setting peaks were RSS 836.35546875 / cgroup 993.296875 MiB and
  worst success p95 40149.566499 ms. Native AMD EPYC 7763; 27 trials total.
- [x] (2026-10-22 17:43Z) Complete and independently validate 80 MiB:
  174 slots passed three trials; 175 had an OOM. At 1024 offered requests per
  pod, clean-setting peaks were RSS 831.625 / cgroup 1010.81640625 MiB and
  worst success p95 59932.801431 ms. All confirmations completed without client
  errors, but latency is close to the technical 60-second watchdog. Native
  AMD EPYC 7763; 28 trials total.
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
- The first selected 100 MiB run stopped after 19 checkpointed trials when a
  264-slot condition had one connection reset and one more server denial than
  received client 503s. Exact counter equality was incorrectly asserted even
  for failed client observations. Preserve the raw failed point; record the
  counter gap and require zero gap only for clean classification. Repeat
  100 MiB after verifying this surgical analysis fix. The request workload,
  resources, mock, driver and per-condition runner are unchanged.
- The completed initial 10 MiB search's 39-slot clean result was bounded by
  the driver's 30-second inactivity timeout, not memory: 128-slot trials had
  no container failures and 866–1013 MiB cgroup peaks but 1–2 idle-timeout
  responses at roughly 40-second success latency. Align inactivity with the
  already configured absolute watchdog (60 seconds for this sweep), retain
  the original observations, and rerun 10 MiB so a hidden 30-second target
  does not contradict the user's resource-boundary criterion. Fixed-rate
  defaults retain the original 30-second timeout. Validate a real 35-second
  silent-response case before dispatch.

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
- Supersede the single-host requirement for the remaining sizes if needed:
  the corrected first size has required more than forty minutes, so six exact
  repeated boundaries risk exceeding the original 180-minute job watchdog.
  Add a selected-size workflow input and a six-hour per-job watchdog; retain
  all four benchmark script hashes and runtime/workload settings unchanged.
  Complete 5 MiB first, then run remaining sizes independently. Each size's
  slot comparisons and confirmations stay on one native host. Report hardware
  per size and avoid attributing cross-host latency differences solely to body
  size. This changes orchestration, not the requested two-pod resource limits.
  Date/Author: 2026-10-22, Codex.
- Observe pod status after diagnostics and another ten seconds in failure-
  retaining mode; keep the immediate snapshot too. Classify any terminated
  Gateway container or available previous-container logs as a resource failure;
  keep OOMKilled distinct from other terminations or inferred restarts.
  This affects evidence capture after measurement, not the offered workload,
  memory sampler or client timings. Repeat all six sizes on one new native host
  with corrected boundary detection. Date/Author: 2026-10-22, Codex.
- Confirmed boundaries use the three-run criterion during a second binary
  refinement if a single-run candidate fails confirmation. Include failures
  first discovered during confirmation. This avoids a potentially long linear
  descent and prevents reporting a single successful candidate as the limit.
  Date/Author: 2026-10-22, Codex.
- Failed response delivery can leave a server denial without a received 503.
  Store `admission_counter_gap`, classify the condition unclean, and continue
  the existing search. Clean trials still require exact counter agreement;
  do not reclassify resets as successful admission rejections or OOMs.
  Date/Author: 2026-10-22, Codex.

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
were uploaded immediately. This exploratory run was cancelled at 16:14Z after
the status-reporting race was identified; retain all its combined partial
artifacts separately. The 5 MiB sweep completed and 10 MiB partial trials were
captured. It is not the final six-size dataset.

Corrected native six-size run:
<https://github.com/sarattha/relayna-gateway/actions/runs/39812510359>,
harness commit `d92af5c77e388c0950e170d5610497a0d58dd2b1`. Dispatched at 16:15Z
after the complete mandatory verification passed again (437/437 Nextest tests;
all security checks passed; log `/tmp/relayna-capacity-observation-verification.log`).
Focused tests passed for the real stale-status artifact, healthy native control,
cohort expansion, integer refinement, failed candidate confirmations, and
resource failures first discovered during confirmation.

The corrected 5 MiB study completed at approximately 17:01Z: 44 trials,
32 clean / 12 with observed container failures. At the final cohort of 1024
requests per pod, 128 slots passed all three trials; 129 slots OOMed on one of
three trials. The 128-slot group's peaks were RSS 888.546875 MiB and cgroup
1005.71484375 MiB; worst per-trial success p95 36580.570319 ms. All three
verified 206 successful responses from 2048 offered requests, with intentional
503s accounting for the remainder. Native host: Intel Xeon Platinum 8573C,
four vCPUs. Independent source/hash/resource/counter/fresh-pod validation
passed. Cancelled the remainder of this sequential run after preserving 5 MiB;
its completed 5 MiB dataset is primary evidence, and any partial 10 MiB data
remains separate.

Selected-size orchestration passed actionlint and the entire required stack
again at 17:12Z (437 Nextest tests passed; all commands passed). Log:
`/tmp/relayna-capacity-selector-verification.log`. The four benchmark scripts
are byte-identical to `d92af5c`; orchestration commit is
`a6653e1d34dac050ef4a044f1bdb4080c896e570`. Five independent native runs were
dispatched at 17:13Z with unchanged pod resources and workload:

- 10 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39820680177>
- 20 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39820686185>
- 40 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39820691161>
- 80 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39820696151>
- 100 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39820701600>

Run map: `/tmp/relayna-capacity-selected-runs.json`. The source-backed report
is being assembled under
`internal/test-reports/body-admission-size-capacity/amd64/README.md`.
Verification fixtures, recorded checks and a pinned-source artifact validator
are in the sibling `verification/` directory. Temporary report generation
helper: `/tmp/relayna-capacity-html.py`; it has passed desktop/mobile layout
preflight using explicitly labeled pilot data. Complete and verify the final
six-size page before `html_render` and the final response.

Counter-gap handling passed the full verification stack at 17:47Z (437/437
Nextest, all commands passed); log `/tmp/relayna-capacity-counter-verification.log`.
Commit `83d34cf2db5ee29b779fe6d67dcf2c5b68aab3d1` changes failed-trial analysis
only; request driver, mock and per-condition runner are unchanged.
Fresh complete retries:

- 100 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39825735001>
- 20 MiB: <https://github.com/sarattha/relayna-gateway/actions/runs/39826384990>

The original selected 20 MiB run hit the same counter issue at the third
169-slot confirmation after 37 checkpointed trials. Corrected analysis replay
proved a new 165-slot measurement was still needed; no complete result was
fabricated from the partial evidence. Preserve its raw data and use the fresh
20 MiB retry for the final comparison.

The 100 MiB retry completed its search with no resource failure because a
single lost rejection response at 64 slots stopped the strict HTTP-clean
expansion predicate. Its 695 MiB peak is not a resource boundary. Separate
resource stability from response delivery: every request must reconcile to a
verified success or a server admission denial, both memory streams and all
metrics must be complete, admission gauges must return to zero, and there must
be no termination/restart. Lost rejection replies are reported as client errors
and are allowed only when exactly reconciled by the server denial counter gap.
Missing admitted responses still invalidate the trial. Keep the stricter
zero-client-error `clean` field separately. Expand/refine/confirm using the
resource criterion selected by the user; do not introduce an HTTP delivery SLO.
Regression checks cover accounted denial loss, missing admitted responses,
confirmation-only resource failures, and a finite lower bound without OOM.

The 60-second socket-idle alignment passed a real 35-second silent-response
regression and the full repository stack at 18:09Z (437 Nextest tests passed;
log `/tmp/relayna-capacity-idle-verification.log`), commit
`45156c33d49dd90eace49190932f671b8a9df3c4`. The final resource-predicate change
is undergoing the required full stack before native retries. No runtime code
or production configuration is changed.

## Interfaces and Dependencies

Native GitHub-hosted Ubuntu 24.04 amd64, Docker, kind v0.33.0/Kubernetes
v1.36.1, Python standard library, patched Node 22.23.4, PostgreSQL 16, Redis 7,
and the immutable published Debian Gateway v0.1.49 image. Benchmark credentials
stay in Kubernetes Secrets or the temporary driver filesystem and are excluded
from artifacts and logs.
