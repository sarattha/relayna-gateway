# Changelog

All notable changes to Relayna Gateway are documented in this file.

## 0.0.10 - 2026-05-19

### Added

- PostgreSQL database reference documentation covering gateway tables, keys,
  required operational data, and secret-handling expectations.
- Redis key reference documentation covering request rate-limit counters,
  budget counters, reservation keys, TTLs, and operational handling.
- Optional `GATEWAY_ADMIN_TOKEN` first-start bootstrap seeding for fresh
  databases.

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
