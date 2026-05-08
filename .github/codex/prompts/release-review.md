# Release readiness review

You are Codex running in CI. Produce a release readiness report for the
Relayna Gateway repository.

Steps:

1. Determine the latest release tag using local tags only:

       git tag -l 'v*' --sort=-v:refname | head -n1

2. Set `TARGET` to the current commit SHA:

       git rev-parse HEAD

3. Collect diff context for `BASE_TAG...TARGET`:

       git diff --stat BASE_TAG...TARGET
       git diff --dirstat=files,0 BASE_TAG...TARGET
       git diff --name-status BASE_TAG...TARGET
       git log --oneline --reverse BASE_TAG..TARGET

4. Review release risk across these surfaces:

   - public HTTP routes, response shapes, status codes, and headers
   - virtual key format, authentication behavior, and key hashing
   - policy enforcement for routes, models, providers, streaming, tools, and
     task execution
   - LiteLLM and direct provider proxy behavior, including secret stripping
   - streaming passthrough, buffering, cancellation, and first-token telemetry
   - usage event fields, pricing, token accounting, and Studio queryability
   - PostgreSQL schemas, migrations, and rollout/rollback safety
   - Redis key patterns, counters, TTLs, rate limits, and budget state
   - Relayna runtime integration and async task submission behavior
   - telemetry metrics, logs, traces, redaction, and label cardinality
   - packaging metadata, dependency changes, Docker, and CI/release workflows
   - docs, README, changelog, design manifesto, and migration guidance

Output:

- Write a concise Markdown report.
- Include the compare URL:
  `https://github.com/${GITHUB_REPOSITORY}/compare/BASE_TAG...TARGET`.
- Include a clear `Ship` or `Block` call.
- Include risk levels: `High`, `Medium`, `Low`, or `None`.
- If no risks are found, include `No material risks identified`.
- Output only the report, with no code fences and no extra commentary.
