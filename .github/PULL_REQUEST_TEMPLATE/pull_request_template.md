## Summary

<!-- Briefly describe what changed and why. -->

## Type of change

<!-- Mark the relevant items with "x". -->

- [ ] Feature
- [ ] Bug fix
- [ ] Refactor
- [ ] Tests
- [ ] Documentation
- [ ] Tooling / CI
- [ ] Release / packaging

## Affected areas

<!-- Mark the relevant items with "x". -->

- [ ] Gateway API / Axum control routes
- [ ] Gateway proxy / Pingora services
- [ ] Authentication / virtual keys
- [ ] Policy / route or model access
- [ ] Proxy / LiteLLM / provider passthrough
- [ ] Streaming behavior
- [ ] Usage tracking / pricing
- [ ] Rate limits / budgets
- [ ] PostgreSQL schema / migrations
- [ ] Redis counters / state
- [ ] Telemetry / logs / metrics / traces
- [ ] Relayna runtime integration
- [ ] Docs (`docs`, README, changelog, manifesto)
- [ ] Build, packaging, Docker, or CI

## Compatibility and migration notes

<!--
Call out changes to public routes, response shapes, virtual key format,
authentication behavior, policy decisions, usage event fields, PostgreSQL
schemas, Redis keys, environment variables, provider routing, streaming
behavior, or Relayna runtime integration contracts.
-->

- Compatibility impact: <!-- None / additive / migration required / breaking -->
- Migration required: <!-- No / Yes, describe below -->

## Security and secret handling

<!--
Confirm provider keys, LiteLLM master keys, LiteLLM virtual keys, internal
service tokens, and raw Relayna virtual keys are not exposed, persisted, or
logged.
-->

- [ ] Client credentials are stripped before upstream calls when applicable.
- [ ] Internal provider credentials are redacted from logs and responses.
- [ ] Raw virtual keys are returned only when intentionally created and are
      never stored.
- [ ] Prompt/body logging is avoided or explicitly justified.

## Test plan

<!-- List the commands run and any manual verification. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Manual API/proxy verification:
- [ ] Migration verification:
- [ ] Other:

## Reviewer notes

<!-- Mention focused review areas, known limitations, follow-ups, or risks. -->

## Linked issues

<!-- Example: Fixes #123 -->
