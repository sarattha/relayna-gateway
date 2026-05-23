# Contributor Guide

This guide helps agents and contributors work in the Relayna Gateway
repository. It covers the project layout, local workflows, and checks that must
run before pushing a branch or opening a pull request.

Location: `AGENTS.md` at the repository root.

Read `internal/design-manifesto.md` before making product, architecture,
runtime, API, policy, usage, or operational changes. That file is the current
MVP design source of truth.

## Policies & Mandatory Rules

### Mandatory Skill Usage

#### `$karpathy`

Use `$karpathy` when writing, reviewing, or refactoring code in this repository.
It keeps work assumption-aware, simple, surgically scoped, and tied to explicit
verification criteria.

Apply it before implementation work so the plan, edits, and final handoff stay
focused on the user's request. It does not replace `$implementation-strategy`,
`$production-freeze-guard`, `$code-change-verification`, or
`$pr-draft-summary`; use those skills when their trigger conditions apply.

#### `$code-change-verification`

Run `$code-change-verification` before marking work complete when changes affect
Rust runtime code, tests, migrations, packaging, or build/test behavior.

Run it when you change:

- `crates/` or any gateway Rust source.
- `tests/`, `benches/`, or integration fixtures.
- Database migrations, SQLx metadata, seed data, or schema files.
- Build or test configuration such as `Cargo.toml`, `Cargo.lock`, `Makefile`,
  `.cargo/`, Dockerfiles, or CI workflows.

You can skip `$code-change-verification` for docs-only or repo-meta changes
such as `docs/`, `README.md`, `CHANGELOG.md`, `AGENTS.md`, `PLANS.md`,
`.codex/`, or `.github/`, unless a user explicitly asks to run the full
verification stack.

#### `$implementation-strategy`

Before changing runtime code, exported APIs, external configuration, persisted
schemas, wire protocols, route response shapes, authentication behavior,
policy decisions, usage event shapes, rate limit behavior, budget behavior, or
Relayna runtime integration contracts, use `$implementation-strategy` to decide
the compatibility boundary and implementation shape.

Judge compatibility against the latest release tag, not unreleased branch-local
churn. Interfaces introduced or changed after the latest release tag may be
rewritten directly unless they define a released or explicitly supported durable
external state boundary, or the user explicitly asks for a migration path.

#### `$production-freeze-guard`

Relayna Gateway v0.0.14 is the production freeze baseline. Use
`$production-freeze-guard` before adding features or changing public routes,
exported APIs, external configuration, persisted schemas, Redis key/value
formats, authentication behavior, policy decisions, usage event shapes,
provider routing, streaming behavior, telemetry fields, admin UI contracts,
release metadata, or CI/build behavior.

The freeze gate is test-based: future features are allowed only when they keep
the v0.0.14 perimeter tests passing, or when the same change intentionally
updates those tests with compatibility notes. Run:

```bash
node tests/freeze-v0.0.14-perimeter.test.mjs
```

Use `$implementation-strategy` as part of the freeze guard workflow when the
change touches compatibility-sensitive behavior. Use `$code-change-verification`
before marking the work complete when the change affects Rust runtime code,
tests, migrations, packaging, or build/test behavior.

#### `$pr-draft-summary`

When a task finishes with moderate-or-larger changes, invoke
`$pr-draft-summary` in the final handoff to generate the required PR summary
block, branch suggestion, title, and draft description.

Use this by default after runtime code, tests, gateway behavior, build/test
configuration, or docs with behavior impact are changed. Skip it only for
trivial conversation-only work, repo-meta/doc-only tasks without behavior
impact, or when the user explicitly says not to include the PR draft block.

### Admin UI 2.0 Design System

Before changing the Admin UI, read `SKILLS.md` and apply the Admin UI 2.0
Design System guidance. This applies to changes under
`crates/gateway-api/admin-ui/`, generated static assets under
`crates/gateway-api/src/static/admin-ui/`, admin UI tests, and operator-facing
Admin UI documentation.

Keep the Vite/TypeScript source package as the source of truth and regenerate
the checked-in static assets with `npm run build:admin-ui`. Preserve the
existing `/admin-ui`, `/admin-ui/app.js`, and `/admin-ui/app.css` asset
contract unless a compatibility review explicitly changes it.

### ExecPlans

