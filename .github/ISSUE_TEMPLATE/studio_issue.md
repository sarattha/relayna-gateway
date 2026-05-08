---
name: Studio integration issue
about: Report an issue with Relayna Studio consuming Gateway data
title: ''
labels: bug
assignees: ''
---

### Please read this first

- Have you searched existing issues?
- If this is a local deployment, confirm Gateway, PostgreSQL, Redis, and the
  relevant Relayna Studio service are reachable.

### Affected component

- [ ] Usage event storage
- [ ] Usage query/admin API
- [ ] Project/key attribution
- [ ] Route/model/provider attribution
- [ ] Cost or token fields
- [ ] Request correlation IDs
- [ ] Logs / metrics / traces
- [ ] Deployment configuration

### Describe the issue

A clear description of what Studio cannot show, query, or correlate.

### Debug information

- Relayna Gateway version or commit:
- Relayna Studio version or commit:
- Rust version:
- PostgreSQL version:
- Redis version:
- Deployment method:

### Reproduction steps

Provide the smallest sequence that reproduces the issue, including URLs,
payloads, environment variables, database rows, or usage event examples when
relevant. Redact all secrets and prompts.

### Expected behavior

What did you expect Studio to show or return?

### Actual behavior

What happened instead? Include screenshots, Gateway logs, Studio logs, network
responses, or event payloads when useful.
