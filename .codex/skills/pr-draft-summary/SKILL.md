---
name: pr-draft-summary
description: Create a PR title and draft description after substantive Relayna Gateway changes are finished. Trigger when wrapping up a moderate-or-larger change and you need a PR-ready summary block with branch suggestion, title, and draft description.
---

# PR Draft Summary

## Purpose

Produce a PR-ready summary after substantive work is complete: a branch
suggestion, concise title, and draft description that begins with
`This pull request <verb> ...`. The block should be ready to paste into a PR for
`sarattha/relayna-gateway`.

## When to Trigger

- The task is finished or ready for review and touched runtime code, tests,
  gateway behavior, docs with behavior impact, packaging, or build/test
  configuration.
- You are about to send the work-complete response and need the PR block
  included.
- There are commits ahead of the base fork point even if the working tree is
  clean.
- Skip only for trivial conversation-only tasks, repo-meta/doc-only tasks
  without behavior impact, or when the user says not to include a PR draft.

## Inputs to Collect Automatically

Do not ask the user for these. Run local Git commands.

- Current branch: `git rev-parse --abbrev-ref HEAD`.
- Working tree: `git status -sb`.
- Untracked files: `git ls-files --others --exclude-standard`.
- Changed files: `git diff --name-only` and
  `git diff --name-only --cached`.
- Diff sizes: `git diff --stat` and `git diff --stat --cached`.
- Latest release tag:
  `LATEST_RELEASE_TAG=$(git tag -l 'v*' --sort=-v:refname | head -n1)`.
- Base reference:
  `BASE_REF=$(git rev-parse --abbrev-ref --symbolic-full-name @{upstream} 2>/dev/null || echo origin/main)`.
- Base commit:
  `BASE_COMMIT=$(git merge-base --fork-point "$BASE_REF" HEAD || git merge-base "$BASE_REF" HEAD || echo "$BASE_REF")`.
- Commits ahead:
  `git log --oneline --no-merges ${BASE_COMMIT}..HEAD`.

## Category Signals

- Gateway API: `crates/gateway-api/`, Axum control-plane routes, middleware,
  structured errors, health, readiness, admin APIs, and request IDs.
- Gateway core: `crates/gateway-core/`, auth, policy, routing, rate limit,
  budget, usage, and pricing logic.
- Gateway proxy: `crates/gateway-proxy/`, Pingora services, LiteLLM, provider
  passthrough, streaming, and internal Relayna proxy calls.
- Gateway store: `crates/gateway-store/`, migrations, SQLx models,
  PostgreSQL access, and Redis access.
- Gateway telemetry: `crates/gateway-telemetry/`, tracing, metrics,
  OpenTelemetry, correlation, and redaction helpers.
- Tests: `tests/`, `benches/`, or crate-local test modules.
- Docs: `docs/`, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, `mkdocs.yml`,
  and `internal/design-manifesto.md`.
- Build/test config: `Cargo.toml`, `Cargo.lock`, `.cargo/`, `Makefile`,
  Dockerfiles, `.github/`, and `.codex/`.

## Workflow

1. Run the commands above without asking the user. Compute `BASE_REF` and
   `BASE_COMMIT` first so later commands reuse them.
2. If there are no staged, unstaged, untracked, or ahead-of-base changes, reply
   briefly that no code changes were detected and skip the PR block.
3. Infer change type from touched paths and diff content: feature, fix,
   refactor, test, docs-with-impact, tooling, or chore.
4. Flag backward-compatibility risk only when the diff changes released public
   routes, external config, persisted data, Redis state, usage event fields,
   route responses, streaming behavior, or wire protocols. Judge this against
   `LATEST_RELEASE_TAG`, not unreleased branch-only churn.
5. Summarize changes in 1-3 short sentences using the key paths and diff stats.
   Explicitly call out untracked files because `git diff --stat` does not
   include them.
6. Choose the lead verb:
   - feature: `adds`
   - bug fix: `fixes`
   - refactor/performance: `improves`
   - docs/tooling/chore: `updates`
7. Suggest a branch name. If already off `main`, keep the current branch.
   Otherwise propose `feat/<slug>`, `fix/<slug>`, `docs/<slug>`,
   `test/<slug>`, or `chore/<slug>`.
8. If the current branch matches `issue-<number>`, keep that branch suggestion
   and reference `https://github.com/sarattha/relayna-gateway/issues/<number>`
   with an auto-closing line such as `This pull request resolves #<number>.`.
9. Draft the PR title and description using the output template.

## Output Format

When closing out a task and the summary block is desired, add this Markdown
block after any brief status note. If the user says they do not want it, skip
this section.

```markdown
# Pull Request Draft

## Branch name suggestion

git checkout -b <kebab-case suggestion, e.g., feat/gateway-policy-enforcement>

## Title

<single-line imperative title, preferably with a common prefix such as feat:, fix:, docs:, test:, or chore:>

## Description

This pull request <adds/fixes/improves/updates> ...

<Explain what changed, why it changed, behavior or compatibility considerations, and any important review notes. Tests do not need to be listed unless specifically requested.>
```

Keep the block concise. Avoid repeating the same details in multiple sections.