Use an ExecPlan when work is multi-step, spans several files, introduces a new
feature, performs a refactor, changes gateway architecture, affects
compatibility-sensitive behavior, or is likely to take more than about an hour.

Start with the template and rules in `PLANS.md`. Keep the plan self-contained
and update the living sections as work proceeds:

- Progress
- Surprises & Discoveries
- Decision Log
- Outcomes & Retrospective

Call out compatibility risk early only when the change affects behavior shipped
in the latest release tag or a released or explicitly supported durable
external state boundary. Do not treat branch-local interface churn or unreleased
post-tag changes as breaking by default; prefer direct replacement over
compatibility layers in those cases.

If the plan changes virtual key formats, route response shapes, PostgreSQL
schemas, Redis key/value formats, streamed response behavior, provider
credential handling, usage event shapes, admin API contracts, or Relayna runtime
integration contracts, use `$implementation-strategy` before editing code and
record the decision in the ExecPlan.

If you intentionally skip an ExecPlan for complex work, note why in your
response so reviewers understand the choice.

### Preserve User Work

The working tree may contain local changes that you did not make. Do not
revert, overwrite, or reformat unrelated changes. Read the affected files first
and make the smallest change that satisfies the task.

### Pre-Push and PR Checks

Before pushing a branch or creating a pull request, run formatting, linting, and
tests for every Rust area touched by the change.

For gateway Rust changes, run these commands from the repository root once the
workspace exists:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If the repository provides Makefile wrappers, prefer those wrappers only when
they execute the same checks or a documented superset.

The `$code-change-verification` script runs the gateway verification stack in
fail-fast order. If a command fails, fix the issue and rerun the script so every
required command passes in sequence.

### Tests

Add or update tests when changing behavior. Use focused commands while
iterating, but finish with the relevant workspace command when practical:

```bash
cargo test --workspace --all-features
```

For policy, authentication, usage, and proxy behavior, favor tests that prove:

- Invalid, expired, and disabled virtual keys are rejected.
- Client credentials are stripped before upstream provider calls.
- Internal provider credentials are never returned or logged.
- Policy denials use stable status codes and error shapes.
- Usage events are inserted for both success and failure.
- Streaming paths do not buffer complete responses.
- Production freeze perimeter tests continue to pin v0.0.14 routes, error codes,
  config names, migrations, Redis key formats, release metadata, and admin UI
  endpoint assumptions.

### Compatibility

Relayna Gateway is a public control plane for AI traffic. Treat these surfaces
as compatibility-sensitive:

- Public HTTP routes and response shapes.
- Virtual key format and authentication behavior.
- Environment variables and deployment configuration.
- PostgreSQL schemas and migrations.
- Redis key patterns, counters, TTLs, and budget state.
- Usage event fields consumed by Relayna Studio.
- Provider routing and LiteLLM passthrough semantics.
- Relayna runtime integration APIs and task submission contracts.
- Streaming behavior, cancellation handling, and correlation headers.

Use `$implementation-strategy` before editing compatibility-sensitive behavior.
Prefer direct replacement for unreleased branch-local interfaces, and preserve
compatibility or add migration coverage when a change crosses a released API,
persisted data, or wire-protocol boundary.

For post-freeze changes, also use `$production-freeze-guard` and compare impact
against v0.0.14. Do not remove, rename, or change the meaning of a frozen surface
without an explicit compatibility decision and matching perimeter test update.

## Project Structure Guide

### Overview

`relayna-gateway` is the Rust gateway and control plane for Relayna AI traffic.
It validates Relayna virtual keys, enforces policy, forwards OpenAI-compatible
requests to LiteLLM or direct providers, records usage, and integrates with the
Relayna task runtime.

Relayna itself remains the runtime/task execution layer. Relayna Gateway is the
single public entry point for external clients, SDKs, Studio, and Relayna
workers that need metered provider access.

### Important Paths

- `internal/design-manifesto.md`: MVP mission, architecture principles, and
  phase checklist.
- `SKILLS.md`: repository-local UI skill guidance, including the Admin UI 2.0
  Design System rules for future frontend work.
- `crates/gateway-api/`: Axum control API routes, middleware, errors, request
  IDs, health/readiness, admin APIs, and graceful shutdown.
- `crates/gateway-api/admin-ui/`: Vite/TypeScript Admin UI 2.0 source package.
- `crates/gateway-core/`: Authentication, policy, routing, rate limits,
  budgets, usage, and pricing logic.
