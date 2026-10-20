# Two-pod body admission benchmark

This measures the real release-built Gateway proxy, PostgreSQL-backed virtual
key authentication/policy/usage, Redis control state, and a mock registered
service. It is a local Kubernetes experiment, not a production capacity claim.

Requirements: Docker, kind, kubectl, Python 3. Use a local kind cluster; all
benchmark resources live in namespace `gateway-body-bench`. The runner uses an
explicit kubeconfig and never changes the current Kubernetes context.

## Method

Each condition has exactly two Gateway pods, each limited to two CPUs and
2 GiB memory. The serialized-body budget stays at 512 MiB per pod. Sweep
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

## Build and run

Use an existing local kind cluster or create one dedicated to this experiment:

```sh
kind create cluster --name body-bench
kind get kubeconfig --name body-bench > /tmp/relayna-body-bench.kubeconfig
chmod 600 /tmp/relayna-body-bench.kubeconfig
docker pull node:22-alpine
docker pull postgres:16-alpine
docker pull redis:7-alpine
kind load docker-image node:22-alpine postgres:16-alpine redis:7-alpine --name body-bench
docker build -f benches/body-admission/Dockerfile.gateway -t relayna-gateway:body-baseline .
kind load docker-image relayna-gateway:body-baseline --name body-bench
python3 benches/body-admission/run.py \
  --kubeconfig /tmp/relayna-body-bench.kubeconfig \
  --image relayna-gateway:body-baseline --source-commit "$(git rev-parse HEAD)" \
  --label baseline --host-metadata --output target/body-benchmark/baseline
```

Keep the unchanged baseline image and results. After implementing the change,
build a separately tagged image, load it into the same cluster, and run the
same command with its image, source reference, label, and new output directory.
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
data. Output directories cannot overwrite completed conditions. Incomplete
runs must be marked/discarded and rerun into a new output directory.

Summarize results and validate the measured limits, evenly distributed offered
traffic, verified bodies, and agreement between 503s and admission counters:

```sh
python3 benches/body-admission/summarize.py \
  target/body-benchmark/baseline target/body-benchmark/improved \
  --output target/body-benchmark/summary.json
```

## Cleanup

Delete only the experiment's namespace. Delete the cluster only if you created
it specifically for this benchmark.

```sh
kubectl --kubeconfig /tmp/relayna-body-bench.kubeconfig delete namespace gateway-body-bench
kind delete cluster --name body-bench
```
