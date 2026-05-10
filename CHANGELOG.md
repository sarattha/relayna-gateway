# Changelog

All notable changes to Relayna Gateway are documented in this file.

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
