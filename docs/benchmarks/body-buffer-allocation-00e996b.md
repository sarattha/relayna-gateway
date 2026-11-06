# Body-buffer allocation comparison: 00e996b (native arm64)

This development comparison measures the allocation changes in runtime commit
`00e996b1eb6dbca48085ff09ef9a717c0328be81` against `origin/main` at
`9210810c8333d174ed38dc3bfcb09ba3d5b4d266`. It is a focused 5/10 MiB experiment,
not a version bump, release qualification, adjacent resource-edge search or a
production concurrency guarantee. Later results-only documentation commits do
not change the measured runtime.

## Findings

- At 10 MiB / 32 slots, worst cgroup memory changed from **898.42 to 853.58 MiB**,
  leaving **170.42 MiB** candidate headroom. At 40 slots it changed from
  **980.56 to 952.27 MiB**, leaving only **71.73 MiB** (7.0%).
- The change **did not expand the tested settings that survived**. Both versions
  survive 3/3 trials at 16/32/40 slots for 10 MiB; both OOM in 3/3 trials at
  64 and at 128 slots, with both Gateway pods restarting in each failed trial.
- Cgroup results are workload-dependent: at 5 MiB / 40 slots the candidate's
  worst cgroup peak is higher (790.50 versus 754.37 MiB), despite slightly lower
  sampled RSS. Spare allocation capacity is not equivalent to physical RAM.
- Candidate successful-request totals are lower in every surviving group.
  These finite overload cohorts do not establish a throughput improvement.
- Slots govern **simultaneously retained body buffers**. They do not limit every
  active HTTP request: a released upload buffer can free a slot while its
  request still waits for a response. Conversely, offering 2,048 requests/pod
  does not mean 2,048 successful simultaneous requests were sustained.

At 10 MiB / 40 slots, the candidate completes **1,595 verified 200s**, returns
**10,677 admission 503s**, and records **16 transport errors**, from **12,288
requests offered across both pods and all three trials**. Survival relies on
load shedding; only about 13% complete successfully. Forty slots is therefore
not established as a production default. For requests strictly capped at
10 MiB under these resource limits, 32 is a starting setting to validate with
representative traffic; 16 retains more measured headroom. Other policies,
request shapes and network conditions can change the result.

## Evidence

