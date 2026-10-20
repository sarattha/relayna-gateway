# Benchmark and Improve Buffered Body Admission

This ExecPlan is a living document maintained according to `PLANS.md`. Keep
Progress, Surprises & Discoveries, Decision Log, and Outcomes & Retrospective
current throughout the work.

## Purpose / Big Picture

Measure how two Gateway Kubernetes pods behave with exactly 5 MiB JSON request
bodies at buffered-body concurrency limits of 8, 16, 24, 32, and 64. Record
memory, successful and rejected request timing, throughput, and admission
rejections before changing runtime code. Use this evidence to make a focused
buffer-lifetime improvement, repeat the same experiment, and open a draft PR
containing reproducible tooling and measured results.

## Progress

- [x] (2026-10-20) Read contributor guidance, design manifesto, planning rules,
  and the karpathy, implementation-strategy, code-change-verification,
  pr-draft-summary, and yeet skills.
- [x] (2026-10-20) Confirmed Docker, Kubernetes, Rust, and authenticated GitHub
  CLI availability; fast-forwarded the clean task branch to released v0.1.46.
- [x] (2026-10-20) Created the isolated two-pod harness, verified exact body
  size/hash and credentials, and completed pilots at limits 8 and 64.
- [x] (2026-10-20 16:42Z) Captured all fifteen unchanged-release baseline
  conditions: success rates 17.8%, 35.6%, 45.6%, 54.4%, and 90.0%; worst
  sampled per-pod RSS 304.7, 418.7, 456.8, 646.7, and 988.7 MiB respectively.
- [x] (2026-10-20 16:46Z) Implemented buffer-owned admission. The dependency-
  backed regression fails on baseline and passes after the change; ownership,
  byte-growth, clone/slice, and cancellation tests pass.
- [x] (2026-10-20 17:11Z) Passed the complete mandatory verification script,
  including 437/437 Nextest tests with live PostgreSQL/Redis, and the additional
  workspace build. Added regression coverage for no-charge expansion failures.
- [x] (2026-10-20 17:21Z) Completed the identical fifteen-run candidate matrix:
  all 2700 requests succeeded, worst per-pod RSS was 65.3–66.1 MiB across the
  five limits, and successful p95 stayed approximately 12.1 seconds.
- [x] (2026-10-20 17:29Z) Finished the ten-condition 1 GiB burst sweep at all
  five limits. Every baseline setting had an observed OOM. The final image
  had no OOMs or transport failures, with 33/58/68/89/151 successes out of 320
  as limits increased. Remaining 503s are explicitly reported.
- [x] (2026-10-20) Published validated summaries, raw observations, source/image
  provenance, Markdown/SVG/interactive reports, and reproducible baseline
  instructions. Desktop/mobile visual and interaction checks passed.
