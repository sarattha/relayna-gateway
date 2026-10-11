# Close four gateway security findings

## Purpose / Big Picture

Restore Relayna-key admission in canonical direct forwarding, prevent shared
upstream Anthropic batch access, account for large passthrough-priced service
responses, and enforce output-token caps before upstream generation. Add
regressions that prove denial before forwarding and preserve legitimate traffic.

## Progress

- [x] (2026-10-11) Read the manifesto and required engineering/security skills;
  inspect v0.1.43 and preserve the three pre-existing untracked security documents.
- [x] (2026-10-11) Create `codex/fix-security-governance`; start independent
  read-only boundary investigation.
- [x] (2026-10-11) Restore direct-mode admission; regress forbidden models/features,
  TPM and budget denial before upstream receipt in both modes and path aliases.
- [x] (2026-10-11) Reject governed batch lifecycle operations, including shared
  project/key mappings and trailing-slash fallback; preserve caller credentials.
- [x] (2026-10-11) Observe complete upstream JSON with bounded memory and preserve
  streamed delivery; reconcile costs after a 100 KB payload in PostgreSQL/Redis.
- [x] (2026-10-11) Validate explicit provider maxima, aliases, multiplicity and
  batch sums; reject ambiguous fields and unsafe provider extensions.
- [x] (2026-10-11) Complete the one independent candidate review; confirm and close
  model-only wildcard caps, nested provider overrides and compression negotiation.
- [x] (2026-10-11) Run focused regressions, the one independent candidate review
  and the complete mandatory verification stack with live PostgreSQL/Redis.
- [x] (2026-10-11) Commit, push and open review PR #132, then attach it to this
  chat: https://github.com/sarattha/relayna-gateway/pull/132.
- [x] (2026-10-11) Address the existing GitHub review: reject nested model
  overrides regardless of the top-level model policy; make batch admission
  linear in body size and reuse policies for repeated models; regress encoded
  batch paths against the existing ambiguous-path denial.
- [ ] (2026-10-11) Prepare version 0.1.44, changelog, current examples and upgrade
  documentation; rerun release metadata, build, docs and mandatory verification.
- [ ] (2026-10-11) Request exactly one additional Codex review on the final patch,
  address actionable feedback, confirm CI and land PR #132 as authorized.

## Surprises & Discoveries

- The initial GitHub review found an uncapped-model override and quadratic batch
  parsing. Both are addressed. Its encoded-path bypass hypothesis is prevented
  by existing `LiteLlmPassthroughSettings::denies_path`; real Pingora regressions
  deny `%2F`, `%2f`, encoded letters and double encoding before upstream receipt.
  A 3,000-entry batch succeeds, mixed-model total caps still deny/allow correctly,
  and a counted unit test proves one base lookup plus one per distinct model.
- The focused live regression, distinct-model lookup regression, release metadata
  validator, all-feature workspace build, Admin UI tests and strict MkDocs build
  pass for 0.1.44. MkDocs is provided by the existing `.tmp-mkdocs-venv`.
- Direct-mode routes intentionally retain status-only usage/no guardrail rewrite;
  the fix must restore admission without silently changing that metering contract.
- A per-key credential mapping does not prove batch isolation: duplicate secrets
  and project/global fallback can share the upstream namespace.
- Pingora response body callbacks are synchronous. Whole-response buffering or
  asynchronous ownership persistence after delivery would introduce new risks.
- The reviewer identified model-only caps missed by wildcard header admission,
  LiteLLM provider merges after validation, and caller-selected compression of
  priced service JSON. Source inspection confirmed each within the four findings.
- Initial integration setup used wrong store API argument types and omitted the
  service credential; these fixture errors were corrected. The first mandatory
  stack run stopped on Clippy's range-pattern warning; it was corrected.
- Under heavy host load the original replica-sync request timeout (3 seconds)
  and Foundry streaming assertion (650 ms including authentication) failed.
  A diagnostic replica run returned every expected response. Retain its
  12-second convergence deadline, allow 10 seconds for an individual request,
  and replace the Foundry timing assertion with a stronger upstream completion
  barrier. Temporary diagnostic logging was removed.
- The complete workspace and all 424 nextest tests passed, but Trivy blocked the
  mandatory sequence on seven high-severity transitive dependency findings in
  the ignored local service-owner-monitoring prototype. The root frontend
  lockfile and Rust dependencies were clean at Trivy's configured severity.
  Refresh only the four named transitive packages within their existing semver
  ranges, build/test the local prototype, and rerun the complete stack without
  adding suppressions. Its ignored lockfile remains excluded from the PR.

## Decision Log

- 2026-10-11, Codex: the user authorizes preparing 0.1.44 and landing PR #132.
  The existing bot review confirms an uncapped top-level model can select a
  stricter model through `extra_body.model`; reject that selector unconditionally.
  All complete-body callers validate the merged policy, so validate aggregate
  batch maxima/limits there once and cache effective policy per distinct model.
  Percent-encoded paths already fail `denies_path` before wildcard routing;
  add real forwarding regressions rather than introduce a second decoder.
  Preserve the v0.1.43 schema, Redis formats and direct-mode accounting contract.