- `crates/gateway-proxy/`: Pingora proxy services for LiteLLM, direct provider,
  streaming, and internal Relayna proxy adapters.
- `crates/gateway-store/`: PostgreSQL, Redis, models, migrations, and schema
  access.
- `crates/gateway-telemetry/`: tracing, metrics, OpenTelemetry, and log
  redaction helpers.
- `tests/`: Integration tests and black-box gateway behavior tests.
- `.github/`: CI, issue templates, PR templates, and Codex CI prompts.
- `.codex/`: Repository-specific Codex skills and verification scripts.

The initial MVP can start as one crate, but new code should keep module
boundaries compatible with the split above.

### Package Ownership

- Gateway API owns axum control-plane routing, middleware ordering, structured
  errors, request IDs, readiness, admin APIs, and graceful shutdown.
- Gateway core owns identity, policy decisions, route resolution, budget/rate
  limit checks, usage event construction, and pricing.
- Gateway proxy owns Pingora proxy services, upstream request construction,
  sensitive header stripping, LiteLLM credentials, streaming passthrough,
  cancellation, and provider errors.
- Gateway store owns durable schemas, migrations, key lookup, usage inserts,
  Redis counters, and transaction boundaries.
- Gateway telemetry owns structured logs, traces, metrics, redaction, and
  correlation fields.

## Operation Guide

### Prerequisites

- Rust stable toolchain.
- `cargo`, `rustfmt`, and `clippy`.
- PostgreSQL for key, policy, and usage persistence.
- Redis for rate limit and budget counters.
- LiteLLM or an OpenAI-compatible upstream for proxy behavior.
- `make` only when the repository provides wrappers for the same commands.

### Development Workflow

1. Create a focused branch for the change.
2. Read `internal/design-manifesto.md` and identify the affected phase.
3. Sync or prepare local services when database, Redis, or provider behavior is
   involved.
4. Implement the change using the package ownership boundaries above.
5. Add or update tests for behavioral changes.
6. Run the relevant tests and mandatory pre-push/PR checks.
7. Keep commits small and use concise, imperative commit messages.
8. When reporting substantial work as complete, use `$pr-draft-summary` unless
   the documented skip cases apply.

### Common Commands

Rust workspace:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Useful focused commands:

```bash
cargo test -p gateway-core
cargo test -p gateway-proxy
cargo test --test <integration_test_name>
```

Docs and metadata changes usually do not require the full Rust verification
stack unless they also change behavior, build/test configuration, or CI.

### Pull Request & Commit Guidelines

- Use the template at `.github/PULL_REQUEST_TEMPLATE/pull_request_template.md`.
- Include a concise summary, test plan, and linked issue when applicable.
- Keep pull requests focused on one behavior change, fix, or documentation
  update.
- Use concise, imperative commit messages. Conventional prefixes such as
  `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, and `chore:` are preferred
  when they clarify the change.
- Add or update tests for behavior changes when feasible.
- Update docs for user-facing gateway, configuration, policy, usage,
  deployment, or operational changes.
- Mention compatibility or migration considerations when public routes,
  persisted data, response shapes, usage events, config, Redis state, or
  provider/Relayna integration contracts change.
- Run the relevant formatting, linting, and tests before pushing or opening the
  PR.
- Use `$pr-draft-summary` after substantial code work to prepare a branch
  suggestion, PR title, and draft description.

### Review Process & What Reviewers Look For

- Checks pass for the affected workspace.
- Tests cover new behavior, bug fixes, and compatibility boundaries.
- Code follows package ownership and avoids unrelated refactors.
- Public routes, response shapes, persisted data, and wire protocols preserve
  compatibility unless the PR clearly explains the breaking change.
- Secret handling prevents client access to provider keys, LiteLLM master keys,
  LiteLLM virtual keys, and internal service tokens.
- Error handling, retries, timeouts, cancellation, and async lifecycle behavior
  are explicit where relevant.
- Redis counters, budget keys, TTLs, and rate-limit semantics are intentional
  and documented when user-visible.
- PostgreSQL migrations are reversible or have clear rollout notes.
- Observability changes avoid high-cardinality metrics labels and preserve
  useful task, key, project, route, provider, and request diagnostics.
- Streaming proxy paths do not buffer complete LLM responses.
- Documentation and examples match the implemented behavior.
- The PR description states what changed, why, how it was verified, and any
  residual risk reviewers should consider.
