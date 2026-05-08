# Contributing

Thanks for working on Relayna Gateway. This repository is being built around
the Rust gateway design in `internal/design-manifesto.md`; read that file
before changing behavior, architecture, APIs, configuration, storage, or
operational workflows.

## Local setup

Install the Rust stable toolchain with `rustfmt` and `clippy`:

```bash
rustup toolchain install stable
rustup component add rustfmt clippy
```

Gateway development commonly needs PostgreSQL, Redis, and a LiteLLM or
OpenAI-compatible upstream. Do not commit provider keys, LiteLLM master keys,
internal service tokens, raw Relayna virtual keys, or prompt payloads.

## Common commands

Once `Cargo.toml` exists, run the standard Rust checks from the repository root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The root Makefile wraps those commands:

```bash
make check
make format
make lint
make test
```

Before the first Rust workspace is added, the Makefile reports that Cargo checks
are skipped. Do not rely on that skip for runtime, test, migration, packaging,
or build changes once gateway code exists.

## Development workflow

1. Keep changes focused and aligned with the manifesto phase being implemented.
2. Use the crate/module ownership described in `AGENTS.md`.
3. Add or update tests when behavior changes.
4. Update docs when public routes, configuration, policy, usage, deployment, or
   operational behavior changes.
5. Run the relevant verification commands before opening a PR.

Use an ExecPlan, following `PLANS.md`, for multi-step features, refactors,
architecture changes, compatibility-sensitive changes, or work likely to take
more than about an hour.

## Compatibility

Treat these surfaces as compatibility-sensitive:

- Public HTTP routes, response shapes, status codes, and headers.
- Virtual key format, authentication behavior, and secret handling.
- Policy decisions for routes, models, providers, streaming, tools, and task
  execution.
- PostgreSQL schemas, migrations, usage event fields, and query behavior.
- Redis keys, counters, TTLs, rate limits, and budget state.
- LiteLLM/provider proxy semantics and streaming behavior.
- Relayna runtime integration contracts.
- Environment variables, deployment configuration, and telemetry fields.

Use the repository `implementation-strategy` skill before editing these
surfaces. Prefer direct replacement for unreleased branch-local interfaces, but
preserve compatibility or add migration coverage for released external
contracts and durable state.

## Pull requests

- Use `.github/PULL_REQUEST_TEMPLATE/pull_request_template.md`.
- Include what changed, why it changed, compatibility or migration notes, and
  the commands or manual checks you ran.
- Keep provider credentials, raw virtual keys, prompts, and internal service
  tokens out of PR descriptions, logs, screenshots, and test fixtures.
- Call out any residual risk around streaming, buffering, cancellation,
  metering accuracy, migration rollout, or secret redaction.
