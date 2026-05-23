# Codex Roadmap Implementation Goals for Issues #37-#42

This document is a Codex-facing roadmap goal package. It turns GitHub issues
#37, #38, #40, #41, and #42 into implementation-ready goals that can be used by
future Codex runs. Issue #39 is intentionally excluded because GitHub marks it
closed as a duplicate of #38.

Before implementing any goal here, read `AGENTS.md`, `PLANS.md`, and
`internal/design-manifesto.md`. Each phase is multi-file and
compatibility-sensitive, so create and maintain an ExecPlan under
`internal/execplans/` before editing code.

## Master Goal

Implement the canonical Relayna Gateway roadmap phases in dependency order:
#37, #38, #40, #41, then #42. Do not implement #39 directly; treat it as a
duplicate pointer to #38.

The production freeze baseline is `v0.0.14`. Every implementation phase must
use `$production-freeze-guard` and `$implementation-strategy` before changing
public routes, admin APIs, response shapes, authentication behavior, policy,
usage event shapes, PostgreSQL schemas, Redis formats, proxy behavior,
streaming behavior, telemetry fields, CI/build behavior, or deployment
configuration. Use `$code-change-verification` before marking Rust runtime,
test, migration, packaging, or build/test changes complete.

Later phases may rely on earlier phases only after their acceptance criteria are
merged. If a later phase starts before an earlier dependency exists, first add
the smallest prerequisite or explicitly narrow the later phase's scope in its
ExecPlan.

Shared completion gates:

- Freeze perimeter: `node tests/freeze-v0.0.14-perimeter.test.mjs` passes
  unchanged, or intentional perimeter updates include compatibility notes.
