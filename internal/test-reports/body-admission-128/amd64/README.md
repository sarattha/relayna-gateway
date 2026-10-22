# Native amd64: 64 versus 128 body slots on v0.1.47

**128 slots improved burst acceptance at higher memory and latency.** With the
byte budget unchanged at 512 MiB per pod, the five-second 64 RPS burst completed
958/960 requests at 128 slots versus 672/960 at 64 slots. Worst sampled RSS
increased from 397.4 to 582.0 MiB per pod. Under the heavier 128 RPS workload,
128 slots completed more requests but successful-request p95 reached 82.6
seconds, and the byte budget became the main source of overload rejection.
Neither setting establishes sustainable 128 RPS capacity.

There were **26,040 measured requests across 18 conditions and 36 fresh Gateway
pod instances**: 14,042 successful responses, 11,995 overload 503s, and three
`502 upstream_connection` responses. All 14,042 successes verified the exact
upload byte count, SHA-256, and internal service credential. There were no
client transport errors, OOMs, restarts, or failed metrics scrapes. Every pod's
retained-body count and byte gauges returned to zero after response drain.
The 36 warmup requests are additional and excluded from these counts.

**The native workflow finished with a failure conclusion.** Its clean-run gate
correctly rejected the three 502s in the 64-slot stress repetitions. All 18
conditions completed, both summarizers produced results for every matrix,
and raw observations are retained without retries or exclusions. This is a
measured runtime failure under heavy load, not a reporting-tool failure.

## Environment and controls

