# Issue #114 verification

Worktree: `codex/issue-114-two-header-auth`, based on v0.1.33 plus repository metadata.

## Automated regressions

The real Pingora process test uses disposable PostgreSQL and Redis and a local
RSA-signed OIDC issuer. Paused mode accepts malformed JWT text, wrong issuer,
wrong audience, expired claims, missing roles/scopes/groups and a bad signature,
but makes zero discovery/JWKS calls. The same invalid tokens are rejected after
restoring Entra, while a valid token succeeds with the same two headers.

The test rejects missing/empty/malformed bearer headers, missing/empty/invalid
Relayna headers, unknown keys and disabled/revoked/expired keys. Provider-policy
denial still applies. The mock upstream sees the provider default or key-mapped
LiteLLM credential and never the client's Relayna header or bearer.

Core, configuration and Admin API tests cover false defaults, persisted override,
PATCH omission, save/readback, runtime restoration and rejected Apigee combinations.
UI tests execute the real save handler for enable, disable and conflict cases.

## Computer Use

Tested the compiled Admin UI in Safari using the Computer Use plugin on
2026-09-09. Chrome's accessibility capture timed out, so Safari was used.
The test instance and database were separate from integration tests and existing
development services. Only disposable local credentials were used.

Desktop (1292 × 768 window) and mobile (390 × 844 viewport):

- Emergency operator login and Overview loaded successfully.
- Settings enabled the mode, saved it, and displayed the active-state warning.
- Mobile save rejected combining unverified bearer with trusted Apigee headers.
- Mobile save cleared the mode and restored configured Entra without losing the
  issuer, audience or `x-litellm-key` header.
- Monitor Overview, Discover Projects and Govern Settings were reachable.
- Mobile Create project drawer opened and closed with no layout overflow.
- Warning uses existing amber status tokens; help remains visible and controls fit.

Screenshots are local QA artifacts in `/tmp/issue114-ui`.

## Findings and environment notes

The existing process test waited only for the control API, which could race the
Pingora listener. It now also waits for the proxy TCP listener.

On this Mac, a full-suite startup test hit native certificate-store error -36.
Using `SSL_CERT_FILE=/etc/ssl/cert.pem` avoids that machine-specific certificate
lookup failure without changing application code or weakening TLS validation.

A preliminary model-denial case returned 200: existing Pingora policy evaluation
occurs before body-dependent model features are available, while the later body
callback reloads guardrails only. Those released lifecycle paths are unchanged
by this PR. The regression verifies provider-policy denial instead. Model-policy
timing requires a separate focused fix; this PR does not claim to fix it.

The forward migration was also applied inside a rolled-back transaction to a
temporary old-format settings table. An existing row retained false, and directly
writing both enabled flags raised a check violation as expected.

Nextest exposed a cross-binary fixture race: the store integration test disabled
Entra while the process regression was testing restoration. The store test now
uses the same advisory lock as the existing process/Admin API tests while it
mutates shared singleton settings.

## Full verification

In progress. Final results and PR review will be recorded before completion.
