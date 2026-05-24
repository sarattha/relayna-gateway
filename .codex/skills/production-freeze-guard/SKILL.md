---
name: production-freeze-guard
description: Use after the Relayna Gateway v0.1.0 production freeze when adding features or changing public routes, APIs, schemas, config, auth, policy, usage, proxy, streaming, Redis, telemetry, admin UI, or release behavior; requires compatibility review and freeze perimeter tests.
---

# Production Freeze Guard

## Purpose

Relayna Gateway v0.1.0 is the production freeze baseline. Use this skill before
adding features or changing behavior that could affect deployed clients,
operators, Relayna Studio, Relayna workers, stored data, Redis state, proxy
semantics, or deployment configuration.

This skill does not block feature work. It makes compatibility explicit and
keeps the v0.1.0 perimeter tests honest.

## Workflow

1. Establish the baseline.
   - Run `git tag -l 'v*' --sort=-v:refname | head -n1`.
   - The expected production freeze baseline is `v0.1.0`.
   - Compare risky changes with `git diff v0.1.0...HEAD -- <path>` when needed.

2. Identify touched freeze surfaces.
   - Public HTTP routes, status codes, response bodies, and headers.
   - Virtual key format, key hashing, authentication, and operator tokens.
   - Policy decisions for routes, models, providers, streaming, tools, tasks,
     guardrails, rate limits, and budgets.
   - LiteLLM, direct provider, internal service, and Relayna runtime proxy
     behavior.
   - Streaming passthrough, cancellation, buffering, and first-token lifecycle.
   - PostgreSQL migrations, tables, columns, indexes, and usage event fields.
   - Redis key patterns, counters, TTLs, budget reservations, and value formats.
   - Environment variables, defaults, Docker, release metadata, and CI.
   - Telemetry fields, metric labels, trace attributes, and log redaction.
   - Admin API and admin UI endpoint contracts.

3. Use `$implementation-strategy` before editing compatibility-sensitive
   behavior.
   - Prefer additive changes for released public surfaces.
   - Do not remove, rename, or change meaning without an explicit migration or
     compatibility decision.
   - Secret handling may break unsafe behavior only when the PR states why.

4. Update the freeze perimeter tests.
   - Run `node tests/freeze-v0.1.0-perimeter.test.mjs` before editing to see
     the current boundary.
   - If behavior is intentionally additive, update the test in the same PR and
     record the compatibility reason.
   - If a test fails unexpectedly, treat it as a breaking-change signal until
     proven otherwise.

5. Verify before handoff.
   - Run `node tests/freeze-v0.1.0-perimeter.test.mjs`.
   - Run `$code-change-verification` for Rust runtime, tests, migrations,
     packaging, or build/test changes.
   - Include compatibility notes and changed perimeter tests in the final
     handoff.

## Output Expectations

In plans, PR summaries, and handoffs, include:

- Freeze baseline: `v0.1.0`.
- Touched freeze surfaces.
- Compatibility impact: none, additive, migration required, or breaking.
- Perimeter tests added or updated.
- Verification commands and results.