- Rust verification: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`, and `cargo test --workspace
  --all-features` pass for Rust-affecting changes.
- Documentation: update operator-facing docs when behavior, configuration,
  deployment, metrics, traces, schemas, or API contracts change.
- Security: raw virtual keys, operator tokens, worker tokens, provider keys,
  LiteLLM credentials, internal service tokens, prompts, and provider secrets
  must not be logged, persisted in debug bundles, returned to clients, or
  exposed through Admin UI.

## Phase 1 Goal: Security Foundation and Operator Governance (#37)

Goal: strengthen the gateway's security baseline with scoped operator access,
auditable admin mutation history, constant-time worker token verification, and
stable structured errors.

Affected surfaces:

- `gateway-api`: admin route protection, request IDs, error responses, audit
  read endpoints, Admin UI access behavior.
- `gateway-core`: operator roles/scopes, authorization decisions, structured
  error taxonomy, worker-token verification helpers.
- `gateway-store`: operator token schema changes, append-only audit events
  table, audit query APIs.
- `gateway-proxy`: `x-relayna-worker-token` verification and credential
  stripping tests.
- Docs and tests: admin portal, security, database, and freeze perimeter
  coverage.

Compatibility boundary: latest release tag `v0.0.14`. Prefer additive schema
and response changes. Preserve existing `op_live_` token format, existing
admin route paths, and current error envelope fields while adding stable codes
or scope/audit fields as needed.

Implementation milestones:

- Define operator roles and capability scopes, then bind stored operator tokens
  to roles/scopes without exposing raw token material.
- Replace single `require_admin` checks with scope-aware authorization for all
  `/admin-ui/admin/*` routes.
- Add an append-only `audit_events` table and store actor token ID, action,
  target type, target ID, before/after JSON, request ID, IP, user agent, and
  timestamp for admin mutations.
- Add scoped audit read APIs for auditors and a minimal Admin UI view or
  consumable response shape for Relayna Studio.
- Replace direct worker-token equality with constant-time comparison and keep
  `x-relayna-worker-token` stripped from upstream requests.
- Normalize auth, policy, rate-limit, budget, guardrail, and upstream errors so
  every error response includes stable `code`, public `message`, and
  `request_id`.

Acceptance criteria:

- Every protected admin API requires an operator scope and denies insufficient
  scopes with a stable structured error.
- Admin mutations produce append-only audit rows and never write raw secrets.
- Worker token checks pass for exact matches and fail for missing, malformed,
  and mismatched tokens using constant-time comparison.
- Existing clients keep receiving compatible error envelopes with `request_id`.
- Tests cover allowed and denied admin actions, audit insertion, audit reads,
  worker token cases, and structured error mappings.

Required verification:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `$code-change-verification`

Docs updates:

- Document operator roles/scopes, audit event semantics, error code taxonomy,
  and worker token handling in the relevant admin, operations, database, and
  security docs.

## Phase 2 Goal: Policy Governance, Key Lifecycle, and Explainability (#38)

Goal: make Relayna Gateway a stronger policy decision point for project, team,
key, route, model, provider, guardrail, and request governance.

Affected surfaces:

- `gateway-core`: effective policy resolver, lifecycle/risk fields, request
  and response size policy, simulator decision engine.
- `gateway-api`: Admin APIs for policy simulation, key lifecycle metadata, key
  presets, and Admin UI screens.
- `gateway-store`: policy layer persistence, policy version metadata, key risk
  fields, migration coverage.
- `gateway-proxy`: size-limit enforcement before forwarding and during
  response/stream handling.
- Docs and tests: policy, admin portal, database, Redis if request-time
  disable or counters are added, and freeze perimeter.

Compatibility boundary: latest release tag `v0.0.14`. Add policy fields and
Admin API fields compatibly. Existing key policies must continue to evaluate
with current defaults unless explicitly migrated with notes.

Implementation milestones:

- Add an effective policy resolver that combines global, project, team, key,
  route, and model layers with deterministic semantics: deny overrides allow,
  mandatory guardrails are additive, forbidden guardrails override optional
  requests, lower-level budgets can only become stricter, and allowlists
  intersect.
- Persist policy version metadata for audit and debugging.
- Add key lifecycle/risk controls such as daily request/token caps,
  per-request cost and token caps, allowed UTC hours, rotation due dates,
  last-used metadata, and stale-key auto-disable behavior.
- Enforce request and response size policy in proxy paths with stable
  structured error codes.
- Add a policy simulator Admin API that uses the same engine as the real proxy
  path and explains auth, route match, policy merge, guardrail plan,
  rate-limit projection, budget projection, and final decision.
- Add safe key creation presets for developer, production worker, read-only
  service, external partner, and temporary debugging keys.

Acceptance criteria:

- Effective policy decisions are deterministic and test-covered across merge
  edge cases.
- Operators can simulate a request before issuing or changing a key.
- Key lifecycle metadata is visible and actionable through Admin API/Admin UI.
- Request and response limits are enforced consistently across non-streaming
  and streaming routes where applicable.
- New keys can be created from safe presets without exposing raw key material
  after creation.

Required verification:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `$code-change-verification`

Docs updates:

- Document policy inheritance semantics, simulator inputs/outputs, lifecycle
  controls, safe presets, and size-limit errors.

## Phase 3 Goal: Provider Intelligence and Resilient Upstream Orchestration (#40)

Goal: transform Relayna Gateway from a static proxy into an intelligent,
observable provider routing and resilience control plane.

Affected surfaces:

- `gateway-core`: routing strategies, fallback policy, retry safety,
  capability/region/budget constraints, circuit breaker decisions.
- `gateway-proxy`: provider selection, retry/fallback execution, upstream
  latency accounting, redacted debug bundle capture.
- `gateway-store`: provider health state, service registry snapshot versions,
  import diffs, rollback metadata, debug bundle persistence.
- `gateway-api`: Admin APIs and UI for provider health, debug bundles, service
  import preview, validation, activation, and rollback.
- `gateway-telemetry`: metrics and traces for provider selection, fallback,
  circuit transitions, and import validation.

Compatibility boundary: latest release tag `v0.0.14`. Keep current route paths
and upstream credential handling stable. Add routing/fallback behavior
conservatively and fail closed when policy or provider state is ambiguous.

Implementation milestones:

- Add routing strategies for priority, weighted, least latency, least cost,
  health aware, budget aware, region affinity, and capability aware routing.
- Add fallback rules for retry-safe status codes, timeouts, provider failover,
  max attempts, and cooldown periods.
- Implement active health checks, passive health scoring, and
  closed/open/half-open circuit breaker states.
- Add request replay/debug bundles keyed by request ID with route match,
  provider selection, policy trace, guardrail trace, upstream latency,
  retry/fallback history, and redacted request/response hashes.
- Improve service registry imports with preview/diff, removed/changed route
  detection, upstream URL validation, service health validation, snapshot
  version history, rollback, and Studio-friendly responses.

Acceptance criteria:

- Gateway can route between multiple providers using configured strategies.
- Provider health and circuit state influence routing and are visible to
  operators.
- Operators can inspect redacted debug bundles without exposing secrets or full
  prompts.
- Service registry imports support preview, validation, activation, version
  history, and rollback.
- Fallback behavior is integration-tested against simulated provider failures.

Required verification:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `$code-change-verification`

Docs updates:

- Document routing strategies, fallback policy, circuit breaker behavior,
  provider health fields, debug bundle redaction, and service import rollback.

## Phase 4 Goal: Observability, Analytics, and Operational Intelligence (#41)

Goal: give operators and Relayna Studio deeper visibility into traffic, cost,
errors, guardrails, providers, policy decisions, and latency.

Affected surfaces:

- `gateway-telemetry`: Prometheus metrics, OpenTelemetry setup, redaction,
  bounded labels, trace propagation.
- `gateway-core`: analytics dimensions and status/error classification.
- `gateway-api`: analytics APIs, Admin UI dashboards, RBAC-protected usage
  filters and exports.
- `gateway-store`: usage query indexes, analytics aggregation queries, trace
  IDs in usage/debug records where appropriate.
- Docs and tests: operations, deployment, admin portal, metrics/tracing setup,
  and freeze perimeter.

Compatibility boundary: latest release tag `v0.0.14`. Add metrics, spans, and
analytics fields without changing existing API response meanings. Keep metric
labels bounded and avoid high-cardinality request IDs, raw keys, prompts, or
unbounded route values.

Implementation milestones:

- Add histograms for gateway request duration, upstream duration, guardrail
  duration, and first-token latency.
- Add counters/gauges for auth failures, policy denials, rate-limit denials,
  budget denials, provider fallbacks, circuit breaker states, active requests,
  and active streams.
- Add OpenTelemetry spans for gateway request, auth verification, policy
  evaluation, guardrails, rate-limit and budget checks, upstream calls, and
  usage recording.
- Preserve and propagate `traceparent`; include trace IDs in usage or debug
  records where they improve operator workflows.
- Expand usage analytics dashboards and APIs for cost, errors, denials,
  guardrail blocks, fallback rate, expensive requests, and unused keys with
  filters by time, project, key, model, route, provider, service, task ID, run
  ID, and status.
- Enforce Phase 1 RBAC scopes for analytics and exports.

Acceptance criteria:

- Metrics are production-safe and low-cardinality.
- Gateway emits useful OpenTelemetry spans and propagates trace context.
- Admin UI exposes actionable analytics that respect RBAC scopes.
- Relayna Studio can consume analytics-ready APIs.
- Documentation includes Prometheus/Grafana and tracing setup examples.

Required verification:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `$code-change-verification`

Docs updates:

- Document metric names and labels, tracing configuration, dashboard filters,
  analytics API response shapes, and cardinality constraints.

## Phase 5 Goal: Supply-Chain Security and Hardened Deployment (#42)

Goal: harden the release pipeline, container image, and Kubernetes deployment
so Relayna Gateway can be operated safely in production clusters.

Affected surfaces:

- CI/release workflows: security scanners, test ordering, artifact publishing,
  SBOMs, image signing, provenance/attestations.
- Build and packaging: Dockerfile, image metadata, GHCR release behavior,
  dependency policy files.
- Deployment manifests: Kubernetes security contexts, NetworkPolicy, service
  separation, ingress examples.
- Docs: deployment, operations, releases, local-dev exceptions, allowed
  security-tool exceptions.

Compatibility boundary: latest release tag `v0.0.14`. This phase changes
build, release, and deployment behavior rather than runtime API semantics.
Preserve local development workflows or document explicit exceptions when
production hardening requires different defaults.

Implementation milestones:

- Extend `make verify` and CI with practical security checks: `cargo audit`,
  `cargo deny check`, `cargo machete`, `cargo nextest run`, `trivy fs`,
  `trivy image`, `gitleaks detect`, and `semgrep scan`.
- Define failure policy for high/critical vulnerabilities and document allowed
  exceptions with owners and expiration dates.
- Generate SBOMs with Syft or an equivalent tool, scan SBOMs/images with Grype
  or Trivy, sign images with Cosign, and attach provenance or SLSA-style
  attestations where practical.
- Harden container and Kubernetes runtime settings with
  `readOnlyRootFilesystem: true`, `seccompProfile: RuntimeDefault`, explicit
  `runAsUser`, `runAsGroup`, `fsGroup`, `allowPrivilegeEscalation: false`, and
  dropped capabilities.
- Split public proxy and private control-plane services in production
  examples.
- Tighten NetworkPolicy ingress/egress and document internal ingress, VPN, or
  IAP-protected control-plane patterns.

Acceptance criteria:

- CI includes Rust dependency, image, secret, and static-analysis checks with
  documented exception handling.
- Release images have SBOM and signature artifacts.
- Kubernetes manifests follow restricted pod security best practices.
- Production NetworkPolicy examples no longer allow unrestricted ingress or
  egress.
- Deployment docs explain secure production defaults and local development
  exceptions.

Required verification:

- `node tests/freeze-v0.0.14-perimeter.test.mjs`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- `$code-change-verification`
- Run or dry-run each added scanner/signing/SBOM workflow locally or in CI, and
  capture expected failure modes for unavailable credentials.

Docs updates:

- Document security tooling, SBOM/signing workflow, provenance artifacts,
  Kubernetes hardening settings, NetworkPolicy intent, and local-dev
  exceptions.
