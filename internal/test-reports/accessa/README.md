# Accessa verification evidence

## Computer Use — 2026-09-16

Used the Computer Use plugin (`@oai/sky` through Node REPL), Chrome, and the local
gateway at `127.0.0.1:18450`. PostgreSQL/Redis were disposable Docker services.

- Logged in using a disposable local operator token.
- Opened Services and edited an Accessa fixture.
- Expanded Endpoint identity and Accessa; inspected audience, scopes, roles,
  groups, signed Apigee toggle, channel binding and transport controls through
  the accessibility tree.
- Changed audience from `api://accessa` to `api://accessa-ui-verified`, saved,
  observed “Service updated”, reopened and verified the saved audience.
- Inspected the rendered identity fields and guidance. Screenshot:
  [endpoint-settings.png](endpoint-settings.png).
- The first save failed because a concurrent integration suite rotated the test
  database's operator token. Restored the disposable bootstrap token and repeated
  successfully. No production settings were involved.
- Missing-audience rejection and form serialization are covered by the automated
  Admin UI regression test. They are not claimed as a successful manual UI check.

## Automated checks

**Changed production Rust line coverage: 675 / 679 (99.41%)**, against base
`8ee95a6`. Exact uncovered mappings are retained in `coverage.json`.
The instrumented workspace suite passed with live PostgreSQL/Redis. The reusable gate is
`scripts/check-changed-rust-coverage.py`; it requires strictly greater than 98%
coverage for added/changed executable production Rust, excludes test items, and
includes production failure paths. UI serialization has separate Node tests.

Reproduce from a checkout with disposable `DATABASE_URL` and `REDIS_URL`:

```sh
SSL_CERT_FILE=/etc/ssl/cert.pem cargo llvm-cov --workspace --all-features --lcov --output-path /tmp/accessa.lcov
python3 scripts/check-changed-rust-coverage.py /tmp/accessa.lcov 8ee95a6
npm run build:admin-ui
npm test
bash .codex/skills/code-change-verification/scripts/run.sh
```

## Final local verification

- `code-change-verification/scripts/run.sh`: all ten commands passed in sequence
  on 2026-09-16 with disposable PostgreSQL/Redis enabled.
- Nextest: 359 passed, zero skipped, with no leak warnings in the final run.
- The committed patch was rescanned for secrets. A dummy unit-test prefix was
  replaced with the repository's documented fixture; no new scanner exceptions
  were added.
- Trivy: zero HIGH/CRITICAL findings. Gitleaks: no leaks. Semgrep: zero findings
  from the repository's two configured rules.
- Dependency audit and policy checks passed under the repository's existing
  exceptions. Rustls/webpki and event-listener were updated to patched versions.
- `npm run build:admin-ui`, `npm test`, strict MkDocs build, workflow YAML parsing,
  release metadata validation, and `git diff --check` passed.

## Protocol label follow-up

Computer Use verified the rebuilt Services inventory, Routes inventory and saved
service editor. Ordinary services and provider routes show HTTP; the Accessa
registration shows HTTP + WS and labels the exact GET run path WS.
Regression tests cover discovery, missing GET, mismatched prefixes, disabled
registrations and escaped display values. The Admin UI build and npm tests pass.

![Service protocol labels](protocol-labels.png)
![Saved endpoint protocol summary](protocol-settings.png)

The protocol-label follow-up changed presentation only. The later Entra extension
is included in the current coverage report above.

The full mandatory verification stack passed again for this follow-up, including
357 Nextest tests with zero skipped and all configured security checks.

## Per-endpoint Entra extension

The full instrumented workspace suite passed with live PostgreSQL/Redis.
New extension: 64/64 LLVM-mapped changed executable production lines (100%),
relative to 1045bdf. Full PR: 675/679 (99.41%), relative to 8ee95a6.
LLVM emits sparse source-line mappings for async-trait methods; this percentage
describes emitted executable mappings, not branch coverage. Real proxy E2E tests
exercise mixed identity modes, canonical aliases, direct passthrough, trusted
ingress, wrong audience, missing tokens and invalid key rejection. They also check
corrupt or unavailable policy storage and stripped credentials. Admin API tests cover scopes, validation and audit/store
failures. No production error paths are excluded from the coverage calculation.

Computer Use saved Require Entra on `/litellm/*` with audience `api://litellm`
and scope `generate`, reopened the editor, and verified values after full reload.
It separately saved No Entra on `/providers/openai/*` and verified both modes
side by side after reload. Service inventory shows a key-only internal service
alongside Entra-protected Accessa. At 390px, inspected the horizontally scrollable
service inventory and the service editor's No Entra selector; Escape dismisses
the route dialog. Demo database is isolated from integration-test token rotation.

![Mixed route identity policies](mixed-route-identities.png)
![LiteLLM endpoint policy](route-entra-required.png)
![Mixed service identity policies](mixed-service-identities.png)

Final extension verification: all mandatory commands passed in sequence, 359
Nextest tests passed with zero skipped, npm build/tests passed, and strict MkDocs
build passed using `uvx --with mkdocs-material mkdocs build --strict`. Computer Use
also checked Monitor/Discover/Govern navigation at desktop and mobile widths.
