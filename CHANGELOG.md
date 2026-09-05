# Changelog

All notable changes to Relayna Gateway are documented in this file.

## 0.1.33 - 2026-09-06

### Added

- Shared Usage and Traffic request investigation with safe diagnostic snapshots,
  exact internal request correlation, and per-attempt DNS, TCP, TLS, response
  headers, first body byte and first content token timings.
- Persistent field labels and explanations, including blank/zero semantics,
  plus accessible help tooltips that support keyboard, hover, click and Escape.
- Recorded routing mode in request diagnostics and terminal logs. Usage, Traffic
  and investigation explicitly identify LiteLLM passthrough and gateway metering.

### Fixed

- Enable Pingora's verified Rustls upstream HTTPS backend and resolve retry
  decisions on connection/proxy failures.
- Keep sidebar session controls at the viewport bottom, space adjacent controls
  and align table headings and filter actions.
- Allow Traffic failure filters and Usage breakdown selections to be cleared
  by clicking the selected button again; preserve unrelated filters and the
  all-breakdowns choice on refresh.
- Install frontend dependencies in CI, verify rebuilt static assets, and run
  the complete frontend test suite on clean runners.

### Changed

- Workspace, portal indicators and deployment examples target `0.1.33`.
- New diagnostic fields are additive and older records remain readable. Exact
  Traffic ID lookup can retrieve records outside the default history window.
  Existing routes, database schemas and Admin UI asset URLs remain unchanged;
  no migration is required.

## 0.1.32 - 2026-09-05

### Added

- Admin UI 3.0 with grouped navigation, project scope, contextual creation and
  editing drawers, usage breakdown tabs and request investigation drawers.
- Regression coverage for request-body timeouts, canceled navigation, scope
  changes, key expiration, reliability signals and identity deletion.

### Fixed

- Readiness failures no longer erase independently available monitoring data;
  failed sources show unavailable or timestamped stale results with retry.
- Expired keys no longer count as active, and reliability selects the strongest
  sampled error, timeout or fallback signal separately from current availability.
- Obsolete requests cannot replace newer views. Canceled navigation preserves
  workspace, drafts and scope; project changes clear incompatible key filters.
- Service-name validation and service/project identity deletion after confirmation.

### Changed

- Workspace, Admin UI indicators and deployment examples target `0.1.32`.
- Existing admin/owner APIs, authorization, database schemas, usage formats and
  `/admin-ui` asset paths are preserved. This release requires no migration.

## 0.1.31 - 2026-09-03

### Added

- Admin UI Monitor → Traffic with authenticated live updates, pause/resume,
  filters, failure reason counts, request/attempt timelines, and saved history.
- Bounded metadata diagnostics from request arrival, including anonymous and
  unresolved-route failures, per-process identity, replay gaps and disconnections.
- Additive request history storage and usage failure stage/code/source, outcome,
  upstream status and instance attribution.

### Fixed

- Early proxy failures no longer disappear from diagnostics when no route or
  virtual key is available.
- Failed or timed-out usage, debug-bundle and diagnostic writes emit structured
  errors and appear in live recording status. Failure logs precede database writes.
- Interrupted streams remain failures even when HTTP 200 was already delivered.
- Body-processing rejections finish promptly and close non-reusable HTTP/1
  connections before the client sends its next request.
- Gateway errors include request-ID headers and generic proxy failures use a
  correlated JSON envelope; upstream bodies remain passthrough.

### Changed

- Release metadata and deployment examples target `0.1.31`.
- Live diagnostics are instance-scoped and bounded; database history spans
  instances and uses operator-managed retention. No credentials or bodies are
  captured by the monitor.

## 0.1.30 - 2026-08-28

### Added

- Added governed LiteLLM reranking for `POST /rerank`, `POST /v1/rerank`, and
  `POST /v2/rerank`. All aliases share the canonical `/v1/rerank` policy and
  operator route setting while preserving the requested path upstream.
- Added exact export-only start and end timestamps to the Admin UI Usage view.
- Added bounded **All rows** CSV and JSON downloads that retrieve matching
  usage events in ordered 10,000-row API batches without raising the released
  server-side maximum.

### Changed

- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.30` and `v0.1.30`.
- Usage exports now use a total PostgreSQL order of creation time, request ID,
  and internal unique usage-event ID so repeated offset batches cannot omit or
  duplicate tied rows.
- All-row export mode requires a bounded time window and is download-only;
  Preview, Copy URL, and Copy curl remain single-request operations.

### Fixed

- Entra-authenticated Admin UI sessions now send their session CSRF token on
  usage export requests, while break-glass sessions continue to send the
  operator bearer token.
- Export-specific timestamps are validated before inherited Usage timestamps,
  so a valid export override is independent of stale or invalid custom Usage
  fields.
- Usage export controls now use a responsive desktop and narrow-screen layout,
  and incompatible controls are disabled when All rows is selected.

### Security

- Client and provider credential handling is unchanged for rerank aliases;
  governed requests retain policy, rate-limit, budget, guardrail, credential
  stripping/injection, and usage-accounting boundaries.
- Each all-row export request remains capped at 10,000 rows, and the browser
  requires an explicit bounded start and end time before issuing repeated
  requests.

## 0.1.29 - 2026-08-26

### Added

- Added the opt-in `ENTRA_AUTH_DEBUG` structured incident trail for direct
  request-plane Entra JWTs, trusted Apigee identity, browser portal OIDC,
  service/project owner managed identities, and portal cookie sessions.
- Added precise discovery, JWKS, key selection, signature, issuer, tenant,
  audience, timestamp, scope, role, group, binding, session-persistence,
  cookie-emission, browser-return, membership, CSRF, logout, and Relayna-key
  phase/reason diagnostics correlated by Gateway request ID.
- Added a full operator runbook with event schemas, all emitted evidence,
  Docker and Kubernetes procedures, cookie-failure interpretation, sensitive
  data boundaries, retention guidance, and local request examples.

### Changed

- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.29` and `v0.1.29`.
- Local Compose accepts explicit debug, direct request-plane Entra, and trusted
  Apigee overrides so all authorization surfaces can be exercised against the
  development issuer.
- Clarified the Entra/DevOps handoff checklist so every bootstrap administrator
  must match both the verified email and immutable object-ID allowlists before
  those temporary values are removed.

### Fixed

- Portal session and CSRF cookie headers are now constructed atomically and
  fail closed instead of silently creating a durable server session with
  missing or partially emitted browser credentials.
