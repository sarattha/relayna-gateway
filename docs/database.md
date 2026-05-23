# PostgreSQL Database

PostgreSQL is the durable source of truth for Relayna Gateway. It stores
projects, virtual keys, access policy, registered service routes, provider
configuration, route toggles, guardrail configuration, operator tokens, and
usage records.

## Scope

- Relayna Gateway requires PostgreSQL 14 or newer.
- Tables are created in the default `public` schema.
- Migrations enable the `pgcrypto` extension so UUID primary keys can use
  `gen_random_uuid()`.
- `PostgresStore::connect` runs the bundled SQLx migrations from
  `crates/gateway-store/migrations` on startup.
- SQLx also maintains its migration bookkeeping table, `_sqlx_migrations`.

Do not treat this page as a replacement for migrations. The current schema is
defined by the migration files, and this page explains the operational meaning
of that schema.

## Entity Overview

| Area | Tables | Purpose |
| --- | --- | --- |
| Projects | `projects` | Groups project-owned virtual keys and service access. |
| Virtual keys | `api_keys`, `key_policies`, `key_guardrail_policies`, `policy_layers` | Stores key identity, inherited request policy, limits, budgets, lifecycle metadata, and guardrail policy. |
| Services | `service_registrations`, `project_service_links`, `key_service_links` | Registers `/services/<service-name>/*` routes and grants project or individual-key access. |
| Providers and routes | `provider_configs`, `openai_route_settings`, `route_policies` | Stores upstream provider settings and global OpenAI-compatible route toggles. |
| Guardrails | `guardrail_definitions`, `guardrail_execution_events` | Stores guardrail catalog entries and execution audit records. |
| Studio settings | `studio_connection_settings` | Stores the optional Relayna Studio import connection. |
| Operators | `operator_tokens` | Stores hashed tokens for `/admin-ui/admin/*` and `/admin-ui` access. |
| Usage | `usage_events` | Stores request accounting for admin usage views, exports, budget rehydration, and Relayna Studio consumption. |

## Required Operational Data

- At least one active `operator_tokens` row is required for authenticated
  `/admin-ui/admin/*` access after bootstrap. Startup creates one bootstrap token when
  no active token exists. If `GATEWAY_ADMIN_TOKEN` is set, a fresh database is
  seeded from that token. Otherwise startup generates and prints a raw token
  once. After an active token exists, env changes are ignored and token changes
  must go through Admin portal rotation.
- Project-owned keys require a `projects` row and an `api_keys.project_id`
  value that references it. Individual keys must have `project_id` set to
  `NULL`.
- A usable virtual key needs an `api_keys` row. If no `key_policies` row exists,
  runtime code uses default policy values; admin-created keys normally upsert an
  explicit policy row.
- Service routing requires an enabled `service_registrations` row with complete
  runtime fields. Project-owned keys use `project_service_links`; individual
  keys use `key_service_links`.
- OpenAI-compatible route availability is controlled by seeded
  `openai_route_settings` rows for `/v1/chat/completions` and `/v1/responses`.
- Guardrail use depends on `guardrail_definitions` plus per-key
  `key_guardrail_policies`. Migrations seed the built-in `pii-redact`
  definition as enabled but not default-on.
- `usage_events` and `guardrail_execution_events` are append-only operational
  records used by admin usage, exports, budget rehydration, observability, and
  audit workflows.

## Table Reference

### `projects`

Projects group shared service access and project-owned keys.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Unique keys | `name` is unique. |
| Checks | `name` must be non-empty after trimming and at most 120 characters. |
| Referenced by | `api_keys.project_id`, `service_registrations.project_id`, `project_service_links.project_id`, and `guardrail_execution_events.project_id`. |
| Required data | Create a project before creating project-owned virtual keys or project-scoped service links. |

### `api_keys`

