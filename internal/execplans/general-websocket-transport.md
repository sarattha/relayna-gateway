# General service and LiteLLM passthrough WebSockets

This living ExecPlan follows `PLANS.md` at the repository root. Maintain Progress,
Surprises & Discoveries, Decision Log, and Outcomes & Retrospective throughout.

## Purpose / Big Picture

Operators can opt registered HTTP services and caller-owned LiteLLM passthrough
routes into WebSocket transport on the existing public listener. HTTP continues
to work. Relayna authenticates service connections and records transport usage;
LiteLLM authenticates its caller-owned keys and owns model governance, usage and
spend. Relayna-governed LiteLLM WebSocket sessions are explicitly deferred.

## Progress

- [x] (2026-10-20) Read repository guidance, required skills and existing transport.
- [x] (2026-10-20) Establish compatibility boundary and authorized scope.
- [x] (2026-10-20) Add validated, opt-in transport configuration and proxy admission.
- [x] (2026-10-20) Add operator controls and accurate protocol labels; document operation.
- [x] (2026-10-20) Add real gateway WebSocket regression coverage and configuration tests.
- [x] (2026-10-20) Run UI build/tests/typecheck and strict docs build; verify browser service save/readback at desktop/mobile.
- [x] (2026-10-20) Finish mandatory gateway verification in sequence: all 434 Cargo tests and all 434 Nextest tests pass; zero ignored/skipped.
- [x] (2026-10-20) Review diff and prepare PR draft summary on `t3/websocket-proxy-support`.
- [x] (2026-10-20) Pull official LiteLLM `latest` (v1.106.0), pass all 17 live interoperability checks, inspect transport/usage records and clean up the disposable stack.

## Surprises & Discoveries

- Existing Pingora upgrades are intentionally restricted to Accessa run routes.
  `accessa_transport.rs` already bounds frame/message sizes and connection counts.
- Service access settings and canonical route access settings are existing JSONB
  contracts. An optional field can preserve old records without SQL migration.
- GET `/v1/responses` currently falls through to wildcard routing; WebSocket
  requests must honor the canonical Responses mode, enablement and identity.
- Pingora can mark reused upstream connection failures retryable independently
  of provider fallback. Socket errors now explicitly disable both paths.
- Traffic keeps the first failure cause. Independent failure assertions need
  separate test diagnostics; the regression fixture now resets that state.
- The repository Semgrep command scans tracked files only, and Gitleaks scans
  Git history. Explicit supplemental scans cover the new module/test and all
  changed working-tree files. A public RFC handshake nonce tripped the generic
  API-key detector; the test now uses a valid low-entropy deterministic nonce.
- Nextest parallel load exposed a timing-sensitive one-second session-expiry
  fixture: admission plus whole-second rounding could expire it before the
  handshake. The E2E uses five seconds with a longer idle timeout; direct hook
  tests still prove exact expired-deadline rejection without sleeping.

## Decision Log

- 2026-10-20, Codex: user authorizes general transport and caller-owned LiteLLM
  delegation; defer Relayna-governed LiteLLM sessions to a later PR.
- 2026-10-20, Codex: compatibility boundary is v0.1.45. Add optional `websocket`
  settings to existing endpoint access JSON; absence preserves HTTP-only behavior
  and existing Accessa bindings. Keep public HTTP, credentials, Redis formats and
  billing schemas unchanged. Upgrade all replicas before enabling the new JSON.
- 2026-10-20, Codex: reuse bounded Pingora streaming and existing connection leases;
  no full-frame capture or message interpretation. Decline compression offers,
  reject unsolicited extension negotiation and malformed handshakes, and reject
  upgrades on unsupported/governed LiteLLM routes.
- 2026-10-20, Codex: general service sockets are transport-only, with admission
  policy, request rate and budget eligibility checks and a metadata-only usage
  event. They do not apply HTTP body pricing to socket messages. Mandatory
  content guardrails cannot be silently bypassed. LiteLLM caller sockets record
  Traffic diagnostics without creating Relayna key or billing identity.

## Outcomes & Retrospective

Implementation and verification are complete. Registered services and caller-owned
LiteLLM passthrough support opt-in WebSockets with bounded forwarding, credential
handling, connection/idle/session limits and transport diagnostics. Existing
Accessa and HTTP behavior remains covered; Relayna-governed LiteLLM sockets are
rejected as requested. The full mandatory stack passes in sequence on the final
tree, including 434 Cargo tests and 434 Nextest tests with zero ignored/skipped.

Verification exposed two test-fixture issues: sticky first-failure diagnostics
needed separate contexts, and a one-second expiry window was sensitive to
parallel admission load. Both were corrected. A public handshake nonce triggered
a supplemental secret-scan false positive and was replaced without adding an
exemption. Supplemental scans of the new files and complete working-tree patch
also pass. Changes remain local and uncommitted; no PR or deployment was created.

## Context and Orientation

