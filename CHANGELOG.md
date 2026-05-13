# Changelog

All notable changes to Relayna Gateway are documented in this file.

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
