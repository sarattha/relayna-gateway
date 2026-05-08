---
name: implementation-strategy
description: Decide how to implement Relayna Gateway runtime, API, configuration, schema, and behavior changes before editing code. Use when a task changes gateway public behavior and you need to choose the compatibility boundary, whether shims or migrations are warranted, and when unreleased interfaces can be rewritten directly.
---

# Implementation Strategy

## Overview

Use this skill before editing code when a task changes runtime behavior or
anything that may affect compatibility. The goal is to keep implementations
simple while protecting real released contracts.

Relayna Gateway compatibility surfaces include:

- Public HTTP routes, status codes, response bodies, and headers.
- Virtual key format, key hashing, authentication behavior, and key context.
- Route/model/provider policy decisions, streaming permission, tool permission,
  and task execution permission.
- LiteLLM and direct provider proxy behavior, including sensitive header
  stripping and upstream credential handling.
- Streaming passthrough, buffering behavior, cancellation, and first-token
  latency telemetry.
- PostgreSQL schemas, migrations, indexes, and usage event shapes.
- Redis rate-limit keys, budget counters, TTLs, and value formats.
- Environment variables, config files, Docker/Kubernetes deployment behavior,
  and secret sources.
- Relayna runtime integration APIs and async task submission contracts.
- Telemetry fields, metric labels, trace attributes, and log redaction behavior.

## Workflow

1. Identify the touched surface.
   - Runtime code, public API, wire protocols, route responses, persisted
     PostgreSQL data, Redis key/value formats, provider proxy behavior,
     streaming behavior, config, telemetry, or Relayna runtime integration.
   - Build/test tooling, docs, or examples that describe behavior.

2. Establish the compatibility boundary.
   - Prefer the latest release tag as the released gateway boundary.
   - Check tags with `git tag -l 'v*' --sort=-v:refname | head -n1`.
   - If needed, compare with `git show <tag>:<path>` or
     `git diff <tag>...HEAD -- <path>`.
   - Treat current branch churn after the latest release as unreleased unless it
     represents an explicitly supported durable external state boundary.

3. Decide whether compatibility is required.
   - Required when changing released public routes, response shapes, virtual key
     behavior, environment variables, PostgreSQL schemas, Redis key/value
     formats, provider routing, streaming semantics, usage event fields, or
     Relayna runtime contracts.
   - Usually not required for branch-local interfaces introduced after the
     latest release tag, internal helper refactors, or docs-only corrections.

4. Choose the implementation shape.
   - For unreleased interfaces, prefer direct replacement over aliases, shims,
     feature flags, dual paths, or migrations.
   - For released compatibility boundaries, preserve old behavior when feasible
     and add tests that cover both old and new behavior.
   - For persisted data or serialized state, add explicit migration or
     backward-read behavior and tests before changing the writer shape.
   - For public routes or wire protocols, prefer additive changes when clients
     may depend on existing fields.
   - For secrets, prefer fail-closed behavior and redaction over compatibility
     with unsafe logging or passthrough.

5. Plan verification.
   - Gateway changes: run `$code-change-verification` or at minimum the Rust
     workspace stack.
   - Migration changes: include migration apply/rollback or idempotence
     evidence when tooling exists.
   - Proxy/streaming changes: include tests or manual verification that
     credentials are stripped and streamed responses are not fully buffered.

## Default Implementation Stance

- Keep the patch scoped to the established crate or module boundary.
- Prefer deleting or replacing unreleased abstractions instead of preserving
  confusing branch-local shapes.
- Do not add compatibility shims unless the old behavior is released,
  persisted, documented, or explicitly requested.
- If review feedback claims a change is breaking, verify it against the latest
  release tag and actual external impact before accepting the feedback.
- If a change truly crosses a released contract boundary, call that out in the
  plan, implementation notes, tests, and final summary.

## When to Stop and Confirm

Stop and confirm the approach with the user when:

- The change would alter behavior shipped in the latest release tag.
- The change would modify durable external data, protocol formats, or serialized
  state.
- The user explicitly asked for backward compatibility, deprecation, or
  migration support.
- There are two plausible implementation paths with meaningfully different API
  or migration costs.
- The safest security behavior would break an existing caller.

## Output Expectations

When this skill materially affects the implementation approach, state the
decision briefly in the plan or handoff, for example:

- `Compatibility boundary: latest release tag v0.1.0; branch-local route rewrite, no shim needed.`
- `Compatibility boundary: released PostgreSQL usage_events schema; preserve backward reads and add migration coverage.`
- `Compatibility boundary: public /v1/chat/completions response shape; additive headers only, body passthrough unchanged.`
