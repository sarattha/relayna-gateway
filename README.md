# Gateway 1.3.0 immutable evidence

Fresh native amd64 source-built studies of runtime/harness
`4d25895cb8d45ad24a6978d234f3bb7f96b2b177` on 14 November 2026 UTC.
All six sizes, 207 fresh-pod trials, three stable confirmations per selected
setting. Full provenance and hardware appear in the audit JSON.

- `gateway-1.3.0-benchmarks.html`: standalone interactive audited report.
- `gateway-1.3.0-benchmarks.json`, `gateway-1.3.0-trials.csv`: audited summary
  and every trial, including failures and transport errors.
- `gateway-1.3.0-audit.json`, `gateway-1.3.0-audit.py`: independent reanalysis
  of all raw trials, conservative edge labels and reproducible report generator.
- `gateway-1.3.0-resource-evidence.tar.gz`: original raw observations, memory,
  metrics, pod snapshots, environment, failed trials, logs and checksums.
- `gateway-1.3.0-harness-source.tar.gz`: original harness and build inputs.
- `gateway-1.3.0-verification-evidence.tar.gz`, `gateway-1.3.0-verification.json`:
  local verification logs and final runtime/test-head CI identities.
- `gateway-1.3.0-workflows.json`: source benchmark workflow identities.
- `gateway-1.3.0-evidence-SHA256SUMS`: SHA-256 for every published file.

The original harness labels an edge after any adjacent OOM. The audited report
requires OOM in every one of at least three adjacent trials, stable repeated
confirmations and no contradictory lower-slot resource failure. Only 10 MiB
meets that standard; other sizes are tested lower bounds. Numeric observations
are unchanged and original reports remain inside the raw archive.

To reproduce, download these assets into a scratch directory, extract the
resource archive there, and run the audit script from a Gateway Git checkout
that contains the measured commit and original harness. The audit script must
sit beside the extracted `5mib` through `100mib` directories. It writes its
outputs under a `publication` subdirectory. Verify raw files using the included
`raw-SHA256SUMS`. The Git checkout supplies the original reporter/analyzer and
licensed Manrope font. No production credentials, prompts or kubeconfigs are
included; the evidence passed secret scanning.

Hosted documentation copies are published after merge to main, with the
manifest verified in the docs workflow. This archive is not a GitHub release
attachment; release attachments remain the changelog and SPDX SBOM.
