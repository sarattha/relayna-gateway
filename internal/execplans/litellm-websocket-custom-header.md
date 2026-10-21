# Preserve custom LiteLLM credentials on WebSocket upgrades

This living ExecPlan follows `PLANS.md`.

## Purpose / Big Picture

Allow caller-owned LiteLLM WebSockets to exchange messages when the active
provider uses `x-litellm-key`, including LiteLLM 1.100.1 with
`general_settings.litellm_key_header_name: x-litellm-key`. Preserve the selected
caller key and configured raw/bearer custom-header format, while supplying
`Authorization: Bearer <same caller key>` for LiteLLM's initial socket auth.
After Codex review, prepare the next patch release (v0.1.48, subject to a fresh
tag check), verify its metadata and packaging, merge PR #139, and publish the
release through the existing signed-image/GitHub release workflow.

## Progress

- [x] (2026-10-21) Read contributor instructions, manifesto and required skills;
  fetch origin and confirm clean HEAD d2ff478 equals origin/main and v0.1.47.
- [x] (2026-10-21) Inspect credential selection, socket admission and released
  HTTP/service behavior; select additive socket-only upstream header handling.
- [x] (2026-10-21) Implement the 12-line socket-only fix and regression coverage;
  focused real-service E2E passes. Removing the Authorization insertion makes
  the first custom-header upgrade fail with upstream 401 as expected.
- [x] (2026-10-21) Real LiteLLM 1.100.1 and pinned 1.106.0 each pass 16/16
  checks: actual multi-turn frames, alias/first-frame models, HTTP controls,
  spoofed credentials, Luna denial and credential-free Traffic accounting.
- [x] (2026-10-21) Complete the mandatory ordered stack with real PostgreSQL and
  Redis: fmt, Clippy, 437 Cargo tests, audit, deny, machete, 437 Nextest tests
  (zero skipped), Trivy, Gitleaks and Semgrep all pass. Workspace all-features
  build, strict MkDocs and release-metadata validation pass. Docs and Unreleased
  changelog updated. Task-owned containers, processes and private material removed.
