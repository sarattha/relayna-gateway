# Memory-Safe Managed Body Processing

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md`.

## Purpose / Big Picture

Relayna Gateway currently buffers every managed request body and may parse or
copy large JSON bodies several times. Concurrent large requests can therefore
exhaust an AKS pod even though each request stays within its route limit. After
this change, operators can bound aggregate buffered memory and buffered-request
concurrency, large requests take a streaming fast path when no body-dependent
gateway work is required, and managed JSON metadata is extracted without
building repeated full JSON object trees. Operators can observe the protection
through bounded Prometheus metrics and overload rejections.

## Progress

- [x] (2026-07-24 16:27Z) Read repository instructions, relevant runtime code,
  mandatory skills, and the latest release boundary.
- [x] (2026-07-24 16:27Z) Created branch
  `codex/memory-safe-body-processing`.
- [x] (2026-07-24 16:49Z) Added aggregate body admission, configuration, stable
  overload errors, and
  metrics.
- [x] (2026-07-24 16:49Z) Replaced repeated full JSON feature parsing and
  avoided an unchanged-body
  copy.
- [x] (2026-07-24 16:49Z) Added a streaming-safe managed-service request path
  with incremental route
  and policy limit enforcement.
- [x] (2026-07-24 16:49Z) Added unit, contract, process-regression, and
  coverage-oriented tests.
- [x] (2026-07-24 16:52Z) Ran focused checks, instrumented coverage, and the
  complete mandatory workspace verification stack.
- [x] (2026-07-24 16:49Z) Validated the process regression against the live
  local PostgreSQL and Redis environment and visibly confirmed those
  dependencies through Computer Use in Docker Desktop.
- [x] (2026-07-24 17:12Z) Committed and pushed the implementation, opened PR
  #97, monitored its first Codex review, fixed its actionable P2 finding,
  replied with verification evidence, and resolved the handled thread.
- [x] (2026-07-24 17:04Z) Received the first Codex review on PR #97 and
  reproduced its response-side overload-contract concern.
- [x] (2026-07-24 17:11Z) Verified the response-admission review fix with
  focused proxy checks, the live dependency-backed process regression, and the
  complete mandatory verification stack.
- [x] (2026-07-24 17:12Z) Pushed commit `33e0ec9`, replied to the review with
  the fix and test evidence, and resolved thread
  `PRRT_kwDOSX_7Cc6TndT2`.
- [x] (2026-07-24 17:26Z) Changed the branch-local aggregate buffered-byte
  default from 256 MiB to the operator-requested 512 MiB across runtime,
  deployment, tests, and docs.

## Surprises & Discoveries

- Observation: `BoundedBodyRewriter::new` allocates no body capacity, but its
  completion path creates a full preview and later a second forwarded body.
  Evidence: `crates/gateway-proxy/src/body_rewrite.rs`.
- Observation: managed JSON is parsed once before effective guardrail-policy
  lookup and again during body rewriting; guardrail handlers clone the full
  `serde_json::Value`.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs` and
  `crates/gateway-core/src/guardrails.rs`.
- Observation: the application exposes active-request metrics but has no
  aggregate buffered-body accounting or admission limit.
  Evidence: `crates/gateway-telemetry/src/lib.rs` and
  `crates/gateway-api/src/config.rs`.
- Observation: Pingora invokes `proxy_upstream_filter` before it begins
  forwarding filtered request-body chunks, so route, service, key, and
  effective policy metadata are available when selecting buffered versus
  streaming body mode.
  Evidence: `pingora-proxy` 0.8.0 request lifecycle and the dependency-backed
  process regression.
- Observation: a process test using a 512-byte aggregate buffer budget
  forwarded a 4 KiB opaque service upload byte-for-byte and returned
  `503 gateway_overloaded` for a guarded 2 KiB JSON request.
  Evidence:
  `crates/gateway-api/tests/proxy_process_integration.rs`, run against the live
  `relayna-coverage-postgres` and `relayna-coverage-redis` containers.
- Observation: response-body admission originally began on the first upstream
  body chunk, after Pingora had committed upstream response headers. A failure
  could therefore become a generic proxy error or truncated success response
  instead of the stable overload envelope.
  Evidence: Codex review thread `PRRT_kwDOSX_7Cc6TndT2` on PR #97.

## Decision Log

- Decision: preserve the v0.1.21 public success path and make overload
  protection additive.
  Rationale: request/response passthrough and status shapes are released
  compatibility surfaces. A stable `503 gateway_overloaded` will appear only
  when a newly configured pod-level limit is exhausted.
  Date/Author: 2026-07-24 / Codex.
- Decision: use fail-fast, non-waiting admission with RAII release.
  Rationale: waiting while requests hold partial byte reservations can
  deadlock; RAII makes cancellation and error cleanup reliable.
  Date/Author: 2026-07-24 / Codex.
