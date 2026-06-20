# Relayna Gateway

Relayna Gateway is the Rust control plane and proxy for Relayna AI traffic. It gives external clients, SDKs, Studio, and Relayna workers a single governed API surface for model access instead of exposing provider credentials directly.

The gateway validates Relayna virtual keys, enforces policy, forwards OpenAI-compatible traffic to LiteLLM or direct providers, records usage events, and exposes an operator admin portal from the same binary.

## What Runs

- Proxy listener for OpenAI-compatible traffic, including `POST /v1/chat/completions` and `POST /v1/responses`.
- Registered service wildcard routes under `/services/<service-name>/*`, with per-service allowed method controls.
- Control listener for health, readiness, metrics, and admin APIs under
  `/admin-ui/*`.
- PostgreSQL-backed projects, virtual keys, route policies, service links,
  usage records, services, and operator tokens.
- Redis-backed request rate-limit, token rate-limit, budget, and reservation
  state, with budget counters rehydrated from PostgreSQL usage records.
- Embedded Admin UI 2.0 for Monitor, Discover, and Govern workflows across
  project-first keys, services, usage, health, audit, and policy operations.

## Runtime Requirements

- PostgreSQL 14 or newer.
- Redis 6 or newer.
- LiteLLM or another OpenAI-compatible upstream.
- A secure operator workflow for the first bootstrap token, either seeded from
  `GATEWAY_ADMIN_TOKEN` on a fresh database or generated and printed once.

## Documentation Map

- [Architecture](architecture.md) explains the request path, crate ownership, data stores, and trust boundaries.
- [Database](database.md) documents the PostgreSQL schema, required records, keys, and operational data.
- [Redis Keys](redis.md) documents rate-limit, budget, and reservation keys used in Redis.
- [Getting Started](getting-started.md) covers local PostgreSQL, Redis, LiteLLM, and gateway startup.
- [Admin Portal](admin-portal.md) covers the embedded operator console.
- [Current Feature Highlights](current-features.md) summarizes the `v0.1.14`
  feature set, including Admin UI screenshots.
- [LiteLLM Passthrough](litellm-passthrough.md) covers wildcard passthrough,
  canonical route modes, credential translation, `/ui` exposure, and browser
  access constraints.
- [Entra ID Auth](entra-id-auth.md) explains the opt-in Microsoft Entra ID
  front-door authorization mode for provider traffic.
- [Apigee Gateway Path](apigee-gateway-path.md) explains the Apigee JWT
  revalidation and trusted signed-header patterns in front of Relayna Gateway.
- [Guardrails](guardrails.md) covers catalog setup, global runtime config, and per-key overrides.
- [Provider Intelligence](provider-intelligence.md) covers routing strategies,
  fallback, circuit breakers, debug bundles, and service import rollback.
- [Deployment](deployment.md) covers Docker and Kubernetes.
- [Operations](operations.md) covers readiness, logs, metrics, secrets, and releases.
