# relayna-gateway

Relayna Gateway is the planned Rust-based public entry point for Relayna AI
traffic. It sits in front of LLM providers, LiteLLM, Relayna internal APIs, and
Relayna task execution so clients use one governed API surface instead of
direct provider credentials.

The design source of truth for the MVP is
[`internal/design-manifesto.md`](internal/design-manifesto.md).

## Mission

Relayna Gateway is not only a reverse proxy. It is the governance, metering,
and routing layer for AI traffic.

It owns:

- Relayna virtual key authentication.
- Provider and model routing.
- Route, model, streaming, tool, and task policy checks.
- Rate limiting and budget enforcement.
- Usage event recording for Relayna Studio and operators.
- Async task submission into the Relayna runtime.

Relayna itself remains the task execution runtime. The gateway is the public
control plane.

## MVP Target

Phase 1 proves the core proxy path:

- Accept `Authorization: Bearer rk_live_xxx` Relayna virtual keys.
- Validate keys against PostgreSQL.
- Forward `POST /v1/chat/completions` to LiteLLM with internal credentials.
- Preserve request correlation headers.
- Record success and failure usage events.
- Avoid logging prompts or exposing provider, LiteLLM, or internal service
  secrets to clients.

## Architecture Principles

- Gateway owns identity: clients only see Relayna virtual keys.
- Gateway owns policy: access decisions happen before provider calls.
- Gateway owns usage tracking: every request produces a queryable usage event.
- Gateway streams instead of buffering large LLM request or response bodies.
- Gateway and Relayna runtime integrate cleanly: external clients call the
  gateway, and Relayna workers use the gateway for metered provider access.

## Recommended Stack

- Rust workspace with `pingora` for proxy traffic and `axum` for control APIs.
- `tokio`, `tower`, and `tower-http` where they fit the control-plane API.
- HTTP proxying through Pingora upstreams, with `reqwest` or `hyper` reserved
  for non-proxy service calls.
- PostgreSQL persistence with `sqlx`.
- Redis counters and budget cache.
- `serde` / `serde_json` for request and response handling.
- `argon2` for virtual key secret hashing.
- `tracing`, Prometheus metrics, and OpenTelemetry.
- `thiserror` / `anyhow` for error handling.

## Expected Repository Shape

The MVP may start as one crate, but new code should preserve a path to this
workspace split:

```text
crates/
  gateway-api/
  gateway-core/
  gateway-proxy/
  gateway-store/
  gateway-telemetry/
```

Keep control-plane decisions in core/store modules and keep Pingora provider
proxying isolated from authentication, policy, and accounting logic.

## Local Development

This repository is being aligned around the gateway design. Once `Cargo.toml`
exists, use the standard Rust workflow:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

On startup, the gateway runs the bundled PostgreSQL migrations before
bootstrapping operator tokens or serving traffic. This keeps virtual keys,
policies, usage events, service registrations, and operator tokens
database-backed from a fresh database.

## Required Runtime Services

Gateway development commonly depends on:

- PostgreSQL for virtual keys, route policies, usage events, service
  registrations, and operator tokens.
- Redis for rate limiting and budget counters.
- LiteLLM or another OpenAI-compatible upstream for proxy tests.

Never commit provider keys, LiteLLM master keys, internal service tokens, or raw
Relayna virtual keys.

## Documentation

- Design manifesto: [`internal/design-manifesto.md`](internal/design-manifesto.md)
- Internal MVP phase roadmap: [`internal/mvp-phase-roadmap.md`](internal/mvp-phase-roadmap.md)
- Contributor and agent guide: [`AGENTS.md`](AGENTS.md)
- Execution plan rules: [`PLANS.md`](PLANS.md)
