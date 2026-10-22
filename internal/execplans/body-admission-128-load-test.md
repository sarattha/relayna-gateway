# Native amd64 load test of 128 body slots

This ExecPlan follows `PLANS.md`. Maintain Progress, Surprises & Discoveries,
Decision Log, and Outcomes & Retrospective as the experiment proceeds.

## Purpose / Big Picture

Determine the memory, latency, and overload behavior of increasing
`GATEWAY_MAX_BUFFERED_REQUESTS` from 64 to 128 per pod while retaining the
512 MiB serialized-body byte budget. Test the published v0.1.47 image on a
native Linux amd64 GitHub runner with exactly two Gateway pods, each limited
to two CPUs and 1 GiB memory. Use real PostgreSQL/Redis and the existing
mock registered service and fixed 5,242,880-byte JSON workload.

## Progress

- [x] (2026-10-22 07:55Z) Inspect the released workflow, driver, runner,
  summarizers, and configuration. Create an isolated experiment branch from
  v0.1.47; preserve the original implementation branch.
- [x] (2026-10-22 07:55Z) Identify and fix summarizer filename matching that
  would omit three-digit slot counts and multi-digit repetitions.
- [x] (2026-10-22 08:10Z) Actionlint, Python compilation, Node syntax,
  numeric-width regression checks, and the complete repository verification
  stack passed. Nextest ran 437 tests with zero skips. Removed the two local
  verification fixture containers after completion.
- [x] (2026-10-22 08:31Z) All 18 native conditions completed: 26,040 measured
  requests across 36 fresh pods. Workflow 39745691424 at harness commit
  `e322ac368358479c54003e830a78f4ed2dfc5d8a` uploaded its artifacts and removed
  its cluster. Preserve its failure conclusion: three upstream-connection
  502s in the 64-slot stress repetitions failed the clean-run gate.
- [x] (2026-10-22 08:35Z) Independently regenerate all summaries, validate
  provenance/configuration/counts/bounds and final zero body gauges, preserve
  all raw observations under `internal/test-reports/body-admission-128/amd64/`,
  and write the measured memory/timing/overload report.
- [x] (2026-10-22 08:38Z) Prepare the complete report for publication, scan
  its 6.42 MB of observations with Gitleaks (zero leaks), and verify the
  interactive visual at desktop/light and mobile/dark widths without console
  errors or horizontal overflow. The final thread handoff and remote
  `test/body-admission-128-amd64` branch record publication status.

## Surprises & Discoveries

- Both summarizers used `limit-??-rep-?.json`, which silently omits limit 128.
  Match numeric filenames of any width while excluding pod snapshot files.
- The existing native workflow has no dispatch inputs and tests only five
  limits ending at 64. Use a dedicated branch of the existing manual workflow.
- The runtime reached the configured 128 slots and 511.995 MiB sampled body
  bytes under stress. Raising the count moved most admission failures to the
  independent byte budget: 3,990 byte denials and 127 count denials at 128.
- All conditions remained below the 1 GiB pod memory limit with zero OOMs,
  restarts, client transport failures, or metrics errors. The 64-slot stress
  runs nevertheless returned three `502 upstream_connection` errors. The
  measurement workflow correctly failed its clean-run gate; do not hide those
  failures or change the gate to force a green conclusion.
- Client latencies include the driver's HTTP connection-pool queue. A 30-second
  arrival window drained in approximately 93 seconds at 64 and 113 seconds at
  128. The 30-second socket inactivity timeout is not an end-to-end deadline.
- This runner used an EPYC 7763 rather than the earlier native report's EPYC
  9V45. Use within-run 64/128 comparisons; avoid cross-host performance ratios.

## Decision Log

- Test the published v0.1.47 image by immutable OCI index digest
  `sha256:08ef644f5f7564013388ff36ab81e4616c61a605bb56217ce381fcc23b07e67e`
  rather than rebuilding runtime sources. Assert native host/image/node
  architectures and record source, image, binary, allocator, and workload hashes.
  Date/Author: 2026-10-22, Codex.
- Explicitly pass limits 64 and 128; retain production defaults of eight slots
  and 512 MiB. Use three fresh-pod repetitions of each limit in each matrix:
  controlled 12 RPS for 15 seconds with 12-second mock delay; burst 64 RPS for
  five seconds with two-second delay; stress 128 RPS for 30 seconds with
  two-second delay. Every matrix uses 1 GiB per pod. These are finite offered
  loads, not a maximum sustained capacity measurement.
  Date/Author: 2026-10-22, Codex.
- Use the repository verification skill before pushing benchmark code/workflow
  changes. Run verification locally before native measurements; no local
  heavyweight work overlaps measurements on the GitHub host.
  Date/Author: 2026-10-22, Codex.

