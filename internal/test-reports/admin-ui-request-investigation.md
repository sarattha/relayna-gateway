# Admin UI request investigation verification

Date: 2026-09-05. Branch: `codex/admin-ui-3-followup-fixes`, based on merged
Admin UI 3.0 commit `b97696d` (0.1.32). Changes remain uncommitted for followup work.

## Automated checks

- `npm run build:admin-ui` and `npm test`: passed, including shared investigation
  rendering, exact correlation with repeated client IDs, legacy/missing timings,
  failed streams after HTTP 200, escaping and allowlisted diagnostic copying.
- `.codex/skills/code-change-verification/scripts/run.sh`: passed in full on the
  final implementation: fmt, clippy, workspace tests, audit, deny, machete,
  nextest (331 passed), Trivy, Gitleaks and Semgrep.
- `cargo build --workspace --all-features`: passed.
- Explicit `traffic_monitor_integration` with local PostgreSQL and Redis: passed.
  It uses and cleans up an isolated database, covering early failures, fresh and
  reused HTTP, interrupted streaming, exact-ID lookup beyond 24 hours, saved
  usage/debug snapshots, admission failures and independent recording failures.
- New Rust regressions exercise DNS failure/timeout evidence and conditional
  retry resolution, actual TLS handshakes against an ephemeral local certificate,
  and rejection of a mismatched hostname. These run with workspace tests without
  requiring PostgreSQL or Redis.
- Additional Semgrep scan explicitly included the new, untracked Rust,
  TypeScript and test files: three targets, zero findings.

The TLS backend requires Pingora's transitive `rustls-pemfile` wrapper. Its
[maintenance advisory](https://rustsec.org/advisories/RUSTSEC-2025-0134.html) has
no patched release; a specific exception in `deny.toml` is documented with a
2026-10-05 revisit date. Existing unrelated baseline advisory exceptions remain.

## End-to-end HTTPS fixture

A separate native gateway used an isolated temporary database and an HTTPS
server at `localhost`. A generated CA was trusted only by that child process;
system trust and certificate verification were not changed. Fixture processes
and databases were removed after each run.

| Observation | Fresh HTTPS | Reused HTTPS | Reused HTTPS stream |
| --- | ---: | ---: | ---: |
| DNS | 0.803 ms | 0.379 ms | 0.698 ms |
| TCP | 0.162 ms | Reused | Reused |
| TLS | 5.635 ms | Reused | Reused |
| Response headers | 22 ms | 7 ms | 8 ms |
| First body byte | 22 ms | 7 ms | 8 ms |
| First content token | N/A | N/A | 175 ms |
| Attempt duration | 22 ms | 7 ms | 341 ms |

The stream sent a keepalive, a role-only event, then content. The token timestamp
correctly followed the initial body timestamp. An invalid certificate produced
a clean 502, exposing and then validating the conditional-retry fix.

## Chrome checks

- Traffic and Usage opened the same request (`demo-live-c57f538a-434d-46b2-ba58-dc42c72e9f7e`)
  with matching 344 ms total, 0.225 ms TCP, 15 ms headers, 16 ms first body and
  22 tokens. Policy, routing, project/key context and hashes were available.
- Both copy buttons reported success. Concrete endpoint paths and unknown
  Usage fields are excluded from the diagnostic snapshot by an explicit allowlist.
- A 401 invalid-key record showed authentication as the failure stage, zero
  upstream attempts, and an explicit explanation that no connection was attempted.
- An older seeded Usage row (`demo-v3-1-0194`) remained usable and explicitly
  reported missing timing/correlation instead of joining another request's data.
- At 390 × 844, document and dialog widths were 390 px without page overflow.
  Metrics remained legible and context fields stacked vertically.
- Final desktop Traffic drawer retained its visible header and Close action;
  the timeline fit its 670 px content area. Tab reached raw diagnostics, Enter
  expanded them, Tab wrapped to Close, and Escape restored focus to Inspect.
  Usage dismissal similarly restored focus to Debug.
- While additional live requests arrived, the selected completed request kept
  its expanded raw diagnostics and summary focus. Final mobile verification also
  retained the visible, fixed drawer heading with no page overflow.
- The earlier failure-group spacing is preserved at 16 px.

## Local demo and limits

The final native build is running behind `http://127.0.0.1:20381/admin-ui/`, with
the existing realistic project/service/key/usage dataset and a synthetic request
every ten seconds. The default demo upstream is HTTP on a literal IP; its DNS
and TLS fields correctly show that those phases are unnecessary. HTTPS timing
is verified separately above.

Local evidence logs and the TLS fixture are under ignored `target/local-demo/`.
The restart helper is `python3 target/local-demo/run.py`; use `--stop` to stop
the native gateway and request generator while retaining the database.

Timing is gateway-observed. Reused TCP/TLS connections have no new handshake
duration, incomplete/old measurements remain absent, and first-content-token
inspection is bounded to the first 64 KiB of supported SSE events per attempt.
No payload bodies or credentials are added to Traffic storage or copied diagnostics.
Existing routes and asset URLs are preserved; JSON additions have backward reads
and require no database migration.