`api_keys` stores Relayna virtual key identity and lifecycle state. Raw virtual
keys are never stored.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Unique keys | `key_prefix` is unique and is used for lookup before hash verification. |
| Foreign keys | `project_id` references `projects(id)` with `ON DELETE RESTRICT` when `owner_type = 'project'`. |
| Checks | `owner_type` must be `project` or `individual`; project keys require `project_id`, individual keys require `project_id IS NULL`. |
| Lifecycle fields | `disabled`, `revoked_at`, and `expires_at` determine whether a key can authenticate. `rotation_due_at` helps operators plan rotation, and `last_used_at` records the last observed key use when populated by runtime paths. |
| Secret fields | `key_hash` stores an Argon2 hash of the raw `rk_live_...` key. |
| Referenced by | `key_policies`, `key_guardrail_policies`, `key_service_links`, `usage_events`, `guardrail_execution_events`, and legacy `route_policies`. |

### `key_policies`

`key_policies` stores route, model, provider, service, rate-limit, budget, and
feature permissions for a virtual key.

| Key | Details |
| --- | --- |
| Primary key | `key_id`, also a foreign key to `api_keys(id)` with `ON DELETE CASCADE`. |
| Defaults | Routes default to `/v1/chat/completions` and `/v1/responses`; providers default to `litellm`; models and services default to empty arrays. |
| Limits | `rpm_limit`, `tpm_limit`, `daily_budget_usd`, `monthly_budget_usd`, daily request/token caps, per-request cost/token caps, UTC-hour allowlists, stale-key auto-disable days, request/response byte caps, stream duration, SSE event, tool call, and tool schema caps are nullable unless otherwise noted. `NULL` means no database-configured limit for that field. |
| Feature flags | `allow_streaming` and `allow_tools` default to `false`. |
| Versioning | `policy_version` increments when key policy is updated and is surfaced for simulation and debugging. |
| Indexes | `idx_key_policies_limits` supports lookups for keys with configured limits or budgets. |
| Required data | Admin key creation upserts this row. If it is missing, runtime defaults are used. |

### `policy_layers`

`policy_layers` stores optional inherited policy layers for deterministic
effective-policy resolution.

| Key | Details |
| --- | --- |
| Primary key | `id uuid`. |
| Unique keys | `(layer_kind, scope_id)` is unique. |
| Layer kinds | `global`, `project`, `team`, `key`, `route`, and `model`. |
| Payloads | `policy jsonb` stores additive `KeyPolicy` fields; `guardrail_policy jsonb` stores the guardrail layer. |
| Versioning | `policy_version` identifies the layer version used in simulator and runtime debugging. |
| Required data | Optional. Existing per-key policy behavior is preserved when this table has no matching rows. Layer JSON uses neutral defaults so empty layers do not narrow routes, providers, streaming, or tools. |

### `key_guardrail_policies`

`key_guardrail_policies` stores guardrail selection and per-key runtime config
overrides.

| Key | Details |
| --- | --- |
| Primary key | `key_id`, also a foreign key to `api_keys(id)` with `ON DELETE CASCADE`. |
| Policy arrays | `mandatory_guardrails`, `optional_guardrails`, and `forbidden_guardrails` default to empty arrays. |
| Overrides | `guardrail_config_overrides jsonb` defaults to `{}` and stores shallow per-key overrides for selected guardrails. |
| Required data | Only required when a key opts into mandatory, optional, forbidden, or overridden guardrail behavior. |

### `project_service_links`

`project_service_links` grants project-owned keys access to registered
services.

| Key | Details |
| --- | --- |
| Primary key | Composite key on `(project_id, service_name)`. |
| Foreign keys | `project_id` references `projects(id)` with `ON DELETE CASCADE`; `service_name` references `service_registrations(name)` with `ON DELETE CASCADE`. |
| Indexes | `project_service_links_service_name_idx` supports reverse lookup by service. |
| Required data | Required for project-owned keys to call registered service routes. |

### `key_service_links`

`key_service_links` grants individual keys access to registered services.