## Outcomes & Retrospective

128 slots improved the 64 RPS burst from 672/960 successes to 958/960 while
raising worst RSS from 397.4 to 582.0 MiB per pod. At 128 RPS for 30 seconds,
128 slots completed 7,403/11,520 versus 3,929/11,520 at 64 slots, at higher
memory (653.3 versus 439.9 MiB peak RSS; 768.6 versus 483.8 MiB cgroup peak)
and successful-request p95 latency (82.6 versus 63.9 seconds). The 512 MiB
byte budget remained enforced and became the main overload source at 128.

All 14,042 successful responses verified their upload and credential checks.
11,995 valid overload 503s and all three unexpected 502s are preserved. The
results support 128 as tested burst headroom under the specified resources,
not a sustainable capacity or latency claim for the heavier offered workload.
No production configuration or runtime implementation was changed. The exact
cause of the three upstream connection failures was not isolated; process logs
were not exported, so root-cause claims would exceed the collected evidence.

## Context and Orientation

`benches/body-admission/run.py` creates isolated Kubernetes resources in
`gateway-body-bench`, balances offered requests directly between exactly two
Ready pod IPs, starts fresh Gateway pods for every condition, and records
metrics, cgroup memory, RSS, CPU throttling, and pod status. The driver verifies
successful upload byte counts, SHA-256, and internal service credentials.
The retained-body count and serialized-byte budget are independent process
limits; 128 complete 5 MiB bodies exceed the 512 MiB byte budget. Admission
rejections are valid results and must be separated from transport failures.

## Compatibility Boundary

Latest released behavior under test is v0.1.47 at
`d2ff47859356dea354b9817dc0adec514b6a0364`. No runtime code, public API,
deployment defaults, database schema, Redis format, or credential behavior
changes. Only benchmark orchestration and filename discovery are adjusted.

## Plan of Work

Use `.github/workflows/body-admission-benchmark.yml` on
`test/body-admission-128-amd64` to pull the released image, initialize a
throwaway kind cluster, run all three matrices, validate and upload evidence,
and always delete the cluster. Update the two summarizers to discover all
numeric limit/repetition filenames. Preserve evidence and a readable report
under `internal/test-reports/body-admission-128/amd64/` after download.

## Concrete Steps

1. Verify Python compilation, Node syntax, filename discovery against existing
   observations including renamed numeric-width cases, and workflow YAML.
2. Run `bash .codex/skills/code-change-verification/scripts/run.sh` with
   separate local PostgreSQL/Redis fixture containers and explicit URLs.
3. Commit/push the experiment branch. Dispatch
   `gh workflow run body-admission-benchmark.yml --ref test/body-admission-128-amd64`.
4. Wait for the workflow, download its artifact, independently rerun summaries,
   and inspect every run including failure cases. Preserve evidence and report.

## Validation and Acceptance

Expect six fresh-pod conditions per matrix, each with exactly two pods. Per
limit, scheduled requests across three repetitions are 540 controlled, 960
burst, and 11,520 stress (26,040 measurements in total across both limits).
Verify every scheduled request completed, every successful response matches
the payload and credential checks, traffic is balanced, metrics scrapes are
complete, body gauges stay within limits, and 503 responses match admission
counter deltas. Report all restarts/OOMs, timeouts, transport errors, unexpected
HTTP statuses, generator lag, and CPU throttling. Do not treat censored timing
or missing samples as a successful clean run. Preserve invalid runs and their
diagnostic summaries rather than hiding or overwriting them.

## Idempotence and Recovery

Use a new GitHub run/artifact for each attempt. Keep incomplete observations
separate. The workflow always uploads evidence and deletes only its own kind
cluster. Remove only the task-specific local verification containers. Do not
restart stale port-forwards or overwrite the earlier completed benchmark data.

## Artifacts and Notes

Native workflow: https://github.com/sarattha/relayna-gateway/actions/runs/39745691424.
Harness commit: `e322ac368358479c54003e830a78f4ed2dfc5d8a`.
Local verification transcript: `/tmp/relayna-128-verification.log`.
Report and complete raw evidence:
`internal/test-reports/body-admission-128/amd64/README.md`.
Independent audit: `internal/test-reports/body-admission-128/amd64/independent-validation.json`.

## Interfaces and Dependencies

GitHub-hosted Ubuntu 24.04 amd64, Docker, kind v0.33.0 with Kubernetes v1.36.1,
Python standard library, Node 22, PostgreSQL 16, Redis 7, and the exact released
Debian Gateway image. All benchmark credentials are synthetic and remain in
Kubernetes Secrets or the temporary driver filesystem, outside artifacts.