- Portal diagnostics now distinguish database-session creation, header
  construction, header emission, browser return, session resolution, stale or
  mixed cookies, and CSRF failures.

### Security

- Debug mode defaults off and is enabled only by the process environment. Its
  dedicated warning-level target works independently of ordinary `LOG_LEVEL`
  filtering, while startup logs warn that decoded claims can contain personal
  data.
- Compact access/ID tokens and client assertions, signature bytes, OAuth codes,
  state, nonce values, PKCE material, cookie/CSRF values and hashes, private
  keys, trusted Apigee headers/signatures, raw Relayna keys, credentials,
  prompts, and bodies are never logged. OAuth transaction claim values are
  redacted, and Entra provider errors use an explicit safe-field allowlist.
- Authorization-debug detail payloads are bounded to 64 KiB, public error
  responses remain stable, and no diagnostic field is added to Prometheus
  labels or browser-visible session responses.

## 0.1.28 - 2026-08-18

### Added

- Added exact Owner and Viewer project memberships for Entra-authenticated
  portal users, with read-only project cards, dashboards, request logs,
  sanitized request details, endpoint views, and usage exports.
- Added exact project bindings for managed identities. Project monitoring
  reuses the existing `gateway.monitor.read` Entra application role and still
  requires a matching enabled Relayna binding.
- Added `/owner/v1/projects/{project_id}/*` APIs and administrator controls for
  human project assignments and project-monitoring workloads.
- Added regression coverage for server-enforced project scoping, cross-project
  request concealment, durable PostgreSQL access state, and the shared-role
  Entra workload flow.
- Added an isolated local Compose inspection stack with Entra-shaped test
  personas, a mock upstream, and idempotent project-owner usage fixtures.

### Changed

- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.28` and `v0.1.28`.
- Renamed the portal's Service owner workspace label to Owner because the same
  read-only workspace now supports service and project ownership.

### Security

- Project owner queries overwrite caller-supplied `project_id` filters with the
  authorized route project before reading usage. Request IDs from other
  projects use the same not-found response as missing IDs.
- Debug bundles now retain project attribution and are included in project
  request details only when that attribution exactly matches the authorized
  project. Legacy or otherwise unattributed bundles remain operator-only.
- Guardrail block and action counts now join usage on request, virtual key, and
  project attribution so a reused client request ID cannot cross project
  boundaries in dashboards, event rows, or exports.
- Project visibility follows the usage event's persisted `project_id`, not
  service links, so a service shared by multiple projects cannot expose another
  project's traffic.

## 0.1.27 - 2026-08-18

### Added

- Added an additive project, virtual-key, and service usage breakdown to the
  admin usage dashboard so operators can identify which services each project
  consumes through each safe virtual-key prefix.
- Added a compact expandable Project → Virtual key → Service hierarchy that
  honors the existing usage filters, sort order, and top-result limit.
- Added backend aggregation, filter, and Admin UI regression coverage for the
  new hierarchy.

### Changed

- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.27` and `v0.1.27`.

### Security

- The hierarchy resolves virtual-key IDs to stored prefixes only and never
  renders raw virtual-key material. Existing `usage:read` authorization remains
  required, and the change adds no new route or persisted data shape.
- Updated the transitive `h2` dependency to `0.4.16` to address
  `RUSTSEC-2026-0258` (unbounded empty DATA frames).

## 0.1.26 - 2026-08-09

### Added

- Added a shared Entra application contract with separate `gateway.invoke` and
  `gateway.monitor.read` application roles for least-privilege request-plane
  and service-monitoring managed identities.
- Added development OIDC coverage for two distinct workload identities that
  request the same API resource and receive only their assigned role.

### Changed

- Portal OIDC, direct Entra front-door authorization, and owner monitoring now
  use one confidential Web/API application ID through
  `ENTRA_APPLICATION_ID`, following Arcweft's single-registration pattern.
- Replaced `ENTRA_AUDIENCE`, `PORTAL_OIDC_CLIENT_ID`, and
  `OWNER_ENTRA_AUDIENCE`; operators must merge the former Web and API
  configuration before upgrading and request workload tokens for
  `api://<application-id>/.default`.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.26` and `v0.1.26`.

### Security

- Entra v2 access tokens remain subject to exact tenant, issuer, signature,
  timestamp, and application-ID audience validation. The invoke identity still
  requires Relayna virtual-key policy, while monitoring identities still
  require an enabled exact service binding.
- Portal client authentication remains certificate-backed PS256
  `private_key_jwt`; no client secret or delegated product scope is introduced
  by the shared registration.

## 0.1.25 - 2026-08-09

### Added

- Added a responsive service-owner incident chart that plots error rate and P95
  latency while marking validated `X-Relayna-Service-Version` transitions at
  the time Gateway first observes them.
- Added All, Success, and Failure request outcomes; exact status-code filters;
  6-hour, 24-hour, and 7-day ranges; and offset pagination to the service-owner
  request log.
- Added an exact-service request-details API and accessible details drawer that
  return sanitized usage metadata with an optional matching redacted debug
  bundle.

### Changed

- Usage events now persist an optional bounded service version, and usage
  summaries and time-series buckets include P95 latency.
- Service-owner request actions now say **View details** and remain useful when
  demo or historical usage rows do not have a debug bundle.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.25` and `v0.1.25`.

### Fixed

- Fixed the service-owner dashboard's missing incident visualization and
  failure-only request list.
- Fixed reversed request status badges so `success` and `failure` are visible
  labels while `good` and `bad` remain visual tones only.
- Fixed owner request actions that were rendered without click handlers and
  incorrectly depended on the administrator-only debug route.

### Security

- Owner request details require exact service membership, return identical 404
  responses for absent and cross-service request IDs, and exclude request
  bodies, prompts, credentials, raw headers, and unredacted provider errors.
- Invalid or oversized service-version headers are ignored without failing or
  altering proxied responses, including streaming responses.

## 0.1.24 - 2026-08-09

### Added

- Added Microsoft Entra confidential-client OIDC sign-in for the browser portal
  with server-side sessions, pending-member approval, administrator roles, and
  exact Owner or Viewer assignments for registered services.
- Added service-owner views under `/admin-ui` and scoped
  `/owner/v1/services/{service_name}/*` APIs for dashboard aggregates, usage,
  sanitized request and error logs, endpoint breakdowns, and exports.
- Added administrator member management and managed-identity registration for
  workload monitoring. Workload tokens require the configured tenant,
  audience, application role, and an enabled exact service binding.