- Decision: implement the smallest streaming fast path first and avoid adding
  disk spooling in this patch.
  Rationale: fixed-cost or unpriced managed service requests do not need body
  inspection. Disk replay would materially expand proxy architecture, secret
  handling, deployment storage, and compatibility scope.
  Date/Author: 2026-07-24 / Codex.
- Decision: retain the existing guardrail DOM contract in this patch, but parse
  generation metadata with a lightweight borrowed representation and avoid
  parsing it again once a DOM already exists.
  Rationale: rewriting the guardrail trait is independently risky. Admission,
  streaming, and removal of repeated feature parses provide the requested
  protection without coupling this patch to every guardrail provider.
  Date/Author: 2026-07-24 / Codex.
- Decision: limit the streaming fast path to non-JSON registered-service
  requests whose service pricing and effective pre-call guardrail policy do not
  require body inspection.
  Rationale: generation JSON and body-priced or guarded service requests still
  need complete input before a correct policy decision; opaque uploads with no
  body-dependent work can be forwarded safely without process-wide retention.
  Date/Author: 2026-07-24 / Codex.
- Decision: pre-admit post-call response buffering in Pingora's asynchronous
  response-header filter, reserving the declared content length or the full
  configured byte budget when length is unknown.
  Rationale: admission can fail before downstream headers are committed, so
  `fail_to_proxy` can emit the same stable `503 gateway_overloaded` envelope as
  request-side contention. Reserving the full budget for unknown-length guarded
  responses favors bounded memory and response-contract correctness over
  concurrency in that exceptional mode.
  Date/Author: 2026-07-24 / Codex.
- Decision: replace the unreleased branch-local aggregate byte default directly
  with 512 MiB.
  Rationale: the configuration was introduced after v0.1.21 and has not shipped,
  so a compatibility alias or migration would add no value. Explicit operator
  overrides remain unchanged.
  Date/Author: 2026-07-25 / Codex.

## Outcomes & Retrospective

The gateway now admits complete request/response buffering through one shared,
fail-fast request-and-byte budget, reports current use and rejections, and
returns a stable retryable overload response instead of allowing concurrent
large bodies to consume memory without a process bound. Managed JSON metadata
is probed without materializing ignored payload fields; unchanged buffered
bodies are moved into Pingora rather than previewed and copied. Registered
non-JSON service uploads stream when pricing and pre-call guardrails do not need
the complete body.

The dependency-backed process regression proved both sides of the body-mode
decision under an intentionally tiny 512-byte aggregate budget. Full
verification, security scans, and measured coverage passed. PR #97 is open;
its first automated review produced one P2 response-contract finding, which was
fixed, verified, replied to, and resolved.

The shipped configuration examples and runtime fallback now default to eight
simultaneously buffered requests and 512 MiB of aggregate buffered body
reservations. Explicit environment overrides keep their existing semantics.

## Context and Orientation

`crates/gateway-proxy/src/pingora_plane.rs` owns Pingora request routing, body
filtering, policy enforcement, upstream forwarding, and response filtering.
`crates/gateway-proxy/src/body_rewrite.rs` owns the bounded in-memory body
collector. `crates/gateway-core/src/policies.rs` extracts generation metadata,
and `crates/gateway-core/src/errors.rs` owns stable public error shapes.
`crates/gateway-api/src/config.rs` reads deployment configuration, while
`crates/gateway-api/src/main.rs` constructs the shared proxy. The telemetry
crate renders the Prometheus endpoint.

A managed request is one for which Relayna authenticates a virtual key,
enforces policy, potentially applies guardrails or body-based service pricing,
and then forwards to an internal service or provider. A streaming-safe request
is a managed service request whose cost and guardrail decisions do not depend
on the complete body. A body admission lease is per-request state that counts
against process-wide limits and is released when the Pingora request context is
dropped.

## Compatibility Boundary

Compatibility boundary: latest release tag v0.1.21. Existing successful public
routes, upstream request bytes, sensitive-header stripping, usage event shapes,
and streaming response behavior remain unchanged. New environment variables are
additive and receive safe defaults. Route-size violations remain `413
request_body_too_large`; pod-level contention returns the additive `503
gateway_overloaded`. No PostgreSQL or Redis migration is required.

## Plan of Work

Add a small admission module in `gateway-proxy` that owns a concurrent-request
permit counter and aggregate byte counter. The proxy configuration will carry
the shared controller. Each buffered request will acquire a request lease and
grow its byte reservation before copying a body chunk. Exhaustion will set a
stable overload error, stop retaining body bytes, and prevent upstream
forwarding. Telemetry will expose current buffered requests/bytes and rejection
counters without high-cardinality labels.

Extend gateway configuration with aggregate buffered-request and byte limits.
Pass those values through `PingoraLiteLlmConfig`; document and deploy defaults
that leave memory headroom for JSON allocations and normal gateway state.

