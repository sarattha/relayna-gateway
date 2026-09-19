# Traffic filters and authentication profiles verification

Base: `872614b` (released baseline v0.1.37). Branch: `codex/traffic-filters-auth-profiles`.

## Reproduction and fixes

A disposable gateway with PostgreSQL 17, Redis 7 and a local OpenAI-compatible
upstream generated alternating successful and failing requests. In Chrome,
applying a request ID with surrounding spaces originally returned zero rows;
after trimming it returns the matching row in both Live and Saved history.
Mounted regressions also prove that changing project and selecting a new key in
one submission preserves the key, and a buffered live read cannot overwrite
history after switching modes. Existing filter/SSE tests remain in the suite.

## Authentication end-to-end evidence

`authentication_profiles_e2e` runs a real Pingora proxy and Axum admin API with
an isolated migrated database, Redis, mock OIDC/JWKS, signed JWTs and upstream.
It checks three simultaneous caller populations in managed and direct forwarding,
aliases, independent claims, JWT skew/issuer/tenant/expiry, signed Apigee,
missing/ambiguous/disabled bindings, invalid/revoked/expired credentials,
native/trusted-ingress bypass rejection, upstream mapping and header stripping.
All three populations retain rate and budget enforcement in both modes.

Service/route writes cover stale revisions, invalid key references, project
ownership on create and ownership changes, atomic rejected updates, unchanged
legacy uniqueness errors, legacy writers, read-only operator rejection and
auditing. Traffic/Usage snapshots retain historical names and exclude secrets.
A separate real admin API race submitted two different edits at revision zero;
one returned 200 and the other 409. Store failure denies the request. Accessa's mock-chain E2E covers successful
profile-bound handshake, two turns, profile disabling, rejected subsequent turn
without dispatch, and rejected new handshake.

## Computer Use

Used `@oai/sky` through the Computer plugin to operate Chrome against the local
gateway, including desktop and a 390 × 844 responsive viewport.

- Created an Entra profile, renamed key-only profile, saved and reopened.
- Disabled the unused Entra profile and verified persisted state/revision.
- Opened key assignment inspector and saved its explicit route binding.
- Verified Add profile focuses the new row; the Add action is beside its heading,
  rows use consistent spacing, and Save/Cancel remain visible while scrolling.
- Found and fixed an invalid HTML Unicode-v pattern and validation feedback outside
  the dialog. Missing audience now displays and focuses an inline alert.
- Applied a whitespace-padded ID filter to real live traffic, switched to history,
  and inspected the selected profile in Traffic and Usage Debug.

Local screenshots (ignored by the repository's screenshot policy) are in
`internal/test-reports/traffic-profiles/`: `profiles-button-fixed.png`,
`profiles-narrow.png`, `profiles-narrow-header.png`, `profiles-validation.png`, `key-binding-saved.png`,
`profile-traffic-inspect.png`, and `profile-usage-debug.png`.

## Coverage

The unchanged repository gate reports **137/137 = 100% changed executable
production Rust lines** against `872614b`, exceeding issue #120's strict >98%
requirement. Machine-readable details: `traffic-profiles-coverage.json`.

Denominator: changed production Rust lines with LLVM DA records. Integration
tests, benches and `#[cfg(test)]` items are excluded by the existing script;
production failure paths are included. Struct declarations and lines without
LLVM executable mapping do not enter the denominator. Async LLVM mappings are
sparse, so this is not a branch-coverage or whole-workspace 100% claim. The full
LCOV report (including tests and unchanged code) measured 95.48% line coverage.

Commands:

```sh
SSL_CERT_FILE=/etc/ssl/cert.pem DATABASE_URL=<disposable-postgres> REDIS_URL=<disposable-redis> \
  cargo llvm-cov --workspace --all-features --lcov --output-path /tmp/traffic-profiles.lcov
# Additional final rate/budget/ownership tests, retaining the full-suite counters:
cargo llvm-cov --no-clean -p gateway-api --test authentication_profiles_e2e \
  --lcov --output-path /tmp/traffic-profiles-final-focused.lcov
cargo llvm-cov report --lcov --output-path /tmp/traffic-profiles.lcov
python3 scripts/check-changed-rust-coverage.py /tmp/traffic-profiles.lcov 872614b
npm run build:admin-ui
npm test
uvx --with mkdocs-material mkdocs build --strict
python3 scripts/validate-release-metadata.py
```

All dependency-backed tests use disposable services; native TLS tests require
the system certificate bundle above on this Mac. Final mandatory verification
uses `.codex/skills/code-change-verification/scripts/run.sh` (format, Clippy,
workspace tests, audit, deny, machete, nextest, Trivy, Gitleaks and Semgrep).

Final result: all ten mandatory verification commands passed. Nextest ran
368 tests: 368 passed, zero failed, zero skipped. UI tests, reproducible generated
assets, strict docs and release metadata checks also passed. Existing audit
exceptions remain unchanged; no verification exclusions were added.