- Added a development-only OIDC issuer with pending, administrator, and
  service-owner personas for repeatable local authentication testing.
- Added an operations guide for Entra portal rollout, first-administrator
  bootstrap, managed identities, and the development issuer.
- Added a DevOps handoff that inventories the two Entra applications, the
  `gateway.monitor.read` application role, managed-identity count and binding
  rules, issuer settings, raw Kubernetes inputs, and certificate lifecycle.

### Changed

- Existing operator tokens remain supported as emergency break-glass access,
  while normal human administration and service ownership can use Entra.
- The Admin UI now derives navigation and command-palette entries from the
  server-authorized principal, keeps all browser pages under `/admin-ui`, and
  uses Microsoft's official sign-in branding.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.24` and `v0.1.24`.
- Portal confidential-client authentication now uses certificate-backed PS256
  `private_key_jwt` assertions instead of a client secret, following Arcweft's
  short-lived assertion and `x5t#S256` pattern.

### Fixed

- Pending and blocked members can keep emergency operator access available or
  sign out and switch accounts without manually clearing cookies.
- Portal OIDC state is bound to the initiating browser, abandoned login
  transactions and expired portal sessions are pruned, and sign-out completes
  at the identity provider.
- Portal cookies are stripped at the proxy boundary before requests reach
  LiteLLM or registered upstream services.
- Service-owner command navigation is role-scoped, and the owner sidebar keeps
  compact normal-sized navigation items across desktop and mobile layouts.
- The production control Ingress now routes `/owner/v1`, and its NetworkPolicy
  admits only namespaces explicitly labeled for control-plane access.

### Security

- Entra tokens remain server-side; browsers receive opaque HttpOnly sessions,
  and cookie-authenticated mutations require a session-bound CSRF token.
- OIDC callbacks validate state, nonce, PKCE S256, issuer, tenant, audience,
  signature, not-before, and expiry before creating a Relayna session.
- Service visibility is enforced by server-side membership and managed-identity
  binding checks; client-supplied service filters cannot broaden access.
- First-admin ConfigMap bootstrap requires the configured tenant, immutable
  Entra object ID, and email; invalid or mismatched certificate/key material
  fails startup.

## 0.1.23 - 2026-08-06

### Added

- Added endpoint-level usage metadata for authenticated registered-service
  traffic: uppercase HTTP method, query-free upstream-relative concrete path,
  and the most-specific matching synced OpenAPI template.
- Added exact `method`, `endpoint`, and numeric `status_code` usage filters,
  endpoint filter discovery, `METHOD /path` endpoint breakdowns, and matching
  JSON/CSV export fields.
- Added Admin Usage controls and request tables for filtering and inspecting
  successful and failed service endpoints, including concrete fallback paths
  for operations outside the synced OpenAPI catalog.

### Changed

- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.23` and `v0.1.23`.
- Endpoint matching is independent of pricing, so free and unpriced OpenAPI
  operations remain observable.

### Fixed

- Numeric status-code filters now deserialize consistently when embedded in
  flattened filter-value discovery queries.
- The endpoint lookup index uses a bounded digest of the effective endpoint
  while queries retain full-value equality verification.

### Security

- Endpoint usage metadata excludes query strings, credentials, bodies, and
  prompt data. Prometheus metrics remain free of endpoint labels.
- The migration is additive and idempotent; historical usage rows remain
  readable with nullable endpoint metadata and are not heuristically backfilled.

## 0.1.22 - 2026-07-25

### Added

- Added process-wide admission control for requests and responses that retain
  complete bodies in memory. Operators can bound both simultaneous buffered
  work and aggregate serialized body bytes with
  `GATEWAY_MAX_BUFFERED_REQUESTS` and
  `GATEWAY_MAX_INFLIGHT_BUFFER_BYTES`.
- Added bounded Prometheus gauges and rejection counters for buffered body
  admission, plus a stable retryable `503 gateway_overloaded` response when
  process capacity is exhausted.
- Added a streaming-safe request path for registered non-JSON services whose
  pricing and effective pre-call guardrails do not require the complete body.

### Changed

- Managed JSON metadata is analyzed without repeatedly materializing ignored
  payload fields, and unchanged buffered request bodies are moved into Pingora
  without an additional full-body copy.
- Body-admission defaults allow eight simultaneously buffered requests and
  512 MiB (`536870912` bytes) of aggregate buffered request and response data.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.22` and `v0.1.22`.

### Fixed

- Response-side admission now occurs before downstream headers are committed,
  preserving the structured overload response instead of returning a truncated
  upstream success or generic proxy failure.
- Admission leases release request and byte reservations on completion,
  rejection, error, and request-context drop.

### Security

- Process-wide body limits reduce the risk that concurrent large managed
  requests exhaust pod memory while preserving route-level payload limits,
  credential stripping, policy enforcement, and usage accounting.
- Streaming-safe service uploads remain restricted to routes that require no
  body-dependent pricing or pre-call guardrail inspection.

## 0.1.21 - 2026-07-22

### Added

- Added authenticated OpenAPI 3.x endpoint discovery for registered services.
  Operators can preview and explicitly sync a relative `/openapi.json` source,
  review endpoint drift, and persist a durable method/path catalog without
  adding OpenAPI fetches to the proxy request path.
- Added per-endpoint service billing with `none`, `fixed`, and `passthrough`
  cost modes. Relayna runtime, status, event, DLQ, failed-task, execution, and
  health operations default to `none`, while operators can override every
  discovered operation.
- Added multipart request-body pricing selectors so non-file UTF-8 fields such
  as `engine=docint` can select an existing JSON Pointer pricing rule such as
  `/engine`, while uploaded files stay outside pricing metadata.
- Added a dedicated OpenAPI service pricing reference covering Admin UI and API
  workflows, cost precedence, budgets, drift, troubleshooting, and security.

### Changed