- 2026-10-11, Codex: user explicitly authorizes security behavior tightening at
  the latest release boundary. Preserve route/error shapes and intentional
  caller-owned LiteLLM delegation. Do not add unsafe compatibility exemptions.
- 2026-10-11, Codex: request changes belong in gateway-proxy and core policy;
  bounded cost extraction belongs beside usage accounting. Batch strategy will
  reject listing, retrieval, results and cancellation for Relayna keys because
  ownership cannot be established; exact POST creation remains governed. No
  ownership registry or migration is introduced.
- 2026-10-11, Codex: capped requests require supported protocol maxima and fields;
  nonempty provider extensions fail closed. Uncapped extensions retain ordinary
  vendor parameters but cannot override denied model/stream/tool selectors.
  Wildcard body buffering selects model caps without changing its other delegated
  policy/accounting semantics. Priced services negotiate identity encoding,
  including routes whose selectors can switch to passthrough pricing.
- 2026-10-11, Codex: the mandatory Trivy failure is concrete evidence for a
  narrowly scoped local prototype lockfile refresh. Preserve its manifest,
  application source, Sites packaging and gateway assets; do not turn this into
  a dependency redesign or weaken the configured security checks. Keep ignored
  prototype files outside the PR.

## Outcomes & Retrospective

All four fixes are implemented and verified. The real Pingora regression denies
the original unsafe requests before upstream receipt, preserves legitimate
managed/direct/caller-owned traffic, observes cost beyond a 100 KB payload, and
proves first-chunk delivery before upstream completion. The original scan remains
sealed; a separate fix report records remediation evidence.

The complete fail-fast script passed on 2026-10-11 with disposable PostgreSQL and
Redis. `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2` and `NEXTEST_TEST_THREADS=2`
limited contention without disabling checks or tests. Results, in script order:

1. `cargo fmt --all --check`: passed.
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
3. `cargo test --workspace --all-features`: passed, including 170 core unit
   tests, 96 proxy unit tests and the live security regression.
4. `cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2024-0437 --ignore RUSTSEC-2026-0173`:
   passed with five allowed warnings under existing repository configuration.
5. `cargo deny check`: passed with existing configuration warnings.
6. `cargo machete`: passed.
7. `cargo nextest run --workspace --all-features`: 424 passed, zero skipped.
8. `trivy fs --severity HIGH,CRITICAL --exit-code 1 --skip-dirs target --skip-dirs site .`:
   passed, zero reported vulnerabilities at the configured severity.
9. `gitleaks detect --source . --redact`: passed, no leaks found.
10. `semgrep scan --config .semgrep.yml`: passed, zero findings; two configured
    rules on 81 targets, with normal ignore patterns and size exclusions.

The ignored local prototype dependency refresh also passed `npm ci --no-audit
--no-fund`, `npm run build` and all four `npm run test:sites` tests; its local
preview loaded. Its source and manifest were retained and its lockfile is not
part of the PR. No new scanner suppressions were introduced.

## Context and Orientation

Repository: `/Users/jobz/Works/relayna-gateway`. A virtual key authenticates a
Relayna caller; key policies govern route/model/features and spending. Pingora
in `crates/gateway-proxy/src/pingora_plane.rs` owns upstream forwarding and body
callbacks. Core policy/usage live in `crates/gateway-core/src/`; PostgreSQL
stores policies/usage and Redis tracks admission. A passthrough-priced service
reports cost in its JSON response; this differs from intentionally unmetered
LiteLLM direct routing. Existing regressions use mock upstreams and disposable
PostgreSQL databases/Redis namespaces.

## Compatibility Boundary

Latest release tag: v0.1.43, equal to the starting HEAD e1bf6d2. Security fixes
intentionally reject previously unsafe requests. Retain existing public routes,
structured `policy_denied` errors, credential stripping, body bounds, streaming
delivery, and caller-owned credential delegation. No schema change is planned.
Finite output caps may require explicit supported provider generation maxima;
unsupported or ambiguous capped formats must fail closed.

## Plan of Work

First fix the direct-mode early-return predicate and test the actual body
admission path. Independently assess the batch boundary and gate unsafe governed
operations. Add a bounded incremental JSON accounting observer that scans all
response chunks without retaining payload strings. Validate output generation
maximums at every complete-body policy check, including model-specific and batch
paths. Document client-visible restrictions and run focused tests before the
independent bypass/regression review.

## Validation and Acceptance

Run focused core/proxy regressions plus mock upstream integration tests with
local PostgreSQL/Redis. Confirm denied requests never reach upstream, valid
within-cap/direct/caller-owned requests still work, and large responses are
delivered without whole-response retention while their cost is recorded.

From the repository root run:

    bash .codex/skills/code-change-verification/scripts/run.sh

This runs formatting, clippy, workspace tests, cargo audit/deny/machete/nextest,
Trivy, Gitleaks and Semgrep in fail-fast order. Fix failures and rerun the stack.
Keep exact verification results in this plan and the PR description.

## Idempotence and Recovery

Use disposable databases and per-test UUIDs; preserve existing developer
services and files. The changes have no planned migration. If verification is
interrupted, rerun the fail-fast script from the repository root. Stage only
this task's named files; never stage the existing September security documents.