- [Pinned evidence publication](https://gist.github.com/sarattha/572112a3b17db8590f03a9468205349e/ea30f39910fb44db6c28c697573407d3499d9517)
- [Interactive HTML: download and open locally](https://gist.githubusercontent.com/sarattha/572112a3b17db8590f03a9468205349e/raw/4df39bd253f23d7513d6043ac193b6e59bbfc058/comparison.html)
- [Audited aggregate JSON](https://gist.githubusercontent.com/sarattha/572112a3b17db8590f03a9468205349e/raw/8b409cb2a090a678f43612c7e3579ad1c9c53157/comparison.json) and [CSV](https://gist.githubusercontent.com/sarattha/572112a3b17db8590f03a9468205349e/raw/4296b759cd5c5f346ec3a120e6939d67a735a4b0/comparison.csv)
- The publication retains the full **849-file sanitized raw archive**, losslessly
  encoded in twelve text parts, with `restore_evidence.py` and exact generated
  measurement Dockerfiles. Clone/download all files and follow its README.
  A fresh remote clone, every publication checksum and the restored archive
  SHA-256 were verified after upload.

Archive: `arm64-focused-evidence.tar.gz`, **7,560,993 bytes**.
SHA-256: `13913ab71709db958ea9243e9d306074827c719a927378bc636bfa2719da74c8`.
Raw checkpoints, trial observations, memory streams, pod status, logs, failed
trials, HTML/JSON/CSV reports and provenance remain available outside the source
tree. The isolated PostgreSQL fixture password is redacted; original/published
hashes are documented. Provider, operator and internal credentials, kubeconfigs
and real prompt content are excluded. Payloads are synthetic.

## Runtime, hardware and workload

- Measured **2026-10-25 08:56:55 to 10:18:51 UTC**, native Apple M1 Pro / arm64,
  MacBookPro18,3, 10 cores / 32 GiB host RAM, macOS 27.0.1. The workload runs in
  native Linux arm64 Docker/kind containers; no amd64 or emulation.
- Docker Desktop 29.8.2, Linux 7.0.14-linuxkit, 10 CPUs / 19.50 GiB, kind node
  v1.36.1 / arm64. Host, Docker, both images and node architectures match.
- **60 formal trials / 120 unique fresh Gateway pods**: baseline and candidate;
  5 and 10 MiB; 16/32/40/64/128 slots; three fresh-pod repetitions per group.
  Balanced ABBAAB order per setting, reversed on alternating settings.
- Two Gateway pods per trial, each **2 CPUs / 1 GiB memory / 512 MiB serialized
  body budget**. The owned node has an 8 GiB parent cap; recorded parent peak
  is 5.69 GiB, with zero parent-local OOM events.
- Offer **2,048 simultaneous requests per pod (4,096 per trial)**. Each group
  totals 12,288 offered requests. One-string exact-sized JSON; real
  PostgreSQL/Redis authentication, policy and usage; no guardrails or configured
  input-token/TPM limits; direct equal pod routing; HTTP/1.1 keepalive without
  ingress, TLS, compression or retries.
- Upstream pauses reads for **10 seconds**, then delays **2 seconds after
  upload**. Absolute client deadline 180 seconds; runner 300 seconds; outer
  trial 600 seconds. All received 200s pass body length, SHA-256 and credential
  verification. These runs define no production latency SLO.
- Unrelated Docker workloads remain running. Pre-trial snapshots range from
  0.39–2.77 CPU cores and 7.95–8.75 GiB memory outside the owned node; they are
  snapshots, not continuous measurements. No owned build/test/scanner ran
  during the formal measurements. Timing is not an isolated CPU benchmark.
- Both images use production glibc with `MALLOC_MMAP_THRESHOLD_=131072`, pinned
  builder/runtime base digests, Cargo `--locked` release profile and identical
  runtime base layers. Compiled Cargo caches are isolated by full runtime
  commit; workspace packages are cleaned before compiling. Their verified
  executable hashes differ. `build-provenance.json` retains all build commands,
  image layers, base images and generated Dockerfile hashes.

| Identity | Baseline | Candidate |
| --- | --- | --- |
| Runtime commit | `9210810c8333d174ed38dc3bfcb09ba3d5b4d266` | `00e996b1eb6dbca48085ff09ef9a717c0328be81` |
| Image ID | `sha256:d8880890281b7d724c02c1c730a43d0b0959412ba6225eab91e74ea6aeff058e` | `sha256:ba04097b31786f1360748f0f86b6d8799c806db37d81abb3aa9c453f751ee645` |
| Executable SHA-256 | `597bc7f666eed7e08a3f0219d2af1e5b2e3e016241c5e892595f4febf9406c7e` | `31c4ee8ccdfbfe3598d71e3e2e39d1e2b39bc51ec59263011e82a483e384a08f` |

Harness commit: `00e996b1eb6dbca48085ff09ef9a717c0328be81`, clean when measured.
The repository's `run.py`, workload and mock are unchanged. Their SHA-256
values are in `study.json`; generated controller SHA-256:
`3de335758dc8ad51e7a2bdaf6cdf8b1c98e7a3746c0fc98081a67b34e2ee51a3`.
The generated controller explicitly fixes the 2,048/pod cohort; the generic
fixed-slot CLI's default cohort is different and does not reproduce this study.

An initial build-cache attempt reused the baseline executable for the candidate
and is **invalid/excluded**. A second build attempt failed the binary-identity
gate before measurement. Their evidence is retained. Only the `r3` images and
`results-2048-r3` observations contribute to this comparison. Kubernetes Pod
imageID references a shared imported OCI index; child image config and runtime
snapshot identity were verified separately and retained in the archive.

## Memory and resource outcomes

Memory is the highest individual-pod observation across all three trials, not
the sum of pods. RSS is sampled; cgroup peak includes other memory charges.
Their maxima need not occur together. Headroom is 1,024 MiB minus the worst
cgroup peak and is reported only when all three trials are resource-stable.
An asterisk marks failed/OOM groups with **censored memory**, not safe headroom.

| Body MiB | Version | Slots/pod | Stable trials | OOM trials / restarts | Peak RSS / cgroup MiB | Headroom MiB |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 5 | baseline | 16 | 3/3 | 0/3 / 0 | 405.27 / 639.83 | 384.17 |
| 5 | candidate | 16 | 3/3 | 0/3 / 0 | 399.07 / 627.20 | 396.80 |
| 5 | baseline | 32 | 3/3 | 0/3 / 0 | 493.64 / 695.58 | 328.42 |
| 5 | candidate | 32 | 3/3 | 0/3 / 0 | 482.33 / 698.94 | 325.06 |
| 5 | baseline | 40 | 3/3 | 0/3 / 0 | 532.93 / 754.37 | 269.63 |
| 5 | candidate | 40 | 3/3 | 0/3 / 0 | 524.91 / 790.50 | 233.50 |
| 5 | baseline | 64 | 3/3 | 0/3 / 0 | 680.73 / 975.41 | 48.59 |
| 5 | candidate | 64 | 3/3 | 0/3 / 0 | 652.41 / 967.10 | 56.90 |
| 5 | baseline | 128 | 0/3 | 3/3 / 6 | 818.18 / 1,006.98 * | — |
| 5 | candidate | 128 | 0/3 | 3/3 / 6 | 850.42 / 1,024.00 * | — |
| 10 | baseline | 16 | 3/3 | 0/3 / 0 | 495.05 / 702.62 | 321.38 |
| 10 | candidate | 16 | 3/3 | 0/3 / 0 | 479.33 / 689.11 | 334.89 |
| 10 | baseline | 32 | 3/3 | 0/3 / 0 | 662.88 / 898.42 | 125.58 |
| 10 | candidate | 32 | 3/3 | 0/3 / 0 | 643.95 / 853.58 | 170.42 |
| 10 | baseline | 40 | 3/3 | 0/3 / 0 | 741.28 / 980.56 | 43.44 |
| 10 | candidate | 40 | 3/3 | 0/3 / 0 | 726.00 / 952.27 | 71.73 |
| 10 | baseline | 64 | 0/3 | 3/3 / 6 | 836.12 / 1,024.00 * | — |
| 10 | candidate | 64 | 0/3 | 3/3 / 6 | 802.82 / 1,025.27 * | — |
| 10 | baseline | 128 | 0/3 | 3/3 / 6 | 843.59 / 1,019.39 * | — |
| 10 | candidate | 128 | 0/3 | 3/3 / 6 | 850.07 / 1,019.68 * | — |

## Request outcomes and latency

Counts sum both pods across three repetitions; pilot and warmup are excluded.
Percentiles are the **worst per-trial percentiles**, not pooled or averaged
percentiles. Elapsed is the longest trial. A resource-stable trial has no
OOM/restart and reconciled counters; exactly reconciled lost denial responses
can still occur. HTTP-clean additionally requires zero transport errors.
OOM timings omit lost responses and are censored; missing success latency is
shown as unavailable. Expected admission 503s and every transport error remain
visible. No other HTTP status was observed.

| Body MiB | Version | Slots | Verified 200 | Admission 503 | Transport errors | HTTP-clean trials | Success p50 / p95 / p99 s | Rejection p50 / p95 / p99 s | Max elapsed s |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | baseline | 16 | 588 | 11,679 | 21 | 1/3 | 42.81 / 64.76 / 72.76 | 31.51 / 57.83 / 60.05 | 73.46 |
| 5 | candidate | 16 | 580 | 11,697 | 11 | 2/3 | 42.41 / 63.40 / 72.50 | 31.61 / 57.68 / 59.90 | 73.30 |
| 5 | baseline | 32 | 1,266 | 11,006 | 16 | 0/3 | 43.53 / 73.49 / 74.74 | 32.30 / 59.35 / 62.27 | 75.72 |
| 5 | candidate | 32 | 1,174 | 11,100 | 14 | 1/3 | 42.69 / 67.71 / 72.64 | 31.52 / 57.91 / 60.06 | 73.58 |
| 5 | baseline | 40 | 1,546 | 10,728 | 14 | 1/3 | 43.03 / 72.60 / 73.25 | 32.00 / 59.19 / 61.84 | 74.92 |
| 5 | candidate | 40 | 1,466 | 10,812 | 10 | 0/3 | 42.85 / 65.90 / 72.85 | 32.21 / 58.71 / 60.97 | 73.76 |
| 5 | baseline | 64 | 2,504 | 9,766 | 18 | 1/3 | 45.81 / 75.70 / 76.92 | 34.50 / 61.95 / 65.14 | 78.33 |
| 5 | candidate | 64 | 2,344 | 9,928 | 16 | 0/3 | 43.08 / 66.32 / 73.31 | 31.28 / 58.69 / 60.57 | 74.30 |
| 5 | baseline | 128 | 0 | 2 | 12,286 | 0/3 | — / — / — | 6.17 / 6.17 / 6.17 | 6.73 |
| 5 | candidate | 128 | 0 | 20 | 12,268 | 0/3 | — / — / — | 5.61 / 5.89 / 5.90 | 6.40 |
| 10 | baseline | 16 | 579 | 11,684 | 25 | 0/3 | 42.64 / 63.99 / 72.44 | 31.10 / 57.81 / 59.91 | 73.35 |
| 10 | candidate | 16 | 578 | 11,702 | 8 | 1/3 | 42.09 / 64.97 / 71.84 | 30.90 / 57.35 / 59.58 | 72.61 |
| 10 | baseline | 32 | 1,313 | 10,938 | 37 | 0/3 | 44.30 / 75.42 / 76.28 | 33.05 / 61.03 / 64.09 | 77.37 |
| 10 | candidate | 32 | 1,203 | 11,071 | 14 | 1/3 | 43.30 / 72.83 / 73.56 | 31.57 / 58.53 / 60.71 | 74.38 |
| 10 | baseline | 40 | 1,663 | 10,625 | 0 | 3/3 | 45.87 / 76.12 / 80.06 | 35.02 / 64.78 / 67.07 | 81.29 |
| 10 | candidate | 40 | 1,595 | 10,677 | 16 | 1/3 | 43.96 / 75.60 / 76.57 | 32.03 / 61.72 / 64.18 | 77.69 |
| 10 | baseline | 64 | 0 | 242 | 12,046 | 0/3 | — / — / — | 4.89 / 6.21 / 6.21 | 6.52 |
| 10 | candidate | 64 | 0 | 205 | 12,083 | 0/3 | — / — / — | 4.32 / 5.05 / 5.18 | 5.43 |
| 10 | baseline | 128 | 0 | 26 | 12,262 | 0/3 | — / — / — | 6.14 / 6.21 / 6.21 | 6.50 |
| 10 | candidate | 128 | 0 | 598 | 11,690 | 0/3 | — / — / — | 6.76 / 8.48 / 8.67 | 9.50 |

## Unknown-length check and interpretation limits

A separate candidate correctness diagnostic sends 10 MiB bodies as HTTP/1.1
chunked uploads without Content-Length, using a 100 MiB route cap, two slots
and four offered requests per pod. It passes: four full bodies verified,
four expected 503s, zero transport errors, OOMs or restarts. Its low-cohort,
one-trial result is excluded from the capacity matrix.

Buffer growth uses Content-Length only as an advisory bound on incremental
allocation; actual-byte and route limits still apply. Missing/inaccurate hints
fall back to growth, without truncating the body. Token estimation is skipped
only when input-token/TPM limits do not require it. The two changes were measured
together, so their individual effects are not isolated.

[Linux anonymous-memory documentation](https://docs.kernel.org/admin-guide/mm/concepts.html#anonymous-memory)
explains why reserved capacity need not equal resident physical memory. Reducing
a 10 MiB body's capacity from 16 to 10 MiB does not establish a 6 MiB RSS saving
per live request, and does not eliminate OOMs from actual retained data, parsing,
connections and runtime work.

The earlier [66 MiB RSS experiment](https://github.com/sarattha/relayna-gateway/blob/d2ff47859356dea354b9817dc0adec514b6a0364/internal/test-reports/body-admission-5mib/README.md)
offers 12 requests/second across two pods, consumes each upload promptly, then
waits 12 seconds after upload. Its observed improved peak is **one buffered
5 MiB body per pod**, even with 64 configured slots. It demonstrates efficient
release after uploads, not 64 simultaneously retained bodies within 66 MiB.
That experiment is native arm64 too. The present read-paused overload is a
different workload. The v1.0.0 5 MiB selected release trials also use a different
1,024/pod cohort, so their slot observations are not a matched comparison with
this study's 2,048/pod cohort.

No 20/40/80/100 MiB or adjacent-slot search was run. This report cannot replace
the six-size release gate. Additional profiling of simultaneous body, parser,
connection and runtime memory is needed before claiming greater safe capacity.

## Reproduction and verification

Clone/download all files from the pinned evidence publication, restore and
verify the archive with its README, and use the original repository at the
recorded commits for source-hash validation (the published harness fixture
password is redacted). The archive includes generated controllers and all
parameters; the Gist root supplements the exact measurement Dockerfiles.
Preserve existing observations and use a fresh output directory for each rerun.
With the verified images present, the recorded controller command is:

```sh
python3 target/body-memory-benchmarks/arm64-focused-20261025/study.py \
  --repo "$PWD" --cohort 2048 --node-memory-gib 8 \
  --output target/body-memory-benchmarks/arm64-focused-20261025/reproduced-2048
```

The unchanged runtime patch passed the full mandatory verification script,
including Cargo formatting, Clippy/workspace tests and security checks,
**483 Nextest tests / zero skipped**, plus strict MkDocs. Verification used a
clean linked checkout to exclude an unrelated ignored design prototype from
the filesystem scan; the verified and measured runtime patches have the same
stable patch ID. All 60 raw trial audits replayed successfully before and after
sanitization. Publication checksums and the remotely restored archive match.
Both body-size report views were visually inspected at desktop/mobile widths
with no JavaScript errors. Owned clusters and temporary source worktrees were
removed; valid images and local ignored evidence remain available.
