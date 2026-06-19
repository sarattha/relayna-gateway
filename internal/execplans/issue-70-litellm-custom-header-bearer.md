# Issue 70 LiteLLM Custom Header Bearer Credentials

This ExecPlan is a living document. The sections Progress, Surprises &
Discoveries, Decision Log, and Outcomes & Retrospective must stay up to date as
work proceeds.

If `PLANS.md` is present in the repo, maintain this document in accordance with
it and link back to it by path.

## Purpose / Big Picture

Relayna Gateway v0.1.11 can send LiteLLM credentials through a custom header,
but it always sends the raw key as the header value. Some LiteLLM deployments
expect a custom header such as `x-litellm-key` whose value is still formatted as
`Bearer <key>`. After this change, operators can select raw or bearer-formatted
custom header values for LiteLLM provider configs. Existing raw custom-header
deployments remain unchanged by default.

## Progress

- [x] (2026-06-19) Read issue #70, `internal/design-manifesto.md`, `SKILLS.md`,
  and the relevant repository skills.
- [x] (2026-06-19) Established compatibility boundary and implementation
  shape.
- [x] (2026-06-19) Add provider-config model, validation, storage, proxy, and Admin UI
  support for LiteLLM credential header value format.
- [x] (2026-06-19) Add focused tests for raw and bearer custom-header behavior.
- [x] (2026-06-19) Run local verification, freeze perimeter checks, and real-environment
  evidence.
- [x] (2026-06-19) Bump version to 0.1.12 and update CHANGELOG, docs, deployment examples,
  and CI release metadata.
- [x] (2026-06-19) Open a ready-for-review PR and confirm CI is green.

## Surprises & Discoveries

- Observation: The latest release tag is `v0.1.11`; repository guidance still
  names `v0.1.10` as the production freeze baseline, while current CI uses
  `tests/freeze-v0.1.11-perimeter.test.mjs`.
  Evidence: `git tag -l 'v*' --sort=-v:refname | head -n5` and
  `.github/workflows/ci.yml`.

- Observation: Configuring the Issue #70 header as `x-litellm-key` means
  Gateway must also strip other known LiteLLM credential headers such as
  `x-litellm-api-key`; stripping only the configured custom header leaves an
  unnecessary credential-leak path.
  Evidence: real-env harness now sends both client-supplied
  `x-litellm-api-key` and `x-litellm-key` and captures only the injected
  `x-litellm-key: Bearer <credential>` upstream.

## Decision Log

- Decision: Add `credential_header_value_format: "raw" | "bearer"` instead of
  a third credential header mode.
  Rationale: The header location and the header value format are separate
  concerns. Keeping `custom_header` intact and defaulting the new field to
  `raw` preserves released behavior while allowing the Issue #70 LiteLLM shape.
  Date/Author: 2026-06-19 / Codex.

- Decision: Apply the selected value format whenever the active LiteLLM config
  supplies the credential header settings, including service fallback
  credentials, mapped credentials, and direct LiteLLM bearer delegation.
  Rationale: Issue #70 explicitly requires consistent behavior across those
  credential sources.
  Date/Author: 2026-06-19 / Codex.

- Decision: Always strip known LiteLLM credential headers `x-litellm-api-key`
  and `x-litellm-key` before upstream injection, in addition to the configured
  custom header.
  Rationale: Operators may change the configured custom header name. Known
  alternate LiteLLM credential headers should not be forwarded from clients.
  Date/Author: 2026-06-19 / Codex.

## Outcomes & Retrospective

Implemented and verified. Relayna Gateway now supports
`credential_header_value_format: "raw" | "bearer"` for LiteLLM provider
configs, defaults existing custom headers to `raw`, and sends bearer-prefixed
custom header values when configured. Version, changelog, docs, workflow
references, freeze perimeter, Admin UI source/static bundle, and real-env
evidence were updated for `0.1.12`.

Verification completed:

- `npm run build:admin-ui`
- `npm test`
- `python3 scripts/validate-release-metadata.py v0.1.12`
- `node tests/freeze-v0.1.12-perimeter.test.mjs`
- `bash .codex/skills/code-change-verification/scripts/run.sh`
- `cargo build --workspace --all-features`
- `bash internal/test-reports/litellm-real-passthrough/run.sh`

The real LiteLLM passthrough report shows overall PASS and front-door captures
with `x-litellm-key: Bearer sk-key`, `Bearer sk-project`,
`Bearer sk-provider`, and `Bearer sk-client`, with no `Authorization`,
`x-litellm-api-key`, or Relayna client credentials forwarded to the LiteLLM
front door.

