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
aliases, independent claims, JWT skew/issuer/tenant/expiry and signed Apigee identity.
It also checks missing, ambiguous or disabled bindings and invalid credentials,
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
assets, strict docs and release metadata checks also passed. Existing dependency audit exceptions and coverage exclusions remain unchanged.

## Follow-up: dynamic forms, guidance and complete filter audit

The earlier regressions targeted the reproduced failures; they did not establish
coverage of every filter. The follow-up audits every visible Traffic filter:
request ID, service, project ID, key ID, client status, failure reason, all three
outcome choices, and both history date boundaries.

- Live predicate tests cover all six text/numeric filters independently,
  192 combinations with outcome choices, blank/missing metadata, HTTP status
  classes, and HTTP-200 stream interruptions.
- Mounted form tests exercise trimming, uppercase UUIDs, zero-padded status,
  all history query parameters, local-time conversion, the live-only Active
  restriction, reason chips, pagination/reset, empty results, pause/resume and
  rows leaving Active after completion. The delayed-read regression remains.
- Real admin API/PostgreSQL tests verify each saved-history predicate,
  intersected predicates, exact request IDs, inclusive date boundaries,
  same-timestamp cursor tie-breaking and invalid UUID/status/range rejection.
- Newly found bugs: UUID casing/status formatting could hide returned records;
  an old pagination cursor could skip results while a replacement filter loaded.
  Values are now normalized, and pagination resets and disables while loading.

Dynamic endpoint/profile tests prove that inactive groups are hidden and disabled,
only Entra audiences are required, key-only payloads omit Entra fields, and
switching back retains draft values. Service presets dispatch the same mode
change. Every identity/profile/Traffic filter field has reviewed contextual help;
secondary detail is available through accessible help triggers beside labels.

Computer Use verifies inherited/profile mode transitions, Entra/key-only switching,
retained audience drafts, save with hidden Entra fields, tooltip clicks without
checkbox changes, Escape dismissing only the tooltip, and desktop/narrow layout.
Local follow-up screenshots include `dynamic-key-only.png`,
`stable-id-tooltip.png`, and `dynamic-profiles-narrow.png` in the same ignored
screenshot directory. Tests bound the known behavior; they cannot prove that no
possible combination of inputs, timing or deployment state has any future bug.

The added Rust work is integration-test coverage only; production Rust is unchanged.
The instrumented Traffic integration test passed and the unchanged coverage gate
still reports 137/137 (100%) changed executable production Rust lines.

The follow-up history scan flagged report prose as `generic-api-key`; no credential
was present. Rephrased the current sentence and documented an exact historical
commit/file/line false-positive exception in `docs/security-exceptions.md`.

Final follow-up verification: all ten mandatory commands passed again; nextest
ran 368 tests, all passed with zero skipped. UI tests, generated asset build,
strict documentation and release metadata validation also passed.

## Concise guidance follow-up (2026-09-20)

Removed duplicated inline explanations from fields with tooltips, while keeping
accessible descriptions, required/optional labels, the unassigned-key warning
and saved-binding counts. Removed redundant endpoint/profile paragraphs.
Computer Use confirmed the compact desktop and 390px form, on-demand help,
keyboard tooltip access and Escape dismissing help without closing the editor.
The original open form was preserved in its tab. Local screenshots:
`concise-desktop.png` and `concise-narrow.png`.

Validation: `npm test`, `npm run build:admin-ui`, `cargo build -p gateway-api`
and all ten mandatory verification steps passed; nextest passed 368/368 with
zero skips. No production Rust changes or coverage exclusions were introduced.
