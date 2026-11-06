# Relayna Gateway v1.2.0 release evidence

Measured on 6 November 2026 (UTC), native amd64. Runtime and benchmark harness:
`14c42c72eda1358ff26600631ed5f47b3813970c`, including the reviewed rewrite
workspace-accounting correction and authentication test scheduling fix.
This artifact-only branch is separate from the application source tree.

The six full adaptive searches cover 5, 10, 20, 40, 80 and 100 MiB exact JSON:
254 fresh-pod trials, two pods each limited to 2 CPUs / 1 GiB memory /
512 MiB serialized-body budget, and at least three resource-stable confirmations
for each selected setting. All final confirmation cohorts offer 4,096 requests
per pod. Standard fixed body-slot mode is measured; opt-in elastic admission
is not enabled. Source/authentication limits remain active.

Selected settings are resource observations, not safe production defaults or
unconditional maximum concurrency. All six sizes establish tested lower bounds;
none meets this audit's strict repeated adjacent resource-edge criterion. The
10 MiB selected setting includes one trial with two lost denial replies; server
resource counters reconcile, but that trial is not HTTP-clean. Exploration OOMs,
transport errors and censored memory remain in the data. Successful latencies
include deliberate 10-second upstream read and 2-second response delays.
Occupied body slots include readers still uploading; they are not a count of
complete JSON bodies retained simultaneously.

The separate comparison uses the candidate and an immutable v1.0.2 image on the
same runner, with three fresh-pod 5 MiB trials per arm, eight configured slots and
16 offered requests per pod. Candidate runs first. This is a small sequential
sanity check, not a randomized overhead or throughput estimate.

## Files

- `gateway-1.2.0-benchmarks.html`: standalone interactive report; 18 size/metric views.
- `gateway-1.2.0-benchmarks.json`: audited summary and all 254 resource-trial rows.
- `gateway-1.2.0-trials.csv`: matching trial rows; structured cells are JSON.
- `gateway-1.2.0-audit.json`: full image, hardware, harness and workflow provenance,
  confirmation details and six same-runner comparison trials.
- `gateway-1.2.0-workflows.json`: identities and completed outcomes of all seven runs.
- `gateway-1.2.0-resource-evidence.tar.gz`: raw measurements, checkpoints, original
  reports, failure evidence, exact harness source and audit tooling.
- `gateway-1.2.0-latency-evidence.tar.gz`: both arms of the comparison.
- `gateway-1.2.0-superseded-bootstrap-evidence.tar.gz`: retained first attempt,
  invalidated by the reproducible concurrent bootstrap race.
- `gateway-1.2.0-superseded-zlib-evidence.tar.gz`: retained second attempt,
  superseded when CI found newly reported vulnerabilities in the pinned base.
- `gateway-1.2.0-image-security-evidence.tar.gz`: real package/inventory checks,
  native ARM64 protocol smoke, raw/gated vulnerability scans and the regression
  showing the unpatched image still fails. Two exact-version Debian-suite false
  positives are suppressed until 6 December 2026; no unfiltered-zero claim is made.
- `gateway-1.2.0-verification-evidence.tar.gz`: local verification summaries,
  543 passing Nextest cases, 142 proxy tests and rewrite-regression evidence,
  plus the earlier focused authentication stress log.
- `audit-release-evidence.py`: recomputation/validation and report assembly source.
- `publication-validation.json`: archive, credential and report-interaction checks.
- `Manrope-OFL.txt`: license for the embedded report font.
- `gateway-1.2.0-evidence-SHA256SUMS`: SHA-256 manifest for every other file.

The third, completed pre-review study (runtime 58cc984; 239 trials) remains
accessible in its immutable archive:
https://github.com/sarattha/relayna-gateway/tree/9d3d2ce3b876dbaea1e7e3390edac6bdda79e4a5
Its versioned record remains pinned at:
https://github.com/sarattha/relayna-gateway/blob/af313e7975481eba8991326b810e962b8d850569/docs/benchmarks/v1.2.0.md
The rewrite-accounting correction required a fresh study. Earlier attempt
evidence is never combined with final-candidate qualification.
The image-security smoke is native ARM64; capacity studies and both comparison
arms are native amd64. Hardware and sampler differences prevent direct capacity
comparison with earlier releases. Raw findings and limitations are retained.

## Hosting

The documentation workflow checks out this branch by immutable commit, verifies
the SHA-256 manifest and publishes its files under
`https://sarattha.github.io/relayna-gateway/assets/releases/v1.2.0/`.
The release record is
`https://sarattha.github.io/relayna-gateway/benchmarks/v1.2.0/`.
GitHub release attachments remain only CHANGELOG.md and the SPDX SBOM.