PR #71 was opened ready for review at
`https://github.com/sarattha/relayna-gateway/pull/71`. GitHub CI completed with
admin portal, docs, repository metadata, rust, and security checks passing; the
release deploy job was skipped as expected outside a tag release.

## Context and Orientation

Provider config request/response/runtime types live in
`crates/gateway-core/src/provider_configs.rs`. PostgreSQL persistence for these
types lives in `crates/gateway-store/src/postgres.rs`, with schema migrations in
`crates/gateway-store/migrations/`. The Pingora proxy injects LiteLLM upstream
credentials in `crates/gateway-proxy/src/pingora_plane.rs` via
`prepare_upstream_authority_and_credentials`. The Axum Admin UI proxy to
LiteLLM also injects credentials in `crates/gateway-api/src/app.rs`.

The Admin UI source of truth is `crates/gateway-api/admin-ui/src/main.ts`; the
checked-in generated assets under `crates/gateway-api/src/static/admin-ui/`
must be regenerated with `npm run build:admin-ui`.

## Compatibility Boundary

Compatibility boundary: latest release tag `v0.1.11`; released LiteLLM
provider custom headers currently send raw credential values. The new
`credential_header_value_format` field is additive, defaults to `raw`, and does
not change existing provider configs or request behavior unless an operator
selects `bearer`. The production freeze guard baseline remains `v0.1.10`, and
current CI pins the active release perimeter with
`tests/freeze-v0.1.11-perimeter.test.mjs`.

## Plan of Work

Add a `CredentialHeaderValueFormat` enum in gateway-core with `Raw` as the
default and validation helpers. Include it in create, patch, response, and
runtime provider config structs.

Add a PostgreSQL migration that adds `provider_configs.credential_header_value_format`
with a `raw` default and a check constraint for `raw` or `bearer`. Update
provider-config INSERT, SELECT, UPDATE, active runtime lookup, and row mapping.

Update Pingora upstream config to carry the value format, format injected
custom-header credentials as either raw key or `Bearer <key>`, and keep
Authorization bearer mode unchanged.

Update the LiteLLM UI proxy in gateway-api to apply the same configured value
format for custom headers.

Expose the field in the Admin UI create and update forms, regenerate static
assets, and update docs and release metadata for version `0.1.12`.

## Concrete Steps

Run focused checks while iterating:

    cd /Users/jobz/Works/relayna-gateway
    cargo test -p gateway-core provider_configs
    cargo test -p gateway-proxy upstream_header_preparation
    npm run build:admin-ui
    npm test

Final verification:

    node tests/freeze-v0.1.11-perimeter.test.mjs
    bash .codex/skills/code-change-verification/scripts/run.sh
    cargo build --workspace --all-features
    python3 scripts/validate-release-metadata.py v0.1.12

Real-environment verification will use the repository's LiteLLM passthrough
test environment or the available cluster environment, depending on what
credentials and services are available locally.

## Validation and Acceptance

Acceptance criteria:

- Existing provider configs without the new field behave as raw custom-header
  credentials.
- Creating or patching a LiteLLM provider config with
  `credential_header_value_format: "bearer"` causes `custom_header` upstreams
  to send `x-litellm-key: Bearer <credential>`.
- The same value format applies to fallback provider credentials, mapped
  credentials, and direct LiteLLM bearer delegation.
- Client-supplied credentials and Relayna internal headers are stripped before
  upstream injection.
- Admin UI exposes raw vs bearer value format without rendering secrets.
- Release metadata, changelog, docs, and deployment examples agree on `0.1.12`.
- CI checks on the PR pass.

## Idempotence and Recovery

The migration uses `ADD COLUMN IF NOT EXISTS` and an additive check constraint.
If a local database has a partial migration, rerun the migration after checking
whether the constraint already exists. Tests and Admin UI builds are safe to
rerun. If generated Admin UI assets are stale, rerun `npm run build:admin-ui`.

## Artifacts and Notes

Issue #70 reports that `x-litellm-key: Bearer <key>` succeeds directly against
the target LiteLLM deployment while raw custom header values fail with a 401
malformed API key response.

## Interfaces and Dependencies

At completion, provider config JSON accepts and returns:

    "credential_header_value_format": "raw"
    "credential_header_value_format": "bearer"

The persisted database column is:

    provider_configs.credential_header_value_format text NOT NULL DEFAULT 'raw'