Repository: `/Users/jobz/.t3/worktrees/relayna-gateway/t3-82851497`.
`crates/gateway-core/src/endpoint_access.rs` owns plain serialized endpoint access.
`crates/gateway-proxy/src/pingora_plane.rs` owns HTTP admission, proxy callbacks,
credentials and diagnostics. `accessa_transport.rs` owns bounded frame parsing and
per-instance connection leases; `accessa_runtime.rs` retains Accessa-specific
session/admission semantics. `gateway-store` already persists endpoint JSON.
Admin UI source is `crates/gateway-api/admin-ui/src/main.ts`; rebuild generated
assets with `npm run build:admin-ui`. Real gateway tests live under
`crates/gateway-api/tests/` and use disposable PostgreSQL and Redis.

## Compatibility Boundary

v0.1.45 is the latest local release tag. The new JSON option defaults to disabled.
Existing Accessa, HTTP routes, authentication profiles, service path rewriting,
credential substitution and LiteLLM path/method/exposure controls remain in force.
No database/Redis migration or compatibility shim is needed. Older binaries do
not understand the new field, so enable it only after upgrading all replicas.

## Plan of Work

Add optional validated limits to endpoint access JSON, restricted to service
registrations and LiteLLM wildcard/Responses route settings. Separate generic
socket admission from Accessa's app/channel session machinery. Admit only
caller-owned LiteLLM credentials, preserving canonical Responses route checks.
Reuse connection leases, frame parsing and lifecycle diagnostics. Add concise
operator forms and accurate saved protocol labels, then document identity,
timeouts, usage ownership and deployment requirements.

## Validation and Acceptance

Configuration tests prove defaults, limits, round trips and unsupported route
rejection. Real gateway tests prove HTTP coexistence; bidirectional text/binary,
ping/pong/close and subprotocol forwarding; caller credential forwarding; secret
stripping; authentication/policy/path denials; connection/frame/idle limits;
cleanup and diagnostics; and rejection of Relayna-governed LiteLLM sockets.
Run with disposable `DATABASE_URL` and `REDIS_URL`. Existing Accessa tests must
continue to pass.

Run `npm run build:admin-ui`, `npm test`, and browser checks at desktop/mobile.
Finish with `bash .codex/skills/code-change-verification/scripts/run.sh`, whose
fail-fast stack includes fmt, Clippy, workspace tests, audit/deny/machete,
Nextest, Trivy, Gitleaks and Semgrep. Repair failures and rerun from the start.

## Idempotence and Recovery

Use local disposable services, the official LiteLLM image and local provider
fixtures only. No production writes or paid provider calls.
Optional transport settings can be removed to disable new connections; existing
sockets follow their configured lifecycle. Do not retry or fail over upgraded
sessions. Connection leases release on rejected handshake, disconnect and errors.

## Artifacts and Notes

Final mandatory command passed with disposable `DATABASE_URL` / `REDIS_URL` and
`SSL_CERT_FILE=/etc/ssl/cert.pem`:

    bash .codex/skills/code-change-verification/scripts/run.sh

This includes fmt, Clippy, Cargo tests, audit, deny, machete, Nextest, Trivy,
Gitleaks and Semgrep. Explicit Semgrep scanning of the two new Rust files reports
zero findings. Gitleaks directory scanning of all 17 changed/new files reports no
leaks. Temporary gateway/browser sessions and PostgreSQL/Redis containers were
cleaned up.

New files (untracked until commit):
`crates/gateway-proxy/src/websocket.rs`,
`crates/gateway-api/tests/websocket_transport_e2e.rs`,
`docs/websocket-transport.md`, and this ExecPlan.

Focused real Pingora E2E passes against disposable PostgreSQL/Redis and a mock
upstream, including caller credential translation, per-key/route caps, frame
limits in both directions, idle/session expiry, upstream handshake failure,
mandatory guardrails, path/method/identity denials and canonical Responses modes.
Existing Accessa multi-hop E2E also passes.

Admin UI build, `npm test`, typecheck and all 54 React tests pass. Strict MkDocs
build and release metadata validation pass. T3 browser verification created a
local service with WebSocket enabled, saved a 45,000 ms idle timeout, reopened the
editor and confirmed exact readback. Desktop and 390 px mobile controls render
without horizontal page overflow. Caller-owned LiteLLM route controls and
Monitor/Discover/Govern navigation were inspected. Test-suite operator-token
rotation later invalidated the local UI session; no production identity was used.

Operational limits: per-process caps are not distributed; session expiry is
checked on traffic and idle I/O uses its configured timeout; no per-message
billing/content inspection or continuous key revocation check is added. Browser
clients use a credential-supplying backend. The initial regression suite uses mock upstreams. The live LiteLLM image
verification below additionally exercises the actual LiteLLM application, with
local fixtures standing in for external inference providers.


### Live LiteLLM latest-image verification (2026-10-20)

