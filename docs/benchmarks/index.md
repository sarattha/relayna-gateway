# Release benchmark history

Every Gateway version starting with **v0.1.49** must have a completed benchmark
run and a versioned results document before its version bump is merged or its
release is tagged/published. This includes patch, security, documentation and
tooling releases. [Contributor requirements](https://github.com/sarattha/relayna-gateway/blob/main/AGENTS.md#mandatory-benchmarks-for-every-version-bump)
define the release gate.

| Version | Runtime architecture | Coverage | Results |
| --- | --- | --- | --- |
| v1.0.0 candidate | Native arm64 planned | Full six-size release study pending; do not merge/tag/publish | [Validation status](v1.0.0.md) |
| v0.1.49 | Native amd64 | Full 5/10/20/40/80/100 MiB resource search; 217 trials | [Results and interactive report](v0.1.49.md) |

## Required protocol

Use the [source benchmark CLI](https://github.com/sarattha/relayna-gateway/blob/main/benches/body-admission/README.md)
on native amd64 or arm64, including Apple Silicon. Each trial uses two fresh
Gateway pods, each limited to **2 CPUs / 1 GiB**, with a **512 MiB body budget**.
Run the full adaptive resource search for all six sizes, with at least three
fresh-pod trials per confirmed setting. Smoke tests verify tooling but cannot replace this search.

```sh
scripts/bench-body-admission run --build \
  --body-mib 5 10 20 40 80 100 --repetitions 3 \
  --output target/release-benchmarks/vX.Y.Z
```

Measure a committed final candidate runtime; a subsequent runtime/build-input
change requires a new run. Results-only or documentation-only commits may follow
without rerunning an unchanged runtime. Record the measured runtime identity
separately from later documentation commits. Preserve HTML, JSON, CSV, failed trials, raw observations and exact
runtime/harness/hardware provenance in version-attributable source evidence.
Never copy a previous version's results into a new version entry.

## Add the next version

Create `docs/benchmarks/vX.Y.Z.md`, add it to this table and the MkDocs navigation,
and link it from the version's release documentation, changelog and release
notes. Include:

- Measurement date, commands, runtime source/version and image ID/digest,
  harness commit/hashes, CPU model, native architecture and Docker resources.
- All six sizes, resource caps, offered cohort and trial/confirmation counts.
- Stable slot settings or tested bounds, adjacent resource failures, peak RSS
  and cgroup memory, remaining memory headroom, OOMs and restarts.
- Success latency p50/p95/p99, rejection latency, elapsed time, verified bodies,
  HTTP 503s and transport errors, with links to per-trial distributions.
- Interactive HTML and raw JSON/CSV/evidence links, failed/incomplete/censored
  observations, and comparison with the preceding version where comparable.

A run completes when its experiment and evidence are valid, including expected
admission 503s and resource-boundary OOM trials. Setup errors, missing admitted
responses and incomplete measurements must be repaired/rerun before release.
A finite search without an adjacent boundary remains a tested lower bound.
Label architecture, hardware, cohort and methodology differences explicitly;
resource edges need production headroom and are not recommended defaults.
