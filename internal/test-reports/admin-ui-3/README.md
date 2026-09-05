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

## Final release verification (0.1.32)

The full mandatory script now passes in sequence, including cargo audit, deny,
machete, nextest, Trivy, gitleaks and Semgrep. The all-feature workspace release
build and frontend tests pass. **Correction:** the earlier duplicate-advisory
failure came from untracked files in the local advisory cache, not the official
upstream repository. The complete cache was preserved at
`/Users/jobz/.cargo/advisory-db.ui3-preserved-20260905`; a clean official cache
resolved the failure without adding ignores or weakening checks.

A fresh Computer Use attempt still timed out; the authorized Chrome workflow
provides the recorded UI coverage. Browser verification is representative of
major workflows and all 18 surfaces, not an exhaustive combination of every
field and viewport. No known material product defect remains from the executed
checks. Remaining external limitation: Computer Use capture itself.

Final additions: Traffic project change after a key drilldown removed the old
key from the URL and applied Usage filters; same-project key preservation has
unit coverage. Downloaded CSV was parsed from disk: 170 rows, every row matched
the selected Analytics project, no raw-key column. Stopping the isolated gateway
produced a visible disconnected/retrying state; restarting the newly built
0.1.32 binary reconnected automatically to a new instance and displayed the
explicit missed-updates warning. Team policy layer save/readback/delete and
policy simulation passed. The pending development member was approved, assigned
service Viewer then project Owner access, both bindings were removed, and Block
was verified; its fixture status was restored to pending. Current desktop and
mobile screenshots are checked into `docs/assets/screenshots/admin-ui-3/`.

## Earlier automated verification record

- Frontend production build and `npm test`: pass, including body-stall timeout, composed cancellation, 503 response preservation, key lifecycle and sampled reliability thresholds.
- Mandatory script: format, workspace Clippy with warnings denied, and workspace all-feature tests pass; **script cannot finish** because upstream RustSec contains duplicate advisory ID `RUSTSEC-2026-0244` (gettext-rs and gettext-sys). No advisory removed and no new ignore added.
- Remaining checks run independently: cargo deny passes; cargo machete passes; nextest **325 passed, 0 skipped**; Trivy passes using the official GHCR database mirror after the default mirror timed out; gitleaks reports no leaks; Semgrep reports no findings.
- Logs remain in ignored `target/admin-ui-3/`.

## Limits and follow-up coverage

Computer Use could not capture the locked Mac after an initial timeout. The user was notified; authorized Chrome verification continued. This report does not claim completed Computer Use coverage. Responsive checks cover navigation and representative interactions, not every mutation at every width. Studio preview/import/sync now passes against a disposable HTTP catalog fixture. Guardrail create/edit/disable/delete, a reversible auth-header Settings edit, Studio connection save/clear, and service workload registration/disable/enable/delete have also been exercised. Those remaining major workflow checks were completed in the final release pass above; server tests and retained handlers were not used as substitutes for UI readback. No backend contract, migration, public route or secret-handling boundary was changed.

## PR review follow-up (2026-09-05)

All GitHub checks passed at a7891d7. Addressed two Codex findings: reliability evaluates all timeout/error/fallback rates and selects the highest; workspace changes commit only after navigation is accepted. Added mixed-rate unit cases and a real dual-role browser regression covering Cancel (admin shell and draft retained), Confirm (owner landing) and return to Admin. Temporary admin project membership was removed after the test. Frontend tests and the required format/Clippy/workspace sequence pass; local cargo audit still encounters the same upstream duplicate advisory.

Additional browser checks: created opt-in `ui3-disposable-guardrail`, edited description, disabled and confirmed deletion; catalog returned to its original row count. Saved `X-UI3-Test-Key` and read it back, then restored the original gateway key header. Started a local Studio catalog fixture on 20386 returning one service, configured it through Settings, previewed +1/0 invalid, imported and synchronized the service; its disabled/incomplete status correctly reflects missing runtime configuration. Cleared the persisted Studio connection back to the prior unset state. Registered `UI3 disposable workload` for exact service `ui3-disposable`, verified disabled state and re-enabled it. A subsequent refreshed readback disproved the initial deletion result: confirmation lost `event.currentTarget` before constructing the request. Service and project binding deletion now capture the ID before awaiting confirmation. Automated tests simulate currentTarget being cleared while confirmation is pending and prove both confirmation and cancellation. Browser readback confirms the service binding is absent after refresh. A project binding was also registered; Cancel kept its row, Confirm removed it. Imported service remains disabled in the isolated fixture inventory.

Second Codex follow-up: project/key/overview Usage drilldowns now pass requested scope into navigation, which commits it only after dirty-form and pending-write guards accept navigation. Tests cover cancellation, pending writes and acceptance. Deleting the selected project clears its project/key filters only after successful deletion; unrelated selections are preserved. Real browser checks created and deleted an empty selected project (All projects and three surviving rows restored), then canceled an Analytics Usage drilldown from a dirty project form and verified the unchanged scope and URL.

A final consistency pass uses one project-scope synchronizer for navigation,
creation, deletion, header selection and URL restoration. A changed project
clears stale key filters in both Usage and Traffic; same-project filter submissions
retain their supplied criteria. Unit coverage checks both views. Browser
verification drilled into a key, created a new project, and confirmed its new
scope URL contained no inherited key filter before deleting the fixture.

Final chart/accessibility review: owner label granularity now follows the owner
6h/24h/7d selection independently of the admin Overview range. Tests compare
hourly versus daily labels. Dialogs retain original inert states across nested
and out-of-order closes; tests preserve a pre-inert root, and the phone sidebar
kept its inert attribute before and after a creation drawer. Traffic resolves
focus against the current request table on close. Two real local requests
verified this: ui3-focus-origin stayed selected while ui3-focus-update replaced
the table, and closing the drawer focused the new Inspect button in the
originating row. Both temporary request keys were revoked. The complete
mandatory script passed again after these fixes.

Final project-filter regression: Usage and Traffic now clear their own submitted
key when the project changes, including the rendered input and subsequent
query. Regression tests cover unchanged scope, another project, and All projects.
Browser verification changed Analytics plus its key to Order Operations in Usage:
the key cleared from the form, applied filters, and URL. Traffic saved history
changed Order Operations plus its key to Analytics: the key cleared and four
Analytics records loaded. The full verification script, workspace build, frontend
build/tests, and version 0.1.32 metadata validation passed after this change.