| Key | Details |
| --- | --- |
| Primary key | Composite key on `(key_id, service_name)`. |
| Foreign keys | `key_id` references `api_keys(id)` with `ON DELETE CASCADE`; `service_name` references `service_registrations(name)` with `ON DELETE CASCADE`. |
| Indexes | `key_service_links_service_name_idx` supports reverse lookup by service. |
| Required data | Required for individual keys to call registered service routes. |

### `service_registrations`

`service_registrations` defines registered service routes under
`/services/<service-name>/*`.

| Key | Details |
| --- | --- |
| Primary key | `name text`. |
| Unique keys | `studio_service_id` is unique when present. |
| Foreign keys | `project_id` references `projects(id)` with `ON DELETE RESTRICT` when present. |
| Checks | `name` must be lowercase DNS-label style; `source` is `gateway` or `studio`; `sync_status` is `local`, `synced`, `incomplete`, `stale`, or `failed`; `cost_mode` is `fixed`, `passthrough`, or `none`; `timeout_ms` and `max_body_bytes` must be positive. |
| Runtime fields | `route_pattern`, `upstream_base_url`, `enabled`, `allowed_methods`, `timeout_ms`, `max_body_bytes`, `cost_mode`, `estimated_cost_usd`, `credential_secret`, and `fallback_services`. |
| Indexes | `service_registrations_studio_service_id_idx`, `service_registrations_source_status_idx`, and `service_registrations_project_id_idx`. |
| Required data | A service must be enabled and have complete runtime fields before the proxy can forward matching service traffic. |

### `provider_configs`

`provider_configs` stores operator-managed upstream provider settings.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Unique keys | `(provider, name)` is unique. Only one enabled `litellm` config is allowed. |
| Checks | `provider` must be `litellm` or `internal-service`; `name` must be non-empty and at most 120 characters; `base_url` must start with `http://` or `https://`. |
| Secret fields | `credential_secret` stores the internal upstream credential and is treated as write-only by API responses. |
| Required data | Needed when operators configure runtime provider settings through the admin API or portal instead of environment fallback. |

### `openai_route_settings`

`openai_route_settings` stores global enablement for OpenAI-compatible proxy
routes.

| Key | Details |
| --- | --- |
| Primary key | `route_id text`. |
| Unique keys | `route` is unique. |
| Checks | `route_id` is limited to `chat-completions` and `responses`; `route` is limited to `/v1/chat/completions` and `/v1/responses`. |
| Seed data | Migrations insert both supported routes as enabled. |
| Required data | These rows must exist for operators to toggle global OpenAI-compatible route availability. |

### `studio_connection_settings`

`studio_connection_settings` stores the optional Relayna Studio import
connection configured through Admin portal Settings.

| Key | Details |
| --- | --- |
| Primary key | `singleton boolean`, constrained to `true`. |
| Checks | `base_url` must be `NULL` or an HTTP/HTTPS URL. |
| Secret fields | `bearer_token_secret` stores the Studio bearer token and is write-only in API responses. |
| Required data | Optional. When no row or no base URL exists, Gateway can fall back to `RELAYNA_STUDIO_BASE_URL` and `RELAYNA_STUDIO_TOKEN`. |

### `operator_tokens`

`operator_tokens` stores admin authentication tokens. Raw operator tokens are
never stored.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Unique keys | `token_prefix` is unique. A partial unique index allows only one active token where `disabled = false` and `revoked_at IS NULL`. |
| Governance fields | `roles` and `scopes` bind the token to operator capabilities. Bootstrap and rotated owner tokens default to role `owner` and wildcard scope `*`. |
| Lifecycle fields | `disabled`, `revoked_at`, and `last_used_at`. |
| Secret fields | `token_hash` stores an Argon2 hash of the raw operator token. |
| Indexes | `operator_tokens_active_idx` and `operator_tokens_one_active_idx`. |
| Required data | At least one active row is needed for admin access after bootstrap. `GATEWAY_ADMIN_TOKEN` seeds only a fresh database; existing active rows ignore env changes. |

### `audit_events`