- Billable endpoint prices compose with existing service body selectors, so
  OCR `POST /ocr` can retain its service base price while `engine=docint`
  resolves to a fixed `$0.50` rule. An endpoint explicitly set to `none` skips
  unrelated body-selector budget ceilings.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.21` and `v0.1.21`.
- Workspace line coverage is enforced at 95% through dependency-backed Admin
  API, PostgreSQL, Redis, Pingora, startup, policy, and telemetry contracts.

### Fixed

- Multipart service requests no longer fall back to the default fixed price
  when a bounded non-file form field matches a configured pricing selector.
- Filtered guardrail execution queries now build valid PostgreSQL predicates
  for all supported filter combinations.

### Security

- OpenAPI discovery is limited to a relative path on the registered upstream
  origin, disables redirects, omits service credentials, accepts only bounded
  JSON documents, ignores external references, requires `services:update`, and
  audits successful syncs.
- Endpoint billing does not grant endpoint access; virtual-key service policy,
  registered allowed methods, rate limits, budgets, and credential stripping
  remain enforced independently.

## 0.1.20 - 2026-07-18

### Added

- Added inline registered-service timeout controls to the Admin UI Routes view.
  Routes and Services now edit the same persisted `timeout_ms` value with the
  existing `1..=600000` millisecond validation.
- Added structured HTTP 504 `upstream_timeout` JSON responses when a configured
  upstream timeout expires before response headers are committed.

### Changed

- Committed streams retain their original response status when a later
  upstream timeout terminates the stream, while debug bundles record
  `terminal_error=upstream_timeout` for diagnosis.
- Workspace crate versions, Admin UI release indicators, deployment examples,
  and release documentation now target `0.1.20` and `v0.1.20`.

### Fixed

- Terminal timeout failures now create exactly one usage event with status 504
  instead of allowing Pingora's repeated completion callback to insert a
  duplicate row.
- Structured Gateway proxy errors now explicitly return
  `Content-Type: application/json` with no-store caching semantics.

### Security

- Service credentials remain write-only and stripped before upstream calls;
  timeout debug evidence contains bounded operational metadata without raw
  request bodies, responses, virtual keys, or provider credentials.

## 0.1.19 - 2026-07-10

### Added

- Added a live Admin UI Overview that combines usage charts, gateway posture,
  provider-health attention signals, recent operations, and governed-change
  shortcuts from existing Admin APIs.
- Added hash-backed deep links, command navigation, responsive drawer
  navigation, and accessible modal focus, Escape, and restoration behavior.
- Added Chart.js and Tabler Icons Webfont to the embedded Vite bundle, including
  explicit immutable-cache responses for generated font assets.

### Changed

- Refreshed Admin UI 2.0 with the Aurora Teal palette, denser shared components,
  progressive disclosure for long forms, and clearer pending/action feedback.
- Workspace crate versions now share the `0.1.19` release version.
- Deployment examples and release documentation now target the `0.1.19`
  gateway image and `v0.1.19` release tag.

### Fixed

- Statusless provider-health rows with no failure signal no longer inflate the
  Overview risk count.
- Modal close operations now select the top matching dialog independently of
  later notification sections in the page.

### Security

- The redesign preserves operator-token authorization, write-only and
  show-once secret handling, destructive-action confirmation, and all existing
  Admin API contracts.

## 0.1.18 - 2026-07-05

### Added

- Added independent Admin UI pagination for Usage Recent requests, Timeseries,
  and Service timeseries sections so large usage logs stay scannable.
- Added dashboard time-series pagination query parameters and response metadata
  while preserving existing `timeseries` and `service_timeseries` response
  fields for existing dashboard consumers.

### Changed

- The Admin UI Usage filters now expose a shared rows-per-page control used by
  the three paged Usage sections, and applying filters resets each section to
  the first page.
- Workspace crate versions now share the `0.1.18` release version.
- Deployment examples and release documentation now target the `0.1.18`
  gateway image and `v0.1.18` release tag.

### Security

- Usage pagination does not change virtual-key authentication, operator-token
  authorization, provider credential handling, or usage export permissions.

## 0.1.17 - 2026-07-05

### Added

- Added a structured Admin UI pricing-rule editor for service registrations so
  operators can add and remove rules without hand-editing JSON.
- Added operator-facing pricing-rule guidance and examples that clarify
  `json_pointer` values use JSON Pointer selectors such as `/model` and
  `/payload/page_count`; request bodies still use normal key names.

### Changed

- The Admin UI Services page now stacks Create service and Edit service panels
  vertically and wraps pricing-rule controls inside the service form to avoid
  horizontal overflow.
- Workspace crate versions now share the `0.1.17` release version.
- Deployment examples and release documentation now target the `0.1.17`
  gateway image and `v0.1.17` release tag.

### Security

- Service credentials remain write-only while pricing-rule edits continue to use
  the existing service registration payload contract.

## 0.1.16 - 2026-07-04

### Added

- Added configurable timeout, request payload size, and response payload size
  limits for canonical OpenAI-compatible and Anthropic-compatible direct
  LiteLLM passthrough routes.
- Added matching wildcard LiteLLM passthrough limits so operators can raise the
  route-level 1 MiB default for long-context Codex and model harness traffic
  without changing virtual-key policy.
- Added Admin UI controls and Admin API route config endpoints for editing
  passthrough route mode and payload limits in one flow.

### Changed

- Route-level response byte limits are now enforced while response chunks pass
  through the proxy, avoiding full-response buffering.
- Workspace crate versions now share the `0.1.16` release version.
- Deployment examples and release documentation now target the `0.1.16`
  gateway image and `v0.1.16` release tag.

### Security

- Virtual-key policy request and response payload limits remain stricter when
  configured, so operators can set large route defaults for Codex while still
  constraining individual keys or policy layers.

## 0.1.15 - 2026-07-02

### Added

- Added native Anthropic-compatible LiteLLM route settings for Claude and
  Claude Code traffic, including `/v1/messages`,
  `/v1/messages/count_tokens`, `/v1/messages/batches`,
  `/v1/messages/batches/*`, `/v1/messages/batches/*/results`,
  `/v1/messages/batches/*/cancel`, and `/v1/models`.
- Added Admin API and Admin UI route controls for Anthropic Claude routes with
  clear OpenAI and Anthropic sections on the Routes page.
- Added PostgreSQL seed data for Anthropic route settings so operators can
  enable, disable, and switch direct LiteLLM passthrough mode per route.

### Changed

- Direct LiteLLM passthrough mode now applies to both OpenAI-compatible and
  Anthropic-compatible canonical LiteLLM routes while preserving Relayna auth,
  policy, rate-limit, budget, and credential translation behavior.
- Workspace crate versions now share the `0.1.15` release version.
- Deployment examples and release documentation now target the `0.1.15`
  gateway image and `v0.1.15` release tag.

### Security

- Client Relayna, Entra, Apigee, and non-Relayna LiteLLM credentials remain
  stripped or translated before upstream forwarding; Anthropic direct
  passthrough records status-only usage like OpenAI direct passthrough.

## 0.1.14 - 2026-06-20

### Changed

- The Admin UI Policy simulator now blocks incomplete internal-service
  simulations before they reach the backend. Service simulations must use a
  concrete path matching the selected service's configured route pattern or a
  `/services/service-name/...` path, and selected service names must match the
  service segment when the `/services/*` route is used.
- The Policy simulator now clears stale route/provider results before
  validation failures and before new simulation requests, avoiding misleading
  LiteLLM denial output after an operator selects an internal service.
- Workspace crate versions now share the `0.1.14` release version.
- Deployment examples and release documentation now target the `0.1.14`
  gateway image and `v0.1.14` release tag.

### Security

- The service-path validation is client-side only and does not change gateway
  policy enforcement, provider credential handling, persisted schemas, or
  runtime route behavior.

## 0.1.13 - 2026-06-20

### Added

- The Admin UI sidebar now shows the current Relayna Gateway version as a
  persistent `v0.1.13` indicator.
- Policy simulation now returns operator-facing warnings and applied-layer
  details when effective allowlists exclude a simulated request.

### Changed

- The real LiteLLM passthrough fixture now mirrors production topology by
  connecting Relayna Gateway directly to LiteLLM without the test-only
  front-door service.
- Trusted-ingress LiteLLM passthrough now classifies the current LiteLLM
  dashboard route groups, including provider, guardrails, MCP, prompts, files,
  model hub, utility, and v2 dashboard APIs, once operators explicitly
  allowlist the matching methods and paths.
- The Admin UI now makes raw versus bearer custom LiteLLM header values clearer,
  including the common `x-litellm-key: Bearer <key>` deployment shape.
- Active docs, skills, CI/release guidance, Admin UI release posture text, and
  tests no longer reference the obsolete freeze-perimeter workflow.
- Workspace crate versions now share the `0.1.13` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.13` gateway image.
- Release documentation, workflow checks, and operational checklists now target
  `v0.1.13`.

### Security

- Direct LiteLLM fixture validation now proves Gateway injects the configured
  LiteLLM credential header itself when forwarding to LiteLLM.
- Release validation continues through release metadata checks, Admin UI tests,
  and the standard gateway verification stack.

## 0.1.12 - 2026-06-19

### Added

- Added `credential_header_value_format` for LiteLLM provider configs with
  `raw` and `bearer` values. Custom LiteLLM credential headers can now send
  `x-litellm-key: Bearer <credential>` for deployments that require a bearer-
  prefixed custom header value.

### Changed

- Existing custom LiteLLM credential headers keep the `raw` value format by
  default, preserving `x-litellm-api-key: <credential>` behavior for current
  deployments.
- LiteLLM service fallback credentials, key/project credential mappings, direct
  LiteLLM bearer delegation, and the LiteLLM UI proxy all use the configured
  custom-header value format consistently.
- Workspace crate versions now share the `0.1.12` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.12` gateway image.
- Release documentation, workflow checks, and operational checklists now target
  `v0.1.12`.

### Security

- Gateway still strips client-supplied Relayna, Authorization, API-key, worker,
  Apigee/Entra, and configured LiteLLM credential headers before injecting the
  resolved upstream LiteLLM credential.
- Credential values remain write-only in Admin API responses and the Admin UI.

## 0.1.11 - 2026-06-19

### Added

- Added direct LiteLLM bearer delegation for canonical
  `direct_litellm_passthrough` routes. Non-Relayna `Authorization: Bearer ...`
  credentials can now be translated to the configured LiteLLM upstream header
  instead of being rejected by Gateway virtual-key auth.
- Added trusted-ingress LiteLLM dashboard/admin API passthrough coverage for
  explicitly exposed, allowlisted admin paths so browser sessions can remain
  governed by LiteLLM when an external identity-aware ingress already protects
  access.

### Changed

- Updated the freeze perimeter check and related workflow/docs references to the
  new `v0.1.11` baseline test file `tests/freeze-v0.1.11-perimeter.test.mjs`.
- Workspace crate versions now share the `0.1.11` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.11` gateway image.
- Release documentation and operational checklists now treat `v0.1.11` as the
  current release target and production freeze baseline.

### Security

- Relayna `rk_live_...` bearer credentials still use the Relayna-authenticated
  direct passthrough path with mapping lookup, policy checks, rate limits,
  budgets, credential stripping, and status-only usage.
- Direct LiteLLM bearer delegation applies only to non-Relayna bearer
  credentials on canonical direct-mode routes; those credentials are forwarded
  using the configured upstream LiteLLM header mode/name.
- Trusted-ingress dashboard/admin passthrough remains opt-in behind enabled
  passthrough, `trusted_ingress` UI exposure, `explicitly_exposed` admin API
  exposure, and configured method/path allowlists.

## 0.1.10 - 2026-06-19

### Added

- Added a browser-safe LiteLLM UI access path at
  `/admin-ui/litellm-ui/{*path}` that requires a valid operator token and
  proxies directly to LiteLLM with upstream credential injection only.
- Added a new LiteLLM UI exposure mode: `trusted_ingress` for trusted identity-
  aware ingress flows that should allow browser-safe access to `/ui` and its
  support endpoints without Relayna credential headers.
- Added a complete setup walkthrough for LiteLLM passthrough options, including
  wildcard path/method allowlists, `ui_exposure` and `admin_api_exposure` modes,
  canonical route modes, and verified browser access patterns.
- Added captured real-environment LiteLLM passthrough screenshots to
  `docs/litellm-passthrough.md` to document setup and access options.

### Changed

- Updated the freeze perimeter check and related workflow/docs references to the
  new `v0.1.10` baseline test file `tests/freeze-v0.1.10-perimeter.test.mjs`.
- Workspace crate versions now share the `0.1.10` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.10` gateway image.
- Release documentation and operational checklists now treat `v0.1.10` as the
  current release target and production freeze baseline.

### Security

- Operator-only LiteLLM `/ui` flows remain protected by Entra/Apigee + Relayna
  auth; they are still sensitive by default.
- `trusted_ingress` mode intentionally allows `/ui` browser access through trusted
  ingress while keeping `/v1/*` and other non-ui wildcard passthrough paths bound
  to normal Relayna proxy authentication and policy checks.

## 0.1.9 - 2026-06-18

### Added

- LiteLLM wildcard passthrough can now be enabled as a single-ingress mode for
  Gateway deployments that sit in front of LiteLLM. Operators configure path
  and method allowlists, with `/v1/*` `GET` and `POST` as the safe default
  when passthrough is enabled.
- Admin APIs, PostgreSQL storage, and the Admin portal now expose LiteLLM
  passthrough settings for enablement, path/method allowlists, `/ui` exposure,
  and LiteLLM admin API exposure.
- Canonical OpenAI-compatible routes now support per-route mode selection:
  `managed_by_gateway` or `direct_litellm_passthrough`.
- The real LiteLLM harness now verifies wildcard `/v1/models` passthrough,
  path/query preservation, route-mode switching, credential stripping, and
  LiteLLM custom header injection against a real `litellm/litellm` container.

### Changed

- Workspace crate versions now share the `0.1.9` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.9` gateway image.
- Release documentation now treats `v0.1.9` as the current release target while
  establishing `v0.1.9` as the production freeze baseline.

### Security

- Gateway continues to accept Relayna credentials from clients and translate
  them to internal LiteLLM credentials. Client `Authorization`, Relayna key
  headers, Entra/Apigee identity headers, proxy auth, `x-api-key`, and
  client-supplied LiteLLM credential headers are stripped before forwarding.
- Canonical `direct_litellm_passthrough` still enforces route enablement,
  Relayna policy, provider/model permissions, rate limits, and budgets before
  forwarding to LiteLLM. Wildcard non-canonical passthrough records reduced
  status-only usage.
- Sensitive LiteLLM `/ui` and admin-like paths remain blocked by default.
  `operator_only` exposure requires the Gateway Entra/Apigee identity layer;
  `explicitly_exposed` makes the allowlisted sensitive path reachable to
  authenticated Relayna virtual-key clients.

## 0.1.8 - 2026-05-31

### Added

- LiteLLM provider configuration now supports operator-managed credential
  header mode. Operators can keep the default `Authorization: Bearer <key>`
  behavior or send the selected LiteLLM credential through a custom header such
  as `x-litellm-api-key`.
- Admin APIs, PostgreSQL storage, and the Admin portal now support write-only
  LiteLLM virtual-key mappings by Relayna key or project. Runtime credential
  resolution prefers key mapping, then project mapping, then the active
  LiteLLM provider default credential.
- Operator documentation now explains how to configure LiteLLM custom headers
  and key/project credential mappings with captured Admin UI screenshots.

### Changed

- Workspace crate versions now share the `0.1.8` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.8` gateway image.
- Release documentation now treats `v0.1.8` as the current release target while
  preserving `v0.1.8` as the production freeze baseline.

### Security

- LiteLLM mapping secrets are write-only in Admin API responses, audit
  snapshots, and the Admin portal. Gateway strips client credentials before
  forwarding LiteLLM traffic and only sends the resolved internal LiteLLM
  credential upstream.

## 0.1.7 - 2026-05-30

### Added

- The Admin portal Settings page now exposes Entra ID and Apigee front-door
  auth controls that were previously deployment-env only, including enablement
  toggles, tenant and issuer configuration, audience, OIDC discovery URL,
  scope, role, group allowlist, accepted JWT algorithms, JWKS cache TTL, clock
  skew, Relayna key header, and write-only Apigee HMAC secret management.
- Gateway now persists Admin-saved front-door auth settings in PostgreSQL and
  applies them immediately to proxy runtime authentication while preserving
  environment-variable bootstrap behavior.
- Operator documentation now includes a field-by-field Admin UI walkthrough
  with screenshots for the Entra ID and Apigee front-door settings panel.

### Changed

- Workspace crate versions now share the `0.1.7` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.7` gateway image.
- Release documentation now treats `v0.1.7` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

### Fixed

- The Admin portal sidebar now scrolls independently so the `Sign out` action
  remains reachable on small monitors.

## 0.1.6 - 2026-05-30

### Added

- LiteLLM passthrough now includes canonical OpenAI-compatible
  `POST /v1/embeddings` requests alongside `POST /v1/chat/completions` and
  `POST /v1/responses`.
- OpenAI route settings and PostgreSQL seed data now include the `embeddings`
  route so operators can enable or disable embeddings passthrough with the
  existing route controls.
- The real LiteLLM passthrough report now validates chat completions,
  responses, and embeddings through the Entra/Apigee front-door test path.

### Changed

- Workspace crate versions now share the `0.1.6` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.6` gateway image.
- Release documentation now treats `v0.1.6` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

### Fixed

- The LiteLLM real-environment report harness now uses a short non-secret
  operator-token fixture so committed test configuration does not trip the
  `relayna-live-token` secret scanning rule.

## 0.1.5 - 2026-05-30

### Added

- Opt-in Microsoft Entra ID front-door authorization for provider traffic.
  Gateway can now validate Entra JWTs before Relayna virtual-key
  authentication while preserving the existing virtual-key-only path when
  Entra mode is disabled.
- Trusted Apigee gateway mode for deployments that terminate Entra at Apigee
  and forward a signed, sanitized identity header to Relayna Gateway.
- Configurable Relayna virtual-key header for Entra and Apigee gateway modes
  through `ENTRA_RELAYNA_KEY_HEADER`, defaulting to `X-Relayna-Key`.
- Dedicated Entra ID and Apigee gateway path documentation, including request
  contracts, config tables, validation behavior, failure modes, Kubernetes
  rollout guidance, and verification steps.

### Changed

- Workspace crate versions now share the `0.1.5` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.5` gateway image.
- Release documentation now treats `v0.1.5` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

### Security

- Entra mode strips `Authorization`, the configured Relayna key header, legacy
  `X-AIH-API-Key`, `X-Relayna-Key`, Apigee identity proof headers, and other
  sensitive client credentials before forwarding upstream.
- Entra token validation fails closed for malformed bearer headers, unknown
  `kid`, invalid metadata or JWKS, unsupported algorithms, invalid signature,
  wrong issuer, wrong audience, expired or not-yet-valid timestamps, group
  overage, and missing required scope, role, or group.
- Trusted Apigee header mode is disabled by default and requires
  `APIGEE_TRUSTED_HEADER_SECRET`; unsigned or incorrectly signed identity
  headers are rejected with stable Entra/Apigee error codes.

## 0.1.4 - 2026-05-25

### Added

- Registered services can now define `health_check_path` and
  `health_check_method` so active health checks probe a service-specific
  endpoint instead of only the upstream root.
- The Admin portal service create/edit flows expose health-check path and
  method fields, and Studio service imports preserve configured Gateway-owned
  health-check settings on re-import.
- The Admin portal policy simulator can evaluate registered service policy by
  explicit `service_name` for `/services/<service-name>/*` routes.

### Changed

- Workspace crate versions now share the `0.1.4` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.4` gateway image.
- Release documentation now treats `v0.1.4` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

### Fixed

- Database-backed policy simulation now decodes stored policy layers with the
  expected SQL aliases instead of returning store-state errors.
- The Admin portal policy simulator no longer submits stale hidden service
  selections after an operator switches back to non-service routes/providers.
- Admin portal notices now auto-dismiss after successful async actions.

## 0.1.3 - 2026-05-24

### Changed

- Workspace crate versions now share the `0.1.3` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.3` gateway image.
- Release documentation now treats `v0.1.3` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

### Fixed

- Release images now apply available Debian runtime package security upgrades
  before installing runtime dependencies. This lets the Trivy image scan pick
  up fixed Debian security packages such as `libgnutls30` during tag releases.

## 0.1.2 - 2026-05-24

### Added

- First-time Admin portal setup manual with step-by-step provider, service,
  project, policy, and key setup guidance.
- Real Admin UI screenshots for every first-time setup step, captured with demo
  values and redacted credentials.

### Changed

- Workspace crate versions now share the `0.1.2` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.2` gateway image.
- Release documentation now treats `v0.1.2` as the current release target while
  preserving `v0.1.0` as the production freeze baseline.

## 0.1.0 - 2026-05-24

### Added

- Admin UI 2.0 source package and design system, with Monitor, Discover, and
  Govern navigation, reusable operator-console components, responsive layout
  rules, and floating message boxes.
- Scoped operator governance with role/scope metadata, scope-aware admin
  authorization, `insufficient_operator_scope` failures, and append-only audit
  event reads.
- Policy governance workflows for safe key presets, lifecycle metadata,
  inherited policy layers, policy simulation, stricter per-request limits, and
  stable request/response size-limit errors.
- Provider intelligence orchestration with routing strategies, provider health
  state, circuit breaker state, retry-safe fallback policy, redacted debug
  bundles, and service import preview, activation, version history, and
  rollback.
- Observability analytics for trace-aware usage records, usage breakdowns,
  timeseries data, unused-key discovery, task drilldowns, JSON/CSV exports, and
  low-cardinality Prometheus metrics.
- Supply-chain and deployment hardening, including strict CI security scans,
  release metadata validation, SBOM, signing, provenance, hardened Kubernetes
  defaults, and documented temporary security exceptions.
- Current Feature Highlights documentation with sanitized Admin UI screenshots
  for the new operator workflows.

### Changed

- Workspace crate versions now share the `0.1.0` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.1.0` gateway image.
- Release documentation now treats `v0.1.0` as both the feature release target
  and the production freeze baseline for future compatibility checks.

### Security

- Admin UI and provider-intelligence documentation now call out write-only
  credential handling, show-once token behavior, redacted debug bundles,
  sanitized audit snapshots, and bounded metric labels.

## 0.0.14 - 2026-05-22

### Changed

- Workspace crate versions now share the `0.0.14` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.0.14` gateway image.

### Fixed

- Pingora proxy requests now replace the stripped downstream `Host` header with
  the selected upstream host and port before forwarding. This keeps HTTP/1.1
  registered service traffic valid for strict upstream servers such as
  Uvicorn/FastAPI services.

## 0.0.13 - 2026-05-22

### Added

- Redis budget counter rehydration from PostgreSQL usage events during startup
  and periodic reconciliation. Budgeted keys can recover daily and monthly
  spend counters after Redis loss without treating Redis as the billing ledger.
- Token-per-minute enforcement for virtual key `tpm_limit` policy settings
  using Redis minute buckets and the stable `token_rate_limit_exceeded` error.
- Protected admin usage export endpoints:
  `/admin-ui/admin/usage/export.json` and
  `/admin-ui/admin/usage/export.csv`.
- Integration coverage for empty-Redis budget recovery, invalid cost filtering,
  unbudgeted key skipping, reservation preservation, and shared TPM counters.

### Changed

- Workspace crate versions now share the `0.0.13` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.0.13` gateway image.
- Budget reservations now apply to requests with configured preflight estimated
  cost, including non-streaming registered service traffic.
- Usage exports use the same admin usage filters and summary totals as the
  usage dashboard, with default pagination and a maximum page-size clamp.

### Security

- CSV usage exports neutralize spreadsheet formula prefixes before escaping
  cells to reduce spreadsheet injection risk for operator-downloaded reports.
- The new usage export routes require the existing operator token and do not
  expose provider credentials, LiteLLM service keys, or raw virtual keys.

## 0.0.12 - 2026-05-21

### Added

- AKS-safe admin/control base path support under `/admin-ui/*`, including
  relocated health, readiness, metrics, Admin API, and guardrail control
  routes.
- Documentation and deployment examples for operating Relayna Gateway when
  another cluster gateway owns `/`, `/healthz`, `/readyz`, and `/metrics`.

### Changed

- Workspace crate versions now share the `0.0.12` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.0.12` gateway image.
- Root-level admin/control routes are no longer registered; runtime proxy
  routes such as `/services/*`, `/v1/chat/completions`, and `/v1/responses`
  remain unchanged.
- Admin portal requests now use `/admin-ui/admin/*` and `/admin-ui/readyz`.

### Fixed

- Architecture documentation now renders Mermaid diagrams instead of showing
  raw diagram source.
- Admin portal async action failures are surfaced in the notice area, and the
  Services form validates DNS-style service names before submit.

## 0.0.11 - 2026-05-21

### Added

- Optional `GATEWAY_ADMIN_TOKEN` first-start bootstrap seeding for fresh
  databases. When set to a valid `op_live_...` operator token before first
  startup, Gateway stores only its hash and does not print the raw token.

### Changed

- Workspace crate versions now share the `0.0.11` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.0.11` gateway image.
- Operator-token documentation now clarifies that PostgreSQL remains
  authoritative after bootstrap: later `GATEWAY_ADMIN_TOKEN` changes are
  ignored once an active token exists, and Admin portal rotation is the
  supported post-bootstrap change path.

## 0.0.10 - 2026-05-19

### Added

- PostgreSQL database reference documentation covering gateway tables, keys,
  required operational data, and secret-handling expectations.
- Redis key reference documentation covering request rate-limit counters,
  budget counters, reservation keys, TTLs, and operational handling.

### Changed

- Workspace crate versions now share the `0.0.10` release version.
- Deployment examples and the baseline Kubernetes image now target the
  `0.0.10` gateway image.

## 0.0.9 - 2026-05-17

### Added

- Guardrail catalog, policy, discovery, test, and proxy enforcement support for
  JSON requests and responses.
- Built-in `pii-redact` guardrail with pre-call, post-call, and during-call
  modes, sanitized execution records, and opt-in key policy controls.
- Admin portal guardrail catalog CRUD for custom HTTP guardrails, protected
  built-in editing, and key-level mandatory, optional, and forbidden guardrail
  selection.
- Global guardrail runtime config and per-key
  `guardrail_config_overrides`, including support for tuning each selected
  guardrail differently per virtual key.

### Changed

- Workspace crate versions now share the `0.0.9` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.9`
  gateway image.
- Key create and edit forms now use guardrail picker controls and only show
  per-key override editors after mandatory or optional guardrails are selected.

### Security

- Guardrail execution records persist sanitized metadata only and never include
  raw request bodies, response bodies, bearer tokens, or PII mappings.
- HTTP guardrail bearer tokens remain write-only; guardrail API responses expose
  sanitized schema and runtime config fields only.

## 0.0.8 - 2026-05-16

### Added

- Protected Admin API endpoints for reading, updating, testing, and clearing
  the Relayna Studio connection after Gateway startup.
- Admin portal Settings controls for Studio backend URL, write-only bearer
  token replacement, token clearing, persisted settings clearing, and connection
  testing.
- PostgreSQL-backed Studio connection settings with environment-variable
  fallback from `RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN`.

### Changed

- Workspace crate versions now share the `0.0.8` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.8`
  gateway image.
- Studio service import now resolves the effective Studio connection at request
  time, so admin-saved settings take effect without restarting Gateway.

### Security

- Studio bearer token values are write-only in Admin API responses and portal
  fields.

## 0.0.7 - 2026-05-14

### Added

- Project-first service ownership in the admin API and portal. Projects can now
  link multiple services, and project-owned virtual keys inherit access through
  those service links.
- Individual virtual key ownership for keys that should access selected
  services without belonging to a project.
- Usage drilldown filters for project, virtual key, service, route, provider,
  model, and task, with project, key, and service breakdown tables.
- Admin portal service picker modals for Project service links and Individual
  key service links, matching the Studio import modal flow.

### Changed

- Workspace crate versions now share the `0.0.7` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.7`
  gateway image.
- Usage and upstream metadata now preserve `individual` ownership when a key is
  not linked to a project.

### Fixed

- Studio import and service picker modals now constrain wide service tables so
  long route and upstream URL columns scroll instead of overlapping.

## 0.0.6 - 2026-05-13

### Added

- Admin portal `Import from Studio` flow that fetches Relayna Studio service
  exports from `GET /studio/gateway/services` and imports selected services
  into Gateway's service registry.
- Optional Studio connection configuration through `RELAYNA_STUDIO_BASE_URL`
  and `RELAYNA_STUDIO_TOKEN`.
- Explicit `No expiration` controls for virtual key creation and editing in the
  admin portal.
- Documentation for connecting Gateway to Relayna Studio, testing the Studio
  export path, importing services, and operating non-expiring virtual keys.

### Changed

- Workspace crate versions now share the `0.0.6` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.6`
  gateway image.
- Studio service re-imports preserve Gateway-owned runtime fields by default,
  including credentials, enabled state, route overrides, project links, limits,
  fallback services, and cost settings.

### Fixed

- Persisted wildcard service route aliases now strip the matched alias prefix
  before forwarding upstream while preserving query strings.
- Studio catalog fetches now use a bounded request timeout so unavailable or
  stalled Studio backends return `studio_unavailable` instead of leaving the
  admin portal import action stuck.

## 0.0.5 - 2026-05-12

### Added

- Admin project management APIs and portal view for creating project UUIDs and
  linking virtual keys and services to projects.
- Admin provider configuration APIs and portal view for LiteLLM and internal
  service endpoints with write-only credentials.
- Persisted service route-pattern resolution so registered internal routes can
  be selected and used consistently by the proxy.
- Admin portal provider selectors, service route choices, and cost-mode help
  text for fixed and passthrough pricing.

### Changed

- Workspace crate versions now share the `0.0.5` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.5`
  gateway image.

### Fixed

- Overview, Usage, project usage, and key usage cost summaries now report
  numeric zero-cost aggregates instead of `n/a` when no cost rows are present.
- Fixed-cost service requests now record the configured estimate when upstream
  responses do not include passthrough cost fields.

## 0.0.4 - 2026-05-11

### Added

- `GET /services/<service-name>/*` wildcard routing for registered services,
  with forwarding still constrained by each service registration's allowed
  methods.
- PostgreSQL-backed admin controls for globally enabling and disabling
  `/v1/chat/completions` and `/v1/responses`, enabled by default for upgrade
  compatibility.
- Admin portal route controls for OpenAI-compatible routes and registered
  service routes.

### Changed

- Service method editing in the admin portal now uses explicit method
  checkboxes instead of free-form text entry.
- Release publishing now validates that the Git tag, workspace version, and
  matching changelog section agree before Docker login, image publishing, or
  GitHub release creation.
- Workspace crate versions now share the `0.0.4` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.4`
  gateway image.

### Fixed

- Service wildcard `GET` requests can now resolve as service wildcard traffic
  instead of being rejected as unsupported routes.
- Disabled OpenAI-compatible routes return a stable `403 disabled_route` error
  after authentication and record terminal usage for the denied call.

## 0.0.3 - 2026-05-10

### Added

- GitHub Container Registry publishing in the tag-based release workflow.
- Release image tags for full semver, major-minor, and latest aliases.

### Changed

- Workspace crate versions now share the `0.0.3` release version.
- Deployment examples and the baseline Kubernetes image now target the `0.0.3`
  gateway image.

## 0.0.2 - 2026-05-10

### Added

- Release-ready container packaging for the gateway proxy and embedded admin UI in a single Docker image.
- Material for MkDocs documentation covering architecture, local setup, Docker, Kubernetes, operations, and release flow.
- Admin portal static asset tests and CI coverage for the operator console.
- GitHub Pages documentation deployment and release-note extraction from this changelog.

### Changed

- Workspace crate versions now share the `0.0.2` release version.
- README now describes the implemented gateway, admin portal, dependencies, and deployment entry points instead of MVP targets.

### Notes

- `v0.0.2` should be created after these release-prep changes are committed so the tag points at the release content.
