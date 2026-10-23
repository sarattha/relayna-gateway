# Two-pod body admission benchmark

This measures the real release-built Gateway proxy, PostgreSQL-backed virtual
key authentication/policy/usage, Redis control state, and a mock registered
service. It is a local Kubernetes experiment, not a production capacity claim.

## Run from source on amd64 or arm64

Requirements: Python **3.10+**, Docker with Linux containers, kind, and kubectl.
Apple Silicon uses native Linux arm64 containers in Docker Desktop. Host,
Docker engine, image, and Kubernetes node architectures must match; the CLI
rejects emulation. Allocate enough Docker resources for two Gateway pods plus
PostgreSQL, Redis, the mock, driver and Kubernetes (the native smoke run used
10 CPUs / 20 GiB for the VM). Each Gateway is still capped at 2 CPUs / 1 GiB.
Do not run builds or other heavy work during measurements.

The executable resolves source paths from its own location and works from any
working directory. From the repository root:

```sh
scripts/bench-body-admission doctor

# Quick smoke: two JSON sizes, one slot per pod, one trial per size.
scripts/bench-body-admission run \
  --build \
  --body-mib 5 100 --slots 1 --repetitions 1 \
  --output target/body-smoke

# Repeat the original 5 MiB slot matrix, plus 128, with three trials each.
scripts/bench-body-admission run --build \
  --body-mib 5 --slots 8 16 24 32 64 128 \
  --output target/body-slot-sweep

# Discover resource edges for all six sizes with the production source image.
scripts/bench-body-admission --json run --build \
  --output target/body-capacity

# Recover/regenerate a complete or partial report offline, without Docker.
scripts/bench-body-admission report target/body-capacity
```

Choose `--build` or `--image`, never both. Builds use the production Dockerfile
and finish before measurements. For release comparisons, use an immutable native image digest or a
multi-platform index that includes your architecture; reports record the resolved image ID,
image source/version labels, harness commit, dirty state, file hashes, host,
Docker and node details. Tags can change. Nothing installs into your global
PATH. You can invoke `/absolute/path/to/repo/scripts/bench-body-admission`
from another directory.

Every `run` owns a randomly named **isolated kind cluster**, private temporary
kubeconfig and synthetic credentials. It never changes your Kubernetes context.
The cluster is deleted after completion, setup/measurement failures, SIGINT or
SIGTERM. Existing output directories are refused to preserve evidence. Setup
may pull large images and a full capacity search can take hours per body size.
Use a new output directory for every run; `--body-mib 10` runs one size.

Outputs in the requested directory:

| File | Meaning |
| --- | --- |
| `report.html` | Self-contained interactive summary, memory/latency/occupancy charts and error details; open it in any browser |
| `report.json`, `trials.csv` | Normalized summary and all trial measurements for further comparison |
| `run.json`, `provision.log` | Status, provenance, setup/cleanup diagnostics |
| `<size>mib/summary.json` | Atomic search checkpoint, including failed/incomplete trial intent |
| `<size>mib/slots-*/` | Raw request results, metrics, pod snapshots, per-pod memory and redacted diagnostics |

A report is created **before setup** and refreshed after each size and on exit,
including failed and gracefully interrupted runs. `--json` (before the command)
keeps stdout machine-readable and sends progress to stderr. Exit codes: `0` for
a completed experiment, `1` for setup/evidence/cleanup failure, `2` for invalid
arguments, `130` for SIGINT, `143` for SIGTERM. Expected observed OOM trials and
admission 503s are experimental results, not a CLI failure. A successful offline
`report` returns `0` even when the underlying run failed; its JSON status still
identifies that failed run. Invalid commands or an existing output directory do
not create/replace a report.