- [Native workflow run 39745691424](https://github.com/sarattha/relayna-gateway/actions/runs/39745691424).
- Harness commit: [`e322ac368358479c54003e830a78f4ed2dfc5d8a`](https://github.com/sarattha/relayna-gateway/commit/e322ac368358479c54003e830a78f4ed2dfc5d8a).
- Published runtime: v0.1.47, source
  `d2ff47859356dea354b9817dc0adec514b6a0364`.
- Immutable release image:
  `ghcr.io/sarattha/relayna-gateway@sha256:08ef644f5f7564013388ff36ab81e4616c61a605bb56217ce381fcc23b07e67e`.
- Loaded image/config ID:
  `sha256:641dc023b3dddc391b65d4d7d0d9b5b1a7627ded60bfe286ec3809918ed7a2f6`.
- Native Linux amd64 host, image, and Kubernetes node architecture were asserted
  before measurements. No architecture emulation or runtime rebuild was used.
- Ubuntu 24.04.5 LTS, AMD EPYC 7763, four vCPUs, 15.61 GiB Docker-visible RAM,
  Docker 28.0.4, kind v0.33.0, Kubernetes v1.36.1.
- Exactly two Ready Gateway pods per condition. Each has a two-CPU limit,
  1 GiB memory limit, and requests of 500m CPU / 256 MiB. The byte budget is
  536,870,912 serialized bytes per pod throughout. The official image's
  `MALLOC_MMAP_THRESHOLD_=131072` setting is used without a pod override.
- Three fresh-pod repetitions for each limit and workload. One successful
  zero-delay warmup upload per pod, followed by 500 ms idle, precedes each run.
- Exactly 5,242,880 bytes of JSON with one string field. HTTP/1.1 keepalive;
  no TLS, compression, ingress, retries, or autoscaling. Traffic alternates
  directly between the two pod IPs. PostgreSQL/Redis are real; the registered
  service is a mock that incrementally hashes/counts uploads.
- All loads below are **total offered rates across both pods**, not per pod.
  Slots and body budgets are per pod/process.

| Workload | Total offered RPS | Arrival window | Mock delay after upload | Repetitions per limit | Measured requests per limit |
|---|---:|---:|---:|---:|---:|
| Controlled | 12 | 15 s | 12 s | 3 | 540 |
| Burst | 64 | 5 s | 2 s | 3 | 960 |
| Stress | 128 | 30 s | 2 s | 3 | 11,520 |

## Memory and timing

Memory is the worst individual pod across the three repetitions, rather than
the two-pod sum. Latency percentiles pool **successful responses only** and
include client connection-pool queueing, upload, Gateway work, mock delay,
and response receipt. They do not isolate pure Gateway processing time.

| Workload | Slots/pod | 200 / offered | Overload 503 | Other HTTP | RSS MiB/pod | Cgroup peak MiB/pod | Success p50 s | Success p95 s | Success p99 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Controlled | 64 | 540/540 | 0 | 0 | 63.2 | 48.7 | 12.073 | 12.080 | 12.086 |
| Controlled | 128 | 540/540 | 0 | 0 | 62.4 | 48.7 | 12.073 | 12.079 | 12.088 |
| Burst | 64 | 672/960 | 288 | 0 | 397.4 | 398.9 | 8.453 | 8.870 | 9.291 |
| Burst | 128 | 958/960 | 2 | 0 | 582.0 | 580.4 | 8.978 | 9.437 | 9.854 |
| Stress | 64 | 3,929/11,520 | 7,588 | 3 | 439.9 | 483.8 | 43.896 | 63.852 | 64.552 |
| Stress | 128 | 7,403/11,520 | 4,117 | 0 | 653.3 | 768.6 | 51.764 | 82.618 | 83.183 |

The stress arrival window lasted 30 seconds, but total response drain took
92.9–93.6 seconds at 64 slots and 112.5–113.2 seconds at 128 slots. The driver
uses up to 256 HTTP connections per target pod; its queue wait is included in
client latency. Its 30-second socket inactivity timeout is not a total
wall-clock deadline. Long client latency can therefore exceed 30 seconds.

The stress success counts per repetition were 1,308 / 1,314 / 1,307 at 64
slots and 2,493 / 2,479 / 2,431 at 128. Successful-request p95 ranged from
63.5–64.3 seconds at 64 slots and 82.3–83.0 seconds at 128. Higher acceptance
and longer response drain were consistent across all three repetitions.

Rejected-request p95 was 4.22 seconds at 64 slots and 4.34 seconds at 128 in
the burst; under stress it rose to 56.60 and 75.49 seconds respectively.
Queued overload responses therefore also incurred substantial client latency.

## Which limit rejected requests?

| Workload | Slots/pod | Peak retained slots/pod | Peak sampled serialized MiB/pod | Slot-limit 503s | Byte-budget 503s |
|---|---:|---:|---:|---:|---:|
| Controlled | 64 | 1 | 5.0 | 0 | 0 |
| Controlled | 128 | 1 | 5.0 | 0 | 0 |
| Burst | 64 | 64 | 320.0 | 288 | 0 |
| Burst | 128 | 113 | 505.3 | 0 | 2 |
| Stress | 64 | 64 | 320.0 | 7,588 | 0 |
| Stress | 128 | 128 | 511.995 | 127 | 3,990 |

At 128 slots the 512 MiB byte budget becomes the main admission bottleneck.
128 complete 5 MiB bodies would require 640 MiB, while partial uploads can
occupy more than 102 slots without holding 128 complete bodies. Sampled slot
and byte peaks need not occur at the same instant. The count and byte gauges
remained within their configured bounds in every sample. Every overload 503
agreed with the corresponding admission-counter deltas.

The serialized-byte budget does not bound total pod memory. At 128 slots,
worst stress cgroup charging reached 768.6 MiB against the 1 GiB pod limit;
sampled process RSS reached 653.3 MiB. This fixture stayed within its memory
limit, but the test does not validate more complex JSON shapes, guardrail
expansion, TLS/ingress, other concurrent work, or a production latency target.
No CPU-quota throttling was recorded. Host CPU utilization was not measured,
so this does not rule out CPU or shared-host contention.

## Unexpected upstream responses

The 64-slot stress runs returned three HTTP 502 responses carrying
`upstream_connection`: two in repetition 2 and one in repetition 3. Their
client elapsed times were 42.94, 45.23, and 50.14 seconds. These are recorded
HTTP failures, distinct from client transport errors or admission 503s.
The exact upstream connection failure cause was not isolated by this test.
Gateway/mock process logs were not exported, so the observations do not prove
a specific CPU, networking, or upstream timeout root cause.

The native gate expected only 200 and admission 503 outcomes. It intentionally
failed on these 502s. [validation.json](validation.json), the
[stress diagnostic summary](stress-probes.json), and the raw
[repetition 2](stress/limit-64-rep-2.json) and
[repetition 3](stress/limit-64-rep-3.json) retain the failure evidence. The
workflow uploaded all measurements and successfully removed its own cluster.

## Interpretation for configuration

128 slots is a usable **burst-headroom setting for this tested fixture** when
retaining the 512 MiB byte budget and the tested 1 GiB pod memory limit. It
substantially reduces burst rejections, at approximately 582 MiB peak RSS per
pod instead of 397 MiB. It does not make the offered heavy load fast: stress
still rejected 4,117/11,520 requests and successful-request p95 exceeded 82
seconds. Raising the byte budget is not justified by this experiment.

Use these results to choose between admission headroom and memory/latency for
the actual workload. Neither setting met a low-latency objective at the stress
rate. No production configuration, runtime implementation, default value,
version, or release was changed by this follow-up.

This VM used an AMD EPYC 7763; the earlier native five-limit confirmation used
an EPYC 9V45. Do not treat absolute burst results across those separate hosts
as a runtime regression or improvement ratio. The paired 64/128 comparisons
here use the same published image and host. See the
[earlier report](../../body-admission-5mib/amd64/README.md) for its separate
hardware, resource settings, and methods.

## Validation and artifacts

The strict and diagnostic summaries were independently regenerated from raw
files. All numeric values matched after canonicalizing per-pod list order.
An independent audit verified 26,040 complete outcomes, 36 distinct pod
instances, exact configured pod resources, byte/count bounds, payload and
image identities, balanced offered traffic, all successful response checks,
and zero terminal retained-body gauges. Maximum observed metrics scrape gaps
were 254 ms controlled, 297 ms burst, and 344 ms stress. No measurements were
discarded to obtain a clean result.

RSS is sampled at 250 ms plus command overhead and may miss brief spikes.
Kernel `memory.peak` includes startup, warmup, file-cache charges, and sampler
processes. Shared-page RSS accounting differs from cgroup charging.

Before pushing the harness commit, the complete repository verification stack
passed: formatting, workspace Clippy/tests, dependency audit/deny/machete,
437/437 Nextest tests with zero skips, Trivy, full-history Gitleaks, and
Semgrep. Python compilation, Node syntax, actionlint, and synthetic filename
checks for 128-slot and multi-digit repetition files also passed. The same
harness commit passed all [GitHub CI jobs](https://github.com/sarattha/relayna-gateway/actions/runs/39745690705).
The native measurement workflow's failure remains documented separately above.

- [Independent audit and unexpected results](independent-validation.json).
- [Controlled summary](controlled-summary.json), [diagnostics](controlled-probes.json), and [raw files](controlled/).
- [Burst summary](burst-summary.json), [diagnostics](burst-probes.json), and [raw files](burst/).
- [Stress summary](stress-summary.json), [diagnostics](stress-probes.json), and [raw files](stress/).
- [Host](host.txt), [image](image.json), [release reference](release-image-ref.txt),
  [binary hash](binary-sha256.txt), [workload hashes](workload-sha256.txt),
  [dependency images](dependency-images.json), [runtime source](source-commit.txt),
  and [harness source](harness-commit.txt).

Rerun this experiment with:

    gh workflow run body-admission-benchmark.yml --ref test/body-admission-128-amd64

The branch's manual workflow uses the immutable release image above and explicit
64/128 limits. See `benches/body-admission/README.md` and `run.py --help` for
direct local-cluster execution. Production defaults remain eight slots and
512 MiB.
