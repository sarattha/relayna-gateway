# Relayna Gateway Contributor Guide

Relayna Gateway is the Rust control plane for AI traffic: authentication,
policy, provider routing, metering, and Relayna runtime integration. Read
`docs/architecture.md` and the relevant feature documentation before changing
behavior or architecture.

## Working rules

- Preserve unrelated local changes. Read affected files before editing and keep
  each change focused on the requested outcome.
- Keep gateway-core framework-agnostic. Pingora owns proxy traffic; Axum owns
  control-plane APIs.
- Add or update tests for behavior changes and update the corresponding public
  documentation and examples.
- Keep provider keys, LiteLLM credentials, internal service tokens, raw virtual
  keys, and prompts out of logs, reports, screenshots, and committed fixtures.
- Keep generated reports and screenshots under ignored `target/` or `output/`
  directories. Retain release evidence outside the source tree and link it from
  the versioned benchmark documentation.

## Required skills

- Use `$karpathy` before writing, reviewing, or refactoring code. Keep assumptions
  explicit, edits small, and verification tied to the request.
- Use `$implementation-strategy` before changing runtime behavior, public APIs,
  configuration, schemas, authentication, policy, usage, rate limits, budgets,
  streaming, or provider/Relayna integration contracts.
- Run `$code-change-verification` for Rust code, tests, integration fixtures,
  migrations, packaging, or build/test configuration changes. Documentation and
  repository metadata alone do not require the Rust stack.
- Use `$pr-draft-summary` after substantive code, test, tooling, or behavior
  documentation changes. Skip it for metadata/docs-only changes without behavior
  impact, or when the user asks to omit it.

## Package ownership

- `crates/gateway-api/`: Axum routing, middleware, errors, request IDs,
  health/readiness, admin APIs, and graceful shutdown.
- `crates/gateway-core/`: identity, policy, route resolution, rate limits,
  budgets, usage construction, and pricing.
- `crates/gateway-proxy/`: Pingora services, upstream requests, credential
  stripping, streaming, cancellation, and provider errors.
- `crates/gateway-store/`: PostgreSQL schemas and migrations, Redis counters,
  persistence, and transaction boundaries.
- `crates/gateway-telemetry/`: logs, traces, metrics, redaction, and correlation.
- `tests/`: integration tests, browser/static UI checks, and reusable E2E
  harnesses under `tests/e2e/`.
- `benches/body-admission/`: release benchmark harness and methodology.
- `docs/`: public architecture, configuration, operations, and release docs.
- `.codex/`: repository skills and verification scripts.

## Compatibility

Judge compatibility against the latest release tag, not unreleased branch
changes. Prefer direct replacement for unreleased interfaces unless they define
an explicitly supported durable external state boundary or the user requests a
migration path.

Preserve released contracts or provide migration coverage for:

- HTTP routes, status codes, response shapes, and correlation headers.
- Virtual key formats, authentication, and policy decisions.
- Environment variables and deployment configuration.
- PostgreSQL schemas and usage event fields consumed by Relayna Studio.
- Redis keys, counters, TTLs, rate limits, and budget state.
- Provider routing, LiteLLM passthrough, and credential handling.
- Relayna runtime APIs and task submission contracts.
- Streaming and cancellation behavior.

## Admin UI

Read `SKILLS.md` and apply the Admin UI 4.0 Design System before changing UI
source, generated assets, UI tests, or operator-facing UI documentation.

The Vite/TypeScript package at `crates/gateway-api/admin-ui/` is the source of
truth. Regenerate checked-in assets at
`crates/gateway-api/src/static/admin-ui/` with `npm run build:admin-ui`; do not
edit them by hand. Preserve `/admin-ui`, `/admin-ui/app.js`, and
`/admin-ui/app.css` unless compatibility review explicitly changes them.

Run `npm run build:admin-ui` and `npm test` after UI source changes, plus the
Rust verification stack when required by the touched areas.

## Verification

Use `bash .codex/skills/code-change-verification/scripts/run.sh` for the required
verification stack. It runs fail-fast; fix failures and rerun it so required
checks pass in sequence. The Rust checks include:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Use focused tests while iterating. Before pushing or opening a PR, finish the
required checks for the affected areas. `make metadata-check` validates
repository metadata; `make docs-check` builds public documentation strictly.

Authentication, policy, usage, and proxy tests should cover rejected invalid,
expired, and disabled keys; credential stripping and redaction; stable denial
responses; usage on success and failure; and streaming without buffering whole
responses.

## Release benchmarks

Starting with **v0.1.49**, every version bump requires fresh body-admission
benchmarks and versioned results, including patch, security, docs, and tooling
releases. Do not mark a release ready, merge its version bump, tag it, or publish
it while execution or documentation is incomplete.

- Follow `benches/body-admission/README.md` using `scripts/bench-body-admission`.
  Run the full adaptive search for **5, 10, 20, 40, 80, and 100 MiB JSON**, with
  at least **three fresh-pod trials per confirmed setting**, two Gateway pods,
  and per-pod limits of **2 CPUs / 1 GiB memory / 512 MiB serialized-body
  budget**. Smoke/pilot runs do not satisfy this requirement.
- Measure the committed final candidate after version/runtime changes, using
  `--build` or an immutable `--image`. Record the runtime commit, resolved image
  ID/digest, and harness provenance. Runtime/build input changes require a new
  run; later results-only or documentation-only commits may follow separately.
- Run natively on amd64 or arm64, including Apple Silicon. Host, Docker, image,
  and kind node architectures must match. Record host/CPU model (or explicitly
  unavailable), Docker resources, architecture, hardware, and offered cohort.
- Save local results under `target/release-benchmarks/vX.Y.Z`. Publish raw
  observations, checkpoints, HTML/JSON/CSV reports, failed trials, provenance,
  and checksums as durable version-attributable release assets or external
  artifacts. Keep credentials and kubeconfigs out. Preserve earlier evidence;
  historical reports remain accessible through pinned Git revisions.
- Add `docs/benchmarks/vX.Y.Z.md`; link it from the benchmark index, MkDocs
  navigation, release documentation, changelog, and release notes. Link the
  interactive HTML and raw JSON/CSV evidence. Include commands/date, exact
  runtime/harness identity, architecture/hardware, caps, sizes, offered cohort,
  repetitions, slots, peak RSS/cgroup memory and headroom, success latency
  p50/p95/p99, rejection latency, elapsed time, verified successes, 503s,
  transport errors, OOMs, and restarts. State missing/censored measurements.
- Compare preceding versions only when conditions are comparable; explain
  regressions and hardware/cohort/method differences. Distinguish repeated
  adjacent resource edges from finite lower bounds or incomplete searches.
  Observed edges need production headroom; expected admission 503s and
  intentional resource-boundary OOMs remain results. Retain failure reports,
  fix invalid/non-resource failures, and rerun into a new directory.

The immutable baseline is documented in `docs/benchmarks/v0.1.49.md`. Older
releases do not require retrospective reruns; earlier-version measurements do
not substitute for a new version's run.

## Pull requests

Use `.github/PULL_REQUEST_TEMPLATE/pull_request_template.md`. Explain the
problem, resulting behavior, verification, compatibility/migration notes, and
residual risks. Keep commits focused with concise imperative messages;
conventional prefixes are welcome. Do not push or open a PR before the relevant
checks pass.
