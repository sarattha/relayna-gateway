# Gateway v1.0.1 release evidence

Measured runtime and harness: 52c56f9104f43cb108b7ccd647f6911433f8dcfe.
Native amd64, six independent full adaptive runs, 212 fresh-pod trials.
Raw observations are unchanged; failed/OOM trials are retained. Audited summaries use a stricter edge interpretation than the original reports.

Open gateway-1.0.1-benchmarks.html for the interactive report. Full provenance is in gateway-1.0.1-audit.json and gateway-1.0.1-workflows.json. Separate fixed-slot latency arms used different CPUs; no controlled latency comparison is claimed.

The two evidence archives include the exact execution harness and all raw downloaded artifacts. audit-release-evidence.py rebuilds audited data from those artifacts under target/release-benchmarks/v1.0.1; write-benchmark-docs.py builds documentation and adjusts published report links. Run them from the measured source checkout. Both scripts retain their original local output location; change that output path when reproducing elsewhere.

Verify before use: shasum -a 256 -c gateway-1.0.1-evidence-SHA256SUMS.
The HTML embeds official Manrope 4.505; its OFL is included. These artifacts are published by documentation hosting and are not GitHub release attachments.
