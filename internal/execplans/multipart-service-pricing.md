# Multipart-Aware Service Pricing

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

This document is maintained in accordance with `PLANS.md` at the repository
root.

## Purpose / Big Picture

Operators can already attach fixed, passthrough, or no-cost pricing rules to a
registered internal service using JSON Pointer selectors such as `/engine`.
After this change, the same rule matches both an `application/json` body such as
`{"engine":"docint"}` and a `multipart/form-data` text field named `engine`
whose value is `docint`. File parts remain byte-for-byte passthrough data and
are not collected in memory for pricing.

The observable outcome is that a multipart OCR upload with a configured
`/engine = docint` fixed rule records that rule's price and rule name in the
usage event. The gateway reserves a conservative fixed-cost ceiling before
opening the upstream request and reconciles the reservation to the selected
price after the streamed request body is parsed.

## Progress

- [x] (2026-07-21 16:00Z) Read `AGENTS.md`, `internal/design-manifesto.md`,
  `PLANS.md`, and the required repository skills.
- [x] (2026-07-21 16:00Z) Confirmed the compatibility boundary and traced the
  pricing/body lifecycle in Gateway v0.1.20.
- [x] (2026-07-21 16:12Z) Added content-type-aware multipart metadata
  extraction over the proxy's existing bounded request buffer without copying
  file fields into selector metadata.
- [x] (2026-07-21 16:12Z) Moved actual service pricing resolution to
  request-body completion while preserving JSON matching.
- [x] (2026-07-21 16:12Z) Added maximum-fixed-cost preflight estimation; final
  reservation reconciliation already consumes the resolved usage cost.
- [x] (2026-07-21 16:12Z) Added focused core/proxy regression tests and operator
  documentation.
- [x] (2026-07-21 16:19Z) Ran the mandatory verification stack: formatting,
  Clippy, workspace tests, nextest, dependency audit/policy, Trivy, gitleaks,
  and Semgrep all passed.

## Surprises & Discoveries

- Observation: `resolve_service_cost_for_ctx` currently runs from Pingora's
  `proxy_upstream_filter`, but `request_body_filter` fills `body_prefix` later
  during upstream proxying. The cached resolution therefore sees an empty body
  and selects the service default.
  Evidence: `crates/gateway-proxy/src/pingora_plane.rs` calls the resolver in
  `proxy_upstream_filter` before its `request_body_filter` body-capture path.

- Observation: the deployed OCR API accepts `POST /ocr` as
  `multipart/form-data` with `engine` as a text form field and `file` as a file
  part. JSON deserialization cannot inspect this request encoding.
  Evidence: the live OCR OpenAPI schema in `vm-machine01`, namespace `common`.

- Observation: pricing rules are intended to influence both usage accounting
  and pre-upstream budget decisions, but a streaming proxy cannot know a body
  field before receiving it. Reserving the maximum configured fixed cost is a
  safe ceiling that avoids an additional file-sized pricing buffer and can be
  reconciled when the selector becomes known.

- Observation: non-passthrough proxy requests are already collected by
  `BoundedBodyRewriter` up to the configured route/service body limit so
  guardrail processing can inspect or rewrite them before emitting the final
  upstream body chunk.
  Evidence: `crates/gateway-proxy/src/body_rewrite.rs` suppresses intermediate
  chunks and emits the bounded buffer at end-of-stream.

- Observation: the first parser candidate, `multer 3.1.0`, pulled the yanked
  `spin 0.9.8` crate and failed `cargo deny`.
  Evidence: the first mandatory verification run reported the exact
  `spin -> multer -> gateway-proxy` dependency chain. Replacing it with
  `multra 1.1.0` removed that chain and the repeated dependency checks passed.

## Decision Log

- Decision: preserve the existing `pricing_rules` schema and JSON Pointer
  syntax. For multipart bodies, non-file UTF-8 fields become top-level string
  properties, so `/engine` selects the `engine` form field.
  Rationale: registered services do not need migrations or duplicated pricing
  configuration, and existing JSON behavior remains intact.
  Date/Author: 2026-07-21 / Codex.

- Decision: run the asynchronous multipart parser over a borrowed view of the
  proxy's existing bounded request buffer. Retain at most 64 KiB of textual
  selector metadata and do not copy file contents into pricing state.
  Rationale: this adds no second file-sized allocation, preserves the existing
  upstream body behavior, and supports fields that appear after large file
  parts. Removing the pre-existing guardrail request buffer is a separate proxy
  architecture concern outside this pricing change.
  Date/Author: 2026-07-21 / Codex.

- Decision: use the highest fixed estimate from the service default and all
  rules as the preflight policy/budget estimate, then reconcile to the actual
  resolved cost after body parsing.
  Rationale: the gateway cannot safely know a streamed multipart selector before
  proxying, and under-reserving the default `0.01` for a `0.5` variant would
  weaken budget enforcement. A conservative ceiling is safe and requires no
  public API change.
  Date/Author: 2026-07-21 / Codex.

- Decision: use `multra 1.1.0` with bounded parsing constraints.
  Rationale: it provides the required async reader API and boundary validation
  without the yanked transitive dependency found in the initial candidate.
  Date/Author: 2026-07-21 / Codex.

## Outcomes & Retrospective

Gateway now resolves the existing JSON Pointer service-pricing rules from
either JSON request bodies or top-level multipart text fields. The OCR shape
`engine=docint` therefore selects the configured `0.5` rule even when a large
file part comes first. File fields and bounded/invalid textual metadata do not
become selector values, and upstream request bytes are not modified.