At the user's request, pulled `docker.litellm.ai/berriai/litellm:latest` and read
its installed package version with `importlib.metadata.version("litellm")`:
**1.106.0**, matching the latest published release at the time of this run
([v1.106.0](https://github.com/BerriAI/litellm/releases/tag/v1.106.0), published
2026-10-18). The pulled ARM64 image digest was:

    sha256:2e4c0371f3a8c28e4da131d77abaa8341cd3b0c2fcc4d34d7c5900431af7b9d0

Built the current gateway tree with `cargo build -p gateway-api --all-features`,
then ran the built executable with disposable PostgreSQL 16 and Redis 7. LiteLLM
used a separate database in the disposable PostgreSQL container and issued real
LiteLLM virtual keys. The test path was client -> Relayna/Pingora -> actual
LiteLLM container -> deterministic local HTTPS/WSS provider fixture. No external
provider inference, paid API credential or production service was used.

All **17/17** direct-baseline and gateway interoperability checks pass:

- HTTP Responses coexistence, directly and through Relayna.
- Native Responses WebSockets directly and through Relayna; two turns on one
  gateway connection, including `previous_response_id` round-trip rewriting.
- `/responses` alias and model selection from the first frame without a query.
- LiteLLM-managed Responses WebSockets over Anthropic-compatible HTTP SSE,
  directly and through Relayna. This is LiteLLM's provider bridge, separate from
  the deferred Relayna-governed session feature.
- Realtime WebSockets directly and through Relayna, including `session.created`
  and a client `session.update` / server `session.updated` exchange.
- Invalid LiteLLM virtual keys and model-restricted LiteLLM keys are rejected
  with HTTP 403 on both direct and proxied handshakes.
- Disabled canonical Responses routes, Relayna-governed Responses mode, and
  denied wildcard paths retain their expected 403/404 gateway rejections.

Responses subprotocol negotiation succeeds and extension offers are declined by
Relayna. Native text deltas arrive before completion (the fixture intentionally
spaces events), demonstrating incremental forwarding. All five successful
correlated gateway sockets record status 101, a closed lifecycle and frames in
both directions, with no Relayna key identity and zero Relayna per-key usage
rows. LiteLLM persists its own success/usage logs. Provider-side assertions confirm
that only the configured provider credential reaches the fixture.

The initial fixture used plain HTTP for LiteLLM's native provider backend, which
v1.106.0 rejected with `ssl argument is incompatible with a ws:// URI` on both
direct and proxied connections. Switching the local provider to HTTPS/WSS with a
trusted test certificate resolves it. The wildcard fixture initially inherited
the local gateway's unverified-bearer requirement, correctly producing
`litellm_authentication_conflict`; explicitly configuring caller-only access with
`skip_entra: true` resolves it. The final runner also waits for the gateway
listener before testing. None of these fixture corrections required a runtime
code change.

Final run tag: `1792492530`. Local evidence is in
`/tmp/relayna-litellm-live/results.json`, `results.log`, `traffic-results.json`,
`provider-events.jsonl` and `build.log`; the pull transcript is
`/tmp/relayna-litellm-pull.log`. The temporary test scripts are `provider.py`,
`setup.py` and `test_live.py` in that same temporary directory. The three
containers and local gateway/provider processes are stopped after verification;
temporary caller/master keys and the TLS private key are removed. The pulled
LiteLLM image remains available locally. Only this verification record changed
in the repository for the live-image follow-up; the previously completed full
mandatory stack remains applicable to the unchanged runtime/test patch.


### v0.1.46 release and PR follow-up

The user authorized a version bump, changelog/documentation updates, PR creation,
monitoring, and merging when checks and reviews are satisfied. Codex code review
may be requested at most twice. The released compatibility boundary remains
v0.1.45; v0.1.46 is the additive release candidate. Release metadata, deployment
examples and Admin UI version indicators are aligned to v0.1.46. Generated assets
are rebuilt from their TypeScript/React sources. The release notes describe
caller-owned LiteLLM governance, transport-only service accounting, strict JSON
rollout/rollback and actual LiteLLM v1.106.0 fixture-backed interoperability.

Release follow-up progress:

- [x] Bump workspace/lockfile and current image/UI/documentation references.
- [x] Add dated changelog and deployment/rollback instructions.
- [x] Run the full ordered verification stack, UI checks and strict docs build.
- [ ] Commit, push and open/link a reviewable PR; request Codex review (maximum 2).
- [ ] Address findings, confirm checks/review readiness and merge the PR.

This follow-up does not create a release tag or publish an image; the authorized
release operation is the versioned PR and its merge. CI and review requests are
tracked through the linked PR and T3's PR watcher.


Release candidate verification passed in full on v0.1.46: 434 Cargo tests and
434 Nextest tests, zero ignored/skipped, plus all ordered security checks.
`cargo build --workspace --all-features`, rebuilt UI tests/typecheck/React
coverage, strict MkDocs and `validate-release-metadata.py v0.1.46` also pass.
The staged patch directory secret scan reports no leaks. Evidence:
`/tmp/relayna-websocket-v0146-verification.log`,
`/tmp/relayna-websocket-v0146-build.log` and
`/tmp/relayna-websocket-release-ui-tests.log`. Disposable PostgreSQL and Redis
were stopped after the test runners completed. The candidate is ready for PR
publication; review requests and merge readiness remain tracked in the PR and
T3 watcher, with at most two Codex review requests authorized.