SIGKILL, power loss, or host failure cannot run cleanup. Regenerate the report
from the checkpoints, then delete **only** the cluster named in that run's
`run.json`: `kind delete cluster --name relayna-body-bench-<recorded-id>`.
An unfinished checkpoint stays visibly incomplete; the report never invents
missing memory/latency measurements or calls a one-trial smoke run a confirmed
limit. Keep the entire directory to preserve downloadable JSON/CSV links.

## Interpret the resource search

Both fixed sweeps (`--slots ...`) and adaptive searches (omit `--slots`) use two
fresh Gateway pods per trial, each with **2 CPU / 1 GiB limits and a 512 MiB
serialized-body budget**. Fixed sweeps run exactly the specified slot values,
`--repetitions` times each (default three). Adaptive mode searches 5, 10, 20,
40, 80 and 100 MiB JSON by default. Discovery starts below and above the
complete-body arithmetic bound; if all settings survive, it doubles the slot
ceiling and offered cohort up to `--max-search-slots` (default 4096). The bound
must cover the initial grid. Refinement and confirmations use the same final
offered cohort. All earlier cohorts remain in CSV/JSON; the summary/chart never
combines them into one confirmation.

The mock pauses upstream reads for **10 seconds**, then waits **2 seconds**
after upload. Requests have a **60-second** absolute deadline, including socket
inactivity. JSON has one string field with the exact requested serialized size; HTTP/1.1
keepalive uses no TLS, compression or retries. The driver alternates directly
between pod IPs, so this does not measure ingress/load-balancer distribution.
Body length, SHA-256 and credential stripping are verified against
the streaming mock. The fixture uses Node.js 22.23.4, PostgreSQL 16 and Redis 7.
Historical reports keep their original fixture versions and source snapshots.

The report labels a **confirmed resource edge** only when at least three trials
at the candidate setting are resource-stable, the adjacent setting was tested
at least three times with an observed OOM, and no lower-slot resource failure
contradicts that edge at the same cohort. A finite search without such an edge
is a **tested lower bound**. A fixed sweep is a **tested setting** unless it
also proves the repeated adjacent edge. Missing evidence stays incomplete.
These are observed workload/resource boundaries, not safe production defaults.
Leave memory headroom and account for your actual JSON, dependencies, traffic
mix and latency target. The slot ceiling is not preallocated memory, and
floor(512 / body MiB) counts only complete serialized bodies, not all request
processing memory. Thus increasing slots can still exhaust the 1 GiB pod.

## GitHub Actions

The `Native body admission benchmark` manual workflow calls the same CLI on
native `ubuntu-24.04` amd64 or `ubuntu-24.04-arm` arm64 hosts. By default it builds the checked-out source natively before measurement.
Select a prebuilt native image with `-f image=...`. The historical v0.1.49
release image is amd64-only, so arm64 users should build from source. GitHub's [runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
lists the native runner labels.

```sh
gh workflow run body-admission-benchmark.yml --ref <branch> \
  -f architecture=arm64 -f mode=pilot -f body_mib=5
gh workflow run body-admission-benchmark.yml --ref <branch> \
  -f architecture=amd64 -f mode=full -f body_mib=all
```

Pilot runs use slots 1 and 4, one repetition, for the requested size(s). Full
runs use the adaptive search and three confirmations. A selected size receives
up to six hours; select one size per workflow for independent longer searches.
The workflow always attempts partial report recovery and artifact upload,
including failed runs. Cancellation/runner loss may leave only the last uploaded
checkpoint, or no artifact if the runner is killed before upload. Download the
artifact and open `report.html`. Preserve host provenance when comparing sizes
or architectures.

