# Codex Execution Plans (ExecPlans)

This file defines how to write and maintain an ExecPlan: a self-contained,
living specification that a contributor can follow to deliver observable,
working behavior in this repository.

## When to Use an ExecPlan

Use an ExecPlan for multi-step or multi-file work, new features, refactors,
architecture changes, compatibility-sensitive behavior, or tasks expected to
take more than about an hour.

An ExecPlan is optional for small fixes, typos, narrow docs updates, or
single-file changes. If you skip it for substantial work, state why in your
handoff.

## How to Use This File

Read this file before drafting a plan. Start from the skeleton below and embed
all needed context: paths, commands, definitions, environment assumptions, and
acceptance criteria.

While implementing, move directly to the next milestone when possible. Keep the
living sections current at every stopping point so another contributor can
resume from the plan alone.

When scope changes, revise the affected plan sections instead of appending
contradictory notes.

## Non-Negotiable Requirements

- Self-contained and beginner-friendly. Define Relayna Gateway-specific terms
  such as virtual key, key policy, route policy, usage event, LiteLLM upstream,
  provider passthrough, Relayna runtime integration, rate limit, budget counter,
  and streaming proxy when they matter.
- Living document. Keep Progress, Surprises & Discoveries, Decision Log, and
  Outcomes & Retrospective updated as work proceeds.
- Outcome-focused. Describe what a client, operator, or Relayna Studio can do
  after the change and how to observe it.
- Explicit acceptance. State behaviors, commands, and observable outputs that
  prove success.
- Compatibility-aware. Record the compatibility boundary when the work touches
  public HTTP routes, response shapes, external config, PostgreSQL schemas,
  Redis state, provider credentials, usage event fields, streaming behavior, or
  Relayna runtime integration contracts.

## Formatting Rules

The default envelope is a single fenced code block labeled `md` when sharing an
ExecPlan in chat. Do not nest other triple-backtick fences inside it; indent
commands, transcripts, and diffs instead.

If a file contains only the ExecPlan, omit the enclosing code fence.

Use blank lines after headings. Prefer prose for plan narrative. Checklists are
permitted only in the Progress section, where they are required.

## Guidelines

Anchor the plan on observable outcomes. For internal changes, specify tests,
sample requests, logs, metrics, traces, database rows, Redis keys, or API
responses that demonstrate the behavior.

Name repository context explicitly: full paths, crates, modules, functions,
Makefile targets, working directories, required services, environment
variables, migrations, and provider test setup.

Keep milestones independently verifiable. Each milestone should advance the
goal, describe the expected result, and name proof that the result works.

Be idempotent and safe. Explain how to retry commands, handle partially applied
changes, roll back risky migrations, clear stale Redis counters, and recover
from interrupted test runs.

Validation is required. State exact commands and expected results. Prefer the
standard Rust workspace targets:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`

Use `$code-change-verification` for substantial gateway runtime, test,
migration, packaging, or build/test changes. Use `$implementation-strategy`
before editing compatibility-sensitive behavior.

## Living Sections

These sections must be present and maintained:

- Progress: checkbox list with timestamps. Every pause should update what is
  done and what remains.
- Surprises & Discoveries: unexpected behaviors, constraints, bugs, or useful
  evidence discovered during work.
- Decision Log: each meaningful decision, rationale, date, and author.
- Outcomes & Retrospective: what was achieved, remaining gaps, and lessons
  learned compared with the original purpose.

## Prototyping and Parallel Paths

Prototypes are allowed to reduce risk. Keep them additive, clearly labeled, and
validated. Remove or retire prototype code before completing the task unless it
is intentionally part of the final design.

Parallel implementations are acceptable when comparing approaches. Describe how
to validate each path and how to retire the losing path safely.

## ExecPlan Skeleton

```md
# <Short Plan Title>

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Explain the client-visible, operator-visible, or Relayna Studio-visible behavior
gained after this change and how to observe it.

## Progress

- [x] (2026-05-08 00:00Z) Example completed step.
- [ ] Example incomplete step.
- [ ] Example partially completed step. Completed: X. Remaining: Y.

## Surprises & Discoveries

- Observation: ...
  Evidence: ...

## Decision Log

- Decision: ...
  Rationale: ...
  Date/Author: ...

## Outcomes & Retrospective

Summarize outcomes, gaps, and lessons learned. Compare the result to the
original purpose.

## Context and Orientation

Describe the current state relevant to this task as if the reader knows nothing
about the codebase. Name key files and modules by full path. Define non-obvious
terms.

Include affected Relayna Gateway surfaces when relevant:

- Gateway API crate or module for Axum control-plane routes, middleware,
  errors, health, readiness, admin APIs, and graceful shutdown.
- Gateway core crate or module for authentication, policy, routing, rate limit,
  budget, usage, and pricing logic.
- Gateway proxy crate or module for Pingora services, LiteLLM, provider
  passthrough, streaming, and Relayna internal API calls.
- Gateway store crate or module for PostgreSQL, Redis, models, migrations, and
  transaction boundaries.
- Gateway telemetry crate or module for tracing, metrics, OpenTelemetry, and
  redaction.
- Tests under `tests/` or crate-local test modules.
- Build, packaging, and CI files such as `Cargo.toml`, `Cargo.lock`,
  `Makefile`, Dockerfiles, and `.github/workflows/`.

## Compatibility Boundary

State whether the change affects released behavior. If it does, identify the
latest release tag used for comparison and explain the compatibility strategy.

Examples:

- `Compatibility boundary: latest release tag v0.1.0; branch-local route rewrite, no shim needed.`
- `Compatibility boundary: released PostgreSQL usage_events schema; add forward migration and backward-safe reads.`
- `Compatibility boundary: public /v1/chat/completions response shape; additive headers only, existing body passthrough unchanged.`

## Plan of Work

Describe the sequence of edits and additions in prose. For each edit, name the
file, the area of the file, and what will change.

## Concrete Steps

List exact commands to run, including working directory and expected short
outputs when useful.

Examples:

    cd /Users/jobz/Works/relayna-gateway
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

## Validation and Acceptance

Describe behavioral acceptance criteria and the commands that prove them.

Include expected API responses, database rows, Redis keys, logs, metrics,
traces, screenshots, or test results when relevant.

## Idempotence and Recovery

Explain how to safely rerun steps. Describe how to recover from partial
application, failed migrations, stale local services, exhausted Redis counters,
or interrupted test runs.

## Artifacts and Notes

Include concise transcripts, diffs, sample payloads, or snippets as indented
examples.

## Interfaces and Dependencies

Prescribe crates, modules, function signatures, environment variables, service
dependencies, data formats, and API shapes that must exist at the end.
```

## Revising a Plan

When the scope shifts, rewrite affected sections so the document remains
coherent and self-contained. After significant edits, add a short note in the
Decision Log or Outcomes & Retrospective explaining what changed and why.