`audit_events` is the append-only admin audit trail. It records admin mutations
and is queryable through the scoped admin audit API.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Actor | `actor_token_id` references the operator token that authorized the action. |
| Action fields | `action`, `target_type`, and optional `target_id` describe the mutation. |
| Change fields | `before_json` and `after_json` store redacted structured snapshots when available. Raw virtual keys, operator tokens, provider credentials, and internal service tokens must not be written. |
| Request fields | `request_id`, optional `ip`, optional `user_agent`, and `created_at`. |
| Indexes | `audit_events_created_at_idx`, `audit_events_actor_created_at_idx`, and `audit_events_target_created_at_idx`. |

### `usage_events`

`usage_events` records gateway request outcomes for usage summaries and
operator visibility.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Foreign keys | `key_id` references `api_keys(id)` with `ON DELETE RESTRICT`. `project_id` is nullable after project-first key support and is not currently constrained by a foreign key. |
| Request fields | `request_id`, `route`, `model`, `provider`, `service_name`, `task_id`, `run_id`, and `fallback_count`. |
| Accounting fields | `status`, `status_code`, `latency_ms`, `input_tokens`, `output_tokens`, `total_tokens`, and `estimated_cost`. |
| Indexes | Lookup indexes cover key, project, request ID, provider, service, model, and task time-series queries. |
| Required data | Written for successful and failed request paths. Preserve this table for billing, diagnostics, budget counter rehydration, usage exports, and Relayna Studio usage views. |

Admin usage export endpoints read this table with the same filters used by the
usage dashboard. JSON exports include summary totals plus row details. CSV
exports include row details only and neutralize spreadsheet formula prefixes
before sending the response.

### `guardrail_definitions`

`guardrail_definitions` stores the global guardrail catalog.

| Key | Details |
| --- | --- |
| Primary key | `name text`. |
| Runtime fields | `description`, `modes`, `default_on`, `failure_policy`, `config_schema`, `config`, and `enabled`. |
| Seed data | Migrations upsert `pii-redact` with `pre_call`, `post_call`, and `during_call` modes, `fail_closed`, and `restore_output: true`. |
| Required data | A guardrail must exist here before a key policy can reference it. |

### `guardrail_execution_events`

`guardrail_execution_events` stores audit and observability records for
guardrail execution.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Foreign keys | `key_id` references `api_keys(id)` with `ON DELETE SET NULL`; `project_id` references `projects(id)` with `ON DELETE SET NULL`. |
| Event fields | `request_id`, `route`, `model`, `provider`, `guardrail_name`, `mode`, `action`, `failure_policy`, `latency_ms`, `reason`, and `metadata`. |
| Indexes | Lookup indexes cover request ID, key, project, guardrail, and mode/action time-series queries. |
| Required data | Written when guardrails run. Preserve it for guardrail audit trails and admin summaries. |

### `route_policies`

`route_policies` is a legacy per-route policy table from the initial migration.
Current runtime and admin paths use `key_policies.allowed_routes` instead.

| Key | Details |
| --- | --- |
| Primary key | `id uuid` generated with `gen_random_uuid()`. |
| Unique keys | `(key_id, route)` is unique. |
| Foreign keys | `key_id` references `api_keys(id)` with `ON DELETE CASCADE`. |
| Current role | Retained by migration history. Do not build new behavior on this table unless the runtime is intentionally changed to use it again. |

## Secret Handling

- Raw virtual keys and raw operator tokens are shown only at creation/bootstrap
  time. PostgreSQL stores only lookup prefixes and Argon2 hashes.
- `provider_configs.credential_secret`,
  `service_registrations.credential_secret`, and
  `studio_connection_settings.bearer_token_secret` are internal secrets and
  should be treated as write-only from API and UI responses.
- Back up PostgreSQL as sensitive data because hashes, provider credentials,
  Studio credentials, service credentials, policies, and usage records are all
  operationally sensitive.
- Prefer the admin API or Admin portal for changes. Manual SQL updates should
  be reserved for recovery operations with a reviewed rollback plan.