Resource stability requires no container failure, complete memory/metric
evidence, returned-to-zero admission gauges, and every admitted request to
complete with a verified body. Lost rejection responses remain client errors
and are allowed only when exactly accounted for by server admission denials.
The separate `clean` field additionally requires zero client errors. Neither
latency nor rejection-response delivery silently defines the resource limit.
See the [six-size native resource study](https://github.com/sarattha/relayna-gateway/blob/8903a62191d461f12c53d10108b1c880452945eb/internal/test-reports/body-admission-size-capacity/amd64/README.md)
for measured boundaries and all retained observations.

Existing ARM64 results remain separate. See the
[original native amd64 confirmation](https://github.com/sarattha/relayna-gateway/blob/8903a62191d461f12c53d10108b1c880452945eb/internal/test-reports/body-admission-5mib/amd64/README.md)
for the earlier baseline/candidate comparison and original workload.

## Fixed-rate 5 MiB method

The original fixed-rate conditions have exactly two Gateway pods, each limited
to two CPUs and 2 GiB memory. The serialized-body budget stays at 512 MiB per
pod. Sweep
`GATEWAY_MAX_BUFFERED_REQUESTS` through 8, 16, 24, 32, and 64. The default load
is 12 requests/second for 15 seconds, with three repetitions and fresh Gateway
pods for each repetition. The runner waits for old pods to disappear before
starting. The mock and driver are separate pods without CPU limits; make sure
the Docker VM has sufficient spare CPU and memory.

The driver schedules requests at a fixed rate, alternating directly between
the two pod IPs. This guarantees equal offered traffic and avoids Kubernetes
connection-affinity effects. It does not measure ingress or service load
balancing. The workload is valid JSON with a single string field, exactly
5,242,880 bytes including JSON syntax, using HTTP/1.1 keepalive without TLS,
compression, or retries. The mock incrementally hashes/counts the uploaded
body, waits twelve seconds after receiving the complete upload, and returns
size/hash/credential verification. It does not retain complete bodies.

The arrival rate and mock delay deliberately exercise up to 72 concurrent
waiting requests per pod without overwhelming the authentication/CPU path.
A separate 64 requests/second, two-second-delay overload probe is retained
with the report; it must not be silently mixed into this matrix.

The candidate image includes the production Dockerfile's explicit glibc
`MALLOC_MMAP_THRESHOLD_=131072` default. The unmodified release baseline does
not. This is part of the implementation being compared, not a workload change.
Image allocator settings are recorded in `environment.json`. Use
`--malloc-mmap-threshold 131072` only for a separately labeled allocator
diagnostic; omit the flag for the baseline/candidate matrix.
Use `--memory-limit-mib 1024` only for a separately labeled 1 GiB probe; the
main comparison defaults to 2048 MiB for both versions.

Each pod gets one successful warmup request on a zero-delay mock endpoint,
followed by 500 ms idle time.
Client latency measures request dispatch through complete response receipt;
TTFB measures dispatch through response headers. Successful and rejected
requests have separate distributions. Report both success throughput over the
offered-load window and throughput including final response drain. Dispatch
lag detects a saturated load generator. All client errors and unexpected
statuses remain in the raw data.

Gateway Prometheus gauges/counters are sampled every 250 ms. Concurrent
`kubectl exec` samplers collect cgroup-v2 memory current/peak, process RSS,
and CPU-throttled microseconds every 250 ms (plus command overhead). Cgroup
memory includes charged file cache and sampler processes, while process RSS
measures Gateway resident pages. Sampled peaks can miss brief spikes; cgroup
`memory.peak` captures the kernel's lifetime high water mark, including startup
and warmup. Fresh pods prevent earlier conditions from contaminating it.
Pod snapshots record restarts/OOMs. Memory is reported per pod rather than
presenting the two-pod sum as one pod's usage.

## Historical fixed-rate baseline comparison

Use an existing local kind cluster or create one dedicated to this experiment:

```sh
kind create cluster --name body-bench
kind get kubeconfig --name body-bench > /tmp/relayna-body-bench.kubeconfig
chmod 600 /tmp/relayna-body-bench.kubeconfig
docker pull node:22.23.4-alpine
docker pull postgres:16-alpine
docker pull redis:7-alpine
kind load docker-image node:22.23.4-alpine postgres:16-alpine redis:7-alpine --name body-bench
BODY_BENCH_BASELINE_TREE="$(mktemp -d)/source"
git worktree add --detach "$BODY_BENCH_BASELINE_TREE" v0.1.46
docker build -f "$(pwd)/benches/body-admission/Dockerfile.baseline" \
  -t relayna-gateway:body-baseline "$BODY_BENCH_BASELINE_TREE"
git worktree remove "$BODY_BENCH_BASELINE_TREE"
kind load docker-image relayna-gateway:body-baseline --name body-bench
python3 benches/body-admission/run.py \
  --kubeconfig /tmp/relayna-body-bench.kubeconfig \
  --image relayna-gateway:body-baseline --source-commit "$(git rev-parse v0.1.46)" \
  --label baseline --host-metadata --output target/body-benchmark/baseline
```

Keep the unchanged baseline image and results. Build the improved image from
the current source using `Dockerfile.gateway`, load it into the same cluster,
and use its image, source reference, label, and new output directory:

```sh
docker build -f benches/body-admission/Dockerfile.gateway -t relayna-gateway:body-improved .
kind load docker-image relayna-gateway:body-improved --name body-bench
python3 benches/body-admission/run.py \
  --kubeconfig /tmp/relayna-body-bench.kubeconfig \
  --image relayna-gateway:body-improved --source-commit "$(git rev-parse HEAD)" \
  --label improved --host-metadata --output target/body-benchmark/improved
```

Record any uncommitted source patch with the candidate's source reference.
Do not build images or run other heavy jobs concurrently with measurement.

Use `--limits 8 --repetitions 1 --duration 5` for a pilot before committing to
the full matrix. Other options are `--rps`, `--delay-ms`, and `--duration`;
every setting is recorded in the artifacts. Once the pilot establishes a
method, use exactly the same settings for baseline and candidate.

The runner generates benchmark-only operator/service credentials in a
Kubernetes Secret and a real virtual key in the driver's temporary filesystem.
Credentials never appear in artifacts. Do not run in a production namespace.
Setup is repeatable; reusing the namespace retains only synthetic benchmark
data. Output directories cannot overwrite completed conditions. Preserve and
mark incomplete runs, then rerun into a new output directory. The resource
search uses `--retain-failures` to capture unsuccessful warmups, pod snapshots,
memory samples, and redacted diagnostics without discarding failed conditions.
That mode retains an immediate pod snapshot and observes status again at least
ten seconds after measurement: Kubernetes restart/OOM status can lag process
death. Successful retrieval of a Gateway's previous-container logs is also
restart evidence, even if the immediate snapshot still reports zero restarts.
The observation window is outside the recorded client timing and memory run.

Summarize results and validate the measured limits, evenly distributed offered
traffic, verified bodies, and agreement between 503s and admission counters:

```sh
python3 benches/body-admission/summarize.py \
  target/body-benchmark/baseline target/body-benchmark/improved \
  --output target/body-benchmark/summary.json
uv run --with matplotlib==3.11.2 python benches/body-admission/plot.py \
  target/body-benchmark/summary.json --output target/body-benchmark/comparison.svg
python3 benches/body-admission/html_report.py \
  target/body-benchmark/summary.json --output target/body-benchmark/comparison.html
```

For a separately labeled overload sweep, add `--rps 64 --duration 5
--delay-ms 2000 --memory-limit-mib 1024 --repetitions 1` to both runs. Keep
these outputs separate from the main matrix. `summarize_probes.py` accepts
those directories and explicitly reports transport failures, restarts/OOMs,
and censored timing instead of treating crashed runs as valid latency data.

## Cleanup

Delete only the experiment's namespace. Delete the cluster only if you created
it specifically for this benchmark.

```sh
kubectl --kubeconfig /tmp/relayna-body-bench.kubeconfig delete namespace gateway-body-bench
kind delete cluster --name body-bench
```
