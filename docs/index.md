# Relayna Gateway

Relayna Gateway is the Rust control plane and proxy for Relayna AI traffic. It gives external clients, SDKs, Studio, and Relayna workers a single governed API surface for model access instead of exposing provider credentials directly.

The gateway validates Relayna virtual keys, enforces policy, forwards OpenAI-compatible traffic to LiteLLM or direct providers, records usage events, and exposes an operator admin portal from the same binary.

## What Runs

- Proxy listener for OpenAI-compatible traffic, including `POST /v1/chat/completions` and `POST /v1/responses`.
- Registered service wildcard routes under `/services/<service-name>/*`, with per-service allowed method controls.
- Control listener for health, readiness, metrics, admin APIs, and `/admin-ui`.
- PostgreSQL-backed projects, virtual keys, route policies, service links,
  usage records, services, and operator tokens.
- Redis-backed rate limit and budget state.
- Embedded static admin UI for project-first key, service, usage, and health
  operations.

## Runtime Requirements

- PostgreSQL 14 or newer.
- Redis 6 or newer.
- LiteLLM or another OpenAI-compatible upstream.
- A secure operator workflow for the bootstrap token printed on first startup.

## Documentation Map

- [Architecture](architecture.md) explains the request path, crate ownership, data stores, and trust boundaries.
- [Getting Started](getting-started.md) covers local PostgreSQL, Redis, LiteLLM, and gateway startup.
- [Admin Portal](admin-portal.md) covers the embedded operator console.
- [Guardrails](guardrails.md) covers catalog setup, global runtime config, and per-key overrides.
- [Deployment](deployment.md) covers Docker and Kubernetes.
- [Operations](operations.md) covers readiness, logs, metrics, secrets, and releases.
