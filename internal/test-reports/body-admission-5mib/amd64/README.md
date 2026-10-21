# Native amd64 confirmation for 0.1.47

All **20 improved conditions** passed on a native Linux amd64 GitHub-hosted runner after rebasing onto `origin/main` at `c8c0f6a`. The controlled matrix completed **2,700/2,700 requests**, with **62.0–63.3 MiB worst sampled RSS per individual pod**. The separate burst completed all 320 requests at 64 slots; smaller limits returned valid `503 gateway_overloaded` responses. No condition had an OOM, restart, transport failure, unexpected status, byte-budget denial, failed metrics scrape, or CPU-quota throttling.

The two matrices contain **4,300 scheduled measurements**: 3,795 successes and 505 valid overload responses. Warmup requests are additional and excluded from these counts. Every success verified the exact byte count, SHA-256 and configured internal credential.

## Provenance and controls

[Successful native workflow run](https://github.com/sarattha/relayna-gateway/actions/runs/39523901020). The host assertion, image architecture and Kubernetes node architecture all verified x86_64/amd64; this was not architecture emulation.

- Measured source: [`5ecb0534f6d1969f1b3f66caa2d3a70dceb94050`](https://github.com/sarattha/relayna-gateway/commit/5ecb0534f6d1969f1b3f66caa2d3a70dceb94050), before the release-only version, embedded UI badge, and documentation updates.
- Runtime: Ubuntu 24.04.5 LTS hosted VM, AMD EPYC 9V45, four vCPUs, 15.61 GiB Docker-visible RAM; Kubernetes v1.36.1 in kind v0.33.0.
- Exactly two Ready Gateway pods per condition, each with a 2 CPU limit and requests of 500m CPU / 256 MiB. No autoscaling or ingress; balanced direct pod-IP routing.
- Main matrix: 2 GiB per pod, 12 RPS for 15 seconds, twelve-second mock delay after receiving the full upload, three fresh-pod repetitions per limit (540 measurements per limit).
- Burst: 1 GiB per pod, 64 RPS for five seconds, two-second mock delay, one fresh-pod repetition per limit (320 measurements per limit).
- Limits: 8, 16, 24, 32, 64 body slots per process; 512 MiB serialized-body budget throughout.
- Payload: exactly 5,242,880 bytes of JSON with one string field; HTTP/1.1 keepalive, no TLS, compression or retries. Real PostgreSQL/Redis authentication, policy and usage; a streaming hash/count mock service.
- One successful zero-delay warmup upload per pod, then 500 ms idle. No builds or scans ran on the benchmark host during measurement.
- Debian glibc 2.36-9+deb12u14, image allocator default `MALLOC_MMAP_THRESHOLD_=131072`, no pod override.
- Runtime image ID: `sha256:d899866bae3a84f5d32b330966396b12fdc77a7e21d9d63821822eb0c1f469c6`.
- Binary SHA-256: `320825a1c86ed07cb4e00760dbfc46dea4774e81edda5f651bdd1a7ecb0a3635`.
- Payload SHA-256: `37439e9de7c7e8c68d54d1bbde5bad4aa394b147b39a683fa21d9ce6d79d36f4`.

Node/PostgreSQL/Redis versions, dependency image IDs/digests, host details, workload hashes, and node information are retained alongside the raw data. The mock and driver sources are unchanged from the original experiment. Native results cover the improved candidate only; the earlier ARM64 baseline uses different hardware and does not establish an amd64 before/after ratio.

## Controlled matrix

| Slots/pod | Success | Peak RSS MiB/pod | Cgroup peak MiB/pod | Success p50 ms | Success p95 ms | Success p99 ms | TTFB p95 ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 8 | 540/540 | 63.3 | 48.5 | 12046.9 | 12054.2 | 12073.8 | 12054.1 |
| 16 | 540/540 | 62.2 | 48.5 | 12049.6 | 12055.5 | 12084.5 | 12055.4 |
| 24 | 540/540 | 62.0 | 48.4 | 12047.8 | 12054.2 | 12073.6 | 12054.1 |
| 32 | 540/540 | 62.1 | 48.5 | 12048.7 | 12056.1 | 12086.3 | 12056.0 |
| 64 | 540/540 | 62.4 | 48.6 | 12048.2 | 12053.9 | 12070.1 | 12053.7 |

All controlled settings observed a peak of one retained body and 5 MiB serialized bytes per pod. There were no admission denials. Arrival-window success throughput was 12 RPS; drain-inclusive throughput is retained in the summaries. Successful p95 includes the fixed 12-second service delay and does not isolate pure Gateway overhead.

## Separate burst matrix

| Slots/pod | Success | Overload 503s | Peak RSS MiB/pod | Cgroup peak MiB/pod | Success p50 ms | Success p95 ms | Reject p95 ms | Observed slots/pod |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 8 | 123/320 | 197 | 99.4 | 90.5 | 3246.1 | 3805.5 | 1008.9 | 8 |
| 16 | 174/320 | 146 | 146.1 | 140.4 | 3979.0 | 4490.3 | 1463.7 | 16 |
| 24 | 228/320 | 92 | 186.4 | 186.7 | 4019.1 | 4811.1 | 1624.7 | 24 |
| 32 | 250/320 | 70 | 232.3 | 231.7 | 4403.1 | 5256.3 | 2092.8 | 32 |
| 64 | 320/320 | 0 | 314.5 | 320.9 | 4786.1 | 5519.7 | — | 56 |

Larger limits admitted more requests and retained more memory in this finite burst. At 64 slots, sampled body occupancy reached 56 and serialized bytes reached 253.4 MiB per pod. Completing this five-second arrival cohort does not establish sustainable 64 RPS capacity. Burst timing has one repetition per setting and a different mock delay/memory limit from the controlled matrix; compare those conditions separately.

## Measurement and validation

Memory columns are the worst individual pod, not the sum of both replicas. Controlled conditions pool six pod instances (three repetitions); each burst condition has two. RSS was sampled from `/proc/1/status` at 250 ms plus sampler overhead after readiness, including warmup. Cgroup-v2 `memory.peak` also captures startup, file-cache charges and sampler processes. RSS/shared-page accounting differs from cgroup charging. Sampled RSS may miss short spikes. Fresh pods remove preceding conditions' allocator history.

Success and rejection latency distributions are separate. Dispatch-to-complete-response timing includes upload, Gateway work, service delay and network/scheduling time. Dispatch p99 stayed below 0.9 ms in the controlled matrix and below 4.7 ms in the burst. All observed 503s agreed with admission counter deltas, and retained slots/bytes stayed within configured bounds.

The strict summarizer was rerun independently after downloading the artifact. Both summaries matched every value after canonicalizing per-pod list order (filesystem iteration can reorder that list). Exact payload/workload hashes, balanced requests, completeness, response verification, pod counts, gauge bounds, scrape success, and counter/status agreement passed. The successful workflow also gated controlled 100% success and zero unexpected results/restarts/OOMs in both matrices.

The rebased source passed the full mandatory local verification stack with live PostgreSQL/Redis (437/437 Nextest tests, zero skips), the workspace build, and GitHub CI. Release metadata/UI/documentation changes are verified separately before publication.

## Reproduce and inspect

- [Controlled summary](controlled-summary.json) and [raw observations](controlled/).
- [Burst summary](burst-summary.json) and [raw observations](burst/).
- [Host metadata](host.txt), [image metadata](image.json), [dependency images](dependency-images.json), [binary hash](binary-sha256.txt), and [workload hashes](workload-sha256.txt).
- [Benchmark instructions](../../../../benches/body-admission/README.md). Run `gh workflow run body-admission-benchmark.yml --ref v0.1.47` for another native amd64 confirmation.
- [Original ARM64 before/after experiment](../README.md).

The workflow deletes its isolated kind cluster after collecting the artifacts. Defaults remain eight slots and 512 MiB; production sizing needs representative service workloads, payload shapes, ingress/TLS, steady load and replica balancing.
