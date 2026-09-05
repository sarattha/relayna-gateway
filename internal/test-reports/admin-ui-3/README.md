# Admin UI 3.0 production verification

Date: 2026-09-05. Branch: `codex/admin-ui-3`; baseline: `42aa804`, release `v0.1.31`.

## Environment and reproducibility

The current Vite build was served at `http://127.0.0.1:20381/admin-ui` through a local asset/fault proxy. Dependencies are isolated in Docker Compose project `relayna-admin-ui-3`: PostgreSQL 20432, Redis 20379, development OIDC 20390 and mock upstream 20382. The initial baseline image did not have current Traffic endpoints; final Live/History testing used the freshly built native `target/debug/gateway-api`, control port 20384 and proxy port 20385. No production services or credentials were used. Temporary generated certificates and token material stay outside tracked files.

Run `npm ci`, `npm run build:admin-ui`, and `npm test`. `tests/admin-ui-browser.mjs` exports authenticated-tab checks for the supported Chrome runtime: `checkAdminSurfaces(tab, names)`, `checkContextAndDrawers(tab)` and `checkUsageRecovery(tab, configureFaults)`. Run surface groups of four or five to fit browser tool deadlines. The recovery callback configures the local QA proxy to return 503 for `/admin-ui/admin/usage/events`, then clears the fault. Seed fixtures come from `deploy/local/seed.sql`. The local proxy and Compose overrides remain in ignored `target/admin-ui-3/`; they are environment scaffolding, not production deployment configuration.

## Executed coverage

| Area | Evidence / result |
| --- | --- |
| Admin navigation | All 14 surfaces reached at 1440×1000, 390×844 and 820×1180; no document horizontal overflow. JSON inventories accompany this report. Managed identities remain reachable through People → Workload identities. |
| Owner navigation | My projects, Project dashboard, My services and Service dashboard exercised with real development OIDC identities. Project owner sees Analytics Platform; service owner sees orders-api. Empty scopes are explicit. Phone project-owner and tablet service-owner views checked. |
| Access states | Viewer membership, pending membership and blocked membership checked through disposable database fixtures and reload; fixtures restored. Admin navigation absent for owner/viewer sessions. |
| Project workflows | Create, service picker/link/save, delete cancellation; scope carries to Usage and survives refresh. |
| Virtual keys | Review/create/show-once, edit expiration, disable and revoke. Raw credential shape checked without recording its value; close removes credential textarea. Exact expiry boundary also covered by unit tests. |
| Services / routes | Create, edit health path/timeout, disable, route timeout save; invalid uppercase/space name rejected by native pattern validation. |
| Providers | Create and confirmed delete of a disposable internal-service configuration. |
| Drafts / keyboard | Create drawer retains input after close/reopen; dirty navigation Cancel preserves view, Confirm discards. Background inert and Escape behavior checked. Debug request focus restores to originating button. |
| Usage | Scoped results and CSV preview (168 seeded rows), request investigation drawer, breakdown tabs; failed request exits loading and Retry restores applied results. |
| Monitoring faults | Readiness 503 shown as not ready while independent metrics remain; cached provider observations labeled stale with timestamp. Expired key excluded from active count. Delayed Projects/Keys responses did not overwrite subsequent Routes/Health navigation. |
| Traffic | Two real current-runtime proxy calls returned 200 and appeared live, with matching status, elapsed time and upstream attempt timeline. Pause/Resume and saved history checked. Disposable API-created test key revoked. Project filter updates header and URL scope. |

Screenshots are local evidence under this directory (ignored by repository screenshot policy). The approved prototype evidence under `internal/design/admin-ui-3/` is separate from these production checks.

## Automated verification

- Frontend production build and `npm test`: pass, including body-stall timeout, composed cancellation, 503 response preservation, key lifecycle and sampled reliability thresholds.
- Mandatory script: format, workspace Clippy with warnings denied, and workspace all-feature tests pass; **script cannot finish** because upstream RustSec contains duplicate advisory ID `RUSTSEC-2026-0244` (gettext-rs and gettext-sys). No advisory removed and no new ignore added.
- Remaining checks run independently: cargo deny passes; cargo machete passes; nextest **325 passed, 0 skipped**; Trivy passes using the official GHCR database mirror after the default mirror timed out; gitleaks reports no leaks; Semgrep reports no findings.
- Logs remain in ignored `target/admin-ui-3/`.

## Limits and follow-up coverage

Computer Use could not capture the locked Mac after an initial timeout. The user was notified; authorized Chrome verification continued. This report does not claim completed Computer Use coverage. Responsive checks cover navigation and representative interactions, not every mutation at every width. Studio import needs a configured Studio fixture; successful import/sync was not exercised. Full policy/guardrail, membership/workload-identity and settings mutation matrices, export file readback and live network-drop recovery remain additional coverage; existing server tests and retained handlers do not substitute for those UI checks. No backend contract, migration, public route or secret-handling boundary was changed.

## PR review follow-up (2026-09-05)

All GitHub checks passed at a7891d7. Addressed two Codex findings: reliability evaluates all timeout/error/fallback rates and selects the highest; workspace changes commit only after navigation is accepted. Added mixed-rate unit cases and a real dual-role browser regression covering Cancel (admin shell and draft retained), Confirm (owner landing) and return to Admin. Temporary admin project membership was removed after the test. Frontend tests and the required format/Clippy/workspace sequence pass; local cargo audit still encounters the same upstream duplicate advisory.