- [x] (2026-10-21) Commit 3d67438, push the focused branch and open/link
  [PR #139](https://github.com/sarattha/relayna-gateway/pull/139). The committed
  patch also passes a scoped Gitleaks history scan.
- [ ] Request and monitor Codex review of PR #139; address any findings.
- [ ] After review, bump the next patch version, update changelog/release/upgrade
  documentation and image examples, and rerun mandatory verification.
- [ ] Validate final hosted CI/review state, merge, and publish the new tag/release.
- [ ] Verify published image/release artifacts and signatures; finish cleanup.

## Surprises & Discoveries

Latest release is v0.1.47, including the previous WebSocket timing/TLS fix.
Provider validation already blocks `authorization` as a custom header regardless
of case. HeaderMap lookup/replacement is case insensitive. Caller parsing removes
the Bearer prefix before credential selection, preventing double prefixing.
Explicit Codex review requests were used on both #136 and #138. The user's newer
instruction explicitly requests Codex review on this PR and release publication,
superseding the earlier review-budget and no-release scope for this task.
Actions was enabled on task entry; leave its setting unchanged.

Both actual LiteLLM versions reproduce custom-only WS403 / HTTP200 and
Authorization-only WS403 / HTTP401. Patched Relayna opens the socket and relays
delta/completed events for Sol. Luna query returns 403; a Luna first frame
returns an error frame without provider contact. The echo E2E's initial expected
400 for a malformed dual-identity request was corrected to the existing 401.

## Decision Log

- 2026-10-21, Codex: use `ctx.socket && ctx.direct_litellm_passthrough` after
  ordinary header preparation to add Authorization from the selected upstream
  credential. Both canonical Responses and wildcard routes set that flag only
  after selecting the caller's parsed key. Stored credentials and governed
  sessions cannot reach it. No new setting, fallback, retry or migration.
- 2026-10-21, user authorization: the brief explicitly authorizes this released
  wire-behavior change and preserving custom-header semantics; no additional
  approach confirmation is necessary.
- 2026-10-21, user: request Codex review, then bump version/changelog/docs, merge
  and create a release. This supersedes the original Unreleased-only handoff.
  Keep existing tags immutable and use the next available patch version. Make
  one fresh explicit Codex review request for PR #139 under this authorization.

## Outcomes & Retrospective

Implementation, focused E2E, both live-version matrices and mandatory
workspace/security checks pass. PR review/merge remain. Sanitized evidence is in
`internal/test-reports/litellm-websocket-custom-header/README.md` and its two JSON
result files. The separate uncorrelated Luna denial is not evidence of this
handshake bug and is outside the fix's explanation.

The complete ordered stack passed with real DATABASE_URL/REDIS_URL. Both Cargo
and Nextest ran all 437 tests; Nextest reported zero skipped. Existing audit
exceptions and scanner exclusions are unchanged. The all-features workspace
build, strict MkDocs and v0.1.47 release-metadata checks also pass. Task-owned
gateway/LiteLLM/provider processes and PostgreSQL/Redis containers are stopped
and removed; generated TLS private keys, container environment files and raw
server logs are removed. The original parent worktree remains clean.

PR #139 is mergeable, with hosted CI running and GitHub's review decision
`REVIEW_REQUIRED`. The latest user instruction authorizes a new explicit Codex
review request and expands the outcome to release publication after review.
Actions remains enabled exactly as it was on task entry; no setting was changed.

## Context and Orientation

Working directory:
`/Users/jobz/.t3/worktrees/relayna-gateway/fix-litellm-websocket-custom-header`;
branch `fix/litellm-websocket-custom-header`, based on origin/main d2ff478.

`crates/gateway-proxy/src/pingora_plane.rs` selects upstream credentials and
strips client headers in `prepare_upstream_authority_and_credentials`.
`upstream_request_filter` owns the outgoing request. `websocket.rs` admits only
caller-owned LiteLLM sessions or registered-service sessions. A caller-owned key
is a LiteLLM virtual key whose model permissions remain enforced by LiteLLM;
Relayna does not substitute stored keys or create per-key usage events for it.
`crates/gateway-api/tests/websocket_transport_e2e.rs` exercises real Pingora with
disposable PostgreSQL/Redis and an echo upstream. Documentation lives in
`docs/websocket-transport.md` and `docs/litellm-passthrough.md`.

## Compatibility Boundary

Latest release v0.1.47: additive Authorization only for caller-owned LiteLLM
WebSocket upstream handshakes. Preserve custom header names/value formats, HTTP
translation, registered-service/Accessa credentials, admission, model policy,
opaque frame streaming, and response/diagnostic redaction. No schema/Redis/API
shape changes or configuration knobs. Existing standard Authorization socket
requests still produce exactly one Bearer header.

## Plan of Work

Add the small socket-only header insertion in the Pingora upstream request
filter. Extend the echo E2E to require both headers for custom LiteLLM upgrades,
check raw/bearer formats and mixed-case names across canonical/wildcard routes,
prove bidirectional frames and credential stripping, and retain HTTP/service
controls. Add documentation and initially an Unreleased changelog entry. After
Codex review, move the entry into the next patch release, update workspace and
lockfile versions and current image/upgrade examples, and rerun the full stack.
Merge only a verified patch, then create a new annotated tag at the merged main
commit so the existing workflow publishes the signed image and GitHub release.
Use separate
task-owned live fixtures to exercise actual LiteLLM with a real Sol-only key and
deterministic local HTTPS/WSS provider. Capture sanitized results and versions.

## Concrete Steps

From this worktree, with disposable real DATABASE_URL and REDIS_URL:

    cargo test -p gateway-api --test websocket_transport_e2e --all-features
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    python3 scripts/validate-release-metadata.py v0.1.47

Run real LiteLLM 1.100.1 from the existing pinned PyPI venv using a new database,
fresh fixture-only master key and generated virtual key limited to gpt-6.1-sol.
Run the cached 1.106.0 image by digest rather than mutable latest. Test HTTP,
socket multi-turn responses, absent model query, spoofed alternate headers and
denied gpt-6-luna in both query and first frame. No paid calls.

## Validation and Acceptance

Custom-header HTTP remains 200 without upstream Authorization. Custom-header
WebSocket now returns 101 and relays response delta/completed frames for Sol.
Both upstream auth headers use the same caller key; inbound alternate keys,
Relayna keys and Entra proof do not become compatibility credentials. Denied
Luna remains denied and does not reach the provider. Standard Authorization
contains one value; no Bearer Bearer. Service sockets keep service-only auth.
Traffic and response headers contain no credentials. All mandatory checks pass
in order, with no env-skipped database/Redis integration tests.

## Idempotence and Recovery

Use uniquely named task containers/databases and fresh schemas; do not modify
other worktrees or stop unrelated services. Remove only task-owned services,
keys and raw secret-bearing logs on cleanup. Restart the ordered verification
script after fixing a failure. Preserve sanitized evidence. Use T3 PR linking
immediately after creation and its watcher for monitoring. The newest user
instruction authorizes Codex review and a new patch release. Never rewrite old
release tags. Inspect remote tag/release state before retrying publication.

## Artifacts and Notes

Previous reproduction under /tmp/relayna-litellm-11001-repro proves 17 expected
statuses, but positive socket evidence there was handshake-only. This task must
add frame-exchange evidence. Cached 1.106.0 image digest:
sha256:2e4c0371f3a8c28e4da131d77abaa8341cd3b0c2fcc4d34d7c5900431af7b9d0.

## Interfaces and Dependencies

No new dependencies or exported interfaces. Use existing Pingora request header
replacement, credential parsing and caller-owned admission flags. PostgreSQL,
Redis, PyPI LiteLLM 1.100.1 and cached Docker LiteLLM 1.106.0 are local test
dependencies only.
