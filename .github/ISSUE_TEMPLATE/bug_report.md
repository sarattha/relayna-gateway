---
name: Bug report
about: Report incorrect Relayna Gateway behavior
title: ''
labels: bug
assignees: ''
---

### Please read this first

- Have you read the relevant README, docs, or design manifesto section?
- Have you searched existing issues for the same problem?
- If this is about local infrastructure, confirm PostgreSQL, Redis, and the
  configured LiteLLM/provider upstream are reachable.

### Affected area

Mark all that apply:

- [ ] Gateway startup / config
- [ ] Health / readiness
- [ ] Authentication / virtual keys
- [ ] Policy / route or model access
- [ ] OpenAI-compatible route
- [ ] LiteLLM proxying
- [ ] Direct provider passthrough
- [ ] Streaming
- [ ] Usage tracking / pricing
- [ ] Rate limits / budgets
- [ ] PostgreSQL schema / migrations
- [ ] Redis counters / state
- [ ] Telemetry / logs / metrics / traces
- [ ] Relayna runtime integration
- [ ] Packaging / installation
- [ ] Documentation

### Describe the bug

A clear description of what is wrong.

### Debug information

- Relayna Gateway version or commit:
- Rust version:
- OS:
- PostgreSQL version:
- Redis version:
- LiteLLM/provider upstream:
- Deployment method:

### Reproduction steps

Provide the smallest runnable example, command sequence, request payload, or
service setup that reproduces the issue. Redact all secrets.

### Expected behavior

What did you expect to happen?

### Actual behavior

What happened instead? Include status codes, error messages, logs, response
bodies, database rows, Redis keys, or screenshots when useful. Redact prompts,
keys, and provider credentials.

### Additional context

Anything else that helps explain the issue.