Refactor body collection so the final chunk is appended before inspection and
the owned buffer is taken rather than previewed. Add a lightweight generation
analysis type that deserializes only the top-level fields required for policy,
pricing, and routing; reuse an existing JSON DOM when guardrails need it.

Determine a request body mode after route and service metadata are available.
Keep passthrough behavior unchanged. Stream managed service request chunks when
there are no service body-pricing rules and no effective pre-call body
guardrails; otherwise use bounded buffering. Enforce route limits incrementally
on both paths and retain only the existing diagnostic prefix while streaming.

Add regression tests for admission contention and release, chunked byte
accounting, unchanged-body forwarding, single-pass analysis, large service
streaming, oversized and overloaded error shapes, metrics, and cancellation or
drop cleanup. Extend process integration coverage where the existing harness
can prove byte-identical upstream forwarding and overload behavior.

## Concrete Steps

From `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-core policies
    cargo test -p gateway-proxy
    cargo test -p gateway-telemetry
    cargo test -p gateway-api
    bash .codex/skills/code-change-verification/scripts/run.sh

Build and run the real local stack using the repository's existing environment
or integration harness, then use Computer Use to submit concurrent managed
requests and inspect the operator metrics/error behavior in the visible
environment.

## Validation and Acceptance

The change is accepted when:

- Aggregate buffered requests never exceed the configured concurrent limit.
- Aggregate reserved body bytes never exceed the configured byte limit.
- Leases are released after success, rejection, error, and request-context
  drop.
- A route-size violation is still a 413 with code
  `request_body_too_large`.
- Admission exhaustion is a 503 with code `gateway_overloaded`.
- A fixed/unpriced managed service request can forward a body larger than the
  in-memory aggregate budget without buffering it.
- Body-dependent pricing and guardrail requests remain buffered and preserve
  existing decisions.
- Successful unchanged request bodies arrive upstream byte-for-byte unchanged.
- The Prometheus endpoint exposes current buffered request/byte gauges and
  admission rejection totals.
- Formatting, clippy, and all workspace tests pass.
- The real environment visibly demonstrates one successful streaming request
  and one deterministic overload or configured-limit rejection.

## Idempotence and Recovery

All tests and verification commands are safe to rerun. Admission leases use
drop-based cleanup, so interrupted requests cannot leave persistent counters.
No migration or external durable state is changed. If real-environment
validation is interrupted, stop the local gateway and repeat with the same
temporary configuration. Git changes remain isolated on the feature branch.

## Artifacts and Notes

Focused validation completed:

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  passed.
- `cargo test -p gateway-proxy --all-features` passed 63 tests.
- The dependency-backed `proxy_process_integration` test passed against live
  PostgreSQL on port 25432 and Redis on port 26380.
- `cargo llvm-cov -p gateway-proxy --all-features --summary-only` reported
  81.91% line coverage overall; the new `body_admission.rs` module reported
  88.99% line coverage.
- The instrumented process regression passed and reported 6.63% line coverage
  for the large `gateway-api` process surface exercised by that single test.
- `.codex/skills/code-change-verification/scripts/run.sh` passed formatting,
  Clippy, workspace tests, cargo-audit, cargo-deny, cargo-machete, nextest (281
  tests), Trivy, Gitleaks, and Semgrep.
- After the first review fix, the same full verification stack passed again
  with 282 nextest tests, and the real-environment process regression proved a
  large guarded upstream response returns `503 gateway_overloaded` before
  downstream headers are committed.
- After changing the branch-local aggregate byte default to 512 MiB,
  `cargo test -p gateway-proxy body_admission --all-features` and
  `cargo test -p gateway-api --test config_contract --all-features` passed,
  followed by the complete verification stack with 283 nextest tests and clean
  Trivy, Gitleaks, and Semgrep scans.

Record full verification output, PR URL, check state, and review-thread
dispositions here as work proceeds.

Delivery artifacts:

- Pull request: `https://github.com/sarattha/relayna-gateway/pull/97`.
- Initial CI: Rust, docs, security, admin portal, and repository metadata
  checks passed.
- First review: Codex review of `fa0ddd2`, with one actionable P2 thread.
- Review fix: `33e0ec9`; response admission moved before downstream header
  commitment, with a live process regression for the stable overload envelope.
- Thread disposition: replied with implementation and verification evidence,
  then resolved `PRRT_kwDOSX_7Cc6TndT2`.

## Interfaces and Dependencies

The completed implementation will expose additive environment variables for
maximum concurrent buffered requests and maximum aggregate buffered bytes.
`PingoraLiteLlmConfig` will accept the corresponding limits while preserving
existing builder construction. `PingoraContext` will own any active admission
lease. `GatewayError` will include a stable overload variant. Telemetry will
provide bounded global gauges/counters. No new third-party dependency is
expected.
