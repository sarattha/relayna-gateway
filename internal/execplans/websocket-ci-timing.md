# Stabilize WebSocket E2E timing and patch TLS dependencies

This living ExecPlan follows `PLANS.md`. Keep Progress, Surprises & Discoveries,
Decision Log, and Outcomes & Retrospective current.

## Purpose / Big Picture

Make the WebSocket E2E verify connection caps while admitted sockets remain
live, then test idle closure separately. Pass mandatory verification and open a
focused follow-up PR for review and authorized merge. The user also approved
minimal dependency fixes after fresh TLS advisories blocked audit. Do not create
a release, change v0.1.46, or touch unrelated PR #137.

## Progress

- [x] (2026-10-20) Confirm the new thread/worktree starts clean at a089b57.
- [x] (2026-10-20) Give normal fixture phases 30 seconds of idle time; use one
  second only for idle closure and assert both sockets echo after the cap checks.
- [x] (2026-10-20) A disposable 1.5-second pause reproduces the original
  `ws-route-cap` HTTP 101 failure; the fixed probe and ordinary focused E2E pass.
- [x] (2026-10-20) Formatting, Clippy and all 434 Cargo tests pass with real
  PostgreSQL/Redis. Fresh advisories stop audit; user authorizes dependency fixes.
- [x] (2026-10-20) Investigate TLS paths and complete one independent candidate
  review. Confirm SQLx 0.9 rejects a password accepted by PostgreSQL/SQLx 0.8;
  remove that upgrade and all related source changes.
- [x] (2026-10-20) Precisely update rustls-webpki to 0.103.16 and the actual root
  data to 1.0.10. Keep SQLx 0.8.6 and its data-free 0.26.11 reexport wrapper.
- [x] (2026-10-20) Document the wrapper-only audit exception and pair it with
  version bans. The real graph passes; probes using 0.26.10 and 1.0.7 fail bans.
- [x] (2026-10-20) Patched upstream WebPKI distrust/cross-sign regression passes;
  the tested verifier source matches the resolved crate.
- [x] (2026-10-20) Repeat PostgreSQL TLS controls on the retained graph: trusted
  custom CA, encrypted queries/migrations, wrong-host and untrusted-CA rejection,
  and an ordinary trusted verify-ca connection all pass.
- [x] (2026-10-20) Pass the complete ordered stack: fmt, Clippy, 434 Cargo
  tests, audit, deny, machete, 434 Nextest tests with zero skipped, Trivy,
  Gitleaks and Semgrep. The all-features workspace build also passes.
- [ ] Open/link the PR, validate Linux CI, monitor review, and merge when ready.
- [x] (2026-10-20) Remove task-owned local containers and generated private keys.
- [ ] Restore Actions to disabled after the temporary hosted validation.

## Surprises & Discoveries

The original fixture shared a one-second idle deadline during sequential
Argon2/database admissions. Legitimate expiry releases capacity before the
route-cap assertion. No production cap or timeout correction is needed.

Fresh RustSec data reports RUSTSEC-2026-0382 in rustls-webpki 0.103.15 and
RUSTSEC-2026-0381 in roots 1.0.7 plus wrapper 0.26.11. The wrapper contains only
`pub use parent::TLS_SERVER_ROOTS`, so its actual data is fixed by upgrading the
1.x parent. The advisory version predicate still reports the wrapper.

SQLx 0.9 removes the wrapper, but a live SCRAM probe confirms that it rejects a
PostgreSQL-supported password containing U+E000. SQLx 0.8.6 authenticates and
queries with the identical URL. This rules out the broader SQLx upgrade here.

## Decision Log

- 2026-10-20, Codex: use a 30-second normal fixture deadline, one second only
  for idle closure; retain both HTTP 429 caps and explicit session expiry.
- 2026-10-20, user: include minimal TLS dependency fixes needed for validation.
- 2026-10-20, Codex: retain SQLx 0.8.6 after confirming the candidate regression.
  Update only the real verifier and root data. Keep trust sources, database
  authentication, SQL/bind behavior and all production WebSocket policy intact.
- 2026-10-20, Codex: allow the known data-free 0.26.11 wrapper, with a dated,
  documented RUSTSEC-2026-0381 exception in audit/deny. Paired cargo-deny bans
  reject roots below 0.26.11 and roots above 0.26.11 but below 1.0.10. Thus old
  actual root data cannot pass the required CI/local stack. Revisit by 2026-11-15.

## Outcomes & Retrospective

The fixture has reproduced-before/passed-after evidence. Precisely patched TLS
libraries close the reported paths while retaining released database behavior.
Candidate review prevented an unrelated password compatibility regression.
The final ordered stack and workspace build pass in full. Hosted Linux CI and
review/merge follow-up remain tracked in this T3 thread and the focused PR.

## Context and Orientation

Worktree: `/Users/jobz/.t3/worktrees/relayna-gateway/fix-websocket-ci-timing`;
branch: `fix/websocket-ci-timing`. The fixture is
`crates/gateway-api/tests/websocket_transport_e2e.rs`. `Cargo.lock` records the
only two dependency updates. `deny.toml` owns the guarded root-version policy;
`.github/workflows/ci.yml` and the Bash verification script use the same audit
exception and then run deny. No runtime Rust source changes remain.

## Compatibility Boundary

Latest release: v0.1.46. Public routes, transport admission/caps/timeouts, SQLx
version/features, database credentials, schemas, Redis formats and accounting
remain unchanged. TLS verification intentionally rejects the distrusted public
keys/root removed by upstream security fixes. No durable migration, version
bump, new release, image publication or tag rewrite is authorized.

## Plan of Work

Finish focused TLS controls, challenge both sides of the root policy, and run
the full ordered stack. Keep probe code and forced delays out of the patch.
Prepare a concise PR using the repository template and register it immediately.
Use T3's watcher for hosted CI/review and merge follow-up.

## Validation and Acceptance

Use task-owned PostgreSQL 16 and Redis 7 with real `DATABASE_URL` and `REDIS_URL`;
env-skipped E2Es are invalid. Run from the worktree root:

    cargo test -p gateway-api --test websocket_transport_e2e --all-features
    SSL_CERT_FILE=/etc/ssl/cert.pem bash .codex/skills/code-change-verification/scripts/run.sh

The script must pass in order: fmt, Clippy, Cargo tests, audit, deny, machete,
Nextest, Trivy, Gitleaks and Semgrep. Prove trusted custom-CA PostgreSQL access,
encrypted queries/migrations, wrong-host rejection and untrusted-CA rejection.
Verify patched WebPKI distrust and a legitimate certificate-chain control.

Hosted Linux Rust CI must use real PostgreSQL/Redis. Temporarily enable Actions
only for this validation and restore the initial disabled setting before merge
or handback. At most one additional explicit GitHub Codex review request remains.

## Idempotence and Recovery

Use fresh databases for probes that intentionally panic; remove probe source
before final checks. Preserve unrelated work and the original WebSocket worktree.
Stop/remove only this task's labeled containers and generated secrets. No paid
inference or production services are needed. After a required check fails, fix
its cause and restart the ordered script.

## Interfaces and Dependencies

Security references: [root-set correction](https://github.com/rustls/webpki-roots/pull/133),
[verifier correction](https://github.com/rustls/webpki/pull/546), and
[PostgreSQL SCRAM password fallback](https://www.postgresql.org/docs/current/sasl-authentication.html).
No new runtime abstraction, vendored dependency or compatibility shim is added.