Preflight policy and budget enforcement reserve the highest configured fixed
service/rule estimate because the selector is unavailable before request-body
delivery. Final usage accounting and budget reconciliation use the actual
matched cost and pricing-rule name. Operators must allow the highest fixed
variant in `max_cost_per_request`; otherwise the conservative preflight check
rejects the request before its selector is known.

Verification completed successfully with `cargo fmt --all --check`, workspace
Clippy with warnings denied, all workspace/all-feature and doc tests, all 234
nextest cases, cargo audit/deny, Trivy, gitleaks, and Semgrep. No database,
Redis, API, or deployment migration is required.

## Context and Orientation

`crates/gateway-core/src/services.rs` owns service pricing rule validation and
resolution. A `ServicePricingRule` contains a JSON Pointer, an exact string
value, a cost mode, and an optional fixed estimate.

`crates/gateway-proxy/src/pingora_plane.rs` owns the proxy lifecycle.
`PingoraContext.body_prefix` retains a bounded request prefix for feature
extraction, while `BoundedBodyRewriter` already retains non-passthrough request
bodies up to the route limit for guardrail processing. Service budget
reservation happens earlier in `proxy_upstream_filter`.

`usage_events` already stores `cost_source`, `cost_mode`, and
`pricing_rule_name`. This change does not alter PostgreSQL, Redis key formats,
public routes, response shapes, credentials, or the body bytes forwarded to the
upstream service.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.20`. This is an additive
extension to released pricing-rule behavior and a correction to when released
body-based rules are resolved. Existing JSON bodies, pricing-rule payloads,
service routes, response bodies, database rows, and Redis key formats remain
compatible. Multipart bodies are forwarded unchanged. Requests may now be
conservatively rejected by `max_cost_per_request` when any configured fixed
variant exceeds that limit; this is intentional fail-closed budget behavior and
will be documented.

## Plan of Work

Add framework-independent helpers to `gateway-core` that resolve rules against
an already parsed selector document and compute the highest configured fixed
estimate for preflight enforcement. Keep the existing byte-slice JSON resolver
as a compatible wrapper.

Add `multra` to `gateway-proxy` and capture the request content type in
`PingoraContext`. When an internal service has pricing rules and the content
type has a valid multipart boundary, parse a borrowed view of the existing
bounded request buffer. The parser drains file fields, retains bounded non-file
UTF-8 values, and returns a JSON object after the final boundary.

Replace the premature cached rule resolution in `proxy_upstream_filter` with a
conservative preflight cost. Resolve or replace the actual service cost after
the body filter reaches end-of-stream: multipart uses the extracted selector
object and other content types preserve the existing JSON resolver. Final usage
and reservation reconciliation then use the selected rule.

Add tests in `gateway-core` for parsed-value resolution and fixed-cost ceilings.
Add proxy tests that split boundaries across chunks, place a file larger than
the retained metadata limit before the `engine` field, ignore file contents,
and fall back safely for malformed or oversized metadata. Update operator docs
to explain multipart field mapping and conservative preflight enforcement.

## Concrete Steps

Run from `/Users/jobz/Works/relayna-gateway`:

    cargo test -p gateway-core services::tests
    cargo test -p gateway-proxy multipart
    cargo fmt --all --check
    bash .codex/skills/code-change-verification/scripts/run.sh

The final script must pass formatting, Clippy with warnings denied, and all
workspace tests in fail-fast order.

## Validation and Acceptance

- Existing JSON `{"engine":"docint"}` requests continue to select a
  `/engine = docint` rule.
- Multipart requests with a text part `engine=docint` select the same rule even
  when a large file part precedes the text field or boundaries cross chunks.
- File bytes are not retained as pricing metadata and body chunks remain
  unchanged for upstream forwarding.
- Malformed multipart bodies, missing selectors, non-UTF-8 text values, and
  metadata over the retention limit fall back without exposing or logging body
  contents.
- Preflight policy and reservation cost equals the largest configured fixed
  estimate; final usage and reconciliation use the actual matching rule.
- Usage metadata reports `service_pricing_rule_fixed` and the rule name for a
  matching multipart request.
- Existing workspace formatting, linting, and tests pass.

## Idempotence and Recovery

The implementation has no migration or external-state step. Cargo dependency
resolution and all tests are safe to rerun. If verification fails, fix the
focused error and rerun the complete verification script. Reverting the code,
Cargo manifest/lock changes, docs, tests, and this ExecPlan restores v0.1.20
behavior without database or Redis cleanup.

## Artifacts and Notes

Representative multipart shape:

    --boundary
    Content-Disposition: form-data; name="file"; filename="document.pdf"
    Content-Type: application/pdf
    
    <streamed bytes>
    --boundary
    Content-Disposition: form-data; name="engine"
    
    docint
    --boundary--

The extracted selector document is equivalent to:

    {"engine":"docint"}

## Interfaces and Dependencies

`gateway-core` will expose a resolver that accepts `&serde_json::Value` and a
helper that returns the maximum configured fixed service cost. Existing
`resolve_service_cost(&[u8], ...)` callers remain supported.

`gateway-proxy` will add `multra 1.1.0` with its `tokio-io` adapter
dependencies already compatible with the workspace. Multipart parsing is
internal; no environment variable, API field, database column, or deployment
setting is introduced.
