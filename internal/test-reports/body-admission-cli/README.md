# Native benchmark CLI verification

The maintained entry point is [`scripts/bench-body-admission`](../../../scripts/bench-body-admission).
See the [command guide](../../../benches/body-admission/README.md) for future runs.

- [Native arm64 5 / 100 MiB smoke report](arm64-smoke/report.html): current
  production-Dockerfile image, two Gateway pods, each 2 CPUs / 1 GiB, 512 MiB
  body budget, one configured slot and two offered requests per pod. Both
  conditions verified two successful bodies and returned two intentional 503s,
  with no restarts or OOMs. These are smoke settings, not established limits.
- [Expected setup-failure report](arm64-rejected-amd64-image/report.html): the
  historical v0.1.49 digest has no arm64 manifest. The run failed before cluster
  creation and retained HTML, JSON, CSV and diagnostics. Build source on arm64.
- [Real SIGTERM interruption report](arm64-interrupted/report.html): signal
  delivered after the owned kind node existed during cluster creation. CLI
  returned 143, retained an interrupted report and removed the owned node.
- Host: Apple M1 Pro, native arm64 Docker Desktop Linux VM with 10 CPUs and
  20,940,111,872 bytes memory. Gateway resources retain their requested caps.
  [Doctor provenance](verification/doctor.json) records the final CPU-model
  reporting addition; the completed smoke checkpoint predates that metadata
  field and retains its original harness commit and hashes.
- Final smoke invoked the executable from `/tmp` and proved an external caller
  kubeconfig remained byte-identical. Its owned cluster was automatically
  deleted. Existing unrelated clusters remained running.
- Both native Actions workflow pilots passed at tooling commit `acf1dec`:
  [amd64](https://github.com/sarattha/relayna-gateway/actions/runs/39921157895)
  and [arm64](https://github.com/sarattha/relayna-gateway/actions/runs/39921157754).
  Each used 5 MiB JSON, slots 1/4, eight offered requests per pod, and verified
  successes/expected 503s with no resource or transport failures.
  [Pilot provenance](verification/native-workflow-pilots.json) is retained; the
  ARM runner exposed its architecture without a specific CPU model.
- Standalone browser media emulation verified the review fix: explicit light
  and dark canvas colors provide body-text contrast of 15.57:1 and 14.74:1.
  [Contrast check](verification/standalone-contrast-check.json) records computed
  colors without the chat renderer’s injected theme.
- 21 behavioral tests pass on Python 3.10 and 3.15. Historical adaptive-search
  regressions and four real immutable artifact replays pass; actionlint and
  strict MkDocs build pass. Desktop/light and mobile/dark HTML renderer checks
  exercised every size, memory/latency/occupancy controls and keyboard-selected
  point details, with no console errors or horizontal overflow.

Raw trial evidence, request hashes, pod snapshots, memory streams and run/image
provenance remain beside each report. Reports can be regenerated offline from
these checkpoints. The [full repository verification log](verification/repository-verification.log)
records fmt, Clippy, all 437 workspace tests and 437 Nextest tests (none skipped),
audit with the repository’s four existing exceptions, deny, machete, Trivy,
full-history Gitleaks and Semgrep passing with real isolated PostgreSQL/Redis. No full six-size adaptive search
was repeated on ARM for this CLI feature; the earlier [native amd64 capacity
study](../body-admission-size-capacity/amd64/README.md) remains separate.