- [x] (2026-10-20) Committed and pushed the implementation and evidence; opened
  draft PR [#137](https://github.com/sarattha/relayna-gateway/pull/137) against
  `main` and registered it with this T3 thread. Removed the isolated benchmark
  namespace and task-created verification Redis instance.

## Surprises & Discoveries

- The task branch initially pointed at v0.1.45 while v0.1.46 was already
  released. `origin/main` was a clean fast-forward, so the baseline now uses
  v0.1.46 (a089b57) rather than accidentally reverting the WebSocket release.
- The existing local `desktop` kind cluster has no application workloads.
  Benchmark resources will be restricted to a dedicated namespace, and all
  Kubernetes commands will use an explicit kubeconfig rather than the user's
  current context.
- The 64 RPS / two-second-delay pilot OOM-killed both 1 GiB pods, even at eight
  slots. The 12 RPS / twelve-second-delay pilot exercised all 64 slots without
  CPU queue overload, but a 64-slot pod reached approximately 951 MiB RSS.
  Preserve the overload pilot separately; use 2 GiB pod limits for the main
  matrix so memory can be observed without censoring the high-limit results.
- Request admission currently lasts until upstream response headers, even
  after the body is handed to Pingora. The benchmark must determine whether
  this ties small concurrency limits to upstream processing latency.
- The first full verification run passed proxy/API coverage but a store test
  rejected `REDIS_URL=.../1` because it appends its own `/15` suffix. Use a
  dedicated verification Redis instance with a base URL instead. No production
  code change is needed for this test-environment assumption.
- A new expansion regression caught fixed-service charging when rewritten
  output exceeded the byte budget after budget reservation. Reserve expanded
  body bytes before budget admission, then transfer the lease to the buffer.
- The owner-only candidate accepted 180/180 requests at eight slots but peaked
  at 1122.4 MiB RSS. An otherwise identical allocator diagnostic with
  `MALLOC_MMAP_THRESHOLD_=131072` accepted 180/180 at 65.7 MiB RSS. This
  establishes allocator retention as a material separate issue; releasing
  admission alone is insufficient for the shipped 1 GiB deployment example.

## Decision Log

- Decision: benchmark real release-built Gateway binaries in two Kubernetes
  pods with shared PostgreSQL/Redis and a streaming mock upstream.
  Rationale: in-process simulations do not establish per-pod memory or the
  actual Pingora buffer lifetime. Date/Author: 2026-10-20 / Codex.
- Decision: retain a 512 MiB serialized-body budget and identical gateway pod
  resources, workload, and upstream delay across baseline and improved runs.
  Rationale: changing these together would confound the comparison.
  Date/Author: 2026-10-20 / Codex.
- Decision: after pilot validation, use 12 RPS, twelve-second delayed mock
  responses, and a 2 GiB memory limit per pod for both full matrices. Warmup
  uses the same verified body on a zero-delay endpoint.
  Rationale: isolate up to 72 concurrent service waits per pod from the
  authentication/CPU queue and measure high-limit memory without 1 GiB OOM
  censoring. Preserve the initial overload observations separately.
  Date/Author: 2026-10-20 / Codex.
- Decision: transfer each completed request's admission lease to a
  `Bytes::from_owner` owner containing its forwarded bytes. Release only when
  the final reference disappears, including copies retained for queued writes.
  Rationale: all fifteen baseline runs saturated the configured slot count
  while successful latency stayed approximately 12.1 seconds. Admission should
  account for retained body memory rather than upstream processing time;
  releasing at callback return alone would undercount queued/backpressured
  buffers. Extend byte reservations if a guardrail expands the forwarded body.
  Date/Author: 2026-10-20 / Codex.
- Decision: use fixed offered load, distribute requests evenly between the
  two pod IPs, disable retries, separate success/rejection timing, and repeat
  each condition three times with fresh pods.
  Rationale: fast 503 responses must not make an overloaded condition look
  faster, and deterministic distribution removes connection-affinity bias.
  Date/Author: 2026-10-20 / Codex.
- Compatibility boundary: latest release v0.1.46. Preserve authentication,
  complete-body policy and guardrail decisions, upstream bytes, routes, error
  envelopes, configuration defaults, persisted state, and byte/concurrency
  bounds. Any lifetime optimization must keep admission attached to retained
  memory until its last owner releases it. The user explicitly authorized
  improving Gateway after collecting the baseline; no migration is planned.
  Date/Author: 2026-10-20 / Codex.
- Decision: pin glibc's mmap threshold to 128 KiB in the official and benchmark
  container images, and document the matching startup setting for native GNU
  libc deployments. Do not add an allocator dependency or unsafe runtime FFI.
  Rationale: the controlled diagnostic reduced worst per-pod RSS from 1122.4
  to 65.7 MiB without changing requests or success counts. GNU libc documents
  that an explicit threshold disables its dynamic growth and lets large freed
  mappings return to the OS. The environment default remains overridable and
  does not change public HTTP, schema, policy, or Gateway configuration names.
  Date/Author: 2026-10-20 / Codex.
- Decision: add a separately labeled, one-repetition burst sweep at 64 RPS,
  five seconds of arrivals, two-second upstream delay, and 1 GiB per pod for
  both images and all five slot limits.
  Rationale: the repeated eight-slot burst stopped OOM-killing the improved
  pods but still returned many valid overload 503s. The higher offered rate
  needs its own measured capacity tradeoff; it must not be confused with the
  three-repetition controlled body-lifetime matrix.
  Date/Author: 2026-10-20 / Codex.

## Outcomes & Retrospective

The controlled experiment is complete: thirty measured conditions, three
repetitions per setting/version, and 5400 total scheduled requests. At eight
slots, successes rose from 96/540 to 540/540 and worst individual-pod RSS fell
from 304.7 to 66.1 MiB. All improved settings succeeded without restarts or
transport failures. Service-delayed successful p95 remained about 12.1 seconds.

The additional ten-condition burst experiment demonstrates the boundary of the
improvement. Original 1 GiB pods OOMed at every limit; the improved pods stayed
alive and delivered valid overload responses. At 64 slots they still rejected
169/320 requests at 64 RPS, so this patch is not a maximum-throughput solution.
Defaults stay at eight slots and 512 MiB. Extra capacity or further scheduling
work remains a separate operating/engineering decision for higher arrival rates.

The implementation preserves complete-body governance and leased-buffer
backpressure, accounts for rewritten output before budgeting, and ships a
measured glibc allocation default without unsafe FFI or new Rust dependencies.
Full verification passed. Delivered as draft PR
[#137](https://github.com/sarattha/relayna-gateway/pull/137) on
`t3/investigate-gateway-overload`. The working branch includes the complete
reproducible evidence; temporary benchmark resources have been removed.

## Context and Orientation

`crates/gateway-proxy/src/body_admission.rs` bounds retained complete bodies
using one shared process-local controller: eight leases and 512 MiB by default.
An admission lease owns one concurrency slot and its reserved byte count; its
destructor returns both. `gateway_buffered_requests` measures current leases,
not requests per second. `pingora_plane.rs` stores request leases in the
request context until upstream response headers arrive. Registered service JSON
bodies still undergo complete policy analysis before forwarding. PostgreSQL
stores service registrations and hashed virtual keys; Redis supplies rate and
budget state. A virtual key is a client credential authenticated by Gateway;
the mock will receive only the configured internal service credential.

## Plan of Work

Add tooling under `benches/body-admission/` that builds release images, deploys
PostgreSQL, Redis, a mock service and precisely two Gateway pods, seeds a real
service/key through admin APIs, and runs all five conditions. Generate exactly
5,242,880 JSON bytes. The mock must hash and count streamed bytes, delay its
response headers by twelve seconds, and report size/hash for client verification.
Use a fixed offered arrival rate of 12 requests/second for 15 seconds per
condition, three repetitions per limit, with no retries. This is a synthetic
latency-bound workload, not a prediction of production capacity.

Collect client end-to-end and first-byte timings, status/error counts,
generator dispatch delay, Gateway buffer gauges and denial counters, and
per-pod cgroup current/peak memory, process RSS, CPU throttling, and restarts.
Fresh pods for each repetition prevent allocator retention in previous
conditions from biasing the next memory measurement. Keep raw machine-readable
results and a human-readable comparison in `internal/test-reports/`.

After interpreting the baseline, implement the smallest correct runtime
improvement. Review Pingora body ownership before changing release timing.
Regression tests must prove held buffers retain their admission and that
cancellation, rejection, policy, guardrails, and upstream bytes remain correct.
Rerun the exact workload, summarize limitations, and create the authorized draft
PR using the repository template and PR summary skill.

## Concrete Steps

Run all commands from the repository root. The harness will document exact
build, deployment, baseline, comparison, and cleanup commands in its README.
Run focused Rust tests while editing, then:

    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features

The verification script additionally requires cargo-audit, cargo-deny,
cargo-machete, cargo-nextest, Trivy, Gitleaks, and Semgrep. Install missing tools
where practical and record real failures rather than silently skipping steps.

## Validation and Acceptance

Baseline results must exist for 8, 16, 24, 32, and 64 with two Ready Gateway
pods and exactly 5 MiB requests verified by the mock. The improved run must
use the same harness settings and image build profile. Report sampled process
RSS and cgroup memory separately, successful-request timing separately from
rejections, all unexpected statuses/errors, achieved offered load, per-pod
distribution, restarts/OOMs, and metrics proving the exercised limit.

Meaningful ownership regression tests and the full mandatory verification
sequence must pass. The draft PR must include benchmark evidence, method,
compatibility notes, limitations, and a link to the report.

## Idempotence and Recovery

Use a dedicated namespace and explicit kind kubeconfig; never delete or alter
unrelated workloads. Keep each raw run in a uniquely named output directory.
Restart only benchmark Gateway pods between conditions. On interrupted runs,
retain partial evidence, mark it incomplete, and rerun that condition. Cleanup
deletes only the benchmark namespace. No production credentials or services
are used. Preserve the unchanged baseline image for comparison.

## Artifacts and Notes

Baseline source: v0.1.46 / a089b57. Host: Apple Silicon macOS; Docker Desktop
Linux aarch64 VM, 10 CPUs, approximately 19.5 GiB RAM. Existing unrelated VM
workloads may affect timing and will be disclosed in the results.

## Interfaces and Dependencies

Existing public APIs, environment variable names/defaults, PostgreSQL schemas,
Redis keys, usage fields, and Prometheus metric contracts stay compatible.
Benchmark dependencies are Docker, kind, kubectl, Python 3, Node.js in a
benchmark container, PostgreSQL, and Redis. Production dependencies will not be
added without evidence they are necessary.
