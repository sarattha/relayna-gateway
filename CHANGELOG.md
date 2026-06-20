# Changelog

All notable changes to Relayna Gateway are documented in this file.

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
