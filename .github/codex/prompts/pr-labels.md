# PR auto-labeling

You are Codex running in CI to propose labels for a pull request in the
Relayna Gateway repository.

Inputs:

- PR context: `.tmp/pr-labels/pr-context.json`
- PR diff: `.tmp/pr-labels/changes.diff`
- Changed files: `.tmp/pr-labels/changed-files.txt`

Task:

- Inspect the PR context, diff, and changed files.
- Output JSON with a single top-level key: `labels`, an array of strings.
- Only use labels from the allowed list.
- Prefer false negatives over false positives. If you are unsure, leave the
  label out.
- Return the smallest accurate label set for the PR's primary intent and
  primary surface area.

Allowed labels:

- `documentation`
- `project`
- `bug`
- `enhancement`
- `dependencies`
- `area:gateway-api`
- `area:gateway-core`
- `area:gateway-proxy`
- `area:gateway-store`
- `area:gateway-telemetry`
- `area:ci`
- `feature:auth`
- `feature:policy`
- `feature:usage`
- `feature:rate-limit`
- `feature:budget`
- `feature:streaming`
- `feature:admin-api`
- `feature:relayna-runtime`

Important guidance:

- Use direct evidence from changed implementation files and the dominant intent
  of the diff.
- Do not add labels based only on tests, examples, comments, docstrings,
  imports, or incidental helper changes.
- Prefer exactly one of `bug` or `enhancement` unless the PR clearly contains
  two separate first-order outcomes.
- `documentation`: docs-only changes or comments/docstrings without behavior
  changes.
- `project`: repository metadata, AGENTS.md, PLANS.md, issue/PR templates, or
  broad tooling that is not CI-specific.
- `dependencies`: dependency additions, removals, updates, lockfile changes, or
  package metadata dependency changes.
- `area:gateway-api`: primary changes under `crates/gateway-api/` or Axum
  control-plane route/middleware code.
- `area:gateway-core`: primary changes under `crates/gateway-core/` or core
  auth, policy, routing, budget, rate-limit, usage, or pricing logic.
- `area:gateway-proxy`: primary changes under `crates/gateway-proxy/`,
  Pingora services, or LiteLLM/provider/internal proxy behavior.
- `area:gateway-store`: primary changes under `crates/gateway-store/`,
  migrations, SQLx models, PostgreSQL access, or Redis access.
- `area:gateway-telemetry`: primary changes under `crates/gateway-telemetry/`
  or tracing, metrics, OpenTelemetry, correlation, and redaction helpers.
- `area:ci`: primary changes under `.github/workflows/` or CI helper scripts.
- `feature:auth`: virtual keys, key hashing, key context, secret redaction, or
  authentication failures are a primary deliverable.
- `feature:policy`: route, model, provider, streaming, tools, task execution, or
  policy denial behavior is a primary deliverable.
- `feature:usage`: usage events, token/cost accounting, pricing, or Studio
  queryability is a primary deliverable.
- `feature:rate-limit`: Redis request or token rate limiting is a primary
  deliverable.
- `feature:budget`: daily/monthly spend tracking or budget enforcement is a
  primary deliverable.
- `feature:streaming`: SSE passthrough, cancellation, buffering, or first-token
  telemetry is a primary deliverable.
- `feature:admin-api`: key management, policy management, or usage admin routes
  are a primary deliverable.
- `feature:relayna-runtime`: internal Relayna APIs, task submission, or Relayna
  worker metered provider calls are a primary deliverable.

Decision process:

1. Determine the PR's primary intent in one sentence from the title, body, and
   dominant diff.
2. Start with zero labels.
3. Add `bug` or `enhancement` conservatively.
4. Add the minimum area labels needed to describe the main touched workspace.
5. Add feature labels only when the feature area is a primary user-facing or
   operator-facing outcome.
6. Re-check every label and drop labels supported only by secondary edits.

Output:

- JSON only, no code fences, no extra text.
- Example: `{"labels":["enhancement","area:gateway-core","feature:policy"]}`
